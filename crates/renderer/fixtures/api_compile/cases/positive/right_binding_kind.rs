//! Positive control for `cases/negative/wrong_binding_kind.rs`.
//!
//! A dispatch node whose texture bindings match the generated field kinds:
//! a sampled read for `height_in`, a storage write for `height_out`.

#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, ComputePipelineKey, GraphFormat, ResourcePlanner, dispatch,
};

use render_graph_api_checks::generated::shader_atlas::tex_compute::{TexParams, TexParamsBindings};

// `RenderGraph::new` consumes the `ResourcePlanner` the textures were declared
// in, so the caller owns it and the node function borrows it.
fn node(
    resources: &mut ResourcePlanner,
    tex_pipeline: &PipelineHandle<Compute, NoPush>,
    tex_params: &UniformBufferHandle<TexParams>,
) -> ComputeNode<TexParams> {
    let tex_pipeline_key = ComputePipelineKey::from(tex_pipeline);

    let height_in = resources.texture("height_in", 8, 8, GraphFormat::R32Float);
    let height_out = resources.texture("height_out", 8, 8, GraphFormat::R32Float);

    dispatch(tex_pipeline_key, tex_params, [1, 1, 1]).with_param_bindings(TexParamsBindings {
        height_in: height_in.read(),
        height_out: height_out.write(),
    })
}

fn main() {}
