//! Negative case for P2.1: a repeat frame must not compile without
//! `LoopCount`.
//!
//! Identical to `cases/positive/repeat_frame.rs` except that the iteration
//! count is a bare `u32` instead of a `LoopCount`.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, ComputePipelineKey, GpuOnlySlot, PreparedRenderGraph, RepeatNode, dispatch, repeat,
};
use mltrs_renderer::renderer::{
    DrawError, FrameRenderer, GpuOnlyBufferHandle, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::sim_compute::{
    SimParams, SimParamsBindings, SimParamsData,
};

type Graph = RepeatNode<ComputeNode<SimParams>>;

fn graph(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
) -> Graph {
    let sim_pipeline_key = ComputePipelineKey::from(sim_pipeline);

    let slots = GpuOnlySlot::from(particles);

    repeat(
        dispatch(sim_pipeline_key, sim_params, [1, 1, 1]).with_param_bindings(SimParamsBindings {
            particles_in: slots.previous(),
            particles_out: slots.current(),
        }),
    )
}

fn execute(
    prepared: &mut PreparedRenderGraph<Graph>,
    frame: FrameRenderer<'_>,
) -> Result<(), DrawError> {
    prepared.execute(frame, &(3, SimParamsData { delta_time: 0.016 }))
}

fn main() {}
