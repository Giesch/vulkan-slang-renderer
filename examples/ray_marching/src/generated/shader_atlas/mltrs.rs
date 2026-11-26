// GENERATED FILE (do not edit directly)

//! shared types from slang module: mltrs.slang

use serde::Serialize;

#[allow(unused_imports)]
use mltrs::renderer::gpu_read::{GPURead, GpuByteLengthMismatch, GpuReadErrors, InvalidGpuEnumTag};
#[allow(unused_imports)]
use mltrs::renderer::render_graph::GPUWrite;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct Projection {
    pub matrix: glam::Mat4,
}

impl GPUWrite for Projection {}
const _: () = assert!(std::mem::size_of::<Projection>() == 64);
const _: () = assert!(std::mem::offset_of!(Projection, matrix) == 0);
const _: () = assert!(std::mem::size_of::<glam::Mat4>() == 64);

impl GPURead for Projection {
    const GPU_SIZE: usize = 64;

    fn read_gpu(bytes: &[u8]) -> eros::Result<Self, GpuReadErrors> {
        if bytes.len() != Self::GPU_SIZE {
            return eros::IntoUnion::union(Err(GpuByteLengthMismatch {
                type_name: "Projection",
                expected: Self::GPU_SIZE,
                actual: bytes.len(),
            }));
        }

        Ok(Self {
            matrix: GPURead::read_gpu(&bytes[0..64])?,
        })
    }
}
