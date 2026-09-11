//! Negative case for P2.1: an optional frame must not compile without the
//! `Option` wrapper.
//!
//! Identical to `cases/positive/optional_frame.rs` except that `execute`
//! receives the body frame directly instead of `Some(body_frame)`. A bare
//! body input would weaken the completeness contract: it cannot express an
//! absent group.

#![allow(dead_code)]

use glam::Vec2;
use mltrs_renderer::renderer::render_graph::{
    OptionalNode, RenderGraph, UploadNode, optional, upload,
};
use mltrs_renderer::renderer::{DrawError, FrameRenderer, StorageBufferHandle};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;

type Graph = OptionalNode<UploadNode<Particle>>;

fn graph(points: &StorageBufferHandle<Particle>) -> Graph {
    optional(upload(points))
}

fn execute(graph: &mut RenderGraph<Graph>, frame: FrameRenderer<'_>) -> Result<(), DrawError> {
    graph.execute(
        frame,
        &vec![Particle {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
        }],
    )
}

fn main() {}
