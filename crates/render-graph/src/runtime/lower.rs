//! Lowering from sealed typed nodes into the plain-data description.
use super::desc::*;
use super::validate::GraphError;
use super::{BufferBindingKind, GraphBinding, SampledRef, StorageRef, StorageTexAccess};
use std::collections::HashMap;

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

#[derive(Clone, Copy)]
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
        request: crate::commands::IndirectRequest,
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
    dispatches: usize,
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
            dispatches: 0,
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
                SampledRef::External(id) => ResourceRef::External(self.intern_import(id.raw())),
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

    /// A binding that fails to lower drops from the schema fields and the
    /// bindings together, so the two lists stay aligned and the lowering
    /// error does not cascade into shape errors.
    fn resolve_bindings(
        &mut self,
        input: Vec<GraphBinding>,
    ) -> (Vec<ResourceFieldKind>, Vec<(FieldKey, ResourceRef)>) {
        let mut fields = vec![];
        let mut bindings = vec![];
        for binding in input {
            let field = match &binding {
                GraphBinding::SampledTex(_) => ResourceFieldKind::SampledTex,
                GraphBinding::StorageTex(_) => ResourceFieldKind::StorageTex,
                GraphBinding::Buffer(_) => ResourceFieldKind::BufAddr,
            };
            if let Some(resource) = self.binding(binding) {
                fields.push(field);
                bindings.push((FieldKey(bindings.len() as u16), resource));
            }
        }

        (fields, bindings)
    }

    /// The data value of an existing uniform decl, identified by its byte size:
    /// two sightings of one slot agree when they write the same bytes through
    /// the same bindings.
    fn uniform_data_size(&self, id: UniformId) -> Option<u32> {
        let data = self.desc.uniforms[id.0 as usize].source.data?;
        let ValueKind::Bytes { schema } = self.desc.values[data.0 as usize].kind else {
            return None;
        };

        Some(self.schemas.get(schema)?.size)
    }

    fn uniform(&mut self, input: UniformInput) -> UniformId {
        let data_size = input.data_size;
        let (fields, bindings) = self.resolve_bindings(input.bindings);
        if let Some(id) = self.uniforms.get(&input.slot).copied() {
            let same_source = self.uniform_data_size(id) == Some(data_size)
                && self.desc.uniforms[id.0 as usize].source.bindings == bindings;
            if !same_source {
                self.errors
                    .push(GraphError::UniformSourceConflict { slot: input.slot });
            }

            return id;
        }

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
            let (fields, bindings) = self.resolve_bindings(push.bindings);
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
        let name = format!("dispatch{}", self.dispatches);
        self.dispatches += 1;

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
                ..
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
        let schema = self.schema("picking.cursor".into(), 8, vec![]);
        self.value("picking.cursor".into(), ValueKind::Bytes { schema });
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

#[cfg(test)]
mod tests {
    use crate::bindless::{BindlessHandle, Sampler2D};

    use super::super::desc::{
        BufferKind, DrawCall, FieldKey, GraphFormat, LeafPass, PassDesc, PipelineKind,
        ResourceFieldKind, ResourceRef, SizeClass, SlotSel, TexAccess, TexDecl, TexId, TexUsage,
        UniformId, ValueKind,
    };
    use super::super::validate::{GraphError, validate};
    use super::super::{BufferBindingKind, GraphBinding, GraphTex, RawBufferBinding};
    use super::{LowerCtx, LowerDrawCall, LowerOutput, PushInput, UniformInput};

    const GROUPS: [u32; 3] = [1, 1, 1];

    fn textures(count: u32) -> Vec<TexDecl> {
        (0..count)
            .map(|i| TexDecl {
                name: format!("tex{i}"),
                format: GraphFormat::R32Float,
                size: SizeClass::Fixed(8, 8),
                usage: TexUsage::Storage,
            })
            .collect()
    }

    fn sampled(tex: u32) -> GraphBinding {
        GraphBinding::SampledTex(GraphTex(tex).read())
    }

    fn prev_sampled(tex: u32) -> GraphBinding {
        GraphBinding::SampledTex(GraphTex(tex).read_previous())
    }

    fn storage_write(tex: u32) -> GraphBinding {
        GraphBinding::StorageTex(GraphTex(tex).write())
    }

    fn storage_mutate(tex: u32) -> GraphBinding {
        GraphBinding::StorageTex(GraphTex(tex).mutate())
    }

    fn buf(kind: BufferBindingKind, index: usize) -> GraphBinding {
        GraphBinding::Buffer(RawBufferBinding {
            kind,
            index,
            byte_offset: 0,
        })
    }

    fn external_sampled(slot: u32) -> GraphBinding {
        GraphBinding::SampledTex(BindlessHandle::<Sampler2D>::from_raw(u64::from(slot)).into())
    }

    fn external_storage(slot: u32) -> GraphBinding {
        GraphBinding::StorageTex(super::super::StorageTexBinding {
            inner: super::StorageRef::External(super::super::ExternalTexId::from_raw(u64::from(
                slot,
            ))),
        })
    }

    fn uni(slot: usize, bindings: Vec<GraphBinding>) -> UniformInput {
        UniformInput {
            slot,
            gpu_size: 64,
            data_size: 16,
            bindings,
        }
    }

    fn push(bindings: Vec<GraphBinding>) -> Option<PushInput> {
        Some(PushInput { size: 16, bindings })
    }

    /// the texture ids a uniform decl binds, paired with their access
    fn uniform_refs(out: &LowerOutput, id: UniformId) -> Vec<ResourceRef> {
        out.desc.uniforms[id.0 as usize]
            .source
            .bindings
            .iter()
            .map(|(_, resource)| resource.clone())
            .collect()
    }

    #[test]
    fn nested_repeat_is_an_error() {
        let mut cx = LowerCtx::new(textures(1));
        cx.begin_repeat();
        cx.begin_repeat();
        cx.dispatch(0, GROUPS, uni(0, vec![sampled(0)]), None);
        cx.end_repeat();
        cx.end_repeat();
        let out = cx.finish();

        assert!(out.errors.contains(&GraphError::NestedControlFlow {
            outer: "repeat",
            inner: "repeat",
        }));
    }

    #[test]
    fn repeat_after_exit_is_allowed() {
        let mut cx = LowerCtx::new(textures(1));
        cx.begin_repeat();
        cx.dispatch(0, GROUPS, uni(0, vec![sampled(0)]), None);
        cx.end_repeat();
        cx.begin_repeat();
        cx.dispatch(1, GROUPS, uni(1, vec![sampled(0)]), None);
        cx.end_repeat();
        let out = cx.finish();

        assert!(out.errors.is_empty());
        assert_eq!(out.desc.passes.len(), 2);
        assert!(
            out.desc
                .passes
                .iter()
                .all(|pass| matches!(pass, PassDesc::Repeat { .. }))
        );
    }

    #[test]
    fn a_second_picking_node_is_an_error() {
        let mut cx = LowerCtx::new(vec![]);
        cx.draw(0, LowerDrawCall::VertexCount(3), uni(0, vec![]), None);
        cx.picking(0);
        cx.picking(1);
        let out = cx.finish();

        assert!(out.errors.contains(&GraphError::MultiplePickingNodes));
    }

    #[test]
    fn picking_without_draws_is_an_error() {
        let mut cx = LowerCtx::new(vec![]);
        cx.picking(0);
        let out = cx.finish();

        assert!(out.errors.contains(&GraphError::PickingWithoutDraw));
    }

    #[test]
    fn picking_with_a_draw_is_allowed() {
        let mut cx = LowerCtx::new(vec![]);
        cx.draw(0, LowerDrawCall::VertexCount(3), uni(0, vec![]), None);
        cx.picking(7);
        let out = cx.finish();

        assert!(out.errors.is_empty());
        assert_eq!(out.picking, Some(7));
    }

    /// the slot names one uniform buffer, so two sources that disagree about
    /// its bytes cannot both write it
    #[test]
    fn two_nodes_sharing_a_uniform_slot_reject() {
        let mut cx = LowerCtx::new(textures(1));
        cx.dispatch(0, GROUPS, uni(0, vec![sampled(0)]), None);
        let mut second = uni(0, vec![sampled(0)]);
        second.data_size = 32;
        cx.dispatch(1, GROUPS, second, None);
        let out = cx.finish();

        assert!(
            out.errors
                .contains(&GraphError::UniformSourceConflict { slot: 0 })
        );
        assert_eq!(out.desc.uniforms.len(), 1);
    }

    /// two sightings that agree merge, leaving no orphan value or schema rows
    #[test]
    fn identical_uniform_source_merges() {
        let mut cx = LowerCtx::new(textures(1));
        cx.dispatch(0, GROUPS, uni(4, vec![sampled(0)]), None);
        let before = (cx.desc.values.len(), cx.schemas.schemas.len());
        cx.dispatch(1, GROUPS, uni(4, vec![sampled(0)]), None);
        let after = (cx.desc.values.len(), cx.schemas.schemas.len());
        let out = cx.finish();

        assert!(out.errors.is_empty());
        assert_eq!(out.desc.uniforms.len(), 1);
        assert_eq!(out.uniform_slots, vec![4]);
        assert_eq!(before, after);
        let ids: Vec<_> = out
            .desc
            .passes
            .iter()
            .map(|pass| match pass {
                PassDesc::Leaf(LeafPass::Compute(compute)) => compute.uniform,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(ids, vec![UniformId(0), UniformId(0)]);
    }

    #[test]
    fn same_slot_differing_bindings_reject() {
        let mut cx = LowerCtx::new(textures(2));
        cx.dispatch(0, GROUPS, uni(0, vec![sampled(0)]), None);
        cx.dispatch(1, GROUPS, uni(0, vec![sampled(1)]), None);
        let out = cx.finish();

        assert!(
            out.errors
                .contains(&GraphError::UniformSourceConflict { slot: 0 })
        );
        assert_eq!(out.desc.uniforms.len(), 1);
    }

    #[test]
    fn optional_inside_repeat_rejects() {
        let mut cx = LowerCtx::new(vec![]);
        cx.begin_repeat();
        cx.begin_optional();
        cx.dispatch(0, GROUPS, uni(0, vec![]), None);
        cx.end_optional();
        cx.end_repeat();
        let out = cx.finish();

        assert!(out.errors.contains(&GraphError::NestedControlFlow {
            outer: "repeat",
            inner: "optional",
        }));
    }

    #[test]
    fn repeat_inside_optional_rejects() {
        let mut cx = LowerCtx::new(vec![]);
        cx.begin_optional();
        cx.begin_repeat();
        cx.dispatch(0, GROUPS, uni(0, vec![]), None);
        cx.end_repeat();
        cx.end_optional();
        let out = cx.finish();

        assert!(out.errors.contains(&GraphError::NestedControlFlow {
            outer: "optional",
            inner: "repeat",
        }));
    }

    #[test]
    fn nested_optional_rejects() {
        let mut cx = LowerCtx::new(vec![]);
        cx.begin_optional();
        cx.begin_optional();
        cx.dispatch(0, GROUPS, uni(0, vec![]), None);
        cx.end_optional();
        cx.end_optional();
        let out = cx.finish();

        assert!(out.errors.contains(&GraphError::NestedControlFlow {
            outer: "optional",
            inner: "optional",
        }));
    }

    #[test]
    fn optional_picking_has_an_optional_cursor_value() {
        let mut cx = LowerCtx::new(vec![]);
        cx.draw(0, LowerDrawCall::VertexCount(3), uni(0, vec![]), None);
        cx.begin_optional();
        cx.picking(1);
        cx.end_optional();
        let out = cx.finish();

        assert!(out.errors.is_empty(), "{:?}", out.errors);
        assert!(validate(&out.desc, &out.schemas).is_ok());
        let cursor = out
            .desc
            .values
            .iter()
            .find(|value| value.name == "picking.cursor")
            .unwrap();
        assert!(cursor.optional);
        assert!(matches!(cursor.kind, ValueKind::Bytes { .. }));
    }

    #[test]
    fn empty_optional_scope_is_an_error() {
        let mut cx = LowerCtx::new(vec![]);
        cx.begin_optional();
        cx.end_optional();
        let out = cx.finish();

        assert!(out.errors.contains(&GraphError::EmptyOptionalScope));
    }

    #[test]
    fn optional_gate_is_first_scope_value_and_all_scope_values_optional() {
        let mut cx = LowerCtx::new(vec![]);
        cx.begin_optional();
        cx.upload(0, 8, 16);
        cx.dispatch(0, GROUPS, uni(0, vec![]), None);
        cx.end_optional();
        let out = cx.finish();

        assert!(out.errors.is_empty());
        let PassDesc::When { value, .. } = &out.desc.passes[0] else {
            panic!("expected a when pass")
        };
        assert!(matches!(
            out.desc.values[value.0 as usize].kind,
            ValueKind::Array { .. }
        ));
        assert!(out.desc.values.iter().all(|value| value.optional));
    }

    /// the two slots of one gpu-only buffer are one resource with two
    /// selectors, not two buffers
    #[test]
    fn gpu_only_current_and_previous_intern_to_one_buffer() {
        let mut cx = LowerCtx::new(vec![]);
        cx.dispatch(
            0,
            GROUPS,
            uni(
                0,
                vec![
                    buf(BufferBindingKind::GpuOnlyPrevious, 3),
                    buf(BufferBindingKind::GpuOnlyCurrent, 3),
                ],
            ),
            None,
        );
        let out = cx.finish();

        assert_eq!(out.desc.buffers.len(), 1);
        assert_eq!(out.buffer_indices, vec![3]);
        assert_eq!(out.desc.buffers[0].kind, BufferKind::GpuOnlyFlight);
        let slots: Vec<_> = uniform_refs(&out, UniformId(0))
            .into_iter()
            .map(|resource| match resource {
                ResourceRef::Buf(buffer, _) => buffer.slot,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(slots, vec![SlotSel::Previous, SlotSel::Current]);
    }

    #[test]
    fn external_sampled_handle_interns_once() {
        let mut cx = LowerCtx::new(vec![]);
        cx.dispatch(0, GROUPS, uni(0, vec![external_sampled(9)]), None);
        cx.dispatch(1, GROUPS, uni(1, vec![external_sampled(9)]), None);
        let out = cx.finish();

        assert!(out.errors.is_empty());
        assert_eq!(out.desc.imports.len(), 1);
        assert_eq!(out.import_handles, vec![9]);
    }

    #[test]
    fn mutable_external_storage_binding_rejects() {
        let mut cx = LowerCtx::new(vec![]);
        cx.dispatch(0, GROUPS, uni(0, vec![external_storage(2)]), None);
        let out = cx.finish();

        assert!(out.errors.contains(&GraphError::MutableExternalImport));
        assert!(uniform_refs(&out, UniformId(0)).is_empty());
        let schema = out.schemas.get(out.desc.uniforms[0].schema).unwrap();
        assert!(schema.resource_fields.is_empty());
    }

    /// a failed binding drops from the schema and the bindings together, so
    /// the one lowering error does not cascade into shape errors
    #[test]
    fn failed_binding_drops_its_schema_field() {
        let mut cx = LowerCtx::new(textures(1));
        cx.dispatch(
            0,
            GROUPS,
            uni(0, vec![external_storage(2), sampled(0)]),
            None,
        );
        let out = cx.finish();

        assert_eq!(out.errors, vec![GraphError::MutableExternalImport]);
        let schema = out.schemas.get(out.desc.uniforms[0].schema).unwrap();
        assert_eq!(schema.resource_fields, vec![ResourceFieldKind::SampledTex]);
        assert_eq!(
            out.desc.uniforms[0].source.bindings,
            vec![(FieldKey(0), ResourceRef::Tex(TexId(0), TexAccess::Read))]
        );
        assert!(validate(&out.desc, &out.schemas).is_ok());
    }

    #[test]
    fn dispatch_names_advance_inside_repeat() {
        let mut cx = LowerCtx::new(textures(1));
        cx.begin_repeat();
        cx.dispatch(0, GROUPS, uni(0, vec![sampled(0)]), None);
        cx.dispatch(1, GROUPS, uni(1, vec![sampled(0)]), None);
        cx.end_repeat();
        let out = cx.finish();

        let PassDesc::Repeat { body, .. } = &out.desc.passes[0] else {
            panic!("expected a repeat pass")
        };
        let names: Vec<_> = body
            .iter()
            .map(|leaf| match leaf {
                LeafPass::Compute(compute) => compute.name.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(names, vec!["dispatch0", "dispatch1"]);
    }

    #[test]
    fn top_level_draws_coalesce_into_one_raster_pass() {
        let mut cx = LowerCtx::new(vec![]);
        cx.draw(0, LowerDrawCall::VertexCount(3), uni(0, vec![]), None);
        cx.draw(1, LowerDrawCall::WholeIndexed, uni(1, vec![]), None);
        let out = cx.finish();

        assert_eq!(out.desc.passes.len(), 1);
        let PassDesc::Leaf(LeafPass::Raster(raster)) = &out.desc.passes[0] else {
            panic!("expected one raster pass")
        };
        assert_eq!(raster.draws.len(), 2);
        assert_eq!(raster.draws[0].name, "draw0");
        assert_eq!(raster.draws[1].name, "draw1");
    }

    /// every table row carries a name, because names are the only handle an
    /// error message has on a resource
    #[test]
    fn lowered_tables_carry_diagnostic_names() {
        let mut cx = LowerCtx::new(textures(1));
        cx.upload(2, 8, 16);
        cx.dispatch(
            5,
            GROUPS,
            uni(3, vec![sampled(0), buf(BufferBindingKind::Storage, 2)]),
            None,
        );
        let out = cx.finish();

        assert_eq!(out.desc.textures[0].name, "tex0");
        assert_eq!(out.desc.buffers[0].name, "buf.Storage.2");
        assert_eq!(out.desc.uploads[0].name, "upload0");
        assert_eq!(out.desc.uniforms[0].name, "uniform3");
        assert_eq!(out.desc.pipelines[0].name, "pipeline.Compute.5");
        assert_eq!(
            out.schemas.get(out.desc.uniforms[0].schema).unwrap().name,
            "uniform3"
        );
        assert_eq!(
            out.desc.passes.len(),
            1,
            "the upload is not a pass; only the dispatch is"
        );
    }

    #[test]
    fn external_import_decls_carry_names() {
        let mut cx = LowerCtx::new(vec![]);
        cx.dispatch(0, GROUPS, uni(0, vec![external_sampled(4)]), None);
        let out = cx.finish();

        assert_eq!(out.desc.imports[0].name, "import0");
    }

    #[test]
    fn draw_call_variants_lower_one_to_one() {
        let mut cx = LowerCtx::new(vec![]);
        cx.draw(0, LowerDrawCall::VertexCount(6), uni(0, vec![]), None);
        cx.draw(1, LowerDrawCall::WholeIndexed, uni(1, vec![]), None);
        cx.draw(
            2,
            LowerDrawCall::IndexRange {
                first_index: 3,
                index_count: 9,
            },
            uni(2, vec![]),
            None,
        );
        cx.draw(
            3,
            LowerDrawCall::IndexedIndirect {
                args_index: 7,
                byte_offset: 32,
                draw_count: 5,
                request: crate::commands::IndirectRequest::new::<
                    crate::runtime::tests::DrawIndexedIndirectCommand,
                >(7, 32, 5),
            },
            uni(3, vec![]),
            None,
        );
        let out = cx.finish();

        let PassDesc::Leaf(LeafPass::Raster(raster)) = &out.desc.passes[0] else {
            panic!("expected one raster pass")
        };
        assert!(matches!(raster.draws[0].call, DrawCall::VertexCount(6)));
        assert!(matches!(raster.draws[1].call, DrawCall::WholeIndexed));
        assert!(matches!(
            raster.draws[2].call,
            DrawCall::IndexRange {
                first_index: 3,
                index_count: 9,
            }
        ));
        let DrawCall::IndexedIndirect { args, draw_count } = &raster.draws[3].call else {
            panic!("expected an indirect draw")
        };
        assert_eq!(*draw_count, 5);
        assert_eq!(args.offset, 32);
        assert_eq!(args.slot, SlotSel::Current);
        assert_eq!(
            out.desc.buffers[args.buffer.0 as usize].kind,
            BufferKind::Immutable
        );
    }

    /// the full watercolor shape, with the physical image counts derived by
    /// hand from its binding sets
    #[test]
    fn watercolor_shaped_lowering_parity() {
        const PAPER: u32 = 12;
        let mut cx = LowerCtx::new(textures(14));
        cx.begin_optional();
        cx.upload(0, 32, 4096);
        cx.dispatch(
            0,
            GROUPS,
            uni(
                0,
                vec![
                    storage_mutate(7),
                    storage_mutate(2),
                    storage_mutate(3),
                    storage_mutate(4),
                    storage_mutate(5),
                    storage_mutate(6),
                    buf(BufferBindingKind::Storage, 0),
                ],
            ),
            None,
        );
        cx.end_optional();
        cx.dispatch(
            1,
            GROUPS,
            uni(
                1,
                vec![
                    sampled(0),
                    sampled(1),
                    sampled(2),
                    sampled(7),
                    storage_write(0),
                    storage_write(1),
                    external_sampled(PAPER),
                ],
            ),
            None,
        );
        cx.dispatch(
            2,
            GROUPS,
            uni(2, vec![sampled(0), sampled(1), storage_write(11)]),
            None,
        );
        cx.begin_repeat();
        cx.dispatch(
            3,
            GROUPS,
            uni(3, vec![sampled(11)]),
            push(vec![sampled(2), storage_write(2)]),
        );
        cx.end_repeat();
        cx.dispatch(
            4,
            GROUPS,
            uni(
                4,
                vec![storage_mutate(0), storage_mutate(1), sampled(7), sampled(2)],
            ),
            None,
        );
        cx.dispatch(
            5,
            GROUPS,
            uni(5, vec![]),
            push(vec![sampled(7), storage_write(12)]),
        );
        cx.dispatch(
            6,
            GROUPS,
            uni(6, vec![]),
            push(vec![sampled(12), storage_write(13)]),
        );
        cx.dispatch(
            7,
            GROUPS,
            uni(
                7,
                vec![
                    sampled(7),
                    storage_mutate(6),
                    sampled(13),
                    storage_mutate(2),
                ],
            ),
            None,
        );
        cx.dispatch(
            8,
            GROUPS,
            uni(
                8,
                vec![
                    sampled(3),
                    sampled(4),
                    sampled(5),
                    prev_sampled(0),
                    prev_sampled(1),
                    sampled(7),
                    storage_write(3),
                    storage_write(4),
                    storage_write(5),
                    sampled(8),
                    sampled(9),
                    sampled(10),
                    storage_write(8),
                    storage_write(9),
                    storage_write(10),
                    external_sampled(PAPER),
                ],
            ),
            None,
        );
        cx.dispatch(
            9,
            GROUPS,
            uni(
                9,
                vec![sampled(6), sampled(7), storage_write(6), storage_write(7)],
            ),
            None,
        );
        cx.draw(
            0,
            LowerDrawCall::VertexCount(3),
            uni(
                10,
                vec![
                    sampled(8),
                    sampled(9),
                    sampled(10),
                    external_sampled(PAPER),
                    prev_sampled(7),
                ],
            ),
            None,
        );
        let out = cx.finish();

        assert!(out.errors.is_empty());
        assert_eq!(out.desc.passes.len(), 11);
        assert_eq!(out.desc.uniforms.len(), 11);
        assert_eq!(out.desc.pipelines.len(), 11);
        assert_eq!(out.desc.buffers.len(), 1);
        assert_eq!(out.desc.imports.len(), 1);
        assert_eq!(out.desc.uploads.len(), 1);
        assert_eq!(out.desc.values.len(), 13);
        assert!(matches!(out.desc.passes[0], PassDesc::When { .. }));
        assert!(matches!(out.desc.passes[3], PassDesc::Repeat { .. }));
        assert!(matches!(
            out.desc.passes[10],
            PassDesc::Leaf(LeafPass::Raster(_))
        ));

        let analysis = validate(&out.desc, &out.schemas).expect("watercolor shape must validate");
        assert_eq!(
            analysis.tex_phys,
            vec![2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1]
        );
    }

    #[test]
    fn particles_shaped_lowering_parity() {
        let mut cx = LowerCtx::new(vec![]);
        cx.dispatch(
            0,
            [16, 1, 1],
            uni(
                0,
                vec![
                    buf(BufferBindingKind::GpuOnlyPrevious, 0),
                    buf(BufferBindingKind::GpuOnlyCurrent, 0),
                ],
            ),
            None,
        );
        cx.draw(
            0,
            LowerDrawCall::VertexCount(4096),
            uni(1, vec![buf(BufferBindingKind::GpuOnlyCurrent, 0)]),
            None,
        );
        let out = cx.finish();

        assert!(out.errors.is_empty());
        assert_eq!(out.desc.passes.len(), 2);
        assert_eq!(out.desc.buffers.len(), 1);
        assert_eq!(out.desc.pipelines.len(), 2);
        assert_eq!(out.pipeline_indices, vec![0, 0]);
        assert_eq!(out.desc.pipelines[1].kind, PipelineKind::Graphics);

        let analysis = validate(&out.desc, &out.schemas).expect("particles shape must validate");
        assert!(analysis.tex_phys.is_empty());
        let _ = TexAccess::Read;
    }
}
