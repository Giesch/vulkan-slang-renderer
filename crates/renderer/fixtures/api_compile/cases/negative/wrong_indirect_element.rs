//! Negative control: indirect draw arguments must be an immutable buffer of
//! exactly `DrawIndexedIndirectCommand`; the element type is part of the
//! slot's type.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{GraphicsIndirect, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{GraphicsIndirectPipelineKey, draw_indexed_indirect};
use mltrs_renderer::renderer::{
    GpuUpdatedBufferHandle, GpuUpdatedSlot, ImmutableBufferHandle, IndexBufferHandle,
    UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::{OtherElement, Particle};
use render_graph_api_checks::generated::shader_atlas::render::{
    RenderParams, RenderParamsBindings,
};

fn wrong(
    indirect_pipeline: &PipelineHandle<GraphicsIndirect, NoPush>,
    indices: &IndexBufferHandle,
    indirect_params: &UniformBufferHandle<RenderParams>,
    particles: &GpuUpdatedBufferHandle<Particle>,
    args: &ImmutableBufferHandle<OtherElement>,
) {
    let _ = draw_indexed_indirect(
        GraphicsIndirectPipelineKey::from(indirect_pipeline),
        indices,
        indirect_params,
        args,
        0,
        1,
        RenderParamsBindings {
            particles: GpuUpdatedSlot::from(particles).current().into(),
        },
    );
}

fn main() {}
