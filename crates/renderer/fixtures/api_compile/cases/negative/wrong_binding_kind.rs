//! Negative case for P2.1: a wrong binding kind must not compile.
//!
//! Identical to `cases/positive/right_binding_kind.rs` except that the sampled
//! field `height_in` takes a storage-image binding (`height_in.write()`)
//! instead of a sampled one (`height_in.read()`).

#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, ComputePipelineKey, GraphFormat, ResourcePlanner, dispatch,
};

use render_graph_api_checks::generated::shader_atlas::tex_compute::{TexParams, TexParamsBindings};

fn node(
    resources: &mut ResourcePlanner,
    tex_pipeline: &PipelineHandle<Compute, NoPush>,
    tex_params: &UniformBufferHandle<TexParams>,
) -> ComputeNode<TexParams> {
    let tex_pipeline_key = ComputePipelineKey::from(tex_pipeline);

    let height_in = resources.texture("height_in", 8, 8, GraphFormat::R32Float);
    let height_out = resources.texture("height_out", 8, 8, GraphFormat::R32Float);

    dispatch(tex_pipeline_key, tex_params, [1, 1, 1]).with_param_bindings(TexParamsBindings {
        height_in: height_in.write(),
        height_out: height_out.write(),
    })
}

fn main() {}
