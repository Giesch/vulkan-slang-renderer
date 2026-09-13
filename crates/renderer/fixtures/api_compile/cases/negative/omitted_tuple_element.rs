//! Negative case for P2.1: an omitted frame tuple element must not compile.
//!
//! Identical to `cases/positive/complete_tuple.rs` except that the frame tuple
//! passed to `execute` omits the draw node's element. The failure is the
//! omitted element: `RenderParamsData` appears only in the expected frame
//! type, never in the tuple the call passes.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{Compute, DrawIndexed, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, ComputePipelineKey, DrawIndexedKey, DrawNode, GpuOnlySlot, PreparedRenderGraph,
    dispatch, draw_indexed,
};
use mltrs_renderer::renderer::{
    DrawError, FrameRenderer, GpuOnlyBufferHandle, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::render::{
    RenderParams, RenderParamsBindings,
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
    let sim_pipeline_key = ComputePipelineKey::from(sim_pipeline);
    let render_pipeline_key = DrawIndexedKey::from(render_pipeline);

    let slots = GpuOnlySlot::from(particles);

    (
        dispatch(sim_pipeline_key, sim_params, [1, 1, 1]).with_param_bindings(SimParamsBindings {
            particles_in: slots.previous(),
            particles_out: slots.current(),
        }),
        draw_indexed(
            render_pipeline_key,
            render_params,
            RenderParamsBindings {
                particles: slots.current().into(),
            },
        ),
    )
}

fn execute(
    prepared: &mut PreparedRenderGraph<Graph>,
    frame: FrameRenderer<'_>,
) -> Result<(), DrawError> {
    prepared.execute(frame, &(SimParamsData { delta_time: 0.016 },))
}

fn main() {}
