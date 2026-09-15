#![allow(dead_code)]
#[path = "../support.rs"]
mod support;
use mltrs_render_graph::{backend::FrameBackend, *};
use support::*;
fn wrong<F: FrameBackend<Backend = OtherBackend>>(
    prepared: &mut PreparedRenderGraph<
        IndirectDrawNode<
            Params,
            (),
            mltrs_renderer::renderer::render_graph::DrawIndexedIndirectCommand,
        >,
        mltrs_renderer::renderer::Renderer,
    >,
    frame: F,
) {
    let _ = prepared.execute(frame, &());
}
fn main() {}
