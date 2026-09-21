// GENERATED FILE (do not edit directly)

//! shared types from slang module: tev.slang

use facet::Facet;
use serde::Serialize;

#[allow(unused_imports)]
use mltrs::renderer::gpu_read::GPURead;
#[allow(unused_imports)]
use mltrs::renderer::render_graph::GPUWrite;

// glam must be built without its scalar-math feature (GPU layouts need align-16 Vec4)
const _: () = assert!(std::mem::align_of::<glam::Vec4>() == 16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default, Facet)]
#[repr(u32)]
// variant names come from the shader author, so clippy's shared-prefix lint
// would otherwise force renaming away from the slang spelling
#[allow(clippy::enum_variant_names)]
pub enum GXCompare {
    #[default]
    Never = 0,
    Less = 1,
    Equal = 2,
    LessEqual = 3,
    Greater = 4,
    NotEqual = 5,
    GreaterEqual = 6,
    Always = 7,
}

const _: () = assert!(std::mem::size_of::<GXCompare>() == 4);

impl From<GXCompare> for u32 {
    fn from(value: GXCompare) -> u32 {
        value as u32
    }
}

// A repr(int) enum holding a value outside its declared variants is undefined
// behavior. Data flows CPU -> GPU here, so the CPU never materializes a value it
// did not construct; any future readback must come back through this TryFrom,
// never a transmute or an `as` cast into the enum.
impl TryFrom<u32> for GXCompare {
    type Error = u32;

    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            0 => Ok(Self::Never),
            1 => Ok(Self::Less),
            2 => Ok(Self::Equal),
            3 => Ok(Self::LessEqual),
            4 => Ok(Self::Greater),
            5 => Ok(Self::NotEqual),
            6 => Ok(Self::GreaterEqual),
            7 => Ok(Self::Always),
            other => Err(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default, Facet)]
#[repr(u32)]
// variant names come from the shader author, so clippy's shared-prefix lint
// would otherwise force renaming away from the slang spelling
#[allow(clippy::enum_variant_names)]
pub enum GXAlphaOp {
    #[default]
    And = 0,
    Or = 1,
    Xor = 2,
    Xnor = 3,
}

const _: () = assert!(std::mem::size_of::<GXAlphaOp>() == 4);

impl From<GXAlphaOp> for u32 {
    fn from(value: GXAlphaOp) -> u32 {
        value as u32
    }
}

// A repr(int) enum holding a value outside its declared variants is undefined
// behavior. Data flows CPU -> GPU here, so the CPU never materializes a value it
// did not construct; any future readback must come back through this TryFrom,
// never a transmute or an `as` cast into the enum.
impl TryFrom<u32> for GXAlphaOp {
    type Error = u32;

    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            0 => Ok(Self::And),
            1 => Ok(Self::Or),
            2 => Ok(Self::Xor),
            3 => Ok(Self::Xnor),
            other => Err(other),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(4))]
pub struct GXAlphaCompare {
    pub comp0: GXCompare,
    pub ref0: u32,
    pub comp1: GXCompare,
    pub ref1: u32,
    pub op: GXAlphaOp,
}

impl GPUWrite for GXAlphaCompare {}
const _: () = assert!(std::mem::size_of::<GXAlphaCompare>() == 20);
const _: () = assert!(std::mem::offset_of!(GXAlphaCompare, comp0) == 0);
const _: () = assert!(std::mem::size_of::<GXCompare>() == 4);
const _: () = assert!(std::mem::offset_of!(GXAlphaCompare, ref0) == 4);
const _: () = assert!(std::mem::size_of::<u32>() == 4);
const _: () = assert!(std::mem::offset_of!(GXAlphaCompare, comp1) == 8);
const _: () = assert!(std::mem::size_of::<GXCompare>() == 4);
const _: () = assert!(std::mem::offset_of!(GXAlphaCompare, ref1) == 12);
const _: () = assert!(std::mem::size_of::<u32>() == 4);
const _: () = assert!(std::mem::offset_of!(GXAlphaCompare, op) == 16);
const _: () = assert!(std::mem::size_of::<GXAlphaOp>() == 4);

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
    pub chan_control: [glam::UVec4; 2],
    pub chan_mat_color: glam::Vec4,
    pub chan_amb_color: glam::Vec4,
    pub control: glam::UVec4,
}

impl GPUWrite for TevParams {}
const _: () = assert!(std::mem::size_of::<TevParams>() == 1264);
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
const _: () = assert!(std::mem::offset_of!(TevParams, chan_control) == 1184);
const _: () = assert!(std::mem::size_of::<[glam::UVec4; 2]>() == 32);
const _: () = assert!(std::mem::offset_of!(TevParams, chan_mat_color) == 1216);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(TevParams, chan_amb_color) == 1232);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(TevParams, control) == 1248);
const _: () = assert!(std::mem::size_of::<glam::UVec4>() == 16);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct GXTevColorOverride {
    pub actor_c0: glam::Vec4,
    pub actor_k0: glam::Vec4,
    pub eflight_konst: glam::Vec4,
    pub eflight: u32,
    pub _padding_0: [u8; 12],
}

impl GPUWrite for GXTevColorOverride {}
const _: () = assert!(std::mem::size_of::<GXTevColorOverride>() == 64);
const _: () = assert!(std::mem::offset_of!(GXTevColorOverride, actor_c0) == 0);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(GXTevColorOverride, actor_k0) == 16);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(GXTevColorOverride, eflight_konst) == 32);
const _: () = assert!(std::mem::size_of::<glam::Vec4>() == 16);
const _: () = assert!(std::mem::offset_of!(GXTevColorOverride, eflight) == 48);
const _: () = assert!(std::mem::size_of::<u32>() == 4);

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(C, align(16))]
pub struct GXLights {
    pub dir: [glam::Vec4; 2],
    pub color: [glam::Vec4; 2],
}

impl GPUWrite for GXLights {}
const _: () = assert!(std::mem::size_of::<GXLights>() == 64);
const _: () = assert!(std::mem::offset_of!(GXLights, dir) == 0);
const _: () = assert!(std::mem::size_of::<[glam::Vec4; 2]>() == 32);
const _: () = assert!(std::mem::offset_of!(GXLights, color) == 32);
const _: () = assert!(std::mem::size_of::<[glam::Vec4; 2]>() == 32);

impl GPURead for GXCompare {
    const GPU_SIZE: usize = 4;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        let tag = <u32 as GPURead>::read_gpu(bytes)?;

        Self::try_from(tag).map_err(|tag| anyhow::anyhow!("invalid GXCompare tag: {tag}"))
    }
}

impl GPURead for GXAlphaOp {
    const GPU_SIZE: usize = 4;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        let tag = <u32 as GPURead>::read_gpu(bytes)?;

        Self::try_from(tag).map_err(|tag| anyhow::anyhow!("invalid GXAlphaOp tag: {tag}"))
    }
}

impl GPURead for GXAlphaCompare {
    const GPU_SIZE: usize = 20;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for GXAlphaCompare"
        );

        Ok(Self {
            comp0: GPURead::read_gpu(&bytes[0..4])?,
            ref0: GPURead::read_gpu(&bytes[4..8])?,
            comp1: GPURead::read_gpu(&bytes[8..12])?,
            ref1: GPURead::read_gpu(&bytes[12..16])?,
            op: GPURead::read_gpu(&bytes[16..20])?,
        })
    }
}

impl GPURead for TevParams {
    const GPU_SIZE: usize = 1264;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for TevParams"
        );

        Ok(Self {
            stage_color_in: GPURead::read_gpu(&bytes[0..128])?,
            stage_color_op: GPURead::read_gpu(&bytes[128..256])?,
            stage_alpha_in: GPURead::read_gpu(&bytes[256..384])?,
            stage_alpha_op: GPURead::read_gpu(&bytes[384..512])?,
            stage_dest: GPURead::read_gpu(&bytes[512..640])?,
            stage_order: GPURead::read_gpu(&bytes[640..768])?,
            stage_swap: GPURead::read_gpu(&bytes[768..896])?,
            swap_table: GPURead::read_gpu(&bytes[896..960])?,
            texgen: GPURead::read_gpu(&bytes[960..992])?,
            texgen_mtx: GPURead::read_gpu(&bytes[992..1056])?,
            konst: GPURead::read_gpu(&bytes[1056..1120])?,
            reg: GPURead::read_gpu(&bytes[1120..1184])?,
            chan_control: GPURead::read_gpu(&bytes[1184..1216])?,
            chan_mat_color: GPURead::read_gpu(&bytes[1216..1232])?,
            chan_amb_color: GPURead::read_gpu(&bytes[1232..1248])?,
            control: GPURead::read_gpu(&bytes[1248..1264])?,
        })
    }
}

impl GPURead for GXTevColorOverride {
    const GPU_SIZE: usize = 64;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for GXTevColorOverride"
        );

        Ok(Self {
            actor_c0: GPURead::read_gpu(&bytes[0..16])?,
            actor_k0: GPURead::read_gpu(&bytes[16..32])?,
            eflight_konst: GPURead::read_gpu(&bytes[32..48])?,
            eflight: GPURead::read_gpu(&bytes[48..52])?,
            _padding_0: [0; 12],
        })
    }
}

impl GPURead for GXLights {
    const GPU_SIZE: usize = 64;

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "invalid GPU readback byte length for GXLights"
        );

        Ok(Self {
            dir: GPURead::read_gpu(&bytes[0..32])?,
            color: GPURead::read_gpu(&bytes[32..64])?,
        })
    }
}
