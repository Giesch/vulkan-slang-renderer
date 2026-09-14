#![allow(dead_code)]
#[path = "../support.rs"]
mod support;
use mltrs_render_graph::{backend::IndexedIndirectArgs, *};
use mltrs_renderer::renderer::{Renderer, render_graph::DrawIndexedIndirectCommand};
use support::*;
fn accepts_accessors<T: IndexedIndirectArgs>() {}
fn same(renderer: &mut Renderer, other: &mut OtherBackend) {
    accepts_accessors::<DrawIndexedIndirectCommand>();
    accepts_accessors::<OtherCommand>();
    let _ = RenderGraph::new(ResourcePlanner::new(), node::<DrawIndexedIndirectCommand>())
        .unwrap()
        .prepare(renderer);
    let _ = RenderGraph::new(
        ResourcePlanner::new(),
        (optional(repeat((node::<DrawIndexedIndirectCommand>(),))),),
    )
    .unwrap()
    .prepare(renderer);
    let _ = RenderGraph::new(ResourcePlanner::new(), node::<OtherCommand>())
        .unwrap()
        .prepare(other);
    let _ = RenderGraph::new(
        ResourcePlanner::new(),
        (optional(repeat((node::<OtherCommand>(),))),),
    )
    .unwrap()
    .prepare(other);
}
fn main() {}
