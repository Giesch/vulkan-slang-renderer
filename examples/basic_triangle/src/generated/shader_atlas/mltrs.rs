// GENERATED FILE (do not edit directly)

//! shared types from slang module: mltrs.slang

use serde::Serialize;

#[allow(unused_imports)]
use mltrs::renderer::gpu_read::GPURead;
#[allow(unused_imports)]
use mltrs::renderer::render_graph::GPUWrite;
use mltrs::renderer::render_graph::*;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
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

impl GPURead for MVPMatrices {
    const GPU_SIZE: usize = 192;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for MVPMatrices"
        );

        Ok(Self {
            model: GPURead::read_gpu(&bytes[0..64])?,
            view: GPURead::read_gpu(&bytes[64..128])?,
            proj: GPURead::read_gpu(&bytes[128..192])?,
        })
    }
}

/// Complete graph inputs before resource references resolve to GPU values.
#[derive(Debug, Clone, Copy)]
pub struct MVPMatricesInput {
    pub model: glam::Mat4,
    pub view: glam::Mat4,
    pub proj: glam::Mat4,
}

impl GraphShaderParams for MVPMatrices {
    type Data = Self;
    type Bindings = ();
    type Input = MVPMatricesInput;

    fn input(data: &Self::Data, _bindings: &Self::Bindings) -> Self::Input {
        Self::Input {
            model: data.model,
            view: data.view,
            proj: data.proj,
        }
    }

    fn assemble_input(input: &Self::Input, _resolver: &BindingResolver<'_>) -> Self {
        Self {
            model: input.model,
            view: input.view,
            proj: input.proj,
        }
    }
}

impl GraphBindingSet for MVPMatricesInput {
    fn visit(&self, _f: &mut dyn FnMut(GraphBinding)) {}
}
