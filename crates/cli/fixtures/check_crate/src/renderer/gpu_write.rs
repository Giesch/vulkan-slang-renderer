//! These are stubs of renderer::gpu_write types
//! that generated shaders need to refer to.

pub enum NoVertex {}

/// Stub of the renderer's GPU-data marker. The stub render_graph module
/// defines the graph-local `GPUWrite`/`PushConstantBlock` traits that
/// generated code implements; the one-way blanket impls below mirror the
/// real renderer's bridges from those traits onto these ones.
pub(crate) trait GPUWrite {}

pub(crate) trait PushConstantBlock: GPUWrite {}

impl GPUWrite for NoVertex {}

// one-way bridges, mirroring the real renderer
impl<T: crate::renderer::render_graph::GPUWrite> GPUWrite for T {}
impl<T: crate::renderer::render_graph::PushConstantBlock> PushConstantBlock for T {}
