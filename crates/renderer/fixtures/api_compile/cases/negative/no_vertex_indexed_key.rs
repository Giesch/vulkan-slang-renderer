//! Negative control: a vertex-count graphics handle carries the `NoVertex`
//! interface, which is not an indexed interface. Even when the draw-call kind
//! says `DrawIndexed`, the handle has no conversion to an indexed draw key.
//!
//! `GraphicsPipelineHandle<NoVertex, DrawIndexed>` cannot come from
//! `create_pipeline`; the parameter stands in for a relabeled handle, which
//! the private fields make impossible to build (see
//! `relabel_graphics_handle.rs`).

#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::{DrawIndexed, GraphicsPipelineHandle, NoVertex};
use mltrs_renderer::renderer::render_graph::{DrawIndexedKey, draw_indexed};

use render_graph_api_checks::generated::shader_atlas::mesh::MeshParams;

fn wrong(
    pipeline: &GraphicsPipelineHandle<NoVertex, DrawIndexed>,
    params: &UniformBufferHandle<MeshParams>,
) {
    let _ = draw_indexed(DrawIndexedKey::from(pipeline), params, ());
}

fn main() {}
