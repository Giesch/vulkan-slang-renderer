//! Positive control for `cases/negative/omitted_tuple_element.rs`.
//!
//! A two-node graph (one dispatch, one draw) executes with its complete
//! two-element frame tuple. No case constructs a `Renderer`: the functions
//! take renderer handles as parameters and are type-checked, never run.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, DrawIndexed, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, DrawNode, GpuOnlySlot, RenderGraph, dispatch, draw_indexed,
};
use mltrs_renderer::renderer::{
    DrawError, FrameRenderer, GpuOnlyBufferHandle, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::render::{
    RenderParams, RenderParamsBindings, RenderParamsData,
};
use render_graph_api_checks::generated::shader_atlas::sim_compute::{
    SimParams, SimParamsBindings, SimParamsData,
};

type Graph = (ComputeNode<SimParams>, DrawNode<RenderParams>);

fn graph(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
    render_pipeline: &PipelineHandle<DrawIndexed, NoPush>,
    render_params: &UniformBufferHandle<RenderParams>,
) -> Graph {
    let slots = GpuOnlySlot::from(particles);
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
        draw_indexed(
            render_pipeline,
            render_params,
            RenderParamsBindings {
                particles: slots.current().into(),
            },
        ),
    )
}

fn execute(graph: &mut RenderGraph<Graph>, frame: FrameRenderer<'_>) -> Result<(), DrawError> {
    graph.execute(
        frame,
        &(
            SimParamsData { delta_time: 0.016 },
            RenderParamsData { particle_count: 4 },
        ),
    )
}

fn main() {}
