#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::NoPush;
use mltrs_renderer::renderer::render_graph::*;
use render_graph_api_checks::generated::shader_atlas::push_compute::ScaleParams;

fn node<Node: GraphNode>(_: Node) {}

fn wrong(key: ComputePipelineKey<NoPush>, params: &UniformBufferHandle<ScaleParams>) {
    node(dispatch(key, params, [1, 1, 1]));
}

fn main() {}
