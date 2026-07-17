// GENERATED FILE (do not edit directly)

//! shared types from slang module: particle.slang

use serde::Serialize;

use crate::renderer::gpu_write::GPUWrite;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct Particle {
    pub position: glam::Vec2,
    pub velocity: glam::Vec2,
    pub color: glam::Vec4,
}

impl GPUWrite for Particle {}
const _: () = assert!(std::mem::size_of::<Particle>() == 32);
const _: () = assert!(std::mem::offset_of!(Particle, position) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(Particle, velocity) == 8);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);
const _: () = assert!(std::mem::offset_of!(Particle, color) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
