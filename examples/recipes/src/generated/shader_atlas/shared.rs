// GENERATED FILE (do not edit directly)

//! shared types from slang module: shared.slang

use serde::Serialize;

#[allow(unused_imports)]
use mltrs::renderer::gpu_read::GPURead;
#[allow(unused_imports)]
use mltrs::renderer::render_graph::GPUWrite;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(4))]
pub struct Solution {
    pub maximum_score: u32,
}

impl GPUWrite for Solution {}
const _: () = assert!(std::mem::size_of::<Solution>() == 4);
const _: () = assert!(std::mem::offset_of!(Solution, maximum_score) == 0);
const _: () = assert!(std::mem::size_of::<u32>() == 4);

impl GPURead for Solution {
    const GPU_SIZE: usize = 4;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for Solution"
        );

        Ok(Self {
            maximum_score: GPURead::read_gpu(&bytes[0..4])?,
        })
    }
}
