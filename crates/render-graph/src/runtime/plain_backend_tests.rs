//! Executes plain-data graphs against the recording fake backend.
//!
//! Covers block assembly (`float3` packing, interior padding, and every
//! resource field kind), texture rotation inside `Repeat`, `When` gating of
//! uploads and dispatches, upload staging, direct and indirect draws,
//! picking, binding checks at construction, frame-value checks, and
//! liveness errors.

use std::cell::RefCell;

use super::backend_tests::{FailPoint, Fake};
use super::plain::*;
use super::tests::DrawIndexedIndirectCommand;
use super::*;
use crate::backend::FrameBackend;
use crate::commands::CommandBatch;

type Cmd = DrawIndexedIndirectCommand;

/// One submitted batch, with every staged byte initialized: plain blocks are
/// assembled into zeroed buffers, so reading them as `u8` is sound.
#[derive(Debug, Default)]
struct Captured {
    writes: Vec<(bool, usize, Vec<u8>)>,
    dispatches: Vec<(usize, [u32; 3], Option<Vec<u8>>)>,
    draws: Vec<(usize, String, Option<Vec<u8>>)>,
    picking: Option<(usize, [f32; 2])>,
}

struct PlainFrame<'a> {
    fake: &'a Fake,
    captured: &'a RefCell<Vec<Captured>>,
}

impl FrameBackend for PlainFrame<'_> {
    type Backend = Fake;
    type Error = anyhow::Error;

    fn lookup(&self) -> &dyn FrameLookup {
        self.fake
    }

    fn submit(self, batch: CommandBatch, on_submitted: impl FnOnce()) -> anyhow::Result<()> {
        anyhow::ensure!(self.fake.fail != Some(FailPoint::Wait), "wait failed");
        let mut captured = Captured::default();
        batch.visit_writes(|uniform, index, bytes| {
            // SAFETY: plain-data blocks and uploads are fully initialized bytes
            let bytes = bytes.iter().map(|byte| unsafe { byte.assume_init() });
            captured.writes.push((uniform, index, bytes.collect()));
        });
        let push = |push: Option<&PushConstantBytes>| push.map(|push| push.as_slice().to_vec());
        for (pipeline, groups, bytes) in batch.dispatches() {
            captured
                .dispatches
                .push((*pipeline, *groups, push(bytes.as_ref())));
        }
        for draw in batch.draws() {
            let call = match draw.draw_call() {
                DrawCallConfig::VertexCount(count) => format!("vertex:{count}"),
                DrawCallConfig::Indexed {
                    index_buffer,
                    first_index,
                    index_count,
                } => format!("indexed:{index_buffer}:{first_index}:{index_count}"),
                DrawCallConfig::IndexedIndirect {
                    index_buffer,
                    request,
                } => format!(
                    "indirect:{index_buffer}:{}:{}:{}:{}:{}",
                    request.buffer(),
                    request.offset(),
                    request.draw_count(),
                    request.stride(),
                    request.alignment()
                ),
            };
            captured
                .draws
                .push((draw.pipeline_index(), call, push(draw.push_constants())));
        }
        captured.picking = batch
            .picking()
            .map(|picking| (picking.pipeline_index(), picking.position()));
        self.captured.borrow_mut().push(captured);
        anyhow::ensure!(self.fake.fail != Some(FailPoint::Submit), "submit failed");
        on_submitted();
        anyhow::ensure!(self.fake.fail != Some(FailPoint::Present), "present failed");

        Ok(())
    }
}

/// Executes one frame, returning the captured batch when one was submitted.
fn run(
    graph: &mut PreparedPlainRenderGraph<Fake>,
    fake: &Fake,
    values: &[PlainValue<'_>],
) -> anyhow::Result<Captured> {
    let captured = RefCell::new(vec![]);
    let result = graph.execute(
        PlainFrame {
            fake,
            captured: &captured,
        },
        values,
    );
    let batch = captured.into_inner().pop();
    result?;

    Ok(batch.expect("a successful frame submits"))
}

fn data(key: u16, offset: u32, len: u32, src_offset: u32) -> SchemaField {
    SchemaField {
        key: FieldKey(key),
        offset,
        len,
        kind: SchemaFieldKind::Data { src_offset },
    }
}

fn resource(key: u16, offset: u32, kind: SchemaFieldKind) -> SchemaField {
    SchemaField {
        key: FieldKey(key),
        offset,
        len: 8,
        kind,
    }
}

fn floats(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_ne_bytes())
        .collect()
}

fn handle(raw: u64) -> Vec<u8> {
    raw.to_ne_bytes().to_vec()
}

fn current(buffer: BufferId, offset: u32) -> BufferRef {
    BufferRef {
        buffer,
        slot: SlotSel::Current,
        offset,
        range: None,
    }
}

/// Builds a plain description, its schemas, and its bindings together, so
/// each table row and its backend binding are declared side by side.
struct Fixture {
    desc: GraphDesc,
    schemas: SchemaTable,
    bindings: PlainBindings<Cmd>,
}

impl Fixture {
    fn new(textures: usize) -> Self {
        let mut desc = GraphDesc::default();
        for i in 0..textures {
            desc.textures.push(TexDecl {
                name: format!("tex{i}"),
                format: GraphFormat::R32Float,
                size: SizeClass::Fixed(8, 8),
                usage: TexUsage::Storage,
            });
        }

        Self {
            desc,
            schemas: SchemaTable::default(),
            bindings: PlainBindings::default(),
        }
    }

    fn schema(
        &mut self,
        name: &str,
        size: u32,
        resource_fields: Vec<ResourceFieldKind>,
        fields: Vec<SchemaField>,
    ) -> SchemaId {
        self.schemas.push(SchemaDesc {
            name: name.into(),
            size,
            resource_fields,
            layout: Some(SchemaLayout { fields }),
        })
    }

    fn bytes_value(&mut self, name: &str, size: u32, optional: bool) -> ValueId {
        let schema = self.schema(&format!("{name}.data"), size, vec![], vec![]);

        self.value(name, ValueKind::Bytes { schema }, optional)
    }

    /// A top-level value, or an optional value that gates its own scope.
    fn value(&mut self, name: &str, kind: ValueKind, optional: bool) -> ValueId {
        let id = ValueId(self.desc.values.len() as u32);
        let scope = if optional {
            ValueScope::When { gate: id }
        } else {
            ValueScope::Top
        };

        self.scoped_value(name, kind, scope)
    }

    /// A value inside `scope`; values in an optional scope are optional.
    fn scoped_value(&mut self, name: &str, kind: ValueKind, scope: ValueScope) -> ValueId {
        self.desc.values.push(ValueDecl {
            name: name.into(),
            kind,
            optional: matches!(scope, ValueScope::When { .. }),
            scope,
        });

        ValueId(self.desc.values.len() as u32 - 1)
    }

    fn buffer(
        &mut self,
        kind: BufferKind,
        capacity: Option<u32>,
        elem_size: Option<u32>,
        binding: PlainBuffer<Cmd>,
    ) -> BufferId {
        let id = BufferId(self.desc.buffers.len() as u32);
        self.desc.buffers.push(BufferDecl {
            name: format!("buf{}", id.0),
            kind,
            capacity,
            elem_size,
        });
        self.bindings.buffers.push(binding);

        id
    }

    fn uniform(
        &mut self,
        schema: SchemaId,
        data: Option<ValueId>,
        resources: Vec<ResourceRef>,
        slot: usize,
    ) -> UniformId {
        let id = UniformId(self.desc.uniforms.len() as u32);
        self.desc.uniforms.push(UniformDecl {
            name: format!("uniform{}", id.0),
            schema,
            source: UniformSourceDesc {
                data,
                bindings: keyed(resources),
            },
        });
        self.bindings
            .uniforms
            .push(PlainUniform::new(UniformSlot::<()>::from_backend(slot)));

        id
    }

    fn pipeline(
        &mut self,
        kind: PipelineKind,
        params: SchemaId,
        push: Option<SchemaId>,
        index: usize,
    ) -> PipelineId {
        let id = PipelineId(self.desc.pipelines.len() as u32);
        self.desc.pipelines.push(PipelineDecl {
            name: format!("pipeline{}", id.0),
            kind,
            params,
            push,
        });
        self.bindings.pipelines.push(match kind {
            PipelineKind::Compute => {
                PlainPipeline::compute(ComputePipelineKey::<NoPush>::new(index))
            }
            PipelineKind::Graphics => {
                PlainPipeline::graphics(GraphicsPipelineKey::<NoPush>::new(index))
            }
            PipelineKind::GraphicsIndirect => {
                PlainPipeline::graphics_indirect(GraphicsIndirectPipelineKey::<NoPush>::new(index))
            }
        });

        id
    }

    /// A data-only uniform of `size` bytes, its value, and a pipeline of
    /// `kind` for it.
    fn data_block(
        &mut self,
        kind: PipelineKind,
        size: u32,
        slot: usize,
        pipeline_index: usize,
    ) -> (PipelineId, UniformId, ValueId) {
        let value = self.bytes_value(&format!("uniform{slot}.value"), size, false);
        let schema = self.schema(
            &format!("uniform{slot}"),
            size,
            vec![],
            vec![data(0, 0, size, 0)],
        );
        let uniform = self.uniform(schema, Some(value), vec![], slot);
        let pipeline = self.pipeline(kind, schema, None, pipeline_index);

        (pipeline, uniform, value)
    }

    fn dispatch(&mut self, name: &str, pipeline: PipelineId, uniform: UniformId) -> DispatchDesc {
        DispatchDesc {
            name: name.into(),
            pipeline,
            uniform,
            groups: GroupSource::Fixed([4, 2, 1]),
            push: None,
        }
    }

    fn draw(
        &mut self,
        name: &str,
        pipeline: PipelineId,
        uniform: UniformId,
        call: DrawCall,
    ) -> DrawDesc {
        DrawDesc {
            name: name.into(),
            pipeline,
            uniform,
            call,
            push: None,
        }
    }

    fn raster(&mut self, draws: Vec<DrawDesc>) {
        self.desc
            .passes
            .push(PassDesc::Leaf(LeafPass::Raster(RasterDesc {
                name: "main".into(),
                targets: RasterTargets::Main,
                draws,
            })));
    }

    fn build(self) -> anyhow::Result<PlainRenderGraph<Cmd>> {
        PlainRenderGraph::new(self.desc, self.schemas, self.bindings)
    }

    fn prepare(self, fake: &mut Fake) -> PreparedPlainRenderGraph<Fake> {
        self.build().unwrap().prepare(fake).unwrap()
    }
}

fn keyed(resources: Vec<ResourceRef>) -> Vec<(FieldKey, ResourceRef)> {
    resources
        .into_iter()
        .enumerate()
        .map(|(i, resource)| (FieldKey(i as u16), resource))
        .collect()
}

fn build_error(fixture: Fixture) -> String {
    match fixture.build() {
        Ok(_) => panic!("an invalid plain graph was accepted"),
        Err(error) => error.to_string(),
    }
}

/// `float3 + float` packs the scalar into the vector's tail, a trailing
/// `float3` leaves 4 bytes of padding, and `float3 + float3` leaves padding
/// between the vectors. Every handle and address is 8 bytes, and every
/// padding byte is staged as zero.
#[test]
fn plain_float3_blocks_with_mixed_bindings_assemble_with_zero_padding() {
    let mut fixture = Fixture::new(1);
    let gpu_updated = fixture.buffer(
        BufferKind::GpuUpdatedFlight,
        None,
        Some(16),
        PlainBuffer::gpu_updated(GpuUpdatedSlot::<[f32; 4]>::from_backend(2, 8)),
    );
    let singleton = fixture.buffer(
        BufferKind::Singleton,
        Some(3),
        Some(24),
        PlainBuffer::singleton(SingletonSlot::<[f32; 6]>::from_backend(4, 3)),
    );
    fixture.bindings.imports.push(BindlessHandle::from_raw(77));
    fixture.desc.imports.push(ImportDecl { name: "sky".into() });

    // struct { float3 a; float s; RWTexture2D.Handle out; float4* prev; float3 b; }
    let compute_value = fixture.bytes_value("compute.data", 28, false);
    let compute_schema = fixture.schema(
        "ComputeParams",
        48,
        vec![ResourceFieldKind::StorageTex, ResourceFieldKind::BufAddr],
        vec![
            data(0, 0, 12, 0),
            data(1, 12, 4, 12),
            resource(0, 16, SchemaFieldKind::StorageTex),
            resource(1, 24, SchemaFieldKind::BufAddr),
            data(2, 32, 12, 16),
        ],
    );
    let compute_uniform = fixture.uniform(
        compute_schema,
        Some(compute_value),
        vec![
            ResourceRef::Tex(TexId(0), TexAccess::Write),
            ResourceRef::Buf(
                BufferRef {
                    slot: SlotSel::Previous,
                    ..current(gpu_updated, 0)
                },
                BufAccess::Read,
            ),
        ],
        1,
    );
    let compute_pipeline = fixture.pipeline(PipelineKind::Compute, compute_schema, None, 5);
    let dispatch = fixture.dispatch("simulate", compute_pipeline, compute_uniform);
    fixture
        .desc
        .passes
        .push(PassDesc::Leaf(LeafPass::Compute(dispatch)));

    // struct { float3 a; float3 b; Sampler2D.Handle t; Sampler2D.Handle sky; Vertex* v; }
    let draw_value = fixture.bytes_value("draw.data", 24, false);
    let draw_schema = fixture.schema(
        "DrawParams",
        64,
        vec![
            ResourceFieldKind::SampledTex,
            ResourceFieldKind::SampledTex,
            ResourceFieldKind::BufAddr,
        ],
        vec![
            data(0, 0, 12, 0),
            data(1, 16, 12, 12),
            resource(0, 32, SchemaFieldKind::SampledTex),
            resource(1, 40, SchemaFieldKind::SampledTex),
            resource(2, 48, SchemaFieldKind::BufAddr),
        ],
    );
    let draw_uniform = fixture.uniform(
        draw_schema,
        Some(draw_value),
        vec![
            ResourceRef::Tex(TexId(0), TexAccess::Read),
            ResourceRef::External(ImportId(0)),
            ResourceRef::Buf(current(singleton, 24), BufAccess::Read),
        ],
        2,
    );
    let draw_pipeline = fixture.pipeline(PipelineKind::Graphics, draw_schema, None, 6);
    let draw = fixture.draw(
        "draw",
        draw_pipeline,
        draw_uniform,
        DrawCall::VertexCount(3),
    );
    fixture.raster(vec![draw]);

    let mut fake = Fake::default();
    let mut graph = fixture.prepare(&mut fake);
    let compute_data = floats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    let draw_data = floats(&[1.5, 2.5, 3.5, 4.5, 5.5, 6.5]);
    let batch = run(
        &mut graph,
        &fake,
        &[
            PlainValue::Bytes(&compute_data),
            PlainValue::Bytes(&draw_data),
        ],
    )
    .unwrap();

    // one physical image: storage handle 10, sampled handle 20
    let compute_block = [
        floats(&[1.0, 2.0, 3.0, 4.0]),
        handle(10),
        handle(2000 + 2 * 100),
        floats(&[5.0, 6.0, 7.0]),
        vec![0; 4],
    ]
    .concat();
    let draw_block = [
        floats(&[1.5, 2.5, 3.5]),
        vec![0; 4],
        floats(&[4.5, 5.5, 6.5]),
        vec![0; 4],
        handle(20),
        handle(77),
        handle(3000 + 4 * 100 + 24),
        vec![0; 8],
    ]
    .concat();
    assert_eq!(
        batch.writes,
        vec![(true, 1, compute_block), (true, 2, draw_block)]
    );
    assert_eq!(batch.dispatches, vec![(5, [4, 2, 1], None)]);
    assert_eq!(batch.draws, vec![(6, "vertex:3".into(), None)]);
}

/// The jacobi shape: a push block re-resolves per iteration, so its handles
/// swap each trip; cursors carry across frames only after a submission.
#[test]
fn plain_repeat_rotates_push_handles_and_commits_on_submit() {
    let mut fixture = Fixture::new(1);
    let (pipeline, uniform, uniform_value) = fixture.data_block(PipelineKind::Compute, 4, 1, 3);
    let push = fixture.schema(
        "JacobiPush",
        16,
        vec![ResourceFieldKind::SampledTex, ResourceFieldKind::StorageTex],
        vec![
            resource(0, 0, SchemaFieldKind::SampledTex),
            resource(1, 8, SchemaFieldKind::StorageTex),
        ],
    );
    fixture.desc.pipelines[pipeline.0 as usize].push = Some(push);
    let mut dispatch = fixture.dispatch("jacobi", pipeline, uniform);
    dispatch.push = Some(PushDesc {
        data: None,
        schema: push,
        bindings: keyed(vec![
            ResourceRef::Tex(TexId(0), TexAccess::Read),
            ResourceRef::Tex(TexId(0), TexAccess::Write),
        ]),
    });
    let count = fixture.value("iterations", ValueKind::Count, false);
    fixture.desc.passes.push(PassDesc::Repeat {
        name: "solve".into(),
        count,
        body: vec![LeafPass::Compute(dispatch)],
    });
    assert_eq!(count.0, uniform_value.0 + 1);

    let mut fake = Fake::default();
    let mut graph = fixture.prepare(&mut fake);
    let uniform_bytes = 9u32.to_ne_bytes();
    let pushes = |batch: &Captured| -> Vec<Vec<u8>> {
        batch
            .dispatches
            .iter()
            .map(|(_, _, push)| push.clone().unwrap())
            .collect()
    };
    // two images: storage 10/11, sampled 20/21
    let swap = |read: u64, write: u64| [handle(read), handle(write)].concat();

    let odd = run(
        &mut graph,
        &fake,
        &[PlainValue::Bytes(&uniform_bytes), PlainValue::Count(3)],
    )
    .unwrap();
    assert_eq!(pushes(&odd), vec![swap(20, 11), swap(21, 10), swap(20, 11)]);
    assert_eq!(graph.run.tex[0].cursor, 1);

    let even = run(
        &mut graph,
        &fake,
        &[PlainValue::Bytes(&uniform_bytes), PlainValue::Count(2)],
    )
    .unwrap();
    assert_eq!(pushes(&even), vec![swap(21, 10), swap(20, 11)]);
    assert_eq!(graph.run.tex[0].cursor, 1);

    for fail in [FailPoint::Wait, FailPoint::Submit] {
        fake.fail = Some(fail);
        assert!(
            run(
                &mut graph,
                &fake,
                &[PlainValue::Bytes(&uniform_bytes), PlainValue::Count(1)],
            )
            .is_err()
        );
        assert_eq!(graph.run.tex[0].cursor, 1, "{fail:?} kept the cursor");
    }
    fake.fail = Some(FailPoint::Present);
    assert!(
        run(
            &mut graph,
            &fake,
            &[PlainValue::Bytes(&uniform_bytes), PlainValue::Count(1)],
        )
        .is_err()
    );
    assert_eq!(graph.run.tex[0].cursor, 0, "a submitted frame commits");
}

/// An upload and a dispatch gated by one optional array value.
fn gated_upload() -> Fixture {
    let mut fixture = Fixture::new(0);
    let storage = fixture.buffer(
        BufferKind::Storage,
        Some(4),
        Some(8),
        PlainBuffer::cpu_updated(CpuUpdatedSlot::<[f32; 2]>::from_backend(6, 4)),
    );
    let elem = fixture.schema("Particle", 8, vec![], vec![]);
    let particles = fixture.value("particles", ValueKind::Array { elem, max_len: 4 }, true);
    fixture.desc.uploads.push(UploadDesc {
        name: "particles".into(),
        buffer: storage,
        value: particles,
    });
    let schema = fixture.schema(
        "ParticleParams",
        8,
        vec![ResourceFieldKind::BufAddr],
        vec![resource(0, 0, SchemaFieldKind::BufAddr)],
    );
    let uniform = fixture.uniform(
        schema,
        None,
        vec![ResourceRef::Buf(current(storage, 0), BufAccess::Mutate)],
        1,
    );
    let pipeline = fixture.pipeline(PipelineKind::Compute, schema, None, 2);
    let dispatch = fixture.dispatch("integrate", pipeline, uniform);
    fixture.desc.passes.push(PassDesc::When {
        name: "when particles".into(),
        value: particles,
        body: vec![LeafPass::Compute(dispatch)],
    });

    fixture
}

#[test]
fn plain_when_skips_or_runs_its_upload_and_dispatch() {
    let mut fake = Fake::default();
    let mut graph = gated_upload().prepare(&mut fake);

    let skipped = run(&mut graph, &fake, &[PlainValue::Absent]).unwrap();
    assert!(skipped.writes.is_empty());
    assert!(skipped.dispatches.is_empty());

    let particles = floats(&[1.0, 2.0, 3.0, 4.0]);
    let ran = run(
        &mut graph,
        &fake,
        &[PlainValue::Array {
            count: 2,
            bytes: &particles,
        }],
    )
    .unwrap();
    assert_eq!(
        ran.writes,
        vec![(false, 6, particles), (true, 1, handle(1000 + 6 * 100))]
    );
    assert_eq!(ran.dispatches, vec![(2, [4, 2, 1], None)]);
}

#[test]
fn plain_upload_rejects_overflow_and_length_mismatch() {
    let mut fake = Fake::default();
    let mut graph = gated_upload().prepare(&mut fake);
    let five = vec![0; 40];
    let error = run(
        &mut graph,
        &fake,
        &[PlainValue::Array {
            count: 5,
            bytes: &five,
        }],
    )
    .unwrap_err();
    assert!(error.to_string().contains("at most 4 fit"), "{error}");

    let short = vec![0; 12];
    let error = run(
        &mut graph,
        &fake,
        &[PlainValue::Array {
            count: 2,
            bytes: &short,
        }],
    )
    .unwrap_err();
    assert!(error.to_string().contains("12 bytes for 2"), "{error}");
    assert!(fake.events.borrow().is_empty(), "nothing submitted");

    let mut oversized = gated_upload();
    let ValueKind::Array { max_len, .. } = &mut oversized.desc.values[0].kind else {
        unreachable!()
    };
    *max_len = 5;
    let error = build_error(oversized);
    assert!(error.contains("maximum 5 exceeds capacity 4"), "{error}");

    let mut wrong_capacity = gated_upload();
    wrong_capacity.desc.buffers[0].capacity = Some(3);
    wrong_capacity.desc.values[0].kind = ValueKind::Array {
        elem: SchemaId(0),
        max_len: 3,
    };
    let error = build_error(wrong_capacity);
    assert!(error.contains("backend buffer holds 4"), "{error}");
}

/// Direct, ranged, and indirect draws, with the index count read from the
/// live index buffer.
fn draws() -> Fixture {
    let mut fixture = Fixture::new(0);
    let indices = fixture.buffer(
        BufferKind::Index,
        Some(12),
        None,
        PlainBuffer::index(IndexBufferSlot::from_backend(7, 12)),
    );
    let args = fixture.buffer(
        BufferKind::Immutable,
        Some(3),
        None,
        PlainBuffer::indirect_args(ImmutableSlot::<Cmd>::from_backend(5, 3)),
    );
    let (graphics, uniform, _) = fixture.data_block(PipelineKind::Graphics, 4, 1, 3);
    let (indirect, indirect_uniform, _) =
        fixture.data_block(PipelineKind::GraphicsIndirect, 4, 2, 4);
    let draws = vec![
        fixture.draw("vertex", graphics, uniform, DrawCall::VertexCount(6)),
        fixture.draw(
            "whole",
            graphics,
            uniform,
            DrawCall::WholeIndexed { indices },
        ),
        fixture.draw(
            "range",
            graphics,
            uniform,
            DrawCall::IndexRange {
                indices,
                first_index: 6,
                index_count: 6,
            },
        ),
        fixture.draw(
            "indirect",
            indirect,
            indirect_uniform,
            DrawCall::IndexedIndirect {
                indices,
                args: current(args, 20),
                draw_count: 2,
            },
        ),
    ];
    fixture.raster(draws);

    fixture
}

fn draw_values<'a>(bytes: &'a [u8]) -> [PlainValue<'a>; 2] {
    [PlainValue::Bytes(bytes), PlainValue::Bytes(bytes)]
}

#[test]
fn plain_draws_use_index_buffers_and_exact_indirect_records() {
    let mut fake = Fake::default();
    let mut graph = draws().prepare(&mut fake);
    let bytes = 1u32.to_ne_bytes();
    let batch = run(&mut graph, &fake, &draw_values(&bytes)).unwrap();

    assert_eq!(
        batch.draws,
        vec![
            (3, "vertex:6".into(), None),
            (3, "indexed:7:0:12".into(), None),
            (3, "indexed:7:6:6".into(), None),
            (4, "indirect:7:5:20:2:20:4".into(), None),
        ]
    );
}

#[test]
fn plain_index_ranges_are_checked_at_build_and_execute() {
    let mut past_capacity = draws();
    let PassDesc::Leaf(LeafPass::Raster(raster)) = &mut past_capacity.desc.passes[0] else {
        unreachable!()
    };
    raster.draws[2].call = DrawCall::IndexRange {
        indices: BufferId(0),
        first_index: 6,
        index_count: 7,
    };
    let error = build_error(past_capacity);
    assert!(error.contains("out of bounds"), "{error}");

    // the slot was minted at 24 indices, but the live buffer holds 12
    let mut shrunk = draws();
    shrunk.desc.buffers[0].capacity = Some(24);
    shrunk.bindings.buffers[0] = PlainBuffer::index(IndexBufferSlot::from_backend(7, 24));
    let PassDesc::Leaf(LeafPass::Raster(raster)) = &mut shrunk.desc.passes[0] else {
        unreachable!()
    };
    raster.draws[2].call = DrawCall::IndexRange {
        indices: BufferId(0),
        first_index: 6,
        index_count: 12,
    };
    let mut fake = Fake::default();
    let mut graph = shrunk.prepare(&mut fake);
    let bytes = 1u32.to_ne_bytes();
    let error = run(&mut graph, &fake, &draw_values(&bytes)).unwrap_err();
    assert!(error.to_string().contains("12 indices"), "{error}");
}

#[test]
fn plain_indirect_args_need_whole_records_of_the_backend_type() {
    let with_args = |offset: u32, draw_count: u32| {
        let mut fixture = draws();
        let PassDesc::Leaf(LeafPass::Raster(raster)) = &mut fixture.desc.passes[0] else {
            unreachable!()
        };
        raster.draws[3].call = DrawCall::IndexedIndirect {
            indices: BufferId(0),
            args: current(BufferId(1), offset),
            draw_count,
        };

        fixture
    };

    for (offset, draw_count) in [(4, 1), (40, 2), (0, 0)] {
        let error = build_error(with_args(offset, draw_count));
        assert!(error.contains("not whole 20-byte records"), "{error}");
    }
    assert!(with_args(40, 1).build().is_ok());

    let mut untyped = draws();
    untyped.bindings.buffers[1] =
        PlainBuffer::immutable(ImmutableSlot::<[u8; 20]>::from_backend(5, 3));
    let error = build_error(untyped);
    assert!(error.contains("PlainBuffer::indirect_args"), "{error}");
}

#[test]
fn plain_bindings_must_match_their_declarations() {
    let mut wrong_pipeline = draws();
    wrong_pipeline.bindings.pipelines[1] =
        PlainPipeline::graphics(GraphicsPipelineKey::<NoPush>::new(4));
    let error = build_error(wrong_pipeline);
    assert!(
        error.contains("declared GraphicsIndirect but bound to a Graphics"),
        "{error}"
    );

    let mut wrong_buffer = draws();
    wrong_buffer.bindings.buffers[0] =
        PlainBuffer::singleton(SingletonSlot::<u32>::from_backend(7, 12));
    let error = build_error(wrong_buffer);
    assert!(
        error.contains("declared Index but bound to a Singleton"),
        "{error}"
    );

    let mut missing_uniform = draws();
    missing_uniform.bindings.uniforms.pop();
    let error = build_error(missing_uniform);
    assert!(error.contains("2 uniforms declared but 1 bound"), "{error}");

    let mut unlaid = draws();
    unlaid.schemas.schemas[1].layout = None;
    let error = build_error(unlaid);
    assert!(error.contains("has no field layout"), "{error}");

    let mut stride = Fixture::new(0);
    stride.buffer(
        BufferKind::Singleton,
        Some(3),
        Some(16),
        PlainBuffer::singleton(SingletonSlot::<[f32; 3]>::from_backend(0, 3)),
    );
    let error = build_error(stride);
    assert!(error.contains("backend stride is 12"), "{error}");
}

#[test]
fn plain_frame_values_are_checked_before_staging() {
    let mut fake = Fake::default();
    let mut graph = draws().prepare(&mut fake);
    let bytes = 1u32.to_ne_bytes();
    let long = [0u8; 8];
    for (values, expected) in [
        (vec![PlainValue::Bytes(&bytes)], "2 values declared but 1"),
        (
            vec![PlainValue::Bytes(&bytes), PlainValue::Bytes(&long)],
            "has 8 bytes; its schema is 4",
        ),
        (
            vec![PlainValue::Bytes(&bytes), PlainValue::Count(1)],
            "received Count(1)",
        ),
        (
            vec![PlainValue::Bytes(&bytes), PlainValue::Absent],
            "required value uniform2.value is absent",
        ),
    ] {
        let error = run(&mut graph, &fake, &values).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    assert!(fake.events.borrow().is_empty(), "nothing submitted");
}

#[test]
fn plain_dropped_resources_are_rejected() {
    let bytes = 1u32.to_ne_bytes();
    let mut fake = Fake::default();
    let mut graph = draws().prepare(&mut fake);
    fake.index_buffers_live = false;
    let error = run(&mut graph, &fake, &draw_values(&bytes)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("buf0 (Index, slot 7) was dropped"),
        "{error}"
    );

    fake.index_buffers_live = true;
    fake.live = false;
    let error = run(&mut graph, &fake, &draw_values(&bytes)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("uniform buffer slot 1 was dropped"),
        "{error}"
    );
}

#[test]
fn plain_picking_reads_its_cursor_value() {
    let error = build_error(draws_with_picking(None));
    assert!(error.contains("picking"), "{error}");

    let mut fake = Fake::default();
    let mut graph = draws_with_picking(Some(PickingPipelineKey::new(9))).prepare(&mut fake);
    let bytes = 1u32.to_ne_bytes();
    let position = floats(&[12.5, 7.0]);
    let [first, second] = draw_values(&bytes);
    let picked = run(
        &mut graph,
        &fake,
        &[first, second, PlainValue::Bytes(&position)],
    )
    .unwrap();
    assert_eq!(picked.picking, Some((9, [12.5, 7.0])));

    let skipped = run(&mut graph, &fake, &[first, second, PlainValue::Absent]).unwrap();
    assert_eq!(skipped.picking, None);
}

fn draws_with_picking(pipeline: Option<PickingPipelineKey>) -> Fixture {
    let mut fixture = draws();
    let cursor = fixture.bytes_value("cursor", 8, true);
    fixture.desc.picking = Some(PickingDesc { cursor });
    fixture.bindings.picking = pipeline;

    fixture
}

/// a byte-created buffer steps `addr_at` by its runtime stride
#[test]
fn runtime_stride_slots_offset_by_their_element_size() {
    let singleton = SingletonSlot::<()>::from_backend_with_element_size(0, 4, 24);
    let immutable = ImmutableSlot::<()>::from_backend_with_element_size(0, 4, 24);

    assert_eq!(singleton.addr_at(3).erased().byte_offset, 72);
    assert_eq!(immutable.addr_at(3).erased().byte_offset, 72);
}

/// A second upload and the dispatch's data in the scope `gated_upload` gates
/// on its particles value.
fn multi_value_when() -> Fixture {
    let mut fixture = gated_upload();
    let gate = ValueId(0);
    let extra = fixture.buffer(
        BufferKind::Storage,
        Some(2),
        Some(4),
        PlainBuffer::cpu_updated(CpuUpdatedSlot::<f32>::from_backend(7, 2)),
    );
    let elem = fixture.schema("Weight", 4, vec![], vec![]);
    let weights = fixture.scoped_value(
        "weights",
        ValueKind::Array { elem, max_len: 2 },
        ValueScope::When { gate },
    );
    fixture.desc.uploads.push(UploadDesc {
        name: "weights".into(),
        buffer: extra,
        value: weights,
    });

    fixture
}

#[test]
fn plain_when_scope_values_are_present_together() {
    let mut fake = Fake::default();
    let mut graph = multi_value_when().prepare(&mut fake);
    let particles = floats(&[1.0, 2.0]);
    let weights = floats(&[0.5]);
    let particles_value = PlainValue::Array {
        count: 1,
        bytes: &particles,
    };
    let weights_value = PlainValue::Array {
        count: 1,
        bytes: &weights,
    };

    let skipped = run(&mut graph, &fake, &[PlainValue::Absent, PlainValue::Absent]).unwrap();
    assert!(skipped.writes.is_empty() && skipped.dispatches.is_empty());

    let ran = run(&mut graph, &fake, &[particles_value, weights_value]).unwrap();
    assert_eq!(
        ran.writes,
        vec![
            (false, 6, particles.clone()),
            (false, 7, weights.clone()),
            (true, 1, handle(1000 + 6 * 100)),
        ]
    );
    assert_eq!(ran.dispatches.len(), 1);

    for values in [
        [PlainValue::Absent, weights_value],
        [particles_value, PlainValue::Absent],
    ] {
        let error = run(&mut graph, &fake, &values).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("its optional scope's gate particles"),
            "{error}"
        );
    }
}

#[test]
fn plain_picking_follows_its_optional_scope() {
    let mut fixture = draws();
    let gate = fixture.value("gate", ValueKind::Count, true);
    let schema = fixture.schema("cursor.data", 8, vec![], vec![]);
    let cursor = fixture.scoped_value(
        "cursor",
        ValueKind::Bytes { schema },
        ValueScope::When { gate },
    );
    fixture.desc.picking = Some(PickingDesc { cursor });
    fixture.bindings.picking = Some(PickingPipelineKey::new(9));
    let mut fake = Fake::default();
    let mut graph = fixture.prepare(&mut fake);
    let bytes = 1u32.to_ne_bytes();
    let position = floats(&[3.0, 4.0]);
    let [first, second] = draw_values(&bytes);

    let picked = run(
        &mut graph,
        &fake,
        &[
            first,
            second,
            PlainValue::Count(1),
            PlainValue::Bytes(&position),
        ],
    )
    .unwrap();
    assert_eq!(picked.picking, Some((9, [3.0, 4.0])));

    let skipped = run(
        &mut graph,
        &fake,
        &[first, second, PlainValue::Absent, PlainValue::Absent],
    )
    .unwrap();
    assert_eq!(skipped.picking, None);

    let mixed = run(
        &mut graph,
        &fake,
        &[
            first,
            second,
            PlainValue::Absent,
            PlainValue::Bytes(&position),
        ],
    );
    assert!(mixed.is_err());
}

/// `repeat((upload(buf), dispatch(...)))`: a zero-trip frame skips the
/// upload as well as the dispatch, as the typed `RepeatNode` does
#[test]
fn plain_zero_trip_repeat_skips_its_upload() {
    let mut fixture = Fixture::new(0);
    let count = fixture.value("trips", ValueKind::Count, false);
    let storage = fixture.buffer(
        BufferKind::Storage,
        Some(4),
        Some(4),
        PlainBuffer::cpu_updated(CpuUpdatedSlot::<f32>::from_backend(6, 4)),
    );
    let elem = fixture.schema("Sample", 4, vec![], vec![]);
    let samples = fixture.scoped_value(
        "samples",
        ValueKind::Array { elem, max_len: 4 },
        ValueScope::Repeat { count },
    );
    fixture.desc.uploads.push(UploadDesc {
        name: "samples".into(),
        buffer: storage,
        value: samples,
    });
    let schema = fixture.schema("Empty", 4, vec![], vec![]);
    let uniform = fixture.uniform(schema, None, vec![], 1);
    let pipeline = fixture.pipeline(PipelineKind::Compute, schema, None, 2);
    let dispatch = fixture.dispatch("accumulate", pipeline, uniform);
    fixture.desc.passes.push(PassDesc::Repeat {
        name: "repeat".into(),
        count,
        body: vec![LeafPass::Compute(dispatch)],
    });
    let mut fake = Fake::default();
    let mut graph = fixture.prepare(&mut fake);
    let bytes = floats(&[1.0, 2.0]);
    let sample_value = PlainValue::Array {
        count: 2,
        bytes: &bytes,
    };

    let zero = run(&mut graph, &fake, &[PlainValue::Count(0), sample_value]).unwrap();
    assert!(zero.writes.is_empty() && zero.dispatches.is_empty());

    let two = run(&mut graph, &fake, &[PlainValue::Count(2), sample_value]).unwrap();
    let uploads: Vec<_> = two.writes.iter().filter(|(uniform, ..)| !uniform).collect();
    assert_eq!(uploads, vec![&(false, 6, bytes.clone())]);
    assert_eq!(two.dispatches.len(), 2);
}

/// a push block with a data half copies it from its own bytes value
#[test]
fn plain_push_data_assembles_beside_bindings() {
    let mut fixture = Fixture::new(1);
    let (pipeline, uniform, _) = fixture.data_block(PipelineKind::Compute, 4, 1, 3);
    let push_value = fixture.bytes_value("push.data", 4, false);
    let push = fixture.schema(
        "BlurPush",
        16,
        vec![ResourceFieldKind::SampledTex],
        vec![
            data(0, 0, 4, 0),
            resource(0, 8, SchemaFieldKind::SampledTex),
        ],
    );
    fixture.desc.pipelines[pipeline.0 as usize].push = Some(push);
    let mut dispatch = fixture.dispatch("blur", pipeline, uniform);
    dispatch.push = Some(PushDesc {
        data: Some(push_value),
        schema: push,
        bindings: keyed(vec![ResourceRef::Tex(TexId(0), TexAccess::Read)]),
    });
    fixture
        .desc
        .passes
        .push(PassDesc::Leaf(LeafPass::Compute(dispatch)));
    let mut fake = Fake::default();
    let mut graph = fixture.prepare(&mut fake);
    let uniform_bytes = 9u32.to_ne_bytes();
    let radius = floats(&[2.5]);

    let batch = run(
        &mut graph,
        &fake,
        &[
            PlainValue::Bytes(&uniform_bytes),
            PlainValue::Bytes(&radius),
        ],
    )
    .unwrap();
    let expected = [radius, vec![0; 4], handle(20)].concat();
    assert_eq!(batch.dispatches, vec![(3, [4, 2, 1], Some(expected))]);
}

/// one gpu-updated buffer bound through a uniform no command consumes
fn orphan_gpu_binding(binding: PlainBuffer<Cmd>, offset: u32) -> Fixture {
    let mut fixture = Fixture::new(0);
    let buffer = fixture.buffer(BufferKind::GpuUpdatedFlight, None, None, binding);
    let schema = fixture.schema(
        "Orphan",
        8,
        vec![ResourceFieldKind::BufAddr],
        vec![resource(0, 0, SchemaFieldKind::BufAddr)],
    );
    fixture.uniform(
        schema,
        None,
        vec![ResourceRef::Buf(current(buffer, offset), BufAccess::Mutate)],
        1,
    );

    fixture
}

#[test]
fn plain_cpu_and_gpu_updated_slots_carry_their_allocation() {
    let gpu = |len, element_size| {
        PlainBuffer::gpu_updated(GpuUpdatedSlot::<()>::from_backend_with_element_size(
            2,
            len,
            element_size,
        ))
    };
    assert!(orphan_gpu_binding(gpu(2, 16), 31).build().is_ok());

    // checked once, though no command consumes the uniform
    let error = build_error(orphan_gpu_binding(gpu(2, 16), 32));
    assert_eq!(error.matches("past the end").count(), 1, "{error}");
    assert!(error.contains("offset 32 is past the end of buffer buf0 (32 bytes)"));

    let error = build_error(orphan_gpu_binding(gpu(2, usize::MAX), 0));
    assert!(error.contains("byte length overflows u64"), "{error}");

    let mut forged_capacity = orphan_gpu_binding(gpu(2, 16), 0);
    forged_capacity.desc.buffers[0].capacity = Some(4);
    let error = build_error(forged_capacity);
    assert!(error.contains("declares capacity 4 but its backend buffer holds 2"));

    let mut forged_stride = gated_upload();
    forged_stride.desc.buffers[0].elem_size = Some(16);
    forged_stride.schemas.schemas[0].size = 16;
    let error = build_error(forged_stride);
    assert!(
        error.contains("declares 16-byte elements but its backend stride is 8"),
        "{error}"
    );

    let mut byte_stride = gated_upload();
    byte_stride.bindings.buffers[0] = PlainBuffer::cpu_updated(
        CpuUpdatedSlot::<()>::from_backend_with_element_size(6, 4, 8),
    );
    assert!(byte_stride.build().is_ok());
}

/// two uniform declarations staging into one backend buffer would overwrite
/// each other before submission
#[test]
fn plain_uniform_declarations_need_distinct_backend_buffers() {
    let mut fixture = draws();
    fixture.bindings.uniforms[1] = PlainUniform::new(UniformSlot::<()>::from_backend(1));
    let error = build_error(fixture);

    assert!(
        error.contains("uniform uniform1 is bound to backend uniform buffer 1"),
        "{error}"
    );
}

/// two dispatches share a uniform block sampling a texture the first one
/// writes through its push block; the second consumer would restage the
/// block at the next version before either executes
#[test]
fn plain_shared_uniform_rejects_a_texture_written_between_consumers() {
    let mut fixture = Fixture::new(1);
    let schema = fixture.schema(
        "SharedParams",
        8,
        vec![ResourceFieldKind::SampledTex],
        vec![resource(0, 0, SchemaFieldKind::SampledTex)],
    );
    let shared = fixture.uniform(
        schema,
        None,
        vec![ResourceRef::Tex(TexId(0), TexAccess::Read)],
        1,
    );
    let push = fixture.schema(
        "WritePush",
        8,
        vec![ResourceFieldKind::StorageTex],
        vec![resource(0, 0, SchemaFieldKind::StorageTex)],
    );
    let writer = fixture.pipeline(PipelineKind::Compute, schema, Some(push), 2);
    let reader = fixture.pipeline(PipelineKind::Compute, schema, None, 3);
    let mut first = fixture.dispatch("write", writer, shared);
    first.push = Some(PushDesc {
        data: None,
        schema: push,
        bindings: keyed(vec![ResourceRef::Tex(TexId(0), TexAccess::Write)]),
    });
    let second = fixture.dispatch("read", reader, shared);
    for dispatch in [first, second] {
        fixture
            .desc
            .passes
            .push(PassDesc::Leaf(LeafPass::Compute(dispatch)));
    }

    let error = build_error(fixture);
    assert!(
        error.contains("uniform0's uniform block is shared by several commands"),
        "{error}"
    );
}

/// consumers of one uniform block resolve it to the same bytes, so it is
/// staged once per frame
#[test]
fn plain_shared_uniform_stages_once() {
    let mut fake = Fake::default();
    let fixture = draws();
    let PassDesc::Leaf(LeafPass::Raster(raster)) = &fixture.desc.passes[0] else {
        unreachable!()
    };
    assert_eq!(raster.draws[0].uniform, raster.draws[2].uniform);
    let mut graph = fixture.prepare(&mut fake);
    let bytes = 1u32.to_ne_bytes();

    let batch = run(&mut graph, &fake, &draw_values(&bytes)).unwrap();
    assert_eq!(
        batch.writes,
        vec![(true, 1, bytes.to_vec()), (true, 2, bytes.to_vec())]
    );
    assert_eq!(batch.draws.len(), 4);
}
