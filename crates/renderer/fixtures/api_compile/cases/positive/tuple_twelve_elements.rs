//! Positive case for P2.1: a twelve-element node tuple compiles, exercising
//! the tuple limit boundary. Each element keeps its own frame input, so the
//! frame tuple has twelve elements in node order.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, GpuOnlySlot, GraphNode, RenderGraph, dispatch,
};
use mltrs_renderer::renderer::{
    DrawError, FrameRenderer, GpuOnlyBufferHandle, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::sim_compute::{
    SimParams, SimParamsBindings, SimParamsData,
};

type Node = ComputeNode<SimParams>;
type Graph = (
    Node,
    Node,
    Node,
    Node,
    Node,
    Node,
    Node,
    Node,
    Node,
    Node,
    Node,
    Node,
);

fn graph(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
) -> Graph {
    let slots = GpuOnlySlot::from(particles);
    let bindings = SimParamsBindings {
        particles_in: slots.previous(),
        particles_out: slots.current(),
    };
    (
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
        dispatch(sim_pipeline, sim_params, [1, 1, 1], bindings),
    )
}

fn frame_contract() {
    fn frame_is<
        N: GraphNode<
            Frame = (
                SimParamsData,
                SimParamsData,
                SimParamsData,
                SimParamsData,
                SimParamsData,
                SimParamsData,
                SimParamsData,
                SimParamsData,
                SimParamsData,
                SimParamsData,
                SimParamsData,
                SimParamsData,
            ),
        >,
    >() {
    }
    frame_is::<Graph>();
}

fn execute(graph: &mut RenderGraph<Graph>, frame: FrameRenderer<'_>) -> Result<(), DrawError> {
    graph.execute(
        frame,
        &(
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
            SimParamsData { delta_time: 0.016 },
        ),
    )
}

fn main() {}
