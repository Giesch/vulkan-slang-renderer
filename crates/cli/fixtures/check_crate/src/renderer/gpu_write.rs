//! These are stubs of renderer::gpu_write types
//! that generated shaders need to refer to.

pub enum NoVertex {}

pub trait GPUWrite {}

pub trait PushConstantBlock: GPUWrite {}

impl GPUWrite for NoVertex {}
