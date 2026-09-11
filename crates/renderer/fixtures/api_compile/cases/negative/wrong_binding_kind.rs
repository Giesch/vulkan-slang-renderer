//! Negative case for P2.1: a wrong binding kind must not compile.
//!
//! Identical to `cases/positive/right_binding_kind.rs` except that the sampled
//! field `height_in` takes a storage-image binding (`height_in.write()`)
//! instead of a sampled one (`height_in.read()`).

#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{ComputeNode, GraphFormat, GraphResources, dispatch};

use render_graph_api_checks::generated::shader_atlas::tex_compute::{TexParams, TexParamsBindings};

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
            height_in: height_in.write(),
            height_out: height_out.write(),
        },
    )
}

fn main() {}
