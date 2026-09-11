//! Positive control for `cases/negative/repeat_frame_without_loop_count.rs`.
//!
//! A repeat node's frame is `(LoopCount, BodyFrame)`: the loop count is a
//! newtype, so it cannot swap with an adjacent scalar in the frame tuple.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, GpuOnlySlot, GraphNode, LoopCount, RenderGraph, RepeatNode, dispatch, repeat,
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
    let slots = GpuOnlySlot::from(particles);
    repeat(dispatch(
        sim_pipeline,
        sim_params,
        [1, 1, 1],
        SimParamsBindings {
            particles_in: slots.previous(),
            particles_out: slots.current(),
        },
    ))
}

fn frame_contract() {
    fn frame_is<N: GraphNode<Frame = (LoopCount, SimParamsData)>>() {}
    frame_is::<Graph>();
}

fn execute(graph: &mut RenderGraph<Graph>, frame: FrameRenderer<'_>) -> Result<(), DrawError> {
    graph.execute(frame, &(LoopCount(3), SimParamsData { delta_time: 0.016 }))
}

fn main() {}
