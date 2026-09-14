#[path = "../support.rs"]
mod support;
use mltrs_render_graph::*;
use support::*;
fn wrong(renderer: &mut mltrs_renderer::renderer::Renderer) {
    let _ = RenderGraph::new(ResourcePlanner::new(), node::<OtherCommand>())
        .unwrap()
        .prepare(renderer);
}
fn main() {}
