/// Marker type for shaders that don't use vertex input buffers.
pub enum NoVertex {}

pub trait GPUWrite {}

impl GPUWrite for NoVertex {}
