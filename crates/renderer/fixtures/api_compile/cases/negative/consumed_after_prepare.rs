//! Negative control: `prepare` consumes the logical graph, so the same
//! graph cannot be prepared twice.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, ComputePipelineKey, RenderGraph, ResourcePlanner, dispatch,
};
use mltrs_renderer::renderer::{GpuUpdatedBufferHandle, GpuUpdatedSlot, Renderer, UniformBufferHandle};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::sim_compute::{SimParams, SimParamsBindings};

fn wrong(graph: RenderGraph<(ComputeNode<SimParams>,)>, renderer: &mut Renderer) {
    let _first = graph.prepare(renderer).unwrap();
    let _second = graph.prepare(renderer).unwrap();
}

#[allow(unused_variables)]
fn build(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuUpdatedBufferHandle<Particle>,
) -> RenderGraph<(ComputeNode<SimParams>,)> {
    let slots = GpuUpdatedSlot::from(particles);
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
