//! The frozen TEV subset, as a typed IR. Building it **is** the gate: every
//! field is one of the variants `shaders/source/tev.slang` actually implements,
//! so a material that constructs successfully is a material the shader can
//! render. Anything outside the set is a hard error naming the material and the
//! feature.
//!
//! Validation-only: this writes nothing and changes no output byte, which is why
//! `scripts/link_converted.sha256` is its own correctness gate. It lives in the
//! converter rather than the example because several of the fields it has to
//! assert on — `TexMatrix::projection` / `map_mode`, `fog`, `indirect`,
//! `post_tex_coord_gens`, `post_tex_matrices` — are parsed from MAT3 and dropped
//! before the manifest, so the example can never see them.
//!
//! Plan: llm_notes/link_rendering/phase_08.md Step 1.
#![allow(dead_code)]
// The IR is the gate's record of what it accepted. Most fields are written and
// never read back today: the tests read them, and the intended future consumer
// is `src/tev_pack.rs`, which currently re-derives the same values from the
// manifest's raw bytes because it runs on the library side of the crate.

use crate::bmd::BmdError;
use crate::bmd::mat3::{Mat3, Material, Rgba8, RgbaS16};
use crate::gx::types::*;

/// One material, reduced to the subset the interpreter implements.
#[derive(Debug, Clone, PartialEq)]
pub struct TevMaterialDesc {
    pub name: String,
    /// Exactly `num_tev_stages` entries, in stage order (1..=8).
    pub stages: Vec<TevStageDesc>,
    /// Exactly `num_tex_gens` entries, in texcoord order (1..=2).
    pub texgens: Vec<TexGenDesc>,
    pub konst: [Rgba8; 4],
    /// MAT3 order, **not** register order: entry *i* loads `GX_TEVREG{i}`
    /// (`J3DMatBlock.cpp`'s `loadTevColor` writes register `i + 1`), and entry 3
    /// is never loaded at all because `patchTevReg`'s loop stops one short. So
    /// `PREV` gets no MAT3 value. Getting this backwards is silent — stage 0 of
    /// the toon materials degenerates to a no-op and the bands vanish.
    pub regs: [RgbaS16; 4],
    /// Slots 0..4; absent slots default to the identity `[0, 1, 2, 3]`.
    pub swap_tables: [[u8; 4]; 4],
    /// `color_channels[0]` — GX_COLOR0.
    pub chan_color: ChannelDesc,
    /// `color_channels[1]` — GX_ALPHA0. MAT3's four slots are *pairs*
    /// (color0, alpha0, color1, alpha1), which is why `num_color_chans == 1`
    /// still means two live slots.
    pub chan_alpha: ChannelDesc,
    /// `material_colors[0]` / `ambient_colors[0]`: one RGBA register per *pair*,
    /// matching `GXSetChanMatColor(GX_COLOR0A0, …)`. The color channel takes
    /// `.rgb` from it and the alpha channel takes `.a`.
    pub mat_color: Rgba8,
    pub amb_color: Rgba8,
    /// Recorded, not gated: NBT scaling only matters for the BUMP texgens, which
    /// are rejected outright, so an enabled NBT scale cannot reach the shader.
    pub nbt_enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TevStageDesc {
    pub color: ColorCombinerDesc,
    pub alpha: AlphaCombinerDesc,
    /// `None` = `TEXCOORD_NULL`.
    pub tex_coord: Option<u8>,
    /// `None` = `TEXMAP_NULL`.
    pub tex_map: Option<u8>,
    pub channel: ColorChannelId,
    pub ras_swap: u8,
    pub tex_swap: u8,
    pub kcsel: KonstColorSel,
    pub kasel: KonstAlphaSel,
    /// J3D's leading `TevStage` byte. Recorded, not gated: its meaning is
    /// undocumented, so rejecting on an unverified value would only reject
    /// cl.bdl.
    pub tev_mode: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColorCombinerDesc {
    pub inputs: [CombineColor; 4],
    pub op: TevOp,
    pub bias: TevBias,
    pub scale: TevScale,
    pub clamp: bool,
    pub dest: Register,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlphaCombinerDesc {
    pub inputs: [CombineAlpha; 4],
    pub op: TevOp,
    pub bias: TevBias,
    pub scale: TevScale,
    pub clamp: bool,
    pub dest: Register,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TexGenDesc {
    pub ty: TexGenType,
    pub src: TexGenSrc,
    /// `None` = `GX_IDENTITY`.
    pub matrix: Option<TexMtxDesc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TexMtxDesc {
    /// `(GX code - 30) / 3`, i.e. `TEXMTXn` → `n`.
    pub slot: u8,
    pub center: [f32; 3],
    pub scale: [f32; 2],
    pub rotation: u16,
    pub translation: [f32; 2],
    /// Recorded, gate-irrelevant: with unit scale and zero rotation the Maya and
    /// standard compositions coincide, and the gate rejects anything else.
    pub is_maya: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelDesc {
    pub lighting: bool,
    pub diffuse: DiffuseFunction,
    pub attenuation: AttenuationFunction,
    pub lit_mask: u8,
}

/// Max stages the interpreter's uniform arrays hold.
const MAX_STAGES: usize = 8;
/// Max texgens. Two, not four: `FragVertex` packs the texcoords into a single
/// `float4` varying because the reflection rejects array varyings
/// (`src/shaders/reflection/parameters.rs`'s `TypeKind::Array` arm requires a
/// uniform binding). Widening means adding a `texcoord23` varying.
const MAX_TEXGENS: usize = 2;
/// GX lights the channel evaluator supports; `lit_mask` may only select these.
const MAX_LIGHTS: u8 = 2;
/// `TEXMTXn` = `TEXMTX_BASE + 3n`. `GX_IDENTITY` (60) is matched by variant
/// rather than by value here.
const TEXMTX_BASE: u8 = 30;
const IDENTITY_SWAP: [u8; 4] = [0, 1, 2, 3];

/// The master plan's error format for an unimplemented feature.
fn unsupported(name: &str, feature: impl std::fmt::Display) -> BmdError {
    BmdError::Invariant(format!(
        "material {name}: unsupported {feature} — extend tev.slang + tev_ir.rs"
    ))
}

/// A structural problem: the data is inconsistent rather than merely beyond us.
fn invariant(name: &str, what: impl std::fmt::Display) -> BmdError {
    BmdError::Invariant(format!("material {name}: {what}"))
}

/// Gates every material in the chunk, returning one desc per slot.
pub fn describe_all(mat3: &Mat3) -> Result<Vec<TevMaterialDesc>, BmdError> {
    mat3.materials
        .iter()
        .zip(&mat3.names)
        .map(|(m, name)| TevMaterialDesc::try_from((name.as_str(), m)))
        .collect()
}

/// `Material` carries no name, so the name rides along — the error format
/// requires it.
impl TryFrom<(&str, &Material)> for TevMaterialDesc {
    type Error = BmdError;

    fn try_from((name, m): (&str, &Material)) -> Result<Self, Self::Error> {
        let n_stages = count(name, m.num_tev_stages, "num_tev_stages")?;
        let n_texgens = count(name, m.num_tex_gens, "num_tex_gens")?;
        let n_chans = count(name, m.num_color_chans, "num_color_chans")?;

        if n_stages == 0 || n_stages > MAX_STAGES {
            return Err(unsupported(
                name,
                format_args!("{n_stages} TEV stages (max {MAX_STAGES})"),
            ));
        }
        if n_texgens == 0 || n_texgens > MAX_TEXGENS {
            return Err(unsupported(
                name,
                format_args!(
                    "{n_texgens} texgens (max {MAX_TEXGENS}: FragVertex packs both \
                     texcoords into one float4 varying)"
                ),
            ));
        }
        if n_chans != 1 {
            return Err(unsupported(
                name,
                format_args!("{n_chans} color channel pairs (only 1)"),
            ));
        }

        check_material_features(name, m)?;

        // The three lists output.rs compacts with .iter().flatten(). Their
        // siblings (orders, kcsels, kasels, swap_modes) stay slot-indexed, so a
        // hole would silently pair stages[i] with the wrong kcsels[i].
        dense_prefix(name, "tev_stages", &m.tev_stages, n_stages, true)?;
        dense_prefix(name, "tex_coord_gens", &m.tex_coord_gens, n_texgens, true)?;
        // Prefix only: all four color_channels slots are populated on every
        // cl.bdl material while num_color_chans is 1, so a tail check would
        // reject the whole model. Slots 1..4 being live is what makes ALPHA0
        // readable at all.
        dense_prefix(
            name,
            "color_channels",
            &m.color_channels,
            2 * n_chans,
            false,
        )?;

        let texgens = (0..n_texgens)
            .map(|i| texgen_desc(name, m, i))
            .collect::<Result<Vec<_>, _>>()?;
        let stages = (0..n_stages)
            .map(|i| stage_desc(name, m, i, n_texgens))
            .collect::<Result<Vec<_>, _>>()?;

        let mut swap_tables = [IDENTITY_SWAP; 4];
        for (slot, table) in swap_tables.iter_mut().enumerate() {
            if let Some(t) = &m.swap_tables[slot] {
                *table = t.rgba;
            }
        }

        Ok(TevMaterialDesc {
            name: name.to_string(),
            stages,
            texgens,
            konst: konst_colors(name, m)?,
            regs: reg_colors(name, m)?,
            swap_tables,
            chan_color: channel_desc(name, m, 0)?,
            chan_alpha: channel_desc(name, m, 1)?,
            mat_color: m.material_colors[0]
                .ok_or_else(|| invariant(name, "channel pair 0 has no material register color"))?,
            amb_color: m.ambient_colors[0]
                .ok_or_else(|| invariant(name, "channel pair 0 has no ambient register color"))?,
            nbt_enabled: m.nbt_scale.as_ref().is_some_and(|n| n.enable),
        })
    }
}

fn count(name: &str, value: Option<u8>, field: &str) -> Result<usize, BmdError> {
    value
        .map(|v| v as usize)
        .ok_or_else(|| invariant(name, format_args!("{field} is absent")))
}

/// The `Some` slots must fill `count` contiguously from 0. With `tail_empty`,
/// every slot past `count` must additionally be `None`, so the compacted list
/// output.rs emits is exactly `count` long.
fn dense_prefix<T>(
    name: &str,
    list: &str,
    slots: &[Option<T>],
    count: usize,
    tail_empty: bool,
) -> Result<(), BmdError> {
    for (i, slot) in slots.iter().enumerate().take(count) {
        if slot.is_none() {
            return Err(invariant(
                name,
                format_args!(
                    "{list} has a hole at slot {i} of {count}; output.rs's .flatten() \
                     would desync the compacted list from the slot-indexed ones"
                ),
            ));
        }
    }
    if tail_empty {
        for (i, slot) in slots.iter().enumerate().skip(count) {
            if slot.is_some() {
                return Err(invariant(
                    name,
                    format_args!(
                        "{list} slot {i} is populated but only {count} are active; \
                         output.rs's .flatten() would emit it and shift every index"
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn check_material_features(name: &str, m: &Material) -> Result<(), BmdError> {
    if let Some(ind) = &m.indirect
        && ind.enable
    {
        return Err(unsupported(
            name,
            format_args!("indirect texturing ({} stages)", ind.num_stages),
        ));
    }
    if let Some(fog) = &m.fog
        && fog.enable
    {
        return Err(unsupported(name, format_args!("fog ({})", fog.fog_type)));
    }
    if let Some(i) = m.post_tex_coord_gens.iter().position(Option::is_some) {
        return Err(unsupported(name, format_args!("post-texgen at slot {i}")));
    }
    if let Some(i) = m.post_tex_matrices.iter().position(Option::is_some) {
        return Err(unsupported(
            name,
            format_args!("post-texmatrix at slot {i}"),
        ));
    }
    // Measured absent on all 24. If it ever appears we must not silently ignore
    // it: the shader takes its lights from the example, not the material.
    if let Some(i) = m.light_colors.iter().position(Option::is_some) {
        return Err(unsupported(
            name,
            format_args!("per-material light color at slot {i}"),
        ));
    }
    Ok(())
}

fn konst_colors(name: &str, m: &Material) -> Result<[Rgba8; 4], BmdError> {
    let mut out = [[0u8; 4]; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = m.konst_colors[i]
            .ok_or_else(|| invariant(name, format_args!("konst color slot {i} is absent")))?;
    }
    Ok(out)
}

fn reg_colors(name: &str, m: &Material) -> Result<[RgbaS16; 4], BmdError> {
    let mut out = [[0i16; 4]; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = m.tev_colors[i].ok_or_else(|| {
            invariant(name, format_args!("TEV register color slot {i} is absent"))
        })?;
    }
    Ok(out)
}

fn channel_desc(name: &str, m: &Material, slot: usize) -> Result<ChannelDesc, BmdError> {
    let c = m.color_channels[slot]
        .as_ref()
        .ok_or_else(|| invariant(name, format_args!("color channel slot {slot} is absent")))?;

    // Only register-sourced colors are implementable: cl.bdl has no vertex
    // colors at all (phase_03), so there is nothing to interpolate.
    if c.mat_src != ColorSrc::Register {
        return Err(unsupported(
            name,
            format_args!("channel {slot} material color source {}", c.mat_src),
        ));
    }
    if c.amb_src != ColorSrc::Register {
        return Err(unsupported(
            name,
            format_args!("channel {slot} ambient color source {}", c.amb_src),
        ));
    }
    if c.lit_mask & !((1u8 << MAX_LIGHTS) - 1) != 0 {
        return Err(unsupported(
            name,
            format_args!(
                "channel {slot} lit mask {:#04x} (only lights 0..{})",
                c.lit_mask,
                MAX_LIGHTS - 1
            ),
        ));
    }
    // diffuse and attenuation are unrestricted: all three diffuse functions are
    // a table lookup in the shader, and attenuation is deliberately forced to
    // 1.0 there because a hardcoded directional light has no position.
    Ok(ChannelDesc {
        lighting: c.lighting_enabled,
        diffuse: c.diffuse,
        attenuation: c.attenuation,
        lit_mask: c.lit_mask,
    })
}

fn texgen_desc(name: &str, m: &Material, i: usize) -> Result<TexGenDesc, BmdError> {
    let g = m.tex_coord_gens[i]
        .as_ref()
        .ok_or_else(|| invariant(name, format_args!("texcoord {i} has no texgen")))?;

    match (g.ty, g.src) {
        (TexGenType::Mtx2x4, TexGenSrc::Tex0) | (TexGenType::Srtg, TexGenSrc::Color0) => {}
        (TexGenType::Mtx2x4, src) | (TexGenType::Srtg, src) => {
            return Err(unsupported(
                name,
                format_args!("texgen source {src} on texcoord {i} (want TEX0 or COLOR0)"),
            ));
        }
        (ty, src) => {
            return Err(unsupported(
                name,
                format_args!("texgen type {ty} from {src} on texcoord {i}"),
            ));
        }
    }

    let matrix = match g.matrix {
        TexGenMatrix::Identity => None,
        code => {
            let raw = code as u8;
            if raw < TEXMTX_BASE {
                return Err(unsupported(
                    name,
                    format_args!("texgen matrix {code} on texcoord {i} (position matrix)"),
                ));
            }
            let slot = (raw - TEXMTX_BASE) / 3;
            let tm = m
                .tex_matrices
                .get(slot as usize)
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    invariant(
                        name,
                        format_args!("texcoord {i} selects {code}, whose matrix slot is absent"),
                    )
                })?;

            // Only *referenced* matrices are validated. Every cl.bdl material
            // emits an unreferenced identity in slot 0 whose `center` differs
            // between materials; gating it would reject the model for a value
            // nothing reads.
            if tm.projection != TexMtxProjection::Mtx2x4 {
                return Err(unsupported(
                    name,
                    format_args!("texmatrix slot {slot} projection {}", tm.projection),
                ));
            }
            if tm.map_mode != TexMtxMapMode::None {
                return Err(unsupported(
                    name,
                    format_args!("texmatrix slot {slot} map mode {}", tm.map_mode),
                ));
            }
            // Unit scale + zero rotation is what lets the packer's composition
            // collapse to a translate, which in turn makes the `center` term and
            // the Maya-vs-standard convention cancel. The general form is
            // written out in tev_pack, but it is unverified against the game, so
            // the gate keeps it unreachable.
            if tm.scale != [1.0, 1.0] {
                return Err(unsupported(
                    name,
                    format_args!("texmatrix slot {slot} scale {:?} (non-unit)", tm.scale),
                ));
            }
            if tm.rotation != 0 {
                return Err(unsupported(
                    name,
                    format_args!("texmatrix slot {slot} rotation {}", tm.rotation),
                ));
            }

            Some(TexMtxDesc {
                slot,
                center: tm.center,
                scale: tm.scale,
                rotation: tm.rotation,
                translation: tm.translation,
                is_maya: tm.is_maya,
            })
        }
    };

    // SRTG ignores the matrix entirely; a non-identity one there would be a
    // silent no-op rather than an error, so say so.
    if g.ty == TexGenType::Srtg && matrix.is_some() {
        return Err(unsupported(
            name,
            format_args!(
                "texture matrix {} on the SRTG texgen at texcoord {i}",
                g.matrix
            ),
        ));
    }
    Ok(TexGenDesc {
        ty: g.ty,
        src: g.src,
        matrix,
    })
}

fn stage_desc(
    name: &str,
    m: &Material,
    i: usize,
    n_texgens: usize,
) -> Result<TevStageDesc, BmdError> {
    let s = m.tev_stages[i]
        .as_ref()
        .ok_or_else(|| invariant(name, format_args!("stage {i} is absent")))?;
    let order = m.tev_orders[i]
        .as_ref()
        .ok_or_else(|| invariant(name, format_args!("stage {i} has no TEV order")))?;

    check_op(name, i, "color", s.color_op, s.color_bias)?;
    check_op(name, i, "alpha", s.alpha_op, s.alpha_bias)?;

    match order.channel {
        ColorChannelId::Color0
        | ColorChannelId::Alpha0
        | ColorChannelId::Color0A0
        | ColorChannelId::ColorZero
        | ColorChannelId::Null => {}
        chan => {
            return Err(unsupported(
                name,
                format_args!("raster channel {chan} on stage {i} (only channel 0 exists)"),
            ));
        }
    }

    let tex_coord = match order.tex_coord {
        TexCoordId::Null => None,
        id => {
            let n = id as u8;
            if n as usize >= n_texgens {
                return Err(invariant(
                    name,
                    format_args!("stage {i} reads {id} but only {n_texgens} texgens exist"),
                ));
            }
            Some(n)
        }
    };
    let tex_map = match order.tex_map {
        TexMapId::Null => None,
        id => {
            let n = id as u8;
            // P7 decision 1 binds exactly two samplers, and the measured surface
            // never uses more (examples/toon_link.rs asserts texmaps[2..] empty).
            if n >= 2 {
                return Err(unsupported(
                    name,
                    format_args!("{id} on stage {i} (only TEXMAP0 and TEXMAP1 are bound)"),
                ));
            }
            Some(n)
        }
    };

    let reads_texel = s
        .color_in
        .iter()
        .any(|c| matches!(c, CombineColor::TexC | CombineColor::TexA))
        || s.alpha_in.contains(&CombineAlpha::TexA);
    if reads_texel && (tex_map.is_none() || tex_coord.is_none()) {
        return Err(invariant(
            name,
            format_args!(
                "stage {i} selects TEXC/TEXA but its texmap or texcoord is null \
                 (coord={}, map={})",
                order.tex_coord, order.tex_map
            ),
        ));
    }

    // An absent swap mode is GX's default: table 0 for both, which the packer
    // fills with the identity.
    let (ras_swap, tex_swap) = match &m.swap_modes[i] {
        Some(sm) => (sm.ras_sel, sm.tex_sel),
        None => (0, 0),
    };
    for (which, sel) in [("ras", ras_swap), ("tex", tex_swap)] {
        if sel > 3 {
            return Err(invariant(
                name,
                format_args!("stage {i} {which} swap select {sel} is out of range 0..=3"),
            ));
        }
        if m.swap_tables[sel as usize].is_none() {
            return Err(invariant(
                name,
                format_args!("stage {i} references swap table slot {sel}, which is absent"),
            ));
        }
    }

    Ok(TevStageDesc {
        color: ColorCombinerDesc {
            inputs: s.color_in,
            op: s.color_op,
            bias: s.color_bias,
            scale: s.color_scale,
            clamp: s.color_clamp,
            dest: s.color_reg,
        },
        alpha: AlphaCombinerDesc {
            inputs: s.alpha_in,
            op: s.alpha_op,
            bias: s.alpha_bias,
            scale: s.alpha_scale,
            clamp: s.alpha_clamp,
            dest: s.alpha_reg,
        },
        tex_coord,
        tex_map,
        channel: order.channel,
        ras_swap,
        tex_swap,
        kcsel: m.kcsels[i],
        kasel: m.kasels[i],
        tev_mode: s.tev_mode,
    })
}

fn check_op(name: &str, i: usize, half: &str, op: TevOp, bias: TevBias) -> Result<(), BmdError> {
    // GXTevOp >= 8 are the comparison modes: they reinterpret a/b/c/d entirely,
    // so they are a different combiner rather than a table entry.
    if !matches!(op, TevOp::Add | TevOp::Sub) {
        return Err(unsupported(
            name,
            format_args!("TEV comparison op {op} on stage {i} {half}"),
        ));
    }
    if bias == TevBias::HwbCompare {
        return Err(unsupported(
            name,
            format_args!("compare-mode bias on stage {i} {half}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bmd::mat3::{
        AlphaCompare, ColorChannel, Fog, Indirect, TevOrder, TevStage, TevSwapMode, TevSwapTable,
        TexGen, TexMatrix, ZMode,
    };

    /// A minimal material inside the subset, modeled on `mayuL`: one stage, one
    /// identity texgen, one channel pair. Clone-and-mutate it to build the
    /// rejection cases.
    fn accepted_material() -> Material {
        let chan = |lighting, lit_mask| {
            Some(ColorChannel {
                lighting_enabled: lighting,
                mat_src: ColorSrc::Register,
                lit_mask,
                diffuse: DiffuseFunction::Clamp,
                attenuation: AttenuationFunction::Spot,
                amb_src: ColorSrc::Register,
            })
        };
        Material {
            pe_mode: PixelEngineMode::Translucent,
            cull_mode: Some(CullMode::Back),
            num_color_chans: Some(1),
            num_tex_gens: Some(1),
            num_tev_stages: Some(1),
            z_compare_loc: Some(true),
            z_mode: Some(ZMode {
                test: true,
                func: CompareType::LessEqual,
                write: false,
            }),
            dither: Some(true),
            material_colors: [Some([255; 4]), Some([255; 4])],
            color_channels: [
                chan(false, 2),
                chan(false, 2),
                chan(false, 0),
                chan(false, 0),
            ],
            ambient_colors: [Some([50; 4]), Some([0; 4])],
            light_colors: [None; 8],
            tex_coord_gens: [
                Some(TexGen {
                    ty: TexGenType::Mtx2x4,
                    src: TexGenSrc::Tex0,
                    matrix: TexGenMatrix::Identity,
                }),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            post_tex_coord_gens: [None, None, None, None, None, None, None, None],
            tex_matrices: [
                Some(identity_texmtx()),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            post_tex_matrices: [const { None }; 20],
            texture_indices: [Some(40), None, None, None, None, None, None, None],
            konst_colors: [Some([255; 4]); 4],
            kcsels: [KonstColorSel::K0; 16],
            kasels: [KonstAlphaSel::K0A; 16],
            tev_orders: {
                let mut orders = [const { None }; 16];
                orders[0] = Some(TevOrder {
                    tex_coord: TexCoordId::Texcoord0,
                    tex_map: TexMapId::Texmap0,
                    channel: ColorChannelId::Color0A0,
                });
                orders
            },
            tev_colors: [
                Some([255, 255, 255, 255]),
                Some([255, 255, 255, 255]),
                Some([255, 255, 255, 255]),
                Some([0, 0, 0, 0]),
            ],
            tev_stages: {
                let mut stages = [const { None }; 16];
                stages[0] = Some(one_stage());
                stages
            },
            swap_modes: {
                let mut modes = [const { None }; 16];
                modes[0] = Some(TevSwapMode {
                    ras_sel: 0,
                    tex_sel: 0,
                });
                modes
            },
            swap_tables: {
                let mut tables = [const { None }; 16];
                tables[0] = Some(TevSwapTable { rgba: [0, 1, 2, 3] });
                tables
            },
            fog: Some(Fog {
                fog_type: FogType::Linear,
                enable: false,
                center: 0,
                start_z: 0.0,
                end_z: 0.0,
                near_z: 0.0,
                far_z: 0.0,
                color: [0; 4],
                range_adjustments: [0; 10],
            }),
            alpha_compare: Some(AlphaCompare {
                comp0: CompareType::Always,
                ref0: 0,
                op: AlphaOp::Or,
                comp1: CompareType::Always,
                ref1: 0,
            }),
            blend: None,
            nbt_scale: None,
            indirect: Some(Indirect {
                enable: false,
                num_stages: 0,
            }),
        }
    }

    fn identity_texmtx() -> TexMatrix {
        TexMatrix {
            projection: TexMtxProjection::Mtx2x4,
            map_mode: TexMtxMapMode::None,
            is_maya: false,
            center: [0.5, 0.5, 0.5],
            scale: [1.0, 1.0],
            rotation: 0,
            translation: [0.0, 0.0],
            effect_matrix: [0.0; 16],
        }
    }

    /// `mayuL`'s stage: `PREV = ZERO + mix(ZERO, TEXC, RASC)`, alpha
    /// `mix(ZERO, TEXA, RASA)`.
    fn one_stage() -> TevStage {
        TevStage {
            tev_mode: 0,
            color_in: [
                CombineColor::Zero,
                CombineColor::TexC,
                CombineColor::RasC,
                CombineColor::Zero,
            ],
            color_op: TevOp::Add,
            color_bias: TevBias::Zero,
            color_scale: TevScale::Scale1,
            color_clamp: true,
            color_reg: Register::Prev,
            alpha_in: [
                CombineAlpha::Zero,
                CombineAlpha::TexA,
                CombineAlpha::RasA,
                CombineAlpha::Zero,
            ],
            alpha_op: TevOp::Add,
            alpha_bias: TevBias::Zero,
            alpha_scale: TevScale::Scale1,
            alpha_clamp: true,
            alpha_reg: Register::Prev,
        }
    }

    fn describe(m: &Material) -> Result<TevMaterialDesc, BmdError> {
        TevMaterialDesc::try_from(("test", m))
    }

    /// Every rejection must name the material, and every *unsupported* one must
    /// point at the two files to extend.
    fn reject(m: &Material, needle: &str) {
        let err = describe(m).expect_err("expected rejection").to_string();
        assert!(
            err.contains("material test:"),
            "error does not name the material: {err}"
        );
        assert!(
            err.contains(needle),
            "error {err:?} does not mention {needle:?}"
        );
    }

    #[test]
    fn baseline_fixture_is_accepted() {
        let desc = describe(&accepted_material()).unwrap();
        assert_eq!(desc.name, "test");
        assert_eq!(desc.stages.len(), 1);
        assert_eq!(desc.texgens.len(), 1);
        assert_eq!(desc.texgens[0].matrix, None);
        assert_eq!(desc.stages[0].tex_coord, Some(0));
        assert_eq!(desc.stages[0].tex_map, Some(0));
        assert_eq!(desc.stages[0].channel, ColorChannelId::Color0A0);
        // reg_colors[3] rides along unvalidated; MAT3 never loads it.
        assert_eq!(desc.regs[3], [0, 0, 0, 0]);
        // absent swap table slots default to identity
        assert_eq!(desc.swap_tables, [[0, 1, 2, 3]; 4]);
        assert!(!desc.chan_color.lighting);
        assert_eq!(desc.chan_alpha.lit_mask, 2);
        assert_eq!(desc.mat_color, [255; 4]);
        assert_eq!(desc.amb_color, [50; 4]);
    }

    #[test]
    fn fog_is_rejected() {
        let mut m = accepted_material();
        m.fog.as_mut().unwrap().enable = true;
        reject(&m, "unsupported fog (LINEAR)");
        reject(&m, "extend tev.slang + tev_ir.rs");
    }

    #[test]
    fn indirect_is_rejected() {
        let mut m = accepted_material();
        m.indirect = Some(Indirect {
            enable: true,
            num_stages: 2,
        });
        reject(&m, "unsupported indirect texturing (2 stages)");
    }

    #[test]
    fn three_texgens_rejected() {
        let mut m = accepted_material();
        m.num_tex_gens = Some(3);
        reject(&m, "3 texgens (max 2");
    }

    #[test]
    fn two_channel_pairs_rejected() {
        let mut m = accepted_material();
        m.num_color_chans = Some(2);
        reject(&m, "2 color channel pairs (only 1)");
    }

    #[test]
    fn comparison_op_rejected() {
        let mut m = accepted_material();
        m.tev_stages[0].as_mut().unwrap().color_op = TevOp::CompRgb8Gt;
        reject(&m, "TEV comparison op COMP_RGB8_GT on stage 0 color");

        let mut m = accepted_material();
        m.tev_stages[0].as_mut().unwrap().alpha_bias = TevBias::HwbCompare;
        reject(&m, "compare-mode bias on stage 0 alpha");
    }

    #[test]
    fn subtract_op_is_accepted() {
        // SUB is a sign flip, not a different combiner, so the shader implements
        // it even though cl.bdl never uses it.
        let mut m = accepted_material();
        m.tev_stages[0].as_mut().unwrap().color_op = TevOp::Sub;
        assert_eq!(describe(&m).unwrap().stages[0].color.op, TevOp::Sub);
    }

    #[test]
    fn stage_hole_is_an_invariant() {
        let mut m = accepted_material();
        m.num_tev_stages = Some(2);
        m.tev_stages[1] = Some(one_stage());
        m.tev_orders[1] = m.tev_orders[0].clone();
        m.swap_modes[1] = m.swap_modes[0].clone();
        assert!(describe(&m).is_ok(), "two dense stages should be accepted");

        // punch a hole in the prefix
        m.tev_stages[0] = None;
        reject(&m, "tev_stages has a hole at slot 0 of 2");
        reject(&m, "flatten");
    }

    #[test]
    fn stage_past_the_active_count_is_an_invariant() {
        // The tail check: output.rs would compact this into stages.len() == 2
        // while num_tev_stages says 1, shifting every sibling index.
        let mut m = accepted_material();
        m.tev_stages[1] = Some(one_stage());
        reject(&m, "tev_stages slot 1 is populated but only 1 are active");
    }

    #[test]
    fn live_channel_slots_past_the_pair_count_are_fine() {
        // The opposite policy from stages, and the reason it matters: all 24
        // real materials populate four color_channels while num_color_chans is 1.
        let desc = describe(&accepted_material()).unwrap();
        assert_eq!(desc.chan_color.lit_mask, 2);
    }

    #[test]
    fn vertex_color_source_rejected() {
        let mut m = accepted_material();
        m.color_channels[0].as_mut().unwrap().mat_src = ColorSrc::Vertex;
        reject(&m, "channel 0 material color source Vertex");

        let mut m = accepted_material();
        m.color_channels[1].as_mut().unwrap().amb_src = ColorSrc::Vertex;
        reject(&m, "channel 1 ambient color source Vertex");
    }

    #[test]
    fn lit_mask_beyond_two_lights_rejected() {
        let mut m = accepted_material();
        m.color_channels[0].as_mut().unwrap().lit_mask = 0x0F;
        reject(&m, "channel 0 lit mask 0x0f (only lights 0..1)");
    }

    #[test]
    fn missing_swap_table_is_an_invariant() {
        let mut m = accepted_material();
        m.swap_modes[0].as_mut().unwrap().tex_sel = 2;
        reject(&m, "stage 0 references swap table slot 2, which is absent");
    }

    #[test]
    fn third_texmap_rejected() {
        let mut m = accepted_material();
        m.tev_orders[0].as_mut().unwrap().tex_map = TexMapId::Texmap2;
        reject(&m, "TEXMAP2 on stage 0");
    }

    #[test]
    fn texcoord_past_the_texgen_count_is_an_invariant() {
        let mut m = accepted_material();
        m.tev_orders[0].as_mut().unwrap().tex_coord = TexCoordId::Texcoord1;
        reject(&m, "stage 0 reads TEXCOORD1 but only 1 texgens exist");
    }

    #[test]
    fn texel_input_without_a_texture_is_an_invariant() {
        let mut m = accepted_material();
        m.tev_orders[0].as_mut().unwrap().tex_map = TexMapId::Null;
        reject(
            &m,
            "stage 0 selects TEXC/TEXA but its texmap or texcoord is null",
        );
    }

    #[test]
    fn bump_texgen_and_bad_source_rejected() {
        let mut m = accepted_material();
        m.tex_coord_gens[0].as_mut().unwrap().ty = TexGenType::Bump0;
        reject(&m, "texgen type BUMP0 from TEX0 on texcoord 0");

        let mut m = accepted_material();
        m.tex_coord_gens[0].as_mut().unwrap().src = TexGenSrc::Nrm;
        reject(&m, "texgen source NRM on texcoord 0");
    }

    #[test]
    fn non_unit_texmatrix_rejected() {
        let mut m = accepted_material();
        m.tex_coord_gens[0].as_mut().unwrap().matrix = TexGenMatrix::Texmtx1;
        let mut tm = identity_texmtx();
        tm.translation = [-0.05, 0.0];
        m.tex_matrices[1] = Some(tm.clone());
        // referenced, unit scale, zero rotation: accepted, and the slot is
        // derived from the GX code (33 -> (33-30)/3 == 1)
        let desc = describe(&m).unwrap();
        let mtx = desc.texgens[0].matrix.as_ref().unwrap();
        assert_eq!(mtx.slot, 1);
        assert_eq!(mtx.translation, [-0.05, 0.0]);

        let mut scaled = m.clone();
        scaled.tex_matrices[1].as_mut().unwrap().scale = [2.0, 1.0];
        reject(&scaled, "texmatrix slot 1 scale [2.0, 1.0] (non-unit)");

        let mut rotated = m.clone();
        rotated.tex_matrices[1].as_mut().unwrap().rotation = 8192;
        reject(&rotated, "texmatrix slot 1 rotation 8192");

        let mut mapped = m.clone();
        mapped.tex_matrices[1].as_mut().unwrap().map_mode = TexMtxMapMode::Envmap;
        reject(&mapped, "texmatrix slot 1 map mode Envmap");

        let mut absent = m;
        absent.tex_matrices[1] = None;
        reject(
            &absent,
            "texcoord 0 selects TEXMTX1, whose matrix slot is absent",
        );
    }

    #[test]
    fn unreferenced_texmatrix_is_not_validated() {
        // ear's slot 0 is an unreferenced identity whose `center` differs between
        // materials; gating it would reject the model over a value nothing reads.
        let mut m = accepted_material();
        let tm = m.tex_matrices[0].as_mut().unwrap();
        tm.scale = [3.0, 7.0];
        tm.rotation = 1234;
        tm.map_mode = TexMtxMapMode::Projmap;
        assert!(describe(&m).is_ok());
    }

    #[test]
    fn post_texgen_and_light_color_rejected() {
        let mut m = accepted_material();
        m.post_tex_coord_gens[3] = m.tex_coord_gens[0].clone();
        reject(&m, "post-texgen at slot 3");

        let mut m = accepted_material();
        m.light_colors[5] = Some([1, 2, 3, 4]);
        reject(&m, "per-material light color at slot 5");
    }

    #[test]
    #[ignore = "requires extracted assets (just extract-link); run via just link-verify-p2"]
    fn real_tev_subset_accepted() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/link/raw/cl.bdl");
        let Ok(data) = std::fs::read(path) else {
            eprintln!("skipping: {path} not present");
            return;
        };
        let model = crate::bmd::parse_model(&data).unwrap();
        let descs = describe_all(&model.mat3).expect("all 24 materials must pass the gate");
        assert_eq!(descs.len(), 24);

        // `ear` is the canonical toon material: SRTG, two non-identity swap
        // tables, three konst selects, three stages. Cross-check against
        // assets/link/converted/mat3_dump.txt's "=== material 0 ear ===" block.
        let ear = &descs[0];
        assert_eq!(ear.name, "ear");
        assert_eq!(ear.stages.len(), 3);
        assert_eq!(ear.texgens.len(), 2);

        // texgen 0 is the albedo, texgen 1 is the ramp via SRTG on COLOR0
        assert_eq!(ear.texgens[0].ty, TexGenType::Mtx2x4);
        assert_eq!(ear.texgens[0].src, TexGenSrc::Tex0);
        assert_eq!(ear.texgens[0].matrix, None);
        assert_eq!(ear.texgens[1].ty, TexGenType::Srtg);
        assert_eq!(ear.texgens[1].src, TexGenSrc::Color0);
        assert_eq!(ear.texgens[1].matrix, None);

        // stage 0: PREV = clamp(ZERO + mix(C0, KONST, TEXC)) on the ramp,
        // read through the RRR+A swap table
        let s0 = &ear.stages[0];
        assert_eq!(
            s0.color.inputs,
            [
                CombineColor::C0,
                CombineColor::Konst,
                CombineColor::TexC,
                CombineColor::Zero
            ]
        );
        assert_eq!(s0.color.op, TevOp::Add);
        assert_eq!(s0.color.bias, TevBias::Zero);
        assert_eq!(s0.color.scale, TevScale::Scale1);
        assert!(s0.color.clamp);
        assert_eq!(s0.color.dest, Register::Prev);
        assert_eq!(s0.kcsel, KonstColorSel::K0);
        assert_eq!(s0.kasel, KonstAlphaSel::K0A);
        assert_eq!((s0.tex_coord, s0.tex_map), (Some(1), Some(1)));
        assert_eq!(s0.channel, ColorChannelId::Null);
        assert_eq!((s0.ras_swap, s0.tex_swap), (0, 1));

        // stage 1: the albedo modulate. NOTE kasel is K3_A (31), not K0_A —
        // phase_08.md's worked table said K0_A. konst[3] is white so the value
        // is the same, but the selector is not.
        let s1 = &ear.stages[1];
        assert_eq!(
            s1.color.inputs,
            [
                CombineColor::Zero,
                CombineColor::TexC,
                CombineColor::CPrev,
                CombineColor::Zero
            ]
        );
        assert_eq!(
            s1.alpha.inputs,
            [
                CombineAlpha::Zero,
                CombineAlpha::Konst,
                CombineAlpha::TexA,
                CombineAlpha::Zero
            ]
        );
        assert_eq!(s1.kasel, KonstAlphaSel::K3A);
        assert_eq!((s1.tex_coord, s1.tex_map), (Some(0), Some(0)));
        assert_eq!((s1.ras_swap, s1.tex_swap), (0, 0));

        // stage 2: the warm highlight add, ramp read through GGG+A
        let s2 = &ear.stages[2];
        assert_eq!(
            s2.color.inputs,
            [
                CombineColor::Zero,
                CombineColor::Konst,
                CombineColor::TexC,
                CombineColor::CPrev
            ]
        );
        assert_eq!(s2.kcsel, KonstColorSel::K1);
        assert_eq!((s2.ras_swap, s2.tex_swap), (0, 2));

        assert_eq!(
            ear.swap_tables,
            [[0, 1, 2, 3], [0, 0, 0, 3], [1, 1, 1, 3], [0, 1, 2, 3]]
        );
        // reg_colors[0] is REG0 = mid-gray, which is what stage 0's C0 selects.
        // Under the unshifted reading C0 would be white and stage 0 would be a
        // no-op — the failure mode risk #2 warns about.
        assert_eq!(ear.regs[0], [128, 128, 128, 255]);
        assert_eq!(ear.regs[1], [255, 255, 255, 255]);
        assert_eq!(ear.regs[3], [0, 0, 0, 0]);
        assert_eq!(ear.konst[1], [160, 90, 0, 255]);
        assert_eq!(ear.mat_color, [255, 255, 255, 255]);
        assert_eq!(ear.amb_color, [50, 50, 50, 50]);

        // COLOR0 is lit from lights 0+1; ALPHA0 is not lit at all, so RASA is
        // just the material alpha.
        assert!(ear.chan_color.lighting);
        assert_eq!(ear.chan_color.lit_mask, 3);
        assert_eq!(ear.chan_color.diffuse, DiffuseFunction::Clamp);
        assert!(!ear.chan_alpha.lighting);

        // eyeL carries the only non-identity texture matrix in the model, on its
        // texcoord 1 (the hitomi pupil), and the only unclamped stage.
        let eye_l = descs.iter().find(|d| d.name == "eyeL").unwrap();
        assert_eq!(eye_l.stages.len(), 2);
        let mtx = eye_l.texgens[1].matrix.as_ref().unwrap();
        assert_eq!(mtx.slot, 1);
        assert_eq!(mtx.scale, [1.0, 1.0]);
        assert_eq!(mtx.rotation, 0);
        assert_eq!(mtx.translation, [-0.05, 0.0]);
        assert!(!eye_l.stages[1].color.clamp);
        assert!(!eye_l.stages[1].alpha.clamp);

        // The lit/unlit split is total and the two groups are disjoint.
        let lit = descs.iter().filter(|d| d.chan_color.lighting).count();
        assert_eq!(lit, 12, "expected 12 lit materials");
        assert!(
            descs.iter().all(
                |d| d.chan_color.lighting == d.texgens.iter().any(|g| g.ty == TexGenType::Srtg)
            ),
            "SRTG and lighting must coincide exactly"
        );
        assert!(
            descs.iter().all(|d| !d.chan_alpha.lighting),
            "ALPHA0 is unlit on every material, so RASA is the material alpha"
        );
    }
}
