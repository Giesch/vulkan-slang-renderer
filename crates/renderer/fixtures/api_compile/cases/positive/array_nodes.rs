//! Positive case: an array of one node type is a node. Its length is free of
//! the twelve-element tuple limit, its frame is an array of the same length,
//! and it composes inside a tuple.
//!
//! Frame type: `([SimParamsData; 13], Vec<Particle>)`.

#![allow(dead_code)]

use glam::Vec2;
use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, ComputePipelineKey, GpuOnlySlot, GraphNode, PreparedRenderGraph, StorageSlot,
    UploadNode, dispatch, upload,
};
use mltrs_renderer::renderer::{
    DrawError, FrameRenderer, GpuOnlyBufferHandle, StorageBufferHandle, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::sim_compute::{
    SimParams, SimParamsBindings, SimParamsData,
};

const SIM_COUNT: usize = 13;

type Sims = [ComputeNode<SimParams>; SIM_COUNT];
type Graph = (Sims, UploadNode<Particle>);

fn graph(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
    points: &StorageBufferHandle<Particle>,
) -> Graph {
    let sim_pipeline_key = ComputePipelineKey::from(sim_pipeline);
    let slots = GpuOnlySlot::from(particles);
    let bindings = SimParamsBindings {
        particles_in: slots.previous(),
        particles_out: slots.current(),
    };

    (
        std::array::from_fn(|_| {
            dispatch(sim_pipeline_key, sim_params, [1, 1, 1]).with_param_bindings(bindings)
        }),
        upload(StorageSlot::from(points)),
    )
}

fn frame_contract() {
    fn frame_is<N: GraphNode<Frame = ([SimParamsData; SIM_COUNT], Vec<Particle>)>>() {}
    frame_is::<Graph>();
}

fn execute(
    prepared: &mut PreparedRenderGraph<Graph>,
    frame: FrameRenderer<'_>,
) -> Result<(), DrawError> {
    let points = vec![Particle {
        position: Vec2::ZERO,
        velocity: Vec2::ZERO,
    }];

    prepared.execute(
        frame,
        &(
            std::array::from_fn(|_| SimParamsData { delta_time: 0.016 }),
            points,
        ),
    )
}

fn main() {}
