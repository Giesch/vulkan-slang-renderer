// GENERATED FILE (do not edit directly)

//! shared types from slang module: ray_march_camera.slang

use serde::Serialize;

use super::projection::Projection;
use crate::renderer::gpu_write::GPUWrite;

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct RayMarchCamera {
    pub inverse_view_proj: Projection,
    pub position: glam::Vec3,
}

impl GPUWrite for RayMarchCamera {}
