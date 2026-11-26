//! Negative control: a direct graphics pipeline key cannot drive an
//! indexed-indirect draw. Every pipeline family has its own graph key type,
//! so the mismatch is a compile error at the constructor, not a runtime
//! surprise. (`draw_indexed` and `draw_index_range` take the same
//! `GraphicsPipelineKey` family and get their coverage through this same type
//! distinction.)

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Graphics, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{GraphicsPipelineKey, draw_indexed_indirect};
use mltrs_renderer::renderer::{
    DrawIndexedIndirectCommand, GpuUpdatedBufferHandle, GpuUpdatedSlot, ImmutableBufferHandle,
    IndexBufferHandle, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::render::{
    RenderParams, RenderParamsBindings,
};

fn wrong(
    render_pipeline: &PipelineHandle<Graphics, NoPush>,
    indices: &IndexBufferHandle,
    render_params: &UniformBufferHandle<RenderParams>,
    particles: &GpuUpdatedBufferHandle<Particle>,
    args: &ImmutableBufferHandle<DrawIndexedIndirectCommand>,
) {
    let _ = draw_indexed_indirect(
        GraphicsPipelineKey::from(render_pipeline),
        indices,
        render_params,
        args,
        0,
        1,
        RenderParamsBindings {
            particles: GpuUpdatedSlot::from(particles).current().into(),
        },
    );
}

fn main() {}
