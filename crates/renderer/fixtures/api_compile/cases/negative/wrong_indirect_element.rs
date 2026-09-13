//! Negative control: indirect draw arguments must be an immutable buffer of
//! exactly `DrawIndexedIndirectCommand`; the element type is part of the
//! slot's type.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{DrawIndexedIndirect, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{DrawIndexedIndirectKey, draw_indexed_indirect};
use mltrs_renderer::renderer::{
    GpuOnlyBufferHandle, GpuOnlySlot, ImmutableBufferHandle, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::{OtherElement, Particle};
use render_graph_api_checks::generated::shader_atlas::render::{
    RenderParams, RenderParamsBindings,
};

fn wrong(
    indirect_pipeline: &PipelineHandle<DrawIndexedIndirect, NoPush>,
    indirect_params: &UniformBufferHandle<RenderParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
    args: &ImmutableBufferHandle<OtherElement>,
) {
    let _ = draw_indexed_indirect(
        DrawIndexedIndirectKey::from(indirect_pipeline),
        indirect_params,
        args,
        0,
        1,
        RenderParamsBindings {
            particles: GpuOnlySlot::from(particles).current().into(),
        },
    );
}

fn main() {}
