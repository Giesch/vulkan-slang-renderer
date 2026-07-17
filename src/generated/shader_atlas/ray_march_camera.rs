// GENERATED FILE (do not edit directly)

//! shared types from slang module: ray_march_camera.slang

use serde::Serialize;

use super::projection::Projection;
use crate::renderer::gpu_write::GPUWrite;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct RayMarchCamera {
    pub inverse_view_proj: Projection,
    pub position: glam::Vec3,
    pub _padding_0: [u8; 4],
}

impl GPUWrite for RayMarchCamera {}
const _: () = assert!(std::mem::size_of::<RayMarchCamera>() == 80);
const _: () = assert!(std::mem::offset_of!(RayMarchCamera, inverse_view_proj) == 0);
const _: () = assert!(std::mem::size_of::<Projection>() == 64);
const _: () = assert!(std::mem::offset_of!(RayMarchCamera, position) == 64);
const _: () = assert!(std::mem::size_of::<glam::Vec3>() == 12);
