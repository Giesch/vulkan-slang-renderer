//! Lowering from sealed typed nodes into the plain-data description.
use super::desc::*;
use super::validate::GraphError;
use super::{BufferBindingKind, GraphBinding, SampledRef, StorageRef, StorageTexAccess};
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct NodeAccess {
    pub(crate) reads: Vec<u32>,
    pub(crate) prev_reads: Vec<u32>,
    pub(crate) writes: Vec<u32>,
    pub(crate) mutates: Vec<u32>,
}

pub(crate) struct LowerOutput {
    pub(crate) desc: GraphDesc,
    pub(crate) schemas: SchemaTable,
    pub(crate) errors: Vec<GraphError>,
    pub(crate) picking: Option<usize>,
    pub(crate) uniform_slots: Vec<usize>,
    pub(crate) buffer_indices: Vec<usize>,
    pub(crate) pipeline_indices: Vec<usize>,
    pub(crate) import_handles: Vec<u64>,
}

pub(crate) struct UniformInput {
    pub(crate) slot: usize,
    pub(crate) gpu_size: u32,
    pub(crate) data_size: u32,
    pub(crate) bindings: Vec<GraphBinding>,
}

pub struct PushInput {
    pub(crate) size: u32,
    pub(crate) bindings: Vec<GraphBinding>,
}

pub(crate) enum LowerDrawCall {
    VertexCount(u32),
    WholeIndexed,
    IndexRange {
        first_index: u32,
        index_count: u32,
    },
    IndexedIndirect {
        args_index: usize,
        byte_offset: u64,
        draw_count: u32,
    },
}

enum Scope {
    Top,
    Repeat {
        body: Vec<LeafPass>,
        count: ValueId,
    },
    Optional {
        body: Vec<LeafPass>,
        first: Option<ValueId>,
    },
}

pub struct LowerCtx {
    desc: GraphDesc,
    schemas: SchemaTable,
    errors: Vec<GraphError>,
    scope: Scope,
    ignored: usize,
    picking: Option<usize>,
    draws: usize,
    uniform_slots: Vec<usize>,
    buffer_indices: Vec<usize>,
    pipeline_indices: Vec<usize>,
    import_handles: Vec<u64>,
    buffers: HashMap<(BufferKind, usize), BufferId>,
    imports: HashMap<u64, ImportId>,
    pipelines: HashMap<(PipelineKind, usize), PipelineId>,
    uniforms: HashMap<usize, UniformId>,
}

impl LowerCtx {
    pub(crate) fn new(textures: Vec<TexDecl>) -> Self {
        Self {
            desc: GraphDesc {
                textures,
                ..Default::default()
            },
            schemas: Default::default(),
            errors: vec![],
            scope: Scope::Top,
            ignored: 0,
            picking: None,
            draws: 0,
            uniform_slots: vec![],
            buffer_indices: vec![],
            pipeline_indices: vec![],
            import_handles: vec![],
            buffers: HashMap::new(),
            imports: HashMap::new(),
            pipelines: HashMap::new(),
            uniforms: HashMap::new(),
        }
    }

    fn value(&mut self, name: String, kind: ValueKind) -> ValueId {
        let id = ValueId(self.desc.values.len() as u32);
        let optional = matches!(self.scope, Scope::Optional { .. });
        self.desc.values.push(ValueDecl {
            name,
            kind,
            optional,
        });
        if let Scope::Optional { first, .. } = &mut self.scope {
            first.get_or_insert(id);
        }

        id
    }

    fn schema(&mut self, name: String, size: u32, fields: Vec<ResourceFieldKind>) -> SchemaId {
        self.schemas.push(SchemaDesc {
            name,
            size,
            resource_fields: fields,
            layout: None,
        })
    }

    fn intern_buffer(
        &mut self,
        kind: BufferKind,
        index: usize,
        capacity: Option<u32>,
        elem_size: Option<u32>,
    ) -> BufferId {
        let key = (kind, index);
        if let Some(id) = self.buffers.get(&key).copied() {
            let existing = &mut self.desc.buffers[id.0 as usize];
            existing.capacity = existing.capacity.or(capacity);
            existing.elem_size = existing.elem_size.or(elem_size);

            return id;
        }

        let id = BufferId(self.desc.buffers.len() as u32);
        self.desc.buffers.push(BufferDecl {
            name: format!("buf.{kind:?}.{index}"),
            kind,
            capacity,
            elem_size,
        });
        self.buffer_indices.push(index);
        self.buffers.insert(key, id);

        id
    }

    fn intern_import(&mut self, raw: u64) -> ImportId {
        if let Some(existing) = self.imports.get(&raw) {
            return *existing;
        }
        let id = ImportId(self.desc.imports.len() as u32);
        self.desc.imports.push(ImportDecl {
            name: format!("import{}", id.0),
        });
        self.import_handles.push(raw);
        self.imports.insert(raw, id);

        id
    }

    fn binding(&mut self, binding: GraphBinding) -> Option<ResourceRef> {
        Some(match binding {
            GraphBinding::SampledTex(sampled) => match sampled.inner {
                SampledRef::Graph(tex) => ResourceRef::Tex(TexId(tex.0), TexAccess::Read),
                SampledRef::GraphPrevious(tex) => {
                    ResourceRef::Tex(TexId(tex.0), TexAccess::ReadPrevious)
                }
                SampledRef::External(handle) => {
                    ResourceRef::External(self.intern_import(handle.to_raw()))
                }
            },
            GraphBinding::StorageTex(storage) => match storage.inner {
                StorageRef::Graph(tex, StorageTexAccess::Write) => {
                    ResourceRef::Tex(TexId(tex.0), TexAccess::Write)
                }
                StorageRef::Graph(tex, StorageTexAccess::Mutate) => {
                    ResourceRef::Tex(TexId(tex.0), TexAccess::Mutate)
                }
                StorageRef::External(_) => {
                    self.errors.push(GraphError::MutableExternalImport);
                    return None;
                }
            },
            GraphBinding::Buffer(raw) => {
                let (kind, slot, access) = match raw.kind {
                    BufferBindingKind::Storage => {
                        (BufferKind::Storage, SlotSel::Current, BufAccess::Mutate)
                    }
                    BufferBindingKind::GpuOnlyCurrent => (
                        BufferKind::GpuOnlyFlight,
                        SlotSel::Current,
                        BufAccess::Mutate,
                    ),
                    BufferBindingKind::GpuOnlyPrevious => (
                        BufferKind::GpuOnlyFlight,
                        SlotSel::Previous,
                        BufAccess::Read,
                    ),
                    BufferBindingKind::Immutable => {
                        (BufferKind::Immutable, SlotSel::Current, BufAccess::Read)
                    }
                    BufferBindingKind::Singleton => {
                        (BufferKind::Singleton, SlotSel::Current, BufAccess::Read)
                    }
                };
                let id = self.intern_buffer(kind, raw.index, None, None);
                let offset = match u32::try_from(raw.byte_offset) {
                    Ok(offset) => offset,
                    Err(_) => {
                        self.errors.push(GraphError::BufferOffsetOverflow {
                            offset: raw.byte_offset,
                        });
                        u32::MAX
                    }
                };
                ResourceRef::Buf(
                    BufferRef {
                        buffer: id,
                        slot,
                        offset,
                        range: None,
                    },
                    access,
                )
            }
        })
    }

    fn bindings(&mut self, input: Vec<GraphBinding>) -> Vec<(FieldKey, ResourceRef)> {
        input
            .into_iter()
            .enumerate()
            .filter_map(|(i, binding)| {
                self.binding(binding)
                    .map(|resource| (FieldKey(i as u16), resource))
            })
            .collect()
    }

    fn fields(input: &[GraphBinding]) -> Vec<ResourceFieldKind> {
        input
            .iter()
            .map(|binding| match binding {
                GraphBinding::SampledTex(_) => ResourceFieldKind::SampledTex,
                GraphBinding::StorageTex(_) => ResourceFieldKind::StorageTex,
                GraphBinding::Buffer(_) => ResourceFieldKind::BufAddr,
            })
            .collect()
    }

    fn uniform(&mut self, input: UniformInput) -> UniformId {
        let data_schema = self.schema(
            format!("uniform{}.data", input.slot),
            input.data_size,
            vec![],
        );
        let data = self.value(
            format!("uniform{}.value", input.slot),
            ValueKind::Bytes {
                schema: data_schema,
            },
        );
        let fields = Self::fields(&input.bindings);
        let bindings = self.bindings(input.bindings);
        if let Some(id) = self.uniforms.get(&input.slot).copied() {
            let old = &self.desc.uniforms[id.0 as usize].source;
            let same_source = old.data == Some(data) && old.bindings == bindings;
            if same_source {
                return id;
            }
            self.errors
                .push(GraphError::UniformSourceConflict { slot: input.slot });
            return id;
        }
        let schema = self.schema(format!("uniform{}", input.slot), input.gpu_size, fields);
        let id = UniformId(self.desc.uniforms.len() as u32);
        self.desc.uniforms.push(UniformDecl {
            name: format!("uniform{}", input.slot),
            schema,
            source: UniformSourceDesc {
                data: Some(data),
                bindings,
            },
        });
        self.uniform_slots.push(input.slot);
        self.uniforms.insert(input.slot, id);

        id
    }

    fn pipeline(
        &mut self,
        kind: PipelineKind,
        index: usize,
        params: SchemaId,
        push: Option<SchemaId>,
    ) -> PipelineId {
        if let Some(existing) = self.pipelines.get(&(kind, index)) {
            return *existing;
        }
        let id = PipelineId(self.desc.pipelines.len() as u32);
        self.desc.pipelines.push(PipelineDecl {
            name: format!("pipeline.{kind:?}.{index}"),
            kind,
            params,
            push,
        });
        self.pipeline_indices.push(index);
        self.pipelines.insert((kind, index), id);

        id
    }

    fn push(&mut self, input: Option<PushInput>) -> Option<PushDesc> {
        input.map(|push| {
            let fields = Self::fields(&push.bindings);
            let bindings = self.bindings(push.bindings);
            let schema = self.schema(
                format!("push{}", self.schemas.schemas.len()),
                push.size,
                fields,
            );
            PushDesc {
                data: None,
                schema,
                bindings,
            }
        })
    }

    fn leaf(&mut self, pass: LeafPass) {
        match &mut self.scope {
            Scope::Top => match pass {
                LeafPass::Raster(raster) => {
                    if let Some(PassDesc::Leaf(LeafPass::Raster(last))) =
                        self.desc.passes.last_mut()
                    {
                        last.draws.extend(raster.draws)
                    } else {
                        self.desc
                            .passes
                            .push(PassDesc::Leaf(LeafPass::Raster(raster)))
                    }
                }
                other => self.desc.passes.push(PassDesc::Leaf(other)),
            },
            Scope::Repeat { body, .. } | Scope::Optional { body, .. } => body.push(pass),
        }
    }

    pub(crate) fn dispatch(
        &mut self,
        index: usize,
        groups: [u32; 3],
        input: UniformInput,
        push_input: Option<PushInput>,
    ) {
        let uniform = self.uniform(input);
        let params = self.desc.uniforms[uniform.0 as usize].schema;
        let push = self.push(push_input);
        let pipeline = self.pipeline(
            PipelineKind::Compute,
            index,
            params,
            push.as_ref().map(|push| push.schema),
        );
        let name = format!("dispatch{}", self.desc.passes.len());

        self.leaf(LeafPass::Compute(DispatchDesc {
            name,
            pipeline,
            uniform,
            groups: GroupSource::Fixed(groups),
            push,
        }))
    }

    pub(crate) fn draw(
        &mut self,
        index: usize,
        call: LowerDrawCall,
        input: UniformInput,
        push_input: Option<PushInput>,
    ) {
        let uniform = self.uniform(input);
        let params = self.desc.uniforms[uniform.0 as usize].schema;
        let push = self.push(push_input);
        let pipeline = self.pipeline(
            PipelineKind::Graphics,
            index,
            params,
            push.as_ref().map(|push| push.schema),
        );
        let call = match call {
            LowerDrawCall::VertexCount(x) => DrawCall::VertexCount(x),
            LowerDrawCall::WholeIndexed => DrawCall::WholeIndexed,
            LowerDrawCall::IndexRange {
                first_index,
                index_count,
            } => DrawCall::IndexRange {
                first_index,
                index_count,
            },
            LowerDrawCall::IndexedIndirect {
                args_index,
                byte_offset,
                draw_count,
            } => {
                let args_buffer = self.intern_buffer(BufferKind::Immutable, args_index, None, None);
                DrawCall::IndexedIndirect {
                    args: BufferRef {
                        buffer: args_buffer,
                        slot: SlotSel::Current,
                        offset: u32::try_from(byte_offset).unwrap_or_else(|_| {
                            self.errors.push(GraphError::BufferOffsetOverflow {
                                offset: byte_offset,
                            });
                            u32::MAX
                        }),
                        range: None,
                    },
                    draw_count,
                }
            }
        };
        let draw = DrawDesc {
            name: format!("draw{}", self.draws),
            pipeline,
            uniform,
            call,
            push,
        };
        self.draws += 1;

        self.leaf(LeafPass::Raster(RasterDesc {
            name: "main".into(),
            targets: RasterTargets::Main,
            draws: vec![draw],
        }))
    }

    pub(crate) fn upload(&mut self, index: usize, elem_size: u32, capacity: u32) {
        let buffer =
            self.intern_buffer(BufferKind::Storage, index, Some(capacity), Some(elem_size));
        let elem_schema = self.schema(
            format!("upload{}.elem", self.desc.uploads.len()),
            elem_size,
            vec![],
        );
        let value = self.value(
            format!("upload{}.value", self.desc.uploads.len()),
            ValueKind::Array {
                elem: elem_schema,
                max_len: capacity,
            },
        );

        self.desc.uploads.push(UploadDesc {
            name: format!("upload{}", self.desc.uploads.len()),
            buffer,
            value,
        })
    }

    fn begin(&mut self, which: &'static str) {
        if !matches!(self.scope, Scope::Top) {
            let outer = match self.scope {
                Scope::Repeat { .. } => "repeat",
                Scope::Optional { .. } => "optional",
                Scope::Top => unreachable!(),
            };
            self.errors.push(GraphError::NestedControlFlow {
                outer,
                inner: which,
            });
            self.ignored += 1;
            return;
        }
        self.scope = if which == "repeat" {
            let count = self.value(
                format!("repeat{}.count", self.desc.passes.len()),
                ValueKind::Count,
            );
            Scope::Repeat {
                body: vec![],
                count,
            }
        } else {
            Scope::Optional {
                body: vec![],
                first: None,
            }
        }
    }

    pub(crate) fn begin_repeat(&mut self) {
        self.begin("repeat")
    }

    pub(crate) fn begin_optional(&mut self) {
        self.begin("optional")
    }

    fn end(&mut self, which: &'static str) {
        if self.ignored > 0 {
            self.ignored -= 1;
            return;
        }
        let old = std::mem::replace(&mut self.scope, Scope::Top);

        match old {
            Scope::Repeat { body, count } if which == "repeat" => {
                self.desc.passes.push(PassDesc::Repeat {
                    name: format!("repeat{}", self.desc.passes.len()),
                    count,
                    body,
                })
            }
            Scope::Optional { body, first } if which == "optional" => {
                if let Some(value) = first {
                    if !body.is_empty() {
                        self.desc.passes.push(PassDesc::When {
                            name: format!("when{}", self.desc.passes.len()),
                            value,
                            body,
                        })
                    }
                } else {
                    self.errors.push(GraphError::EmptyOptionalScope)
                }
            }
            other => self.scope = other,
        }
    }

    pub(crate) fn end_repeat(&mut self) {
        self.end("repeat")
    }

    pub(crate) fn end_optional(&mut self) {
        self.end("optional")
    }

    pub(crate) fn picking(&mut self, index: usize) {
        if self.picking.replace(index).is_some() {
            self.errors.push(GraphError::MultiplePickingNodes)
        }
    }

    pub(crate) fn finish(mut self) -> LowerOutput {
        if self.picking.is_some() && self.draws == 0 {
            self.errors.push(GraphError::PickingWithoutDraw)
        }
        LowerOutput {
            desc: self.desc,
            schemas: self.schemas,
            errors: self.errors,
            picking: self.picking,
            uniform_slots: self.uniform_slots,
            buffer_indices: self.buffer_indices,
            pipeline_indices: self.pipeline_indices,
            import_handles: self.import_handles,
        }
    }
}
