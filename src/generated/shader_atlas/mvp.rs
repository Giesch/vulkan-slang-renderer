// GENERATED FILE (do not edit directly)

//! shared types from slang module: mvp.slang

use serde::Serialize;

use crate::renderer::gpu_write::GPUWrite;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Serialize)]
#[repr(C, align(16))]
pub struct MVPMatrices {
    pub model: glam::Mat4,
    pub view: glam::Mat4,
    pub proj: glam::Mat4,
}

impl GPUWrite for MVPMatrices {}
const _: () = assert!(std::mem::size_of::<MVPMatrices>() == 192);
const _: () = assert!(std::mem::offset_of!(MVPMatrices, model) == 0);
const _: () = assert!(std::mem::size_of::<glam::Mat4>() == 64);
const _: () = assert!(std::mem::offset_of!(MVPMatrices, view) == 64);
const _: () = assert!(std::mem::size_of::<glam::Mat4>() == 64);
const _: () = assert!(std::mem::offset_of!(MVPMatrices, proj) == 128);
const _: () = assert!(std::mem::size_of::<glam::Mat4>() == 64);
