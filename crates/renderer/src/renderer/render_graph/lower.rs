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
            let b = &mut self.desc.buffers[id.0 as usize];
            b.capacity = b.capacity.or(capacity);
            b.elem_size = b.elem_size.or(elem_size);
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
        if let Some(x) = self.imports.get(&raw) {
            return *x;
        }
        let id = ImportId(self.desc.imports.len() as u32);
        self.desc.imports.push(ImportDecl {
            name: format!("import{}", id.0),
        });
        self.import_handles.push(raw);
        self.imports.insert(raw, id);
        id
    }
    fn binding(&mut self, b: GraphBinding) -> Option<ResourceRef> {
        Some(match b {
            GraphBinding::SampledTex(x) => match x.inner {
                SampledRef::Graph(t) => ResourceRef::Tex(TexId(t.0), TexAccess::Read),
                SampledRef::GraphPrevious(t) => {
                    ResourceRef::Tex(TexId(t.0), TexAccess::ReadPrevious)
                }
                SampledRef::External(h) => ResourceRef::External(self.intern_import(h.to_raw())),
            },
            GraphBinding::StorageTex(x) => match x.inner {
                StorageRef::Graph(t, StorageTexAccess::Write) => {
                    ResourceRef::Tex(TexId(t.0), TexAccess::Write)
                }
                StorageRef::Graph(t, StorageTexAccess::Mutate) => {
                    ResourceRef::Tex(TexId(t.0), TexAccess::Mutate)
                }
                StorageRef::External(_) => {
                    self.errors.push(GraphError::MutableExternalImport);
                    return None;
                }
            },
            GraphBinding::Buffer(x) => {
                let (kind, slot, access) = match x.kind {
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
                let id = self.intern_buffer(kind, x.index, None, None);
                let off = match u32::try_from(x.byte_offset) {
                    Ok(v) => v,
                    Err(_) => {
                        self.errors.push(GraphError::BufferOffsetOverflow {
                            offset: x.byte_offset,
                        });
                        u32::MAX
                    }
                };
                ResourceRef::Buf(
                    BufferRef {
                        buffer: id,
                        slot,
                        offset: off,
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
            .filter_map(|(i, b)| self.binding(b).map(|r| (FieldKey(i as u16), r)))
            .collect()
    }
    fn fields(input: &[GraphBinding]) -> Vec<ResourceFieldKind> {
        input
            .iter()
            .map(|b| match b {
                GraphBinding::SampledTex(_) => ResourceFieldKind::SampledTex,
                GraphBinding::StorageTex(_) => ResourceFieldKind::StorageTex,
                GraphBinding::Buffer(_) => ResourceFieldKind::BufAddr,
            })
            .collect()
    }
    fn uniform(&mut self, u: UniformInput) -> UniformId {
        let data_schema = self.schema(format!("uniform{}.data", u.slot), u.data_size, vec![]);
        let data = self.value(
            format!("uniform{}.value", u.slot),
            ValueKind::Bytes {
                schema: data_schema,
            },
        );
        let fields = Self::fields(&u.bindings);
        let bindings = self.bindings(u.bindings);
        if let Some(id) = self.uniforms.get(&u.slot).copied() {
            let old = &self.desc.uniforms[id.0 as usize].source;
            if old.data == Some(data) && old.bindings == bindings {
                return id;
            }
            self.errors
                .push(GraphError::UniformSourceConflict { slot: u.slot });
            return id;
        }
        let schema = self.schema(format!("uniform{}", u.slot), u.gpu_size, fields);
        let id = UniformId(self.desc.uniforms.len() as u32);
        self.desc.uniforms.push(UniformDecl {
            name: format!("uniform{}", u.slot),
            schema,
            source: UniformSourceDesc {
                data: Some(data),
                bindings,
            },
        });
        self.uniform_slots.push(u.slot);
        self.uniforms.insert(u.slot, id);
        id
    }
    fn pipeline(
        &mut self,
        kind: PipelineKind,
        index: usize,
        params: SchemaId,
        push: Option<SchemaId>,
    ) -> PipelineId {
        if let Some(x) = self.pipelines.get(&(kind, index)) {
            return *x;
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
    fn push(&mut self, p: Option<PushInput>) -> Option<PushDesc> {
        p.map(|p| {
            let fields = Self::fields(&p.bindings);
            let bindings = self.bindings(p.bindings);
            let schema = self.schema(
                format!("push{}", self.schemas.schemas.len()),
                p.size,
                fields,
            );
            PushDesc {
                data: None,
                schema,
                bindings,
            }
        })
    }
    fn leaf(&mut self, l: LeafPass) {
        match &mut self.scope {
            Scope::Top => match l {
                LeafPass::Raster(r) => {
                    if let Some(PassDesc::Leaf(LeafPass::Raster(last))) =
                        self.desc.passes.last_mut()
                    {
                        last.draws.extend(r.draws)
                    } else {
                        self.desc.passes.push(PassDesc::Leaf(LeafPass::Raster(r)))
                    }
                }
                x => self.desc.passes.push(PassDesc::Leaf(x)),
            },
            Scope::Repeat { body, .. } | Scope::Optional { body, .. } => body.push(l),
        }
    }
    pub(crate) fn dispatch(
        &mut self,
        index: usize,
        groups: [u32; 3],
        u: UniformInput,
        p: Option<PushInput>,
    ) {
        let uniform = self.uniform(u);
        let params = self.desc.uniforms[uniform.0 as usize].schema;
        let push = self.push(p);
        let pipeline = self.pipeline(
            PipelineKind::Compute,
            index,
            params,
            push.as_ref().map(|x| x.schema),
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
        u: UniformInput,
        p: Option<PushInput>,
    ) {
        let uniform = self.uniform(u);
        let params = self.desc.uniforms[uniform.0 as usize].schema;
        let push = self.push(p);
        let pipeline = self.pipeline(
            PipelineKind::Graphics,
            index,
            params,
            push.as_ref().map(|x| x.schema),
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
                let b = self.intern_buffer(BufferKind::Immutable, args_index, None, None);
                DrawCall::IndexedIndirect {
                    args: BufferRef {
                        buffer: b,
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
        let b = self.intern_buffer(BufferKind::Storage, index, Some(capacity), Some(elem_size));
        let s = self.schema(
            format!("upload{}.elem", self.desc.uploads.len()),
            elem_size,
            vec![],
        );
        let v = self.value(
            format!("upload{}.value", self.desc.uploads.len()),
            ValueKind::Array {
                elem: s,
                max_len: capacity,
            },
        );
        self.desc.uploads.push(UploadDesc {
            name: format!("upload{}", self.desc.uploads.len()),
            buffer: b,
            value: v,
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
            let v = self.value(
                format!("repeat{}.count", self.desc.passes.len()),
                ValueKind::Count,
            );
            Scope::Repeat {
                body: vec![],
                count: v,
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
            x => self.scope = x,
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
