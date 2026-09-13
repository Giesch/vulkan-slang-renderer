//! Plain Rust graph commands must be converted to renderer records before upload.
#![allow(dead_code)]

use mltrs_renderer::renderer::Renderer;
use mltrs_renderer::renderer::render_graph::DrawIndexedIndirectCommand;

fn wrong(renderer: &mut Renderer, commands: &[DrawIndexedIndirectCommand]) {
    let _ = renderer.create_immutable_buffer::<DrawIndexedIndirectCommand>(commands.len() as u32);
}

fn main() {}
