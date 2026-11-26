//! Owned Rust copies of the host graphs Roc's `roc_config` returns.
//!
//! The copy happens once at setup, so every Roc allocation is released when
//! the `roc_config_owned` wrapper drops, before any GPU resource exists.
//! The description and schemas convert one-to-one into the
//! `mltrs_render_graph::plain` tables they mirror.

use mltrs::renderer::render_graph::plain::{
    BufAccess, BufferDecl, BufferId, BufferKind, BufferRef, DispatchDesc, DrawCall, DrawDesc,
    FieldKey, GraphDesc, GraphFormat, GroupSource, ImportDecl, ImportId, LeafPass, PassDesc,
    PickingDesc, PipelineDecl, PipelineId, PipelineKind, PushDesc, RasterDesc, RasterTargets,
    ResourceFieldKind, ResourceRef, SchemaDesc, SchemaField, SchemaFieldKind, SchemaId,
    SchemaLayout, SchemaTable, SizeClass, SlotSel, TexAccess, TexDecl, TexId, TexUsage,
    UniformDecl, UniformId, UniformSourceDesc, UploadDesc, ValueDecl, ValueId, ValueKind,
    ValueScope,
};
use mltrs::renderer::{BlendMode, CullMode, DepthCompare};

use crate::roc_platform_abi::*;

/// One validated Roc render graph: the resources the host creates and the
/// description it executes. Description tables index the host tables as
/// `ValidatedRenderGraph.HostGraph` documents: `desc.uniforms[i]` writes
/// `uniform_buffers[uniform_slots[i]]`, and `desc.pipelines[i]` is
/// `pipelines[pipeline_indices[i]]`.
pub struct HostGraphDefinition {
    pub buffers: Vec<HostBuffer>,
    pub uniform_buffers: Vec<HostUniformBuffer>,
    pub pipelines: Vec<HostPipeline>,
    pub import_names: Vec<String>,
    pub desc: GraphDesc,
    pub schemas: SchemaTable,
    pub uniform_slots: Vec<u32>,
    pub pipeline_indices: Vec<u32>,
}

pub struct HostBuffer {
    pub name: String,
    pub role: HostBufferRole,
    pub element_size: u32,
    pub len: u32,
    pub contents: HostBufferContents,
}

/// The renderer constructor a buffer uses; `IndirectArgs` is an immutable
/// buffer of `vkCmdDrawIndexedIndirect` records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostBufferRole {
    CpuUpdated,
    GpuUpdated,
    Immutable,
    Singleton,
    Index,
    IndirectArgs,
}

pub enum HostBufferContents {
    /// packed leading elements
    Bytes(Vec<u8>),
    Indices {
        indices: Vec<u32>,
        vertex_count: u32,
    },
}

pub struct HostUniformBuffer {
    pub name: String,
    pub size: u32,
}

pub struct HostPipeline {
    pub name: String,
    pub kind: PipelineKind,
    pub stages: HostPipelineStages,
    pub reflection_json: String,
    /// indices into `HostGraphDefinition::uniform_buffers`, in
    /// descriptor-set-layout order
    pub uniform_buffers: Vec<u32>,
}

pub enum HostPipelineStages {
    Graphics {
        vertex_spv: Vec<u8>,
        fragment_spv: Vec<u8>,
        raster: HostRasterState,
    },
    Compute {
        compute_spv: Vec<u8>,
    },
}

/// The renderer's `RasterState`, except the stencil mode: an enabled
/// `StencilMode` needs the renderer's `StencilSupport` token, so it is
/// resolved when the pipeline is created.
pub struct HostRasterState {
    pub blend: BlendMode,
    pub cull: CullMode,
    pub depth_test: DepthCompare,
    pub depth_write: bool,
    /// in RGBA order
    pub color_write: [bool; 4],
    pub stencil: HostStencilMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostStencilMode {
    Disabled,
    Write(u8),
    TestEqual(u8),
}

impl HostGraphDefinition {
    /// Calls the app's graph configuration once and copies every graph out
    /// of Roc memory.
    pub fn fetch_config() -> Vec<Self> {
        // Safety: the generated wrapper owns the config until every graph is
        // copied, and releases it on drop.
        let owned = unsafe { roc_config_owned() };

        owned.graphs.as_slice().iter().map(Self::copy).collect()
    }

    fn copy(graph: &ValidatedRenderGraphHostGraph) -> Self {
        Self {
            buffers: graph.buffers.as_slice().iter().map(copy_buffer).collect(),
            uniform_buffers: graph
                .uniform_buffers
                .as_slice()
                .iter()
                .map(|uniform| HostUniformBuffer {
                    name: uniform.name.as_str().to_owned(),
                    size: uniform.size,
                })
                .collect(),
            pipelines: graph
                .pipelines
                .as_slice()
                .iter()
                .map(copy_pipeline)
                .collect(),
            import_names: graph
                .import_decls
                .as_slice()
                .iter()
                .map(|import| import.name.as_str().to_owned())
                .collect(),
            desc: copy_desc(&graph.desc),
            schemas: copy_schemas(&graph.schemas),
            uniform_slots: graph.uniform_slots.as_slice().to_vec(),
            pipeline_indices: graph.pipeline_indices.as_slice().to_vec(),
        }
    }
}

fn copy_buffer(buffer: &ValidatedRenderGraphHostBuffer) -> HostBuffer {
    let role = match buffer.role {
        ValidatedRenderGraphHostBufferRole::CpuUpdated => HostBufferRole::CpuUpdated,
        ValidatedRenderGraphHostBufferRole::GpuUpdated => HostBufferRole::GpuUpdated,
        ValidatedRenderGraphHostBufferRole::Immutable => HostBufferRole::Immutable,
        ValidatedRenderGraphHostBufferRole::Singleton => HostBufferRole::Singleton,
        ValidatedRenderGraphHostBufferRole::Index => HostBufferRole::Index,
        ValidatedRenderGraphHostBufferRole::IndirectArgs => HostBufferRole::IndirectArgs,
    };

    // Safety: each arm borrows the payload its tag selects.
    let contents = match buffer.contents.tag {
        ValidatedRenderGraphHostBufferContentsTag::Bytes => {
            let bytes = unsafe { buffer.contents.borrow_payload_bytes_unchecked() };
            HostBufferContents::Bytes(bytes.as_slice().to_vec())
        }
        ValidatedRenderGraphHostBufferContentsTag::Indices => {
            let indices = unsafe { buffer.contents.borrow_payload_indices_unchecked() };
            HostBufferContents::Indices {
                indices: indices.indices.as_slice().to_vec(),
                vertex_count: indices.vertex_count,
            }
        }
    };

    HostBuffer {
        name: buffer.name.as_str().to_owned(),
        role,
        element_size: buffer.element_size,
        len: buffer.len,
        contents,
    }
}

fn copy_pipeline(pipeline: &ValidatedRenderGraphHostPipeline) -> HostPipeline {
    // Safety: each arm borrows the payload its tag selects.
    let stages = match pipeline.stages.tag {
        ValidatedRenderGraphHostPipelineStagesTag::Graphics => {
            let stages = unsafe { pipeline.stages.borrow_payload_graphics_unchecked() };
            HostPipelineStages::Graphics {
                vertex_spv: stages.vertex_spv.as_slice().to_vec(),
                fragment_spv: stages.fragment_spv.as_slice().to_vec(),
                raster: copy_raster(&stages.raster),
            }
        }
        ValidatedRenderGraphHostPipelineStagesTag::Compute => {
            let stages = unsafe { pipeline.stages.borrow_payload_compute_unchecked() };
            HostPipelineStages::Compute {
                compute_spv: stages.compute_spv.as_slice().to_vec(),
            }
        }
    };

    HostPipeline {
        name: pipeline.name.as_str().to_owned(),
        kind: pipeline_kind(pipeline.kind),
        stages,
        reflection_json: pipeline.reflection_json.as_str().to_owned(),
        uniform_buffers: pipeline.uniform_buffers.as_slice().to_vec(),
    }
}

fn copy_raster(raster: &RasterState) -> HostRasterState {
    let blend = match raster.blend {
        RasterStateBlendMode::Alpha => BlendMode::Alpha,
        RasterStateBlendMode::DstAlpha => BlendMode::DstAlpha,
        RasterStateBlendMode::Opaque => BlendMode::Opaque,
    };
    let cull = match raster.cull {
        RasterStateCullMode::Back => CullMode::Back,
        RasterStateCullMode::Front => CullMode::Front,
        RasterStateCullMode::None => CullMode::None,
    };
    let depth_test = match raster.depth_test {
        RasterStateDepthCompare::Less => DepthCompare::Less,
        RasterStateDepthCompare::LessEqual => DepthCompare::LessEqual,
        RasterStateDepthCompare::Always => DepthCompare::Always,
        RasterStateDepthCompare::Disabled => DepthCompare::Disabled,
    };
    // Safety: each arm borrows the payload its tag selects.
    let stencil = match raster.stencil.tag {
        RasterStateStencilModeTag::Disabled => HostStencilMode::Disabled,
        RasterStateStencilModeTag::Write => {
            HostStencilMode::Write(unsafe { *raster.stencil.borrow_payload_write_unchecked() })
        }
        RasterStateStencilModeTag::TestEqual => HostStencilMode::TestEqual(unsafe {
            *raster.stencil.borrow_payload_test_equal_unchecked()
        }),
    };
    let color_write = &raster.color_write;

    HostRasterState {
        blend,
        cull,
        depth_test,
        depth_write: raster.depth_write,
        color_write: [color_write.r, color_write.g, color_write.b, color_write.a],
        stencil,
    }
}

fn copy_desc(desc: &RenderGraphDescGraphDesc) -> GraphDesc {
    // Safety: the tag selects the payload.
    let picking = match desc.picking.tag {
        RenderGraphDescMaybePickingTag::None => None,
        RenderGraphDescMaybePickingTag::Some => {
            let picking = unsafe { desc.picking.borrow_payload_some_unchecked() };
            Some(PickingDesc {
                cursor: ValueId(picking.cursor),
            })
        }
    };

    GraphDesc {
        textures: desc.textures.as_slice().iter().map(copy_texture).collect(),
        buffers: desc
            .buffers
            .as_slice()
            .iter()
            .map(|buffer| BufferDecl {
                name: buffer.name.as_str().to_owned(),
                kind: buffer_kind(buffer.kind),
                capacity: maybe_u32(&buffer.capacity),
                elem_size: maybe_u32(&buffer.elem_size),
            })
            .collect(),
        imports: desc
            .import_decls
            .as_slice()
            .iter()
            .map(|import| ImportDecl {
                name: import.name.as_str().to_owned(),
            })
            .collect(),
        values: desc.values.as_slice().iter().map(copy_value).collect(),
        uniforms: desc
            .uniforms
            .as_slice()
            .iter()
            .map(|uniform| UniformDecl {
                name: uniform.name.as_str().to_owned(),
                schema: SchemaId(uniform.schema),
                source: UniformSourceDesc {
                    data: maybe_u32(&uniform.source.data).map(ValueId),
                    bindings: copy_bindings(&uniform.source.bindings),
                },
            })
            .collect(),
        pipelines: desc
            .pipelines
            .as_slice()
            .iter()
            .map(|pipeline| PipelineDecl {
                name: pipeline.name.as_str().to_owned(),
                kind: pipeline_kind(pipeline.kind),
                params: SchemaId(pipeline.params),
                push: maybe_u32(&pipeline.push).map(SchemaId),
            })
            .collect(),
        uploads: desc
            .uploads
            .as_slice()
            .iter()
            .map(|upload| UploadDesc {
                name: upload.name.as_str().to_owned(),
                buffer: BufferId(upload.buffer),
                value: ValueId(upload.value),
            })
            .collect(),
        passes: desc.passes.as_slice().iter().map(copy_pass).collect(),
        picking,
    }
}

fn copy_texture(texture: &RenderGraphDescTexDecl) -> TexDecl {
    let format = match texture.format {
        RenderGraphDescGraphFormat::R32Float => GraphFormat::R32Float,
        RenderGraphDescGraphFormat::Rgba32Float => GraphFormat::Rgba32Float,
    };
    // Safety: each arm borrows the payload its tag selects.
    let size = match texture.size.tag {
        RenderGraphDescSizeClassTag::Fixed => {
            let fixed = unsafe { texture.size.borrow_payload_fixed_unchecked() };
            SizeClass::Fixed(fixed._0, fixed._1)
        }
        RenderGraphDescSizeClassTag::Window => SizeClass::Window,
        RenderGraphDescSizeClassTag::WindowDiv => {
            SizeClass::WindowDiv(unsafe { *texture.size.borrow_payload_window_div_unchecked() })
        }
    };
    let usage = match texture.usage {
        RenderGraphDescTexUsage::Storage => TexUsage::Storage,
        RenderGraphDescTexUsage::Color => TexUsage::Color,
        RenderGraphDescTexUsage::Depth => TexUsage::Depth,
    };

    TexDecl {
        name: texture.name.as_str().to_owned(),
        format,
        size,
        usage,
    }
}

fn copy_value(value: &RenderGraphDescValueDecl) -> ValueDecl {
    // Safety: each arm borrows the payload its tag selects.
    let kind = match value.kind.tag {
        RenderGraphDescValueKindTag::Count => ValueKind::Count,
        RenderGraphDescValueKindTag::Groups => ValueKind::Groups,
        RenderGraphDescValueKindTag::Bytes => {
            let bytes = unsafe { value.kind.borrow_payload_bytes_unchecked() };
            ValueKind::Bytes {
                schema: SchemaId(bytes.schema),
            }
        }
        RenderGraphDescValueKindTag::Array => {
            let array = unsafe { value.kind.borrow_payload_array_unchecked() };
            ValueKind::Array {
                elem: SchemaId(array.elem),
                max_len: array.max_len,
            }
        }
    };
    // Safety: each arm borrows the payload its tag selects.
    let scope = match value.scope.tag {
        RenderGraphDescValueScopeTag::Top => ValueScope::Top,
        RenderGraphDescValueScopeTag::When => {
            let when = unsafe { value.scope.borrow_payload_when_unchecked() };
            ValueScope::When {
                gate: ValueId(when.gate),
            }
        }
        RenderGraphDescValueScopeTag::Repeat => {
            let repeat = unsafe { value.scope.borrow_payload_repeat_unchecked() };
            ValueScope::Repeat {
                count: ValueId(repeat.count),
            }
        }
    };

    ValueDecl {
        name: value.name.as_str().to_owned(),
        kind,
        optional: value.optional,
        scope,
    }
}

fn copy_pass(pass: &RenderGraphDescPassDesc) -> PassDesc {
    // Safety: each arm borrows the payload its tag selects.
    match pass.tag {
        RenderGraphDescPassDescTag::Leaf => {
            PassDesc::Leaf(copy_leaf(unsafe { pass.borrow_payload_leaf_unchecked() }))
        }
        RenderGraphDescPassDescTag::When => {
            let when = unsafe { pass.borrow_payload_when_unchecked() };
            PassDesc::When {
                name: when.name.as_str().to_owned(),
                value: ValueId(when.value),
                body: when.body.as_slice().iter().map(copy_leaf).collect(),
            }
        }
        RenderGraphDescPassDescTag::Repeat => {
            let repeat = unsafe { pass.borrow_payload_repeat_unchecked() };
            PassDesc::Repeat {
                name: repeat.name.as_str().to_owned(),
                count: ValueId(repeat.count),
                body: repeat.body.as_slice().iter().map(copy_leaf).collect(),
            }
        }
    }
}

fn copy_leaf(leaf: &RenderGraphDescLeafPass) -> LeafPass {
    // Safety: each arm borrows the payload its tag selects.
    match leaf.tag {
        RenderGraphDescLeafPassTag::Compute => {
            let dispatch = unsafe { leaf.borrow_payload_compute_unchecked() };
            LeafPass::Compute(copy_dispatch(dispatch))
        }
        RenderGraphDescLeafPassTag::Raster => {
            let raster = unsafe { leaf.borrow_payload_raster_unchecked() };
            LeafPass::Raster(copy_raster_pass(raster))
        }
    }
}

fn copy_dispatch(dispatch: &RenderGraphDescDispatchDesc) -> DispatchDesc {
    // Safety: each arm borrows the payload its tag selects.
    let groups = match dispatch.groups.tag {
        RenderGraphDescGroupSourceTag::Fixed => {
            let count = unsafe { dispatch.groups.borrow_payload_fixed_unchecked() };
            GroupSource::Fixed([count.x, count.y, count.z])
        }
        RenderGraphDescGroupSourceTag::Value => GroupSource::Value(ValueId(unsafe {
            *dispatch.groups.borrow_payload_value_unchecked()
        })),
    };

    DispatchDesc {
        name: dispatch.name.as_str().to_owned(),
        pipeline: PipelineId(dispatch.pipeline),
        uniform: UniformId(dispatch.uniform),
        groups,
        push: copy_push(&dispatch.push),
    }
}

fn copy_raster_pass(raster: &RenderGraphDescRasterDesc) -> RasterDesc {
    // Safety: the tag selects the payload.
    let targets = match raster.raster_targets.tag {
        RenderGraphDescRasterTargetsTag::Main => RasterTargets::Main,
        RenderGraphDescRasterTargetsTag::Offscreen => {
            let offscreen = unsafe { raster.raster_targets.borrow_payload_offscreen_unchecked() };
            RasterTargets::Offscreen {
                color: offscreen
                    .color
                    .as_slice()
                    .iter()
                    .copied()
                    .map(TexId)
                    .collect(),
                depth: maybe_u32(&offscreen.depth).map(TexId),
            }
        }
    };

    RasterDesc {
        name: raster.name.as_str().to_owned(),
        targets,
        draws: raster.draws.as_slice().iter().map(copy_draw).collect(),
    }
}

fn copy_draw(draw: &RenderGraphDescDrawDesc) -> DrawDesc {
    // Safety: each arm borrows the payload its tag selects.
    let call = match draw.call.tag {
        RenderGraphDescDrawCallTag::VertexCount => {
            DrawCall::VertexCount(unsafe { *draw.call.borrow_payload_vertex_count_unchecked() })
        }
        RenderGraphDescDrawCallTag::WholeIndexed => {
            let whole = unsafe { draw.call.borrow_payload_whole_indexed_unchecked() };
            DrawCall::WholeIndexed {
                indices: BufferId(whole.indices),
            }
        }
        RenderGraphDescDrawCallTag::IndexRange => {
            let range = unsafe { draw.call.borrow_payload_index_range_unchecked() };
            DrawCall::IndexRange {
                indices: BufferId(range.indices),
                first_index: range.first_index,
                index_count: range.index_count,
            }
        }
        RenderGraphDescDrawCallTag::IndexedIndirect => {
            let indirect = unsafe { draw.call.borrow_payload_indexed_indirect_unchecked() };
            DrawCall::IndexedIndirect {
                indices: BufferId(indirect.indices),
                args: copy_buffer_ref(&indirect.args),
                draw_count: indirect.draw_count,
            }
        }
    };

    DrawDesc {
        name: draw.name.as_str().to_owned(),
        pipeline: PipelineId(draw.pipeline),
        uniform: UniformId(draw.uniform),
        call,
        push: copy_push(&draw.push),
    }
}

fn copy_push(push: &RenderGraphDescMaybePush) -> Option<PushDesc> {
    match push.tag {
        RenderGraphDescMaybePushTag::None => None,
        RenderGraphDescMaybePushTag::Some => {
            // Safety: the tag selects the payload.
            let push = unsafe { push.borrow_payload_some_unchecked() };

            Some(PushDesc {
                data: maybe_u32(&push.data).map(ValueId),
                schema: SchemaId(push.schema),
                bindings: copy_bindings(&push.bindings),
            })
        }
    }
}

fn copy_bindings(
    bindings: &RocListWith<RenderGraphDescResourceBinding, false>,
) -> Vec<(FieldKey, ResourceRef)> {
    bindings
        .as_slice()
        .iter()
        .map(|binding| (FieldKey(binding.key), copy_resource(&binding.resource)))
        .collect()
}

fn copy_resource(resource: &RenderGraphDescResourceRef) -> ResourceRef {
    // Safety: each arm borrows the payload its tag selects.
    match resource.tag {
        RenderGraphDescResourceRefTag::Tex => {
            let tex = unsafe { resource.borrow_payload_tex_unchecked() };
            let access = match tex._1 {
                RenderGraphDescTexAccess::Read => TexAccess::Read,
                RenderGraphDescTexAccess::ReadPrevious => TexAccess::ReadPrevious,
                RenderGraphDescTexAccess::Write => TexAccess::Write,
                RenderGraphDescTexAccess::Mutate => TexAccess::Mutate,
            };
            ResourceRef::Tex(TexId(tex._0), access)
        }
        RenderGraphDescResourceRefTag::Buf => {
            let buf = unsafe { resource.borrow_payload_buf_unchecked() };
            let access = match buf._1 {
                RenderGraphDescBufAccess::Read => BufAccess::Read,
                RenderGraphDescBufAccess::Mutate => BufAccess::Mutate,
            };
            ResourceRef::Buf(copy_buffer_ref(&buf._0), access)
        }
        RenderGraphDescResourceRefTag::External => ResourceRef::External(ImportId(unsafe {
            *resource.borrow_payload_external_unchecked()
        })),
    }
}

fn copy_buffer_ref(buffer: &RenderGraphDescBufferRef) -> BufferRef {
    let slot = match buffer.slot {
        RenderGraphDescSlotSel::Current => SlotSel::Current,
        RenderGraphDescSlotSel::Previous => SlotSel::Previous,
    };

    BufferRef {
        buffer: BufferId(buffer.buffer),
        slot,
        offset: buffer.offset,
        range: maybe_u32(&buffer.range),
    }
}

fn copy_schemas(schemas: &RenderGraphDescSchemaTable) -> SchemaTable {
    SchemaTable {
        schemas: schemas.schemas.as_slice().iter().map(copy_schema).collect(),
    }
}

fn copy_schema(schema: &RenderGraphDescSchemaDesc) -> SchemaDesc {
    // Safety: the tag selects the payload.
    let layout = match schema.layout.tag {
        RenderGraphDescMaybeLayoutTag::None => None,
        RenderGraphDescMaybeLayoutTag::Some => {
            let layout = unsafe { schema.layout.borrow_payload_some_unchecked() };
            Some(SchemaLayout {
                fields: layout.fields.as_slice().iter().map(copy_field).collect(),
            })
        }
    };

    SchemaDesc {
        name: schema.name.as_str().to_owned(),
        size: schema.size,
        resource_fields: schema
            .resource_fields
            .as_slice()
            .iter()
            .map(|field| match field {
                RenderGraphDescResourceFieldKind::SampledTex => ResourceFieldKind::SampledTex,
                RenderGraphDescResourceFieldKind::StorageTex => ResourceFieldKind::StorageTex,
                RenderGraphDescResourceFieldKind::BufAddr => ResourceFieldKind::BufAddr,
            })
            .collect(),
        layout,
    }
}

fn copy_field(field: &RenderGraphDescSchemaField) -> SchemaField {
    // Safety: each arm borrows the payload its tag selects.
    let kind = match field.kind.tag {
        RenderGraphDescSchemaFieldKindTag::Data => {
            let data = unsafe { field.kind.borrow_payload_data_unchecked() };
            SchemaFieldKind::Data {
                src_offset: data.src_offset,
            }
        }
        RenderGraphDescSchemaFieldKindTag::SampledTex => SchemaFieldKind::SampledTex,
        RenderGraphDescSchemaFieldKindTag::StorageTex => SchemaFieldKind::StorageTex,
        RenderGraphDescSchemaFieldKindTag::BufAddr => SchemaFieldKind::BufAddr,
    };

    SchemaField {
        key: FieldKey(field.key),
        offset: field.offset,
        len: field.len,
        kind,
    }
}

fn buffer_kind(kind: RenderGraphDescBufferKind) -> BufferKind {
    match kind {
        RenderGraphDescBufferKind::GpuUpdatedFlight => BufferKind::GpuUpdatedFlight,
        RenderGraphDescBufferKind::Singleton => BufferKind::Singleton,
        RenderGraphDescBufferKind::Immutable => BufferKind::Immutable,
        RenderGraphDescBufferKind::Storage => BufferKind::Storage,
        RenderGraphDescBufferKind::Index => BufferKind::Index,
    }
}

fn pipeline_kind(kind: RenderGraphDescPipelineKind) -> PipelineKind {
    match kind {
        RenderGraphDescPipelineKind::Compute => PipelineKind::Compute,
        RenderGraphDescPipelineKind::Graphics => PipelineKind::Graphics,
        RenderGraphDescPipelineKind::GraphicsIndirect => PipelineKind::GraphicsIndirect,
    }
}

fn maybe_u32(value: &RenderGraphDescMaybeU32) -> Option<u32> {
    match value.tag {
        RenderGraphDescMaybeU32Tag::None => None,
        // Safety: the tag selects the payload.
        RenderGraphDescMaybeU32Tag::Some => Some(unsafe { *value.borrow_payload_some_unchecked() }),
    }
}
