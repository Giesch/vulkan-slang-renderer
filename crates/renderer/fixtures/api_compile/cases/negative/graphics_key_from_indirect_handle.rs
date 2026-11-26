//! Negative control: a `GraphicsIndirect` pipeline cannot drive a direct
//! indexed draw. Its shader reads `SV_DrawIndex`, which only indirect draws
//! supply, so the handle has no conversion into the direct pipeline key.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{GraphicsIndirect, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{SingletonSlot, draw_indexed};
use mltrs_renderer::renderer::{IndexBufferHandle, SingletonBufferHandle, UniformBufferHandle};

use render_graph_api_checks::generated::shader_atlas::indirect_mesh::{
    IndirectMeshParams, IndirectMeshParamsBindings, IndirectVertex,
};

fn wrong(
    pipeline: &PipelineHandle<GraphicsIndirect, NoPush>,
    params: &UniformBufferHandle<IndirectMeshParams>,
    vertices: &SingletonBufferHandle<IndirectVertex>,
    indices: &IndexBufferHandle,
) {
    let _ = draw_indexed(
        pipeline,
        indices,
        params,
        IndirectMeshParamsBindings {
            vertices: SingletonSlot::from(vertices).addr(),
        },
    );
}

fn main() {}
