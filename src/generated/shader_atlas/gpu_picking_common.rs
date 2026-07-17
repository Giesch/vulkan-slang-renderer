// GENERATED FILE (do not edit directly)

//! shared types from slang module: gpu_picking_common.slang

use serde::Serialize;

use crate::renderer::gpu_write::GPUWrite;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct Cube {
    pub position: glam::Vec3,
    pub _padding_0: [u8; 4],
    pub radii: glam::Vec3,
    pub _padding_1: [u8; 4],
}

impl GPUWrite for Cube {}
const _: () = assert!(std::mem::size_of::<Cube>() == 32);
const _: () = assert!(std::mem::offset_of!(Cube, position) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec3>() == 12);
const _: () = assert!(std::mem::offset_of!(Cube, radii) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec3>() == 12);
