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
            type Error = $crate::model_manifest::GxEnumError;
            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $($val => Ok(Self::$variant),)+
                    _ => Err($crate::model_manifest::GxEnumError {
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
    /// TEV register colors (rgba s16), 4 slots (PREV/REG0/REG1/REG2).
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
