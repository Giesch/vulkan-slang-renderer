#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::render_graph::*;
use render_graph_api_checks::generated::shader_atlas::push_compute::{
    ScaleParams, ScaleParamsBindings, ScalePush, ScalePushInput,
};

fn node<Node: GraphNode>(_: Node) {}

use render_graph_api_checks::generated::shader_atlas::plain_compute::PlainParams;

fn complete(
    key: ComputePipelineKey<PushBlock<ScalePush>>,
    params: &UniformBufferHandle<ScaleParams>,
    bindings: ScaleParamsBindings,
    push: ScalePushInput,
) {
    node(
        dispatch(key, params, [1, 1, 1])
            .with_param_bindings(bindings)
            .with_push_constant(push),
    );
    node(
        dispatch(key, params, [1, 1, 1])
            .with_push_constant(push)
            .with_param_bindings(bindings),
    );
}

fn no_bindings(
    key: ComputePipelineKey<NoPush>,
    push_key: ComputePipelineKey<PushBlock<ScalePush>>,
    params: &UniformBufferHandle<PlainParams>,
    push: ScalePushInput,
) {
    let plain: ComputeNode<PlainParams> = dispatch(key, params, [1, 1, 1]);
    node(plain);
    let with_push: ComputeNodeWithPush<PlainParams, ScalePush> =
        dispatch(push_key, params, [1, 1, 1]).with_push_constant(push);
    node(with_push);
}

fn main() {}
