// GENERATED FILE (do not edit directly)

//! shared types from slang module: tev.slang

use serde::Serialize;

use crate::renderer::gpu_write::GPUWrite;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct TevParams {
    pub stage_color_in: [glam::UVec4; 8],
    pub stage_color_op: [glam::UVec4; 8],
    pub stage_alpha_in: [glam::UVec4; 8],
    pub stage_alpha_op: [glam::UVec4; 8],
    pub stage_dest: [glam::UVec4; 8],
    pub stage_order: [glam::UVec4; 8],
    pub stage_swap: [glam::UVec4; 8],
    pub swap_table: [glam::UVec4; 4],
    pub texgen: [glam::UVec4; 2],
    pub texgen_mtx: [glam::Vec4; 4],
    pub konst: [glam::Vec4; 4],
    pub reg: [glam::Vec4; 4],
    pub light_dir: [glam::Vec4; 2],
    pub light_color: [glam::Vec4; 2],
    pub chan_control: [glam::UVec4; 2],
    pub chan_mat_color: glam::Vec4,
    pub chan_amb_color: glam::Vec4,
    pub control: glam::UVec4,
}

impl GPUWrite for TevParams {}
const _: () = assert!(std::mem::size_of::<TevParams>() == 1328);
const _: () = assert!(std::mem::offset_of!(TevParams, stage_color_in) == 0);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 8]>() == 128);
const _: () = assert!(std::mem::offset_of!(TevParams, stage_color_op) == 128);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 8]>() == 128);
const _: () = assert!(std::mem::offset_of!(TevParams, stage_alpha_in) == 256);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 8]>() == 128);
const _: () = assert!(std::mem::offset_of!(TevParams, stage_alpha_op) == 384);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 8]>() == 128);
const _: () = assert!(std::mem::offset_of!(TevParams, stage_dest) == 512);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 8]>() == 128);
const _: () = assert!(std::mem::offset_of!(TevParams, stage_order) == 640);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 8]>() == 128);
const _: () = assert!(std::mem::offset_of!(TevParams, stage_swap) == 768);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 8]>() == 128);
const _: () = assert!(std::mem::offset_of!(TevParams, swap_table) == 896);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 4]>() == 64);
const _: () = assert!(std::mem::offset_of!(TevParams, texgen) == 960);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 2]>() == 32);
const _: () = assert!(std::mem::offset_of!(TevParams, texgen_mtx) == 992);
const _: () = assert!(std::mem::size_of::<[glam::Vec4; 4]>() == 64);
const _: () = assert!(std::mem::offset_of!(TevParams, konst) == 1056);
const _: () = assert!(std::mem::size_of::<[glam::Vec4; 4]>() == 64);
const _: () = assert!(std::mem::offset_of!(TevParams, reg) == 1120);
const _: () = assert!(std::mem::size_of::<[glam::Vec4; 4]>() == 64);
const _: () = assert!(std::mem::offset_of!(TevParams, light_dir) == 1184);
const _: () = assert!(std::mem::size_of::<[glam::Vec4; 2]>() == 32);
const _: () = assert!(std::mem::offset_of!(TevParams, light_color) == 1216);
const _: () = assert!(std::mem::size_of::<[glam::Vec4; 2]>() == 32);
const _: () = assert!(std::mem::offset_of!(TevParams, chan_control) == 1248);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 2]>() == 32);
const _: () = assert!(std::mem::offset_of!(TevParams, chan_mat_color) == 1280);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(TevParams, chan_amb_color) == 1296);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(TevParams, control) == 1312);
const _: () = assert!(std::mem::size_of::<glam::UVec4>() == 16);
