// GENERATED FILE (do not edit directly)

//! shared types from slang module: particle.slang

use serde::Serialize;

use crate::renderer::gpu_write::GPUWrite;

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct Particle {
    pub position: glam::Vec2,
    pub velocity: glam::Vec2,
    pub color: glam::Vec4,
}

impl GPUWrite for Particle {}
const _: () = assert!(std::mem::size_of::<Particle>() == 32);
