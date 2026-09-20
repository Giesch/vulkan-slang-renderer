// GENERATED FILE (do not edit directly)

//! shared types from slang module: skinning.slang

use serde::Serialize;

#[allow(unused_imports)]
use mltrs::renderer::gpu_read::GPURead;
#[allow(unused_imports)]
use mltrs::renderer::render_graph::GPUWrite;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct VertexSkinning {
    pub joints: glam::UVec4,
    pub weights: glam::Vec4,
}

impl GPUWrite for VertexSkinning {}
const _: () = assert!(std::mem::size_of::<VertexSkinning>() == 32);
const _: () = assert!(std::mem::offset_of!(VertexSkinning, joints) == 0);
const _: () = assert!(std::mem::size_of::<glam::UVec4>() == 16);
const _: () = assert!(std::mem::offset_of!(VertexSkinning, weights) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct SkinJoint {
    pub transform: glam::Mat4,
}

impl GPUWrite for SkinJoint {}
const _: () = assert!(std::mem::size_of::<SkinJoint>() == 64);
const _: () = assert!(std::mem::offset_of!(SkinJoint, transform) == 0);
const _: () = assert!(std::mem::size_of::<glam::Mat4>() == 64);

impl GPURead for VertexSkinning {
    const GPU_SIZE: usize = 32;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for VertexSkinning"
        );

        Ok(Self {
            joints: GPURead::read_gpu(&bytes[0..16])?,
            weights: GPURead::read_gpu(&bytes[16..32])?,
        })
    }
}

impl GPURead for SkinJoint {
    const GPU_SIZE: usize = 64;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for SkinJoint"
        );

        Ok(Self {
            transform: GPURead::read_gpu(&bytes[0..64])?,
        })
    }
}
