//! Positive control for the trait identities and their one-way direction:
//! generated types implement the graph traits and reach the renderer traits
//! through the blankets without naming the private renderer traits;
//! the graph primitives (`u8`/`f32`/`u32`) reach both sides.

#![allow(dead_code)]

use mltrs_renderer::renderer::Renderer;
use mltrs_renderer::renderer::render_graph;

fn renderer_path<T: render_graph::GPUWrite>(renderer: &mut Renderer, _: &T) {
    let _ = renderer.create_uniform_buffer::<T>();
}

fn graph_path<T: render_graph::GPUWrite>(_: &T) {}

fn renderer_push_path<T: render_graph::PushConstantBlock>(_: &T) {
    let _: Option<mltrs_renderer::renderer::PushBlock<T>> = None;
}

fn graph_push_path<T: render_graph::PushConstantBlock>(_: &T) {}

fn use_all(
    renderer: &mut Renderer,
    sim_params: &render_graph_api_checks::generated::shader_atlas::sim_compute::SimParams,
    push_block: &render_graph_api_checks::generated::shader_atlas::push_compute::ScalePush,
) {
    // graph impls cover the renderer traits through the one-way blankets
    renderer_path(renderer, sim_params);
    graph_path(sim_params);
    renderer_push_path(push_block);
    graph_push_path(push_block);

    // the primitives implement the graph traits, so they reach the renderer
    // side through the same blankets
    renderer_path(renderer, &0u8);
    renderer_path(renderer, &0.0f32);
    renderer_path(renderer, &0u32);
    graph_path(&0u32);
}

fn main() {}
