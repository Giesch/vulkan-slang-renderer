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
pub struct Vertex {
    pub position: glam::Vec3,
    pub _padding_0: [u8; 4],
    pub normal: glam::Vec3,
    pub _padding_1: [u8; 4],
    pub uv0: glam::Vec2,
    pub _padding_2: [u8; 8],
}

impl GPUWrite for Vertex {}
const _: () = assert!(std::mem::size_of::<Vertex>() == 48);
const _: () = assert!(std::mem::offset_of!(Vertex, position) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec3>() == 12);
const _: () = assert!(std::mem::offset_of!(Vertex, normal) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec3>() == 12);
const _: () = assert!(std::mem::offset_of!(Vertex, uv0) == 32);
const _: () = assert!(std::mem::size_of::<glam::Vec2>() == 8);

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

impl GPURead for Vertex {
    const GPU_SIZE: usize = 48;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for Vertex"
        );

        Ok(Self {
            position: glam::Vec3::from_array(<[f32; 3] as GPURead>::read_gpu(&bytes[0..12])?),
            _padding_0: [0; 4],
            normal: glam::Vec3::from_array(<[f32; 3] as GPURead>::read_gpu(&bytes[16..28])?),
            _padding_1: [0; 4],
            uv0: GPURead::read_gpu(&bytes[32..40])?,
            _padding_2: [0; 8],
        })
    }
}

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
