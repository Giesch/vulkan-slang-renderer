//! Positive control for vertex pulling with hardware index buffers.
//!
//! Generated `pipeline_config()` infers the draw-call kind from the shader: a
//! vertex entry point reading only `SV_VertexID` is `Graphics`, and one reading
//! `SV_DrawIndex` is `GraphicsIndirect`. Vertices are a singleton buffer bound
//! through the params' `ImmutableAddr`, and indexed draws take an
//! `IndexBufferHandle` / `IndexBufferSlot`. No `Renderer` is constructed; the
//! functions are only type-checked.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{
    Graphics, GraphicsIndirect, NoPush, PipelineConfig, PipelineHandle,
};
use mltrs_renderer::renderer::render_graph::{
    DynDrawNode, GraphicsIndirectPipelineKey, GraphicsPipelineKey, IndexBufferSlot, SingletonSlot,
    draw_index_range, draw_indexed, draw_indexed_indirect, draw_vertex_count,
};
use mltrs_renderer::renderer::{
    DrawIndexedIndirectCommand, FrameRenderer, ImmutableBufferHandle, IndexBufferHandle, Renderer,
    SingletonBufferHandle, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::indirect_mesh::{
    self, IndirectMeshParams, IndirectMeshParamsBindings, IndirectVertex,
};
use render_graph_api_checks::generated::shader_atlas::mesh::{
    self, MeshParams, MeshParamsBindings, MeshVertex, Resources,
};

fn graphics_pipeline(
    renderer: &mut Renderer,
    params: &UniformBufferHandle<MeshParams>,
) -> anyhow::Result<PipelineHandle<Graphics, NoPush>> {
    let config: PipelineConfig<'_, Graphics, NoPush> =
        mesh::Shader::init().pipeline_config(Resources {
            mesh_params_buffer: params,
        });

    renderer.create_pipeline(config)
}

fn indirect_pipeline(
    renderer: &mut Renderer,
    params: &UniformBufferHandle<IndirectMeshParams>,
) -> anyhow::Result<PipelineHandle<GraphicsIndirect, NoPush>> {
    let config: PipelineConfig<'_, GraphicsIndirect, NoPush> = indirect_mesh::Shader::init()
        .pipeline_config(indirect_mesh::Resources {
            indirect_mesh_params_buffer: params,
        });

    renderer.create_pipeline(config)
}

/// One vertex buffer and one index buffer, shared by every draw below.
fn mesh_buffers(
    renderer: &mut Renderer,
    vertices: &[MeshVertex],
    indices: &[u32],
) -> anyhow::Result<(SingletonBufferHandle<MeshVertex>, IndexBufferHandle)> {
    let vertex_buffer = renderer.create_singleton_buffer(vertices)?;
    let index_buffer = renderer.create_index_buffer(indices, vertex_buffer.len())?;
    let _: u32 = index_buffer.index_count();

    Ok((vertex_buffer, index_buffer))
}

/// A `Graphics` pipeline lowers into vertex-count and indexed draw nodes.
fn graph_draws(
    pipeline: &PipelineHandle<Graphics, NoPush>,
    params: &UniformBufferHandle<MeshParams>,
    vertices: &SingletonBufferHandle<MeshVertex>,
    indices: &IndexBufferHandle,
) {
    let bindings = MeshParamsBindings {
        vertices: SingletonSlot::from(vertices).addr(),
    };
    let _ = draw_vertex_count(pipeline, params, 3, bindings);
    let _ = draw_indexed(pipeline, indices, params, bindings);
    let _ = draw_index_range(
        GraphicsPipelineKey::from(pipeline),
        IndexBufferSlot::from(indices),
        params,
        0,
        3,
        bindings,
    );
}

/// A `GraphicsIndirect` pipeline lowers into the indexed-indirect draw node.
fn indirect_graph_draw(
    pipeline: &PipelineHandle<GraphicsIndirect, NoPush>,
    params: &UniformBufferHandle<IndirectMeshParams>,
    vertices: &SingletonBufferHandle<IndirectVertex>,
    indices: &IndexBufferHandle,
    args: &ImmutableBufferHandle<DrawIndexedIndirectCommand>,
) {
    let _ = draw_indexed_indirect(
        GraphicsIndirectPipelineKey::from(pipeline),
        indices,
        params,
        args,
        0,
        1,
        IndirectMeshParamsBindings {
            vertices: SingletonSlot::from(vertices).addr(),
        },
    );
}

/// The dynamic graph's indexed draws take the same index buffer slot.
fn dynamic_draws(
    pipeline: &PipelineHandle<Graphics, NoPush>,
    params: &UniformBufferHandle<MeshParams>,
    indices: &IndexBufferHandle,
) {
    let _ = DynDrawNode::indexed(pipeline, indices, params, 16);
    let _ = DynDrawNode::index_range(pipeline, indices, params, 16, 0, 3);
}

/// The frame renderer's manual draw queue binds the index buffer per draw.
fn manual_draws(
    frame: &mut FrameRenderer<'_>,
    pipeline: &PipelineHandle<Graphics, NoPush>,
    indirect: &PipelineHandle<GraphicsIndirect, NoPush>,
    indices: &IndexBufferHandle,
    args: &ImmutableBufferHandle<DrawIndexedIndirectCommand>,
) {
    frame.queue_draw_vertex_count(pipeline, 3);
    frame.queue_draw_indexed(pipeline, indices);
    frame.queue_draw_index_range(pipeline, indices, 0, 3);
    frame.queue_draw_indexed_indirect(indirect, indices, args, 0, 1);
}

fn main() {}
