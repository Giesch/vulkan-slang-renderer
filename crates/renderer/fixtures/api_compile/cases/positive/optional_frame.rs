//! Positive control for `cases/negative/optional_frame_without_option.rs`.
//!
//! An optional node's frame is `Option<BodyFrame>`: presence is the input, so
//! an absent group carries no partial body data.

#![allow(dead_code)]

use glam::Vec2;
use mltrs_renderer::renderer::render_graph::{
    GraphNode, OptionalNode, PreparedRenderGraph, RenderGraph, StorageSlot, UploadNode, optional,
    upload,
};
use mltrs_renderer::renderer::{DrawError, FrameRenderer, StorageBufferHandle};

use render_graph_api_checks::generated::shader_atlas::particle::Particle;

type Graph = OptionalNode<UploadNode<Particle>>;

fn graph(points: &StorageBufferHandle<Particle>) -> Graph {
    optional(upload(StorageSlot::from(points)))
}

fn frame_contract() {
    fn frame_is<N: GraphNode<Frame = Option<Vec<Particle>>>>() {}
    frame_is::<Graph>();
}

fn execute(
    prepared: &mut PreparedRenderGraph<Graph>,
    frame: FrameRenderer<'_>,
) -> Result<(), DrawError> {
    prepared.execute(
        frame,
        &Some(vec![Particle {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
        }]),
    )
}

fn main() {}
