//! Backend-neutral render-graph vocabulary and GPU ABI types.
extern crate self as mltrs_render_graph;

/// Marker for GPU data elements and generated shader parameter blocks.
/// This does not prove that Rust padding is initialized.
pub trait GPUWrite {}

/// A generated shader push-constant block.
pub trait PushConstantBlock: GPUWrite {}

impl GPUWrite for u8 {}
impl GPUWrite for f32 {}
impl GPUWrite for u32 {}

pub mod addr;
pub mod backend;
pub mod bindless;
pub mod commands;
mod runtime;
pub use runtime::*;
