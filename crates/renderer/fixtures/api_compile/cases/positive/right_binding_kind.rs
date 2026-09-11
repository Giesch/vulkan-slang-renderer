//! Positive control for `cases/negative/wrong_binding_kind.rs`.
//!
//! A dispatch node whose texture bindings match the generated field kinds:
//! a sampled read for `height_in`, a storage write for `height_out`.

#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{ComputeNode, GraphFormat, GraphResources, dispatch};

use render_graph_api_checks::generated::shader_atlas::tex_compute::{TexParams, TexParamsBindings};

// `RenderGraph::new` consumes the `GraphResources` the textures were declared
// in, so the caller owns it and the node function borrows it.
fn node(
    resources: &mut GraphResources,
    tex_pipeline: &PipelineHandle<Compute, NoPush>,
    tex_params: &UniformBufferHandle<TexParams>,
) -> ComputeNode<TexParams> {
    let height_in = resources.texture(8, 8, GraphFormat::R32Float);
    let height_out = resources.texture(8, 8, GraphFormat::R32Float);
    dispatch(
        tex_pipeline,
        tex_params,
        [1, 1, 1],
        TexParamsBindings {
            height_in: height_in.read(),
            height_out: height_out.write(),
        },
    )
}

fn main() {}
