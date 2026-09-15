//! Positive control for the logical/prepared lifecycle: `RenderGraph::new`
//! is GPU-free, `prepare` consumes the logical graph against a renderer,
//! and only the prepared type has `execute`.
//!
//! No case constructs a `Renderer`: the functions take one as a parameter
//! and are type-checked, never run.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, DrawVertexCount, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, ComputePipelineKey, DrawVertexCountKey, DrawVertexCountNode, PreparedRenderGraph,
    RenderGraph, ResourcePlanner, dispatch, draw_vertex_count,
};
use mltrs_renderer::renderer::{
    DrawError, FrameRenderer, GpuOnlyBufferHandle, GpuOnlySlot, Renderer, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::render::{
    RenderParams, RenderParamsBindings, RenderParamsData,
};
use render_graph_api_checks::generated::shader_atlas::sim_compute::{
    SimParams, SimParamsBindings, SimParamsData,
};

pub type Graph = (ComputeNode<SimParams>, DrawVertexCountNode<RenderParams>);

/// Logical construction needs no renderer at all.
fn logical_construction(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
    render_pipeline: &PipelineHandle<DrawVertexCount, NoPush>,
    render_params: &UniformBufferHandle<RenderParams>,
) -> RenderGraph<Graph> {
    let slots = GpuOnlySlot::from(particles);
    RenderGraph::new(
        ResourcePlanner::new(),
        (
            dispatch(
                ComputePipelineKey::from(sim_pipeline),
                sim_params,
                [4, 1, 1],
            )
            .with_param_bindings(SimParamsBindings {
                particles_in: slots.previous(),
                particles_out: slots.current(),
            }),
            draw_vertex_count(
                DrawVertexCountKey::from(render_pipeline),
                render_params,
                6,
                RenderParamsBindings {
                    particles: slots.current().into(),
                },
            ),
        ),
    )
    .unwrap()
}

/// Preparation consumes the logical graph and returns the only executable
/// type.
fn preparation(graph: RenderGraph<Graph>, renderer: &mut Renderer) -> PreparedRenderGraph<Graph> {
    graph.prepare(renderer).unwrap()
}

/// Only the prepared graph exposes `execute`.
fn execute(
    prepared: &mut PreparedRenderGraph<Graph>,
    frame: FrameRenderer<'_>,
) -> Result<(), DrawError> {
    prepared.execute(
        frame,
        &(
            SimParamsData { delta_time: 0.016 },
            RenderParamsData { particle_count: 4 },
        ),
    )
}

fn main() {}
