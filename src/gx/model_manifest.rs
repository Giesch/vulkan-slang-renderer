//! Serde types for the converted-model manifest (`link.manifest.json`),
//! shared between the `convert_link` binary (which writes it) and the
//! `toon_link` example (which reads it). Everything is human-inspectable.
//!
//! Design: renderer-facing raster state is typed GX enums that serialize as the
//! canonical GX names (they map straight onto pipeline state, and their
//! discriminants are the GX byte values); TEV interpreter data is kept as the raw
//! GX byte values the shader packs into its `uint4` uniform arrays.
//! `mat3_dump.txt` carries the human-readable equations, so the machine format
//! stays compact.
//!
//! The enums below are the ones the manifest serializes. They live here rather
//! than in `convert_link`'s `gx::types` because the library is the shared crate;
//! `gx::types` re-exports them and keeps the parse-only rest (`ImageFormat`,
//! `Attr`, `PrimitiveType`, …), declared with the same [`gx_enum`] macro.

use std::fmt;

use serde::{Deserialize, Serialize};

// --- shared GX enums --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GxEnumError {
    pub kind: &'static str,
    pub value: u32,
}

impl fmt::Display for GxEnumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {} value {:#x}", self.kind, self.value)
    }
}

impl std::error::Error for GxEnumError {}

/// Declares a GX enum with `TryFrom<u8>` and a canonical name (the exact
/// spelling the MAT3 oracle prints for the same value), used for *both* its
/// `Display` and its serde representation — one literal, so the JSON spelling
/// and the oracle vocabulary cannot drift apart.
///
/// Exported for `convert_link`'s `gx::types`, which declares the GX enums the
/// manifest never serializes — those get the serde impls too, unused but
/// harmless, in exchange for one macro defining the whole GX vocabulary.
#[macro_export]
macro_rules! gx_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $val:literal => $canon:literal,)+ }) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            ::serde::Serialize, ::serde::Deserialize,
        )]
        pub enum $name {
            $(#[serde(rename = $canon)] $variant = $val,)+
        }

        impl TryFrom<u8> for $name {
            type Error = $crate::gx::model_manifest::GxEnumError;
            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $($val => Ok(Self::$variant),)+
                    _ => Err($crate::gx::model_manifest::GxEnumError {
                        kind: stringify!($name),
                        value: value as u32,
                    }),
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(match self {
                    $(Self::$variant => $canon,)+
                })
            }
        }
    };
}

gx_enum! {
    /// GXTexWrapMode (GXEnum.h:432–434)
    WrapMode {
        Clamp = 0x0 => "ClampToEdge",
        Repeat = 0x1 => "Repeat",
        Mirror = 0x2 => "MirroredRepeat",
    }
}

gx_enum! {
    /// GXTexFilter (GXEnum.h:439–444)
    FilterMode {
        Nearest = 0x0 => "Nearest",
        Linear = 0x1 => "Linear",
        NearestMipNearest = 0x2 => "NearestMipmapNearest",
        LinearMipNearest = 0x3 => "LinearMipmapNearest",
        NearestMipLinear = 0x4 => "NearestMipmapLinear",
        LinearMipLinear = 0x5 => "LinearMipmapLinear",
    }
}

gx_enum! {
    /// J3D material pixel-engine mode (gclib PixelEngineMode)
    PixelEngineMode {
        Opaque = 0x1 => "Opaque",
        AlphaTest = 0x2 => "Alpha_Test",
        Translucent = 0x4 => "Translucent",
    }
}

gx_enum! {
    /// GXCullMode (GXEnum.h:17–20); stored as u32 in MAT3's list
    CullMode {
        None = 0x0 => "Cull_None",
        Front = 0x1 => "Cull_Front",
        Back = 0x2 => "Cull_Back",
        All = 0x3 => "Cull_All",
    }
}

gx_enum! {
    /// GXCompare (GXEnum.h:466–473)
    CompareType {
        Never = 0x0 => "Never",
        Less = 0x1 => "Less",
        Equal = 0x2 => "Equal",
        LessEqual = 0x3 => "Less_Equal",
        Greater = 0x4 => "Greater",
        NotEqual = 0x5 => "Not_Equal",
        GreaterEqual = 0x6 => "Greater_Equal",
        Always = 0x7 => "Always",
    }
}

gx_enum! {
    /// GXAlphaOp (GXEnum.h:477–480)
    AlphaOp {
        And = 0x0 => "AND",
        Or = 0x1 => "OR",
        Xor = 0x2 => "XOR",
        Xnor = 0x3 => "XNOR",
    }
}

gx_enum! {
    /// GXColorSrc (GXEnum.h:92–93)
    ColorSrc {
        Register = 0x0 => "Register",
        Vertex = 0x1 => "Vertex",
    }
}

gx_enum! {
    /// GXDiffuseFn (GXEnum.h:110–112)
    DiffuseFunction {
        None = 0x0 => "None_",
        Signed = 0x1 => "Signed",
        Clamp = 0x2 => "Clamp",
    }
}

gx_enum! {
    /// GXAttnFn (GXEnum.h:116–118)
    AttenuationFunction {
        Specular = 0x0 => "Specular",
        Spot = 0x1 => "Spot",
        None = 0x2 => "None_",
    }
}

gx_enum! {
    /// GXBlendMode (GXEnum.h:147–150)
    BlendMode {
        None = 0x0 => "None_",
        Blend = 0x1 => "Blend",
        Logic = 0x2 => "Logic",
        Subtract = 0x3 => "Subtract",
    }
}

gx_enum! {
    /// GXBlendFactor (GXEnum.h:155–164); src/dst-color aliases share values
    BlendFactor {
        Zero = 0x0 => "Zero",
        One = 0x1 => "One",
        SourceColor = 0x2 => "Source_Color",
        InverseSourceColor = 0x3 => "Inverse_Source_Color",
        SourceAlpha = 0x4 => "Source_Alpha",
        InverseSourceAlpha = 0x5 => "Inverse_Source_Alpha",
        DestinationAlpha = 0x6 => "Destination_Alpha",
        InverseDestinationAlpha = 0x7 => "Inverse_Destination_Alpha",
    }
}

gx_enum! {
    /// GXLogicOp (GXEnum.h:168–183)
    LogicOp {
        Clear = 0x0 => "CLEAR",
        And = 0x1 => "AND",
        RevAnd = 0x2 => "REV_AND",
        Copy = 0x3 => "COPY",
        InvAnd = 0x4 => "INV_AND",
        Noop = 0x5 => "NOOP",
        Xor = 0x6 => "XOR",
        Or = 0x7 => "OR",
        Nor = 0x8 => "NOR",
        Equiv = 0x9 => "EQUIV",
        Inv = 0xA => "INV",
        RevOr = 0xB => "REV_OR",
        InvCopy = 0xC => "INV_COPY",
        InvOr = 0xD => "INV_OR",
        Nand = 0xE => "NAND",
        Set = 0xF => "SET",
    }
}

// --- TEV / texgen vocabulary -------------------------------------------------
//
// The manifest carries these as raw `u8`, not as typed fields, so strictly
// speaking they are parse-only. They live here anyway because the *library* side
// needs them too: `tev_pack` re-checks every byte on its way to the GPU
// (parse-don't-validate, the same discipline the converter uses) and renders the
// per-stage equations in `mat3_dump.txt`'s notation for the example's isolation
// printout. One `Display` literal per value means the printout and the dump
// cannot spell the same GX value two different ways.

gx_enum! {
    /// GXTexGenType (GXEnum.h:576–586)
    TexGenType {
        Mtx3x4 = 0x0 => "MTX3x4",
        Mtx2x4 = 0x1 => "MTX2x4",
        Bump0 = 0x2 => "BUMP0",
        Bump1 = 0x3 => "BUMP1",
        Bump2 = 0x4 => "BUMP2",
        Bump3 = 0x5 => "BUMP3",
        Bump4 = 0x6 => "BUMP4",
        Bump5 = 0x7 => "BUMP5",
        Bump6 = 0x8 => "BUMP6",
        Bump7 = 0x9 => "BUMP7",
        Srtg = 0xA => "SRTG",
    }
}

gx_enum! {
    /// GXTexGenSrc (GXEnum.h:590–610)
    TexGenSrc {
        Pos = 0x00 => "POS",
        Nrm = 0x01 => "NRM",
        Binrm = 0x02 => "BINRM",
        Tangent = 0x03 => "TANGENT",
        Tex0 = 0x04 => "TEX0",
        Tex1 = 0x05 => "TEX1",
        Tex2 = 0x06 => "TEX2",
        Tex3 = 0x07 => "TEX3",
        Tex4 = 0x08 => "TEX4",
        Tex5 = 0x09 => "TEX5",
        Tex6 = 0x0A => "TEX6",
        Tex7 = 0x0B => "TEX7",
        Texcoord0 = 0x0C => "TEXCOORD0",
        Texcoord1 = 0x0D => "TEXCOORD1",
        Texcoord2 = 0x0E => "TEXCOORD2",
        Texcoord3 = 0x0F => "TEXCOORD3",
        Texcoord4 = 0x10 => "TEXCOORD4",
        Texcoord5 = 0x11 => "TEXCOORD5",
        Texcoord6 = 0x12 => "TEXCOORD6",
        Color0 = 0x13 => "COLOR0",
        Color1 = 0x14 => "COLOR1",
    }
}

gx_enum! {
    /// GXTexMtx / GXPosNrmMtx (GXEnum.h:729–747); PNMTXn = 3n, TEXMTXn = 30+3n
    TexGenMatrix {
        Pnmtx0 = 0 => "PNMTX0",
        Pnmtx1 = 3 => "PNMTX1",
        Pnmtx2 = 6 => "PNMTX2",
        Pnmtx3 = 9 => "PNMTX3",
        Pnmtx4 = 12 => "PNMTX4",
        Pnmtx5 = 15 => "PNMTX5",
        Pnmtx6 = 18 => "PNMTX6",
        Pnmtx7 = 21 => "PNMTX7",
        Pnmtx8 = 24 => "PNMTX8",
        Texmtx0 = 30 => "TEXMTX0",
        Texmtx1 = 33 => "TEXMTX1",
        Texmtx2 = 36 => "TEXMTX2",
        Texmtx3 = 39 => "TEXMTX3",
        Texmtx4 = 42 => "TEXMTX4",
        Texmtx5 = 45 => "TEXMTX5",
        Texmtx6 = 48 => "TEXMTX6",
        Texmtx7 = 51 => "TEXMTX7",
        Texmtx8 = 54 => "TEXMTX8",
        Texmtx9 = 57 => "TEXMTX9",
        Identity = 60 => "IDENTITY",
    }
}

gx_enum! {
    /// GXTevColorArg (GXEnum.h:294–309)
    CombineColor {
        CPrev = 0x0 => "CPREV",
        APrev = 0x1 => "APREV",
        C0 = 0x2 => "C0",
        A0 = 0x3 => "A0",
        C1 = 0x4 => "C1",
        A1 = 0x5 => "A1",
        C2 = 0x6 => "C2",
        A2 = 0x7 => "A2",
        TexC = 0x8 => "TEXC",
        TexA = 0x9 => "TEXA",
        RasC = 0xA => "RASC",
        RasA = 0xB => "RASA",
        One = 0xC => "ONE",
        Half = 0xD => "HALF",
        Konst = 0xE => "KONST",
        Zero = 0xF => "ZERO",
    }
}

gx_enum! {
    /// GXTevAlphaArg (GXEnum.h:336–343)
    CombineAlpha {
        APrev = 0x0 => "APREV",
        A0 = 0x1 => "A0",
        A1 = 0x2 => "A1",
        A2 = 0x3 => "A2",
        TexA = 0x4 => "TEXA",
        RasA = 0x5 => "RASA",
        Konst = 0x6 => "KONST",
        Zero = 0x7 => "ZERO",
    }
}

gx_enum! {
    /// GXTevOp (GXEnum.h:272–283)
    TevOp {
        Add = 0x0 => "ADD",
        Sub = 0x1 => "SUB",
        CompR8Gt = 0x8 => "COMP_R8_GT",
        CompR8Eq = 0x9 => "COMP_R8_EQ",
        CompGr16Gt = 0xA => "COMP_GR16_GT",
        CompGr16Eq = 0xB => "COMP_GR16_EQ",
        CompBgr24Gt = 0xC => "COMP_BGR24_GT",
        CompBgr24Eq = 0xD => "COMP_BGR24_EQ",
        CompRgb8Gt = 0xE => "COMP_RGB8_GT",
        CompRgb8Eq = 0xF => "COMP_RGB8_EQ",
    }
}

gx_enum! {
    /// GXTevBias (GXEnum.h:287–289) + J3D's 0x3 "compare mode" marker
    TevBias {
        Zero = 0x0 => "ZERO",
        AddHalf = 0x1 => "ADDHALF",
        SubHalf = 0x2 => "SUBHALF",
        HwbCompare = 0x3 => "HWB_COMPARE",
    }
}

gx_enum! {
    /// GXTevScale (GXEnum.h:320–323)
    TevScale {
        Scale1 = 0x0 => "SCALE_1",
        Scale2 = 0x1 => "SCALE_2",
        Scale4 = 0x2 => "SCALE_4",
        Divide2 = 0x3 => "DIVIDE_2",
    }
}

gx_enum! {
    /// GXTevRegID (GXEnum.h:328–331). NOTE the MAT3 `reg_colors` list does *not*
    /// line up with these: entry *i* loads register *i+1*, so `reg_colors[0]` is
    /// `Reg0` and `Prev` gets no MAT3 value. See [`TevConfig::reg_colors`].
    Register {
        Prev = 0x0 => "PREV",
        Reg0 = 0x1 => "REG0",
        Reg1 = 0x2 => "REG1",
        Reg2 = 0x3 => "REG2",
    }
}

gx_enum! {
    /// GXTevKColorSel (GXEnum.h:537–564)
    KonstColorSel {
        One = 0x00 => "_1",
        SevenEighths = 0x01 => "_7_8th",
        SixEighths = 0x02 => "_6_8th",
        FiveEighths = 0x03 => "_5_8th",
        FourEighths = 0x04 => "_4_8th",
        ThreeEighths = 0x05 => "_3_8th",
        TwoEighths = 0x06 => "_2_8th",
        OneEighth = 0x07 => "_1_8th",
        K0 = 0x0C => "K0",
        K1 = 0x0D => "K1",
        K2 = 0x0E => "K2",
        K3 = 0x0F => "K3",
        K0R = 0x10 => "K0_R",
        K1R = 0x11 => "K1_R",
        K2R = 0x12 => "K2_R",
        K3R = 0x13 => "K3_R",
        K0G = 0x14 => "K0_G",
        K1G = 0x15 => "K1_G",
        K2G = 0x16 => "K2_G",
        K3G = 0x17 => "K3_G",
        K0B = 0x18 => "K0_B",
        K1B = 0x19 => "K1_B",
        K2B = 0x1A => "K2_B",
        K3B = 0x1B => "K3_B",
        K0A = 0x1C => "K0_A",
        K1A = 0x1D => "K1_A",
        K2A = 0x1E => "K2_A",
        K3A = 0x1F => "K3_A",
    }
}

gx_enum! {
    /// GXTevKAlphaSel (GXEnum.h:509–533)
    KonstAlphaSel {
        One = 0x00 => "_1",
        SevenEighths = 0x01 => "_7_8th",
        SixEighths = 0x02 => "_6_8th",
        FiveEighths = 0x03 => "_5_8th",
        FourEighths = 0x04 => "_4_8th",
        ThreeEighths = 0x05 => "_3_8th",
        TwoEighths = 0x06 => "_2_8th",
        OneEighth = 0x07 => "_1_8th",
        K0R = 0x10 => "K0_R",
        K1R = 0x11 => "K1_R",
        K2R = 0x12 => "K2_R",
        K3R = 0x13 => "K3_R",
        K0G = 0x14 => "K0_G",
        K1G = 0x15 => "K1_G",
        K2G = 0x16 => "K2_G",
        K3G = 0x17 => "K3_G",
        K0B = 0x18 => "K0_B",
        K1B = 0x19 => "K1_B",
        K2B = 0x1A => "K2_B",
        K3B = 0x1B => "K3_B",
        K0A = 0x1C => "K0_A",
        K1A = 0x1D => "K1_A",
        K2A = 0x1E => "K2_A",
        K3A = 0x1F => "K3_A",
    }
}

gx_enum! {
    /// GXTexCoordID (GXEnum.h:66–75)
    TexCoordId {
        Texcoord0 = 0x00 => "TEXCOORD0",
        Texcoord1 = 0x01 => "TEXCOORD1",
        Texcoord2 = 0x02 => "TEXCOORD2",
        Texcoord3 = 0x03 => "TEXCOORD3",
        Texcoord4 = 0x04 => "TEXCOORD4",
        Texcoord5 = 0x05 => "TEXCOORD5",
        Texcoord6 = 0x06 => "TEXCOORD6",
        Texcoord7 = 0x07 => "TEXCOORD7",
        Null = 0xFF => "TEXCOORD_NULL",
    }
}

gx_enum! {
    /// GXTexMapID (GXEnum.h:32–41)
    TexMapId {
        Texmap0 = 0x00 => "TEXMAP0",
        Texmap1 = 0x01 => "TEXMAP1",
        Texmap2 = 0x02 => "TEXMAP2",
        Texmap3 = 0x03 => "TEXMAP3",
        Texmap4 = 0x04 => "TEXMAP4",
        Texmap5 = 0x05 => "TEXMAP5",
        Texmap6 = 0x06 => "TEXMAP6",
        Texmap7 = 0x07 => "TEXMAP7",
        Null = 0xFF => "TEXMAP_NULL",
    }
}

gx_enum! {
    /// GXChannelID (GXEnum.h:83–88). MAT3's four `color_channels` slots are
    /// *pairs*: slot 0 is `Color0`, slot 1 is `Alpha0`, slot 2 `Color1`, slot 3
    /// `Alpha1` — which is why `num_color_chans` counts pairs, not channels.
    ColorChannelId {
        Color0 = 0x00 => "COLOR0",
        Color1 = 0x01 => "COLOR1",
        Alpha0 = 0x02 => "ALPHA0",
        Alpha1 = 0x03 => "ALPHA1",
        Color0A0 = 0x04 => "COLOR0A0",
        Color1A1 = 0x05 => "COLOR1A1",
        ColorZero = 0x06 => "COLOR_ZERO",
        AlphaBump = 0x07 => "ALPHA_BUMP",
        AlphaBumpN = 0x08 => "ALPHA_BUMP_N",
        Null = 0xFF => "COLOR_NULL",
    }
}

// --- manifest ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub buffers: Buffers,
    pub textures: Vec<TextureEntry>,
    pub materials: Vec<MaterialEntry>,
    pub batches: Vec<Batch>,
    pub skeleton: Skeleton,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Buffers {
    pub vertices: String,
    pub indices: String,
    pub skinning: String,
    /// Interleaved little-endian f32 layout of `vertices`, e.g.
    /// `["position3f", "normal3f", "uv02f"]`.
    pub vertex_layout: Vec<String>,
    pub vertex_count: u32,
    pub index_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureEntry {
    pub name: String,
    /// Path relative to the manifest, e.g. `tex/12_linktexS3TC.png`.
    pub file: String,
    pub wrap_u: WrapMode,
    pub wrap_v: WrapMode,
    pub filter: FilterMode,
    pub mipmaps: bool,
    /// Set on ramp slots whose pixels are replaced at conversion time
    /// (e.g. `ZBtoonEX` ← `toonex`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub runtime_substitution: Option<String>,
}

/// A drawable: a material slot applied to a shape's triangle sub-range of the
/// shared index buffer, in INF1 draw order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub material: u16,
    pub shape: u16,
    pub first_index: u32,
    pub index_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skeleton {
    pub joints: Vec<SkeletonJoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkeletonJoint {
    pub name: String,
    /// Parent joint index, or -1 for the root.
    pub parent: i32,
    pub t: [f32; 3],
    pub r_s16: [i16; 3],
    pub s: [f32; 3],
}

// --- materials --------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialEntry {
    pub name: String,
    /// The shared MAT3 record this slot resolves to (duplicate values mean two
    /// slots share one record — J3D material instancing).
    pub record: u16,
    // Renderer-facing raster state (friendly names).
    pub pe_mode: PixelEngineMode,
    pub cull: CullMode,
    pub z_test: bool,
    pub z_func: CompareType,
    pub z_write: bool,
    pub z_compare_early: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub blend: Option<BlendState>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub alpha_compare: Option<AlphaCompareState>,
    pub dither: bool,
    // Counts and texture bindings.
    pub num_tev_stages: u8,
    pub num_tex_gens: u8,
    pub num_color_chans: u8,
    /// Indices into `Manifest::textures`, one per GX texmap slot (null = unused).
    pub texmaps: Vec<Option<u16>>,
    // TEV interpreter data (raw GX values, ready for shader uniforms).
    pub tev: TevConfig,
    pub texgens: Vec<TexGenState>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tex_matrices: Vec<TexMatrixState>,
    pub channels: Vec<ChannelState>,
    /// Per-channel ambient/material register colors (rgba u8).
    pub material_colors: Vec<Option<[u8; 4]>>,
    pub ambient_colors: Vec<Option<[u8; 4]>>,
    pub light_colors: Vec<Option<[u8; 4]>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlendState {
    pub mode: BlendMode,
    pub src: BlendFactor,
    pub dst: BlendFactor,
    pub logic: LogicOp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlphaCompareState {
    pub comp0: CompareType,
    pub ref0: u8,
    pub op: AlphaOp,
    pub comp1: CompareType,
    pub ref1: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TevConfig {
    pub stages: Vec<TevStageState>,
    pub orders: Vec<Option<TevOrderState>>,
    /// KONST colors (rgba u8), 4 slots.
    pub konst_colors: Vec<Option<[u8; 4]>>,
    /// TEV register colors (rgba s16), 4 slots in **MAT3 list order, which is
    /// not register order**: entry *i* loads `GX_TEVREG{i}`, i.e. `[0]` → REG0,
    /// `[1]` → REG1, `[2]` → REG2, and `[3]` is **never loaded at all**.
    /// `GX_TEVPREV` gets no MAT3 value.
    ///
    /// From `J3DMatBlock.cpp`: `loadTevColor(reg, c)` is
    /// `J3DGDSetTevColorS10(GXTevRegID(reg + 1), c)`, and `patchTevReg`'s loop
    /// runs to `ARRAY_SIZE(mTevColor) - 1`. Consistent with the data —
    /// `reg_colors[3]` is `[0,0,0,0]` on all 24 cl.bdl materials. (An earlier
    /// version of this comment claimed `PREV/REG0/REG1/REG2`; reading it that
    /// way is silent, since it degenerates the toon materials' stage 0 into
    /// `lerp(white, white, ramp)` and the cel bands just vanish.)
    ///
    /// The konst path has no such shift: `loadTevKColor` is
    /// `J3DGDSetTevKColor(GXTevKColorID(reg), …)`.
    pub reg_colors: Vec<Option<[i16; 4]>>,
    /// Per-stage konst color/alpha selects (raw GX values), 16 slots.
    pub kcsels: Vec<u8>,
    pub kasels: Vec<u8>,
    pub swap_modes: Vec<Option<SwapModeState>>,
    /// Swap tables: 4 channel-select values (r,g,b,a) each.
    pub swap_tables: Vec<Option<[u8; 4]>>,
}

/// One TEV stage. Color/alpha inputs are 4 raw GX selector values each; op,
/// bias, scale, reg are raw GX values; clamp is the clamp bit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TevStageState {
    pub color_in: [u8; 4],
    pub color_op: u8,
    pub color_bias: u8,
    pub color_scale: u8,
    pub color_clamp: bool,
    pub color_reg: u8,
    pub alpha_in: [u8; 4],
    pub alpha_op: u8,
    pub alpha_bias: u8,
    pub alpha_scale: u8,
    pub alpha_clamp: bool,
    pub alpha_reg: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TevOrderState {
    /// GX texcoord id (0xFF = none).
    pub tex_coord: u8,
    /// GX texmap id (0xFF = none).
    pub tex_map: u8,
    /// GX raster channel id.
    pub channel: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapModeState {
    pub ras_sel: u8,
    pub tex_sel: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TexGenState {
    /// GX texgen type / source / matrix (raw values).
    pub ty: u8,
    pub src: u8,
    pub matrix: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TexMatrixState {
    /// Slot index into the material's tex-matrix list.
    pub slot: u8,
    pub center: [f32; 3],
    pub scale: [f32; 2],
    pub rotation: u16,
    pub translation: [f32; 2],
    pub effect_matrix: [f32; 16],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelState {
    pub lighting_enabled: bool,
    pub mat_src: ColorSrc,
    pub amb_src: ColorSrc,
    pub diffuse: DiffuseFunction,
    pub attenuation: AttenuationFunction,
    pub lit_mask: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips() {
        let m = Manifest {
            version: 1,
            buffers: Buffers {
                vertices: "link.vtx.bin".into(),
                indices: "link.idx.bin".into(),
                skinning: "link.skin.bin".into(),
                vertex_layout: vec!["position3f".into(), "normal3f".into(), "uv02f".into()],
                vertex_count: 1754,
                index_count: 8622,
            },
            textures: vec![TextureEntry {
                name: "linktexS3TC".into(),
                file: "tex/12_linktexS3TC.png".into(),
                wrap_u: WrapMode::Clamp,
                wrap_v: WrapMode::Clamp,
                filter: FilterMode::Linear,
                mipmaps: false,
                runtime_substitution: None,
            }],
            materials: vec![],
            batches: vec![Batch {
                material: 0,
                shape: 0,
                first_index: 0,
                index_count: 810,
            }],
            skeleton: Skeleton {
                joints: vec![SkeletonJoint {
                    name: "link_root".into(),
                    parent: -1,
                    t: [0.0; 3],
                    r_s16: [0; 3],
                    s: [1.0; 3],
                }],
            },
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.buffers.vertex_count, 1754);
        assert_eq!(back.batches[0].index_count, 810);
        assert_eq!(back.skeleton.joints[0].parent, -1);
    }

    /// The JSON spelling of every GX enum is the canonical GX name, identical to
    /// `Display` — the manifest on disk is the same text it was when these fields
    /// were `String`s, and the MAT3 oracle diff reads the same vocabulary.
    #[test]
    fn enums_serialize_as_canonical_gx_names() {
        fn check<T>(value: T, expected: &str)
        where
            T: Serialize + for<'de> Deserialize<'de> + fmt::Display + PartialEq + fmt::Debug,
        {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            assert_eq!(value.to_string(), expected);
            assert_eq!(serde_json::from_str::<T>(&json).unwrap(), value);
        }

        check(WrapMode::Clamp, "ClampToEdge");
        check(FilterMode::LinearMipLinear, "LinearMipmapLinear");
        check(PixelEngineMode::AlphaTest, "Alpha_Test");
        check(CullMode::Back, "Cull_Back");
        check(CompareType::LessEqual, "Less_Equal");
        check(AlphaOp::Or, "OR");
        check(BlendMode::None, "None_");
        check(
            BlendFactor::InverseDestinationAlpha,
            "Inverse_Destination_Alpha",
        );
        check(LogicOp::Copy, "COPY");
        check(ColorSrc::Register, "Register");
        check(DiffuseFunction::None, "None_");
        check(AttenuationFunction::Spot, "Spot");
    }

    /// The discriminants are the GX byte values, so consumers can hand a manifest
    /// enum straight to a shader uniform (`toon_link`'s alpha-compare codes) and
    /// the converter can parse one out of a MAT3 byte.
    #[test]
    fn discriminants_are_gx_byte_values() {
        assert_eq!(CompareType::LessEqual as u8, 0x3);
        assert_eq!(AlphaOp::Xnor as u8, 0x3);
        assert_eq!(CullMode::Back as u8, 0x2);
        assert_eq!(CompareType::try_from(0x7), Ok(CompareType::Always));
        assert_eq!(
            CullMode::try_from(0x4),
            Err(GxEnumError {
                kind: "CullMode",
                value: 0x4
            })
        );
    }
}
