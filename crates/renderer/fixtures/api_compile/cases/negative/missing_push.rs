//! Negative control: a command built from a push-constant pipeline cannot
//! enter a graph until its complete push input is attached —
//! `ComputeNode<S, PendingPush<B>>` is not a `GraphNode`.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, PipelineHandle, PushBlock};
use mltrs_renderer::renderer::render_graph::{
    ComputePipelineKey, ResourcePlanner, RenderGraph, StorageSlot, dispatch,
};
use mltrs_renderer::renderer::{StorageBufferHandle, UniformBufferHandle};

use render_graph_api_checks::generated::shader_atlas::particle::OtherElement;
use render_graph_api_checks::generated::shader_atlas::push_compute::{
    ScaleParams, ScaleParamsBindings, ScalePush,
};

fn wrong(
    push_pipeline: &PipelineHandle<Compute, PushBlock<ScalePush>>,
    push_params: &UniformBufferHandle<ScaleParams>,
    items: &StorageBufferHandle<OtherElement>,
) {
    let graph = RenderGraph::new(
        ResourcePlanner::new(),
        (dispatch(
            ComputePipelineKey::from(push_pipeline),
            push_params,
            [4, 1, 1],
        )
        .with_param_bindings(ScaleParamsBindings {
            items: StorageSlot::from(items).addr(),
        }),),
    )
    .unwrap();
    let _ = graph;
}

fn main() {}
