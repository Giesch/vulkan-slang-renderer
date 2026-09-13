//! Negative control: a push completion requires the pipeline's own push
//! block. A push-constant pipeline whose block is `ScalePush` does not
//! accept `OtherPush` push input: the push interface is a type parameter of
//! the command, not an erasable detail.

#![allow(dead_code)]

use mltrs_renderer::renderer::StorageBufferHandle;
use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::{Compute, PipelineHandle, PushBlock};
use mltrs_renderer::renderer::render_graph::{StorageSlot, dispatch};

use render_graph_api_checks::generated::shader_atlas::particle::OtherElement;
use render_graph_api_checks::generated::shader_atlas::push_compute::{
    ScaleParams, ScaleParamsBindings, ScalePush,
};
use render_graph_api_checks::generated::shader_atlas::push_other_compute::OtherPushInput;

fn wrong(
    scale_push_pipeline: &PipelineHandle<Compute, PushBlock<ScalePush>>,
    scale_params: &UniformBufferHandle<ScaleParams>,
    items: &StorageBufferHandle<OtherElement>,
    push: OtherPushInput,
) {
    let items_addr = StorageSlot::from(items).addr();
    let _ = dispatch(scale_push_pipeline, scale_params, [4, 1, 1])
        .with_param_bindings(ScaleParamsBindings { items: items_addr })
        .with_push_constant(push);
}

fn main() {}
