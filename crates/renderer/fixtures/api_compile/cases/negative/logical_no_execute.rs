//! Negative control: a logical `RenderGraph` has no `execute` — only the
//! prepared type does.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, ComputePipelineKey, ResourcePlanner, RenderGraph, dispatch,
};
use mltrs_renderer::renderer::{GpuOnlyBufferHandle, GpuOnlySlot, UniformBufferHandle};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::sim_compute::{
    SimParams, SimParamsBindings, SimParamsData,
};

fn wrong(
    graph: &mut RenderGraph<(ComputeNode<SimParams>,)>,
    frame: mltrs_renderer::renderer::FrameRenderer<'_>,
) -> Result<(), mltrs_renderer::renderer::DrawError> {
    graph.execute(frame, &(SimParamsData { delta_time: 0.016 },))
}

#[allow(unused_variables)]
fn build(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
) -> RenderGraph<(ComputeNode<SimParams>,)> {
    let slots = GpuOnlySlot::from(particles);
    RenderGraph::new(
        ResourcePlanner::new(),
        (dispatch(
            ComputePipelineKey::from(sim_pipeline),
            sim_params,
            [4, 1, 1],
        )
        .with_param_bindings(SimParamsBindings {
            particles_in: slots.previous(),
            particles_out: slots.current(),
        }),),
    )
    .unwrap()
}

fn main() {}
