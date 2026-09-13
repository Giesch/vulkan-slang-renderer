//! Negative control: a vertex-count pipeline key cannot drive an indexed
//! draw. Every pipeline family has its own graph key type, so the mismatch
//! is a compile error at the constructor, not a runtime surprise.
//! (`draw_index_range` takes the same `DrawIndexedKey` family and gets its
//! coverage through this same type distinction.)

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{DrawVertexCount, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{DrawVertexCountKey, draw_indexed};
use mltrs_renderer::renderer::{GpuOnlyBufferHandle, GpuOnlySlot, UniformBufferHandle};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::render::{
    RenderParams, RenderParamsBindings,
};

fn wrong(
    render_pipeline: &PipelineHandle<DrawVertexCount, NoPush>,
    render_params: &UniformBufferHandle<RenderParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
) {
    let _ = draw_indexed(
        DrawVertexCountKey::from(render_pipeline),
        render_params,
        RenderParamsBindings {
            particles: GpuOnlySlot::from(particles).current().into(),
        },
    );
}

fn main() {}
