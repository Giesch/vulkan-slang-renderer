//! Serde types for the converted-model manifest (`link.manifest.json`),
//! shared between the `convert_link` binary (which writes it) and the
//! `toon_link` example (which reads it). Everything is human-inspectable.
//!
//! Design: renderer-facing raster state uses friendly enum names (they map
//! straight onto pipeline state); TEV interpreter data is kept as the raw GX
//! byte values the shader packs into its `uint4` uniform arrays. `mat3_dump.txt`
//! carries the human-readable equations, so the machine format stays compact.

use serde::{Deserialize, Serialize};

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
    pub wrap_u: String,
    pub wrap_v: String,
    pub filter: String,
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
    pub pe_mode: String,
    pub cull: String,
    pub z_test: bool,
    pub z_func: String,
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
    pub mode: String,
    pub src: String,
    pub dst: String,
    pub logic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlphaCompareState {
    pub comp0: String,
    pub ref0: u8,
    pub op: String,
    pub comp1: String,
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
    pub mat_src: String,
    pub amb_src: String,
    pub diffuse: String,
    pub attenuation: String,
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
                wrap_u: "ClampToEdge".into(),
                wrap_v: "ClampToEdge".into(),
                filter: "Linear".into(),
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
}
