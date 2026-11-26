//! Positive control for the vertex interfaces of indexed pipelines.
//!
//! A typed config built with `with_vertices` or `with_shared_mesh` keeps its
//! generated vertex type as `TypedVertex<V>` through `create_pipeline`, and a
//! packed-bytes config from `build_indexed_bytes` is `RuntimeVertex`. Both
//! handles lower into the graph's indexed draw keys, typed and dynamic. No
//! `Renderer` is constructed; the functions are only type-checked.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{
    DrawIndexed, DrawIndexedIndirect, GraphicsPipelineHandle, MeshHandle, NoPush,
    PipelineConfig, PipelineConfigBuilder, RuntimeVertex, TypedVertex,
};
use mltrs_renderer::renderer::render_graph::{
    DrawIndexedKey, DynDrawNode, draw_index_range, draw_indexed,
};
use mltrs_renderer::renderer::{PipelineErrors, Renderer, UniformBufferHandle};

use render_graph_api_checks::generated::shader_atlas::mesh::{
    self, MeshParams, MeshVertex, Resources,
};

fn typed_owned_vertices(
    renderer: &mut Renderer,
    params: &UniformBufferHandle<MeshParams>,
    vertices: Vec<MeshVertex>,
    indices: Vec<u32>,
) -> eros::Result<GraphicsPipelineHandle<TypedVertex<MeshVertex>, DrawIndexed>, PipelineErrors> {
    let config: PipelineConfig<'_, TypedVertex<MeshVertex>, DrawIndexed, NoPush> =
        mesh::Shader::init()
            .pipeline_config(Resources {
                mesh_params_buffer: params,
            })
            .with_vertices(vertices, indices);

    renderer.create_pipeline(config)
}

fn typed_shared_mesh(
    renderer: &mut Renderer,
    params: &UniformBufferHandle<MeshParams>,
    mesh_handle: &MeshHandle<MeshVertex>,
) -> eros::Result<GraphicsPipelineHandle<TypedVertex<MeshVertex>, DrawIndexedIndirect>, PipelineErrors> {
    let config = mesh::Shader::init()
        .pipeline_config(Resources {
            mesh_params_buffer: params,
        })
        .with_shared_mesh(mesh_handle)
        .indirect();

    renderer.create_pipeline(config)
}

fn runtime_packed_bytes(
    renderer: &mut Renderer,
    builder: PipelineConfigBuilder<'_>,
    vertex_bytes: Vec<u8>,
    indices: Vec<u32>,
) -> eros::Result<GraphicsPipelineHandle<RuntimeVertex, DrawIndexed>, PipelineErrors> {
    let config: PipelineConfig<'_, RuntimeVertex, DrawIndexed, NoPush> =
        builder.build_indexed_bytes(vertex_bytes, indices);

    renderer.create_pipeline(config)
}

/// Both indexed interfaces lower into the typed graph's indexed draw nodes.
fn graph_draws(
    typed: &GraphicsPipelineHandle<TypedVertex<MeshVertex>, DrawIndexed>,
    runtime: &GraphicsPipelineHandle<RuntimeVertex, DrawIndexed>,
    params: &UniformBufferHandle<MeshParams>,
) {
    let _ = draw_indexed(typed, params, ());
    let _ = draw_indexed(runtime, params, ());
    let _ = draw_index_range(DrawIndexedKey::from(typed), params, 0, 3, ());
    let _ = draw_index_range(DrawIndexedKey::from(runtime), params, 0, 3, ());
}

/// Both indexed interfaces lower into the dynamic graph's indexed draw node.
fn dynamic_draws(
    typed: &GraphicsPipelineHandle<TypedVertex<MeshVertex>, DrawIndexed>,
    runtime: &GraphicsPipelineHandle<RuntimeVertex, DrawIndexed>,
    params: &UniformBufferHandle<MeshParams>,
) {
    let _ = DynDrawNode::indexed(typed, params, 16);
    let _ = DynDrawNode::indexed(runtime, params, 16);
}

fn main() {}
