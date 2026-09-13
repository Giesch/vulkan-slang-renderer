//! Negative control: a no-push pipeline's command takes no push input. The
//! push interface is part of the pipeline's type, so the completion method
//! does not exist on a no-push command.

#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::dispatch;

use render_graph_api_checks::generated::shader_atlas::plain_compute::PlainParams;
use render_graph_api_checks::generated::shader_atlas::push_compute::ScalePushInput;

fn wrong(
    plain_pipeline: &PipelineHandle<Compute, NoPush>,
    params_buffer: &UniformBufferHandle<PlainParams>,
    push: ScalePushInput,
) {
    let _ = dispatch(plain_pipeline, params_buffer, [4, 1, 1]).with_push_constant(push);
}

fn main() {}
