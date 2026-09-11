//! Positive control for `cases/negative/wrong_buffer_element_type.rs`.
//!
//! Each buffer binding points at a buffer whose element type matches the
//! generated pointer field: `Particle` for `SimParamsBindings`, `OtherElement`
//! for `OtherParamsBindings`.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{ComputeNode, GpuOnlySlot, dispatch};
use mltrs_renderer::renderer::{GpuOnlyBufferHandle, UniformBufferHandle};

use render_graph_api_checks::generated::shader_atlas::other_compute::{
    OtherParams, OtherParamsBindings,
};
use render_graph_api_checks::generated::shader_atlas::particle::{OtherElement, Particle};
use render_graph_api_checks::generated::shader_atlas::sim_compute::{SimParams, SimParamsBindings};

fn nodes(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
    other_pipeline: &PipelineHandle<Compute, NoPush>,
    other_params: &UniformBufferHandle<OtherParams>,
    others: &GpuOnlyBufferHandle<OtherElement>,
) -> (ComputeNode<SimParams>, ComputeNode<OtherParams>) {
    let slots = GpuOnlySlot::from(particles);
    let other_slots = GpuOnlySlot::from(others);
    (
        dispatch(
            sim_pipeline,
            sim_params,
            [1, 1, 1],
            SimParamsBindings {
                particles_in: slots.previous(),
                particles_out: slots.current(),
            },
        ),
        dispatch(
            other_pipeline,
            other_params,
            [1, 1, 1],
            OtherParamsBindings {
                items: other_slots.current(),
            },
        ),
    )
}

fn main() {}
