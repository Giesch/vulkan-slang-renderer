//! Positive case for P2.1: nested node tuples compile, and their frame tuples
//! nest to match. Repeat and optional nodes compose inside the nesting with
//! their own frame contracts intact.
//!
//! Frame type:
//! `((SimParamsData, Vec<Particle>), ((LoopCount, TexParamsData), Option<Vec<Particle>>))`.

#![allow(dead_code)]

use glam::Vec2;
use mltrs_renderer::renderer::pipeline::{Compute, NoPush, PipelineHandle};
use mltrs_renderer::renderer::render_graph::{
    ComputeNode, GpuOnlySlot, GraphFormat, GraphNode, GraphResources, LoopCount, OptionalNode,
    RenderGraph, RepeatNode, UploadNode, dispatch, optional, repeat, upload,
};
use mltrs_renderer::renderer::{
    DrawError, FrameRenderer, GpuOnlyBufferHandle, StorageBufferHandle, UniformBufferHandle,
};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;
use render_graph_api_checks::generated::shader_atlas::sim_compute::{
    SimParams, SimParamsBindings, SimParamsData,
};
use render_graph_api_checks::generated::shader_atlas::tex_compute::{
    TexParams, TexParamsBindings, TexParamsData,
};

type Sim = ComputeNode<SimParams>;
type Graph = (
    (Sim, UploadNode<Particle>),
    (
        RepeatNode<ComputeNode<TexParams>>,
        OptionalNode<UploadNode<Particle>>,
    ),
);

// `RenderGraph::new` consumes the `GraphResources` the textures were declared
// in, so the caller owns it and the graph function borrows it.
fn graph(
    resources: &mut GraphResources,
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
    tex_pipeline: &PipelineHandle<Compute, NoPush>,
    tex_params: &UniformBufferHandle<TexParams>,
    points: &StorageBufferHandle<Particle>,
) -> Graph {
    let slots = GpuOnlySlot::from(particles);
    let height_in = resources.texture(8, 8, GraphFormat::R32Float);
    let height_out = resources.texture(8, 8, GraphFormat::R32Float);
    (
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
            upload(points),
        ),
        (
            repeat(dispatch(
                tex_pipeline,
                tex_params,
                [1, 1, 1],
                TexParamsBindings {
                    height_in: height_in.read(),
                    height_out: height_out.write(),
                },
            )),
            optional(upload(points)),
        ),
    )
}

fn frame_contract() {
    fn frame_is<
        N: GraphNode<
            Frame = (
                (SimParamsData, Vec<Particle>),
                ((LoopCount, TexParamsData), Option<Vec<Particle>>),
            ),
        >,
    >() {
    }
    frame_is::<Graph>();
}

fn execute(graph: &mut RenderGraph<Graph>, frame: FrameRenderer<'_>) -> Result<(), DrawError> {
    let points = || {
        vec![Particle {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
        }]
    };
    graph.execute(
        frame,
        &(
            (SimParamsData { delta_time: 0.016 }, points()),
            ((LoopCount(2), TexParamsData { dt: 0.016 }), Some(points())),
        ),
    )
}

fn main() {}
