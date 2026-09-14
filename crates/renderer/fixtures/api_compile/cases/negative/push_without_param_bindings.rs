#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::render_graph::*;
use render_graph_api_checks::generated::shader_atlas::push_compute::{
    ScaleParams, ScalePush, ScalePushInput,
};

fn node<Node: GraphNode>(_: Node) {}

fn wrong(
    key: ComputePipelineKey<PushBlock<ScalePush>>,
    params: &UniformBufferHandle<ScaleParams>,
    push: ScalePushInput,
) {
    node(dispatch(key, params, [1, 1, 1]).with_push_constant(push));
}

fn main() {}
