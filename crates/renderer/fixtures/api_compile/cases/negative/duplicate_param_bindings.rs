#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::NoPush;
use mltrs_renderer::renderer::render_graph::*;
use render_graph_api_checks::generated::shader_atlas::push_compute::{
    ScaleParams, ScaleParamsBindings,
};

fn node<Node: GraphNode>(_: Node) {}

fn wrong(
    key: ComputePipelineKey<NoPush>,
    params: &UniformBufferHandle<ScaleParams>,
    bindings: ScaleParamsBindings,
) {
    let _ = dispatch(key, params, [1, 1, 1])
        .with_param_bindings(bindings)
        .with_param_bindings(bindings);
}

fn main() {}
