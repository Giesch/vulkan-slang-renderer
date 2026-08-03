//! manifest [`MaterialEntry`] → the shader's [`TevParams`]. The manifest carries
//! TEV state as raw GX bytes; this is where they become the interpreter's
//! uniform arrays.
//!
//! Deliberately a *second* gate. `convert_link`'s `tev_ir.rs` validates at
//! conversion time, but an example loads whatever manifest is on disk — possibly
//! one written before the gate existed — so every value that reaches the GPU is
//! re-checked here against the same subset. The two are allowed to overlap; what
//! they must not do is disagree.
//!
//! Plan: llm_notes/link_rendering/phase_08.md Step 3.

use anyhow::{Context, Result, bail, ensure};
use glam::{UVec4, Vec4};

use crate::generated::shader_atlas::tev::TevParams;
use gx::model_manifest::{
    self as mm, ColorChannelId, ColorSrc, CombineAlpha, CombineColor, KonstAlphaSel, KonstColorSel,
    Register, TevBias, TevOp, TevScale, TexCoordId, TexGenMatrix, TexGenSrc, TexGenType, TexMapId,
};

/// Stage slots the uniform arrays hold. Must match `tev.slang`'s `[8]`.
const MAX_STAGES: usize = 8;
/// Texgen slots. Two, because `FragVertex` packs both texcoords into one
/// `float4` varying — see `tev_ir.rs`'s `MAX_TEXGENS`.
const MAX_TEXGENS: usize = 2;
/// Lights `lit_mask` may select; matches `tev.slang`'s `lightDir[2]`.
const MAX_LIGHTS: u32 = 2;
/// `GX_IDENTITY`, the texture-matrix code meaning "no matrix".
const IDENTITY_MTX: u8 = 60;
/// `TEXMTXn` = `TEXMTX_BASE + 3n`.
const TEXMTX_BASE: u8 = 30;
/// GX ids use 0xFF for "none" across texcoord, texmap and channel.
const GX_NULL_ID: u32 = 0xFF;

/// Builds one material's TEV uniform block.
///
/// `light_dir` / `light_color` are left zeroed: the manifest has no light data
/// (`light_colors` is null on every material — the game writes it per frame from
/// `dKy_tevstr_c`), so the caller fills them in each frame.
pub fn pack(material: &mm::MaterialEntry) -> Result<TevParams> {
    pack_inner(material).with_context(|| format!("packing TEV for material {:?}", material.name))
}

fn pack_inner(material: &mm::MaterialEntry) -> Result<TevParams> {
    let n_stages = material.num_tev_stages as usize;
    let n_texgens = material.num_tex_gens as usize;

    ensure!(
        (1..=MAX_STAGES).contains(&n_stages),
        "{n_stages} TEV stages (want 1..={MAX_STAGES})"
    );
    ensure!(
        (1..=MAX_TEXGENS).contains(&n_texgens),
        "{n_texgens} texgens (want 1..={MAX_TEXGENS})"
    );
    ensure!(
        material.num_color_chans == 1,
        "{} color channel pairs (only 1)",
        material.num_color_chans
    );

    // output.rs builds `stages`, `texgens` and `channels` with .iter().flatten(),
    // which drops None slots, while `orders`, `swap_modes`, `kcsels` and
    // `kasels` stay slot-indexed at 16 entries. If a compacted list is not
    // exactly as long as its GX count, index i of one no longer names the same
    // stage as index i of the other and the whole material is silently
    // misconfigured. tev_ir.rs asserts the same thing converter-side; this is the
    // half that is checkable from a manifest alone.
    ensure!(
        material.tev.stages.len() == n_stages,
        "{} compacted stages for num_tev_stages {n_stages} — the manifest's \
         slot-indexed orders/kcsels/kasels no longer line up",
        material.tev.stages.len()
    );
    ensure!(
        material.texgens.len() == n_texgens,
        "{} compacted texgens for num_tex_gens {n_texgens}",
        material.texgens.len()
    );
    // MAT3's four channel slots are pairs (color0, alpha0, color1, alpha1), so
    // one pair still needs two live slots. Every real material populates all
    // four while num_color_chans is 1, hence >= and not ==.
    ensure!(
        material.channels.len() >= 2,
        "{} color channels; need at least 2 for the COLOR0/ALPHA0 pair",
        material.channels.len()
    );

    let mut params = TevParams {
        stage_color_in: [UVec4::ZERO; MAX_STAGES],
        stage_color_op: [UVec4::ZERO; MAX_STAGES],
        stage_alpha_in: [UVec4::ZERO; MAX_STAGES],
        stage_alpha_op: [UVec4::ZERO; MAX_STAGES],
        stage_dest: [UVec4::ZERO; MAX_STAGES],
        // Inactive stages read nothing: TEXCOORD_NULL / TEXMAP_NULL / COLOR_NULL.
        // The shader stops at `control.x` anyway; this makes a miscount show up
        // as a blank sample rather than as stage 0's texture reused.
        stage_order: [UVec4::new(GX_NULL_ID, GX_NULL_ID, GX_NULL_ID, 0); MAX_STAGES],
        stage_swap: [UVec4::ZERO; MAX_STAGES],
        swap_table: [UVec4::new(0, 1, 2, 3); 4],
        texgen: [UVec4::new(0, 0, IDENTITY_MTX as u32, 0); MAX_TEXGENS],
        texgen_mtx: [
            Vec4::new(1.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.0, 0.0, 0.0),
            Vec4::new(1.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.0, 0.0, 0.0),
        ],
        konst: [Vec4::ZERO; 4],
        // reg[0] is PREV: GX leaves it undefined between materials and MAT3 never
        // loads it, so 0 is the only reproducible choice. reg[1..=3] are filled
        // from reg_colors[0..=2] below.
        reg: [Vec4::ZERO; 4],
        // filled per frame by the caller
        light_dir: [Vec4::ZERO; 2],
        light_color: [Vec4::ZERO; 2],
        chan_control: [UVec4::ZERO; 2],
        chan_mat_color: Vec4::ONE,
        chan_amb_color: Vec4::ZERO,
        control: UVec4::ZERO,
    };

    for i in 0..n_stages {
        pack_stage(material, i, n_texgens, &mut params)?;
    }
    for (slot, table) in params.swap_table.iter_mut().enumerate() {
        if let Some(Some(t)) = material.tev.swap_tables.get(slot) {
            *table = UVec4::new(t[0] as u32, t[1] as u32, t[2] as u32, t[3] as u32);
        }
    }
    for i in 0..n_texgens {
        pack_texgen(material, i, &mut params)?;
    }

    for (i, konst) in params.konst.iter_mut().enumerate() {
        // No shift on the konst path: loadTevKColor is GXTevKColorID(reg), plain.
        let c = flat(material.tev.konst_colors.get(i))
            .with_context(|| format!("konst color slot {i} is absent"))?;
        *konst = rgba8(c);
    }
    for i in 0..3 {
        // The register shift, traced to J3DMatBlock.cpp's
        //   loadTevColor(reg, c) { J3DGDSetTevColorS10(GXTevRegID(reg + 1), c) }
        // and patchTevReg's loop, which stops one short of the array:
        //   reg_colors[0] -> REG0, [1] -> REG1, [2] -> REG2, [3] never loaded.
        // Reading it unshifted is silent: `ear`'s stage 0 becomes
        // lerp(white, white, ramp) and the toon band disappears with no other
        // symptom, which is why the ear test below pins it.
        let c = flat(material.tev.reg_colors.get(i))
            .with_context(|| format!("TEV register color slot {i} is absent"))?;
        // S10 registers legitimately hold values outside [0, 255], so these are
        // scaled, not clamped.
        params.reg[i + 1] = Vec4::new(
            c[0] as f32 / 255.0,
            c[1] as f32 / 255.0,
            c[2] as f32 / 255.0,
            c[3] as f32 / 255.0,
        );
    }

    params.chan_control[0] = pack_channel(material, 0)?;
    params.chan_control[1] = pack_channel(material, 1)?;
    // One RGBA register per channel *pair*, matching GXSetChanMatColor's
    // GX_COLOR0A0: the color channel takes .rgb and the alpha channel takes .a.
    params.chan_mat_color = rgba8(
        flat(material.material_colors.first()).context("channel pair 0 has no material color")?,
    );
    params.chan_amb_color = rgba8(
        flat(material.ambient_colors.first()).context("channel pair 0 has no ambient color")?,
    );
    ensure!(
        material.light_colors.iter().all(Option::is_none),
        "material carries per-light colors, but the shader takes its lights from \
         the example"
    );

    params.control = UVec4::new(n_stages as u32, n_texgens as u32, 1, 0);
    Ok(params)
}

fn pack_stage(
    material: &mm::MaterialEntry,
    i: usize,
    n_texgens: usize,
    params: &mut TevParams,
) -> Result<()> {
    let s = &material.tev.stages[i];

    let color_op = gx::<TevOp>(s.color_op, "color_op")?;
    let alpha_op = gx::<TevOp>(s.alpha_op, "alpha_op")?;
    let color_bias = gx::<TevBias>(s.color_bias, "color_bias")?;
    let alpha_bias = gx::<TevBias>(s.alpha_bias, "alpha_bias")?;
    // Comparison ops reinterpret a/b/c/d entirely; the shader degrades them to
    // ADD, so refuse rather than render something plausible and wrong.
    for (half, op, bias) in [
        ("color", color_op, color_bias),
        ("alpha", alpha_op, alpha_bias),
    ] {
        ensure!(
            matches!(op, TevOp::Add | TevOp::Sub),
            "stage {i} {half}: TEV comparison op {op} is not implemented"
        );
        ensure!(
            bias != TevBias::HwbCompare,
            "stage {i} {half}: compare-mode bias is not implemented"
        );
    }
    // Validate the selectors even though the shader takes raw bytes: a value
    // outside the GX vocabulary would silently hit a `default:` arm.
    for c in s.color_in {
        gx::<CombineColor>(c, "color_in")?;
    }
    for a in s.alpha_in {
        gx::<CombineAlpha>(a, "alpha_in")?;
    }
    gx::<Register>(s.color_reg, "color_reg")?;
    gx::<Register>(s.alpha_reg, "alpha_reg")?;

    params.stage_color_in[i] = u8x4(s.color_in);
    params.stage_alpha_in[i] = u8x4(s.alpha_in);
    params.stage_color_op[i] = UVec4::new(
        s.color_op as u32,
        s.color_bias as u32,
        s.color_scale as u32,
        s.color_clamp as u32,
    );
    params.stage_alpha_op[i] = UVec4::new(
        s.alpha_op as u32,
        s.alpha_bias as u32,
        s.alpha_scale as u32,
        s.alpha_clamp as u32,
    );
    gx::<TevScale>(s.color_scale, "color_scale")?;
    gx::<TevScale>(s.alpha_scale, "alpha_scale")?;

    // kcsels/kasels stay slot-indexed at 16 entries; `stages` is compacted. This
    // is the pairing the length check above protects.
    let kcsel = *material
        .tev
        .kcsels
        .get(i)
        .with_context(|| format!("stage {i} has no konst color select"))?;
    let kasel = *material
        .tev
        .kasels
        .get(i)
        .with_context(|| format!("stage {i} has no konst alpha select"))?;
    gx::<KonstColorSel>(kcsel, "kcsel")?;
    gx::<KonstAlphaSel>(kasel, "kasel")?;
    params.stage_dest[i] = UVec4::new(
        s.color_reg as u32,
        s.alpha_reg as u32,
        kcsel as u32,
        kasel as u32,
    );

    let order = material
        .tev
        .orders
        .get(i)
        .and_then(Option::as_ref)
        .with_context(|| format!("stage {i} has no TEV order"))?;
    let tex_coord = gx::<TexCoordId>(order.tex_coord, "tex_coord")?;
    let tex_map = gx::<TexMapId>(order.tex_map, "tex_map")?;
    let channel = gx::<ColorChannelId>(order.channel, "channel")?;
    if tex_coord != TexCoordId::Null {
        ensure!(
            (order.tex_coord as usize) < n_texgens,
            "stage {i} reads {tex_coord} but only {n_texgens} texgens exist"
        );
    }
    if tex_map != TexMapId::Null {
        // P7 binds exactly two samplers, and no cl.bdl material uses more.
        ensure!(
            order.tex_map < 2,
            "stage {i} reads {tex_map}, but only TEXMAP0 and TEXMAP1 are bound"
        );
    }
    ensure!(
        matches!(
            channel,
            ColorChannelId::Color0
                | ColorChannelId::Alpha0
                | ColorChannelId::Color0A0
                | ColorChannelId::ColorZero
                | ColorChannelId::Null
        ),
        "stage {i} reads raster channel {channel}, but only channel pair 0 exists"
    );
    params.stage_order[i] = UVec4::new(
        order.tex_coord as u32,
        order.tex_map as u32,
        order.channel as u32,
        0,
    );

    // An absent swap mode is GX's default: table 0 on both sides.
    let (ras_sel, tex_sel) = match material.tev.swap_modes.get(i).and_then(Option::as_ref) {
        Some(sm) => (sm.ras_sel, sm.tex_sel),
        None => (0, 0),
    };
    for (which, sel) in [("ras", ras_sel), ("tex", tex_sel)] {
        ensure!(
            sel < 4,
            "stage {i} {which} swap select {sel} is out of range"
        );
    }
    params.stage_swap[i] = UVec4::new(ras_sel as u32, tex_sel as u32, 0, 0);
    Ok(())
}

fn pack_texgen(material: &mm::MaterialEntry, i: usize, params: &mut TevParams) -> Result<()> {
    let g = &material.texgens[i];
    let ty = gx::<TexGenType>(g.ty, "texgen type")?;
    let src = gx::<TexGenSrc>(g.src, "texgen source")?;
    let matrix = gx::<TexGenMatrix>(g.matrix, "texgen matrix")?;
    ensure!(
        matches!(
            (ty, src),
            (TexGenType::Mtx2x4, TexGenSrc::Tex0) | (TexGenType::Srtg, TexGenSrc::Color0)
        ),
        "texcoord {i}: texgen {ty} from {src} is not implemented"
    );
    params.texgen[i] = UVec4::new(g.ty as u32, g.src as u32, g.matrix as u32, 0);

    if matrix == TexGenMatrix::Identity {
        return Ok(());
    }
    ensure!(
        g.matrix >= TEXMTX_BASE && (g.matrix - TEXMTX_BASE).is_multiple_of(3),
        "texcoord {i} selects {matrix}, which is not a texture matrix"
    );
    let slot = (g.matrix - TEXMTX_BASE) / 3;
    let tm = material
        .tex_matrices
        .iter()
        .find(|tm| tm.slot == slot)
        .with_context(|| format!("texcoord {i} selects {matrix}, whose slot {slot} is absent"))?;
    let rows = texmtx_rows(tm);
    params.texgen_mtx[2 * i] = rows[0];
    params.texgen_mtx[2 * i + 1] = rows[1];
    Ok(())
}

/// A J3D `TexMatrix` as the two MTX2x4 rows the shader dots against
/// `float4(uv, 1, 1)`, so the translation lands in `.z`.
///
/// The general SRT-about-a-center form:
///     p' = R(rot) · S(scale) · (p − center) + center + translation
///
/// The converter gate guarantees unit scale and zero rotation, which collapses
/// this to a pure translate *and* makes the `center` term cancel (R·S = I, so
/// `center − center`) along with the Maya-vs-standard composition question. The
/// general form is written out and unit-tested for shape, but its rotation and
/// scale branches are **unreachable on cl.bdl and therefore unverified against
/// the game** — the gate is what keeps them unreachable. The rotation unit
/// (s16, π/32768 radians per step) is the J3D convention noclip also uses;
/// nothing in cl.bdl exercises it, since every rotation is 0.
fn texmtx_rows(tm: &mm::TexMatrixState) -> [Vec4; 2] {
    let rot = (tm.rotation as i16) as f32 * std::f32::consts::PI / 32768.0;
    let (sin, cos) = rot.sin_cos();
    let (sx, sy) = (tm.scale[0], tm.scale[1]);
    let (m00, m01) = (sx * cos, -sy * sin);
    let (m10, m11) = (sx * sin, sy * cos);
    let (cx, cy) = (tm.center[0], tm.center[1]);

    // `center − R·S·center` is algebraically exactly zero when the linear part
    // is the identity, but evaluating it in f32 is not: `(t + c) − c` drops
    // bits, so eyeL's clean −0.05 would ship as −0.050000012. That is the only
    // branch cl.bdl ever takes, so keep it exact instead of merely close.
    let identity_linear = m00 == 1.0 && m01 == 0.0 && m10 == 0.0 && m11 == 1.0;
    let (dx, dy) = if identity_linear {
        (0.0, 0.0)
    } else {
        (cx - (m00 * cx + m01 * cy), cy - (m10 * cx + m11 * cy))
    };
    let tx = tm.translation[0] + dx;
    let ty = tm.translation[1] + dy;
    [Vec4::new(m00, m01, tx, 0.0), Vec4::new(m10, m11, ty, 0.0)]
}

/// Slot 0 is GX_COLOR0 and slot 1 is GX_ALPHA0 — MAT3's channels are stored as
/// (color, alpha) pairs, which is why one pair reads two slots.
fn pack_channel(material: &mm::MaterialEntry, slot: usize) -> Result<UVec4> {
    let c = &material.channels[slot];
    // Only register-sourced colors are implementable, and cl.bdl has no vertex
    // colors at all, so there would be nothing to interpolate.
    ensure!(
        c.mat_src == ColorSrc::Register,
        "channel {slot}: material color source {} is not implemented",
        c.mat_src
    );
    ensure!(
        c.amb_src == ColorSrc::Register,
        "channel {slot}: ambient color source {} is not implemented",
        c.amb_src
    );
    ensure!(
        u32::from(c.lit_mask) & !((1 << MAX_LIGHTS) - 1) == 0,
        "channel {slot}: lit mask {:#04x} selects a light beyond 0..{}",
        c.lit_mask,
        MAX_LIGHTS - 1
    );
    Ok(UVec4::new(
        c.lighting_enabled as u32,
        c.diffuse as u32,
        c.attenuation as u32,
        c.lit_mask as u32,
    ))
}

// --- equation rendering ------------------------------------------------------

/// The material's per-stage equations in `mat3_dump.txt`'s exact notation, one
/// line per half: `"stage0 C: PREV = clamp(ZERO + mix(C0, KONST, TEXC))"`.
///
/// Re-implemented rather than shared with the converter's
/// `bmd::mat3_dump::equation`: that lives inside the `convert_link` *binary*, so
/// neither the library nor an example can import it, and `mat3_dump.txt` is
/// covered by `scripts/link_converted.sha256` so the original must not move. The
/// tests below pin the literal strings against the real dump, which is what
/// keeps the two from drifting.
///
/// The one thing this adds over the dump: the *resolved* konst selector. The
/// dump prints a bare `KONST` for every konst input, so it cannot by itself tell
/// you whether a stage took K0 or K3_A.
pub fn stage_equations(material: &mm::MaterialEntry) -> Result<Vec<String>> {
    let n_stages = material.num_tev_stages as usize;
    ensure!(
        material.tev.stages.len() == n_stages,
        "{} compacted stages for num_tev_stages {n_stages}",
        material.tev.stages.len()
    );

    let mut out = Vec::with_capacity(n_stages * 2);
    for (i, s) in material.tev.stages.iter().enumerate() {
        let color_names = names(s.color_in, |v| gx::<CombineColor>(v, "color_in"))?;
        let alpha_names = names(s.alpha_in, |v| gx::<CombineAlpha>(v, "alpha_in"))?;
        out.push(format!(
            "stage{i} C: {}",
            equation(
                &color_names,
                gx::<TevOp>(s.color_op, "color_op")?,
                gx::<TevBias>(s.color_bias, "color_bias")?,
                gx::<TevScale>(s.color_scale, "color_scale")?,
                s.color_clamp,
                gx::<Register>(s.color_reg, "color_reg")?,
            )
        ));
        out.push(format!(
            "stage{i} A: {}",
            equation(
                &alpha_names,
                gx::<TevOp>(s.alpha_op, "alpha_op")?,
                gx::<TevBias>(s.alpha_bias, "alpha_bias")?,
                gx::<TevScale>(s.alpha_scale, "alpha_scale")?,
                s.alpha_clamp,
                gx::<Register>(s.alpha_reg, "alpha_reg")?,
            )
        ));
    }
    Ok(out)
}

/// `reg = clamp?(((d ± mix(a, b, c)) + bias) · scale)` — byte-for-byte the form
/// `bmd::mat3_dump::equation` renders.
fn equation(
    inputs: &[String; 4],
    op: TevOp,
    bias: TevBias,
    scale: TevScale,
    clamp: bool,
    reg: Register,
) -> String {
    let [a, b, c, d] = inputs;
    let core = match op {
        TevOp::Add => format!("{d} + mix({a}, {b}, {c})"),
        TevOp::Sub => format!("{d} - mix({a}, {b}, {c})"),
        _ => format!("compare({op}: {a}, {b} ? {c} : 0) + {d}"),
    };
    let biased = match bias {
        TevBias::Zero => core,
        TevBias::AddHalf => format!("{core} + 0.5"),
        TevBias::SubHalf => format!("{core} - 0.5"),
        TevBias::HwbCompare => format!("{core} [compare-mode bias]"),
    };
    let scaled = match scale {
        TevScale::Scale1 => biased,
        TevScale::Scale2 => format!("({biased}) * 2"),
        TevScale::Scale4 => format!("({biased}) * 4"),
        TevScale::Divide2 => format!("({biased}) / 2"),
    };
    let clamped = if clamp {
        format!("clamp({scaled})")
    } else {
        scaled
    };
    format!("{reg} = {clamped}")
}

/// The konst selectors a stage resolves to, for the isolation printout — the
/// piece `mat3_dump.txt` does not carry.
pub fn stage_konst_selects(material: &mm::MaterialEntry, stage: usize) -> Result<(String, String)> {
    let kcsel = gx::<KonstColorSel>(
        *material
            .tev
            .kcsels
            .get(stage)
            .context("no konst color select")?,
        "kcsel",
    )?;
    let kasel = gx::<KonstAlphaSel>(
        *material
            .tev
            .kasels
            .get(stage)
            .context("no konst alpha select")?,
        "kasel",
    )?;
    Ok((kcsel.to_string(), kasel.to_string()))
}

// --- small helpers -----------------------------------------------------------

/// Parse-don't-validate on a raw manifest byte.
fn gx<T>(value: u8, field: &str) -> Result<T>
where
    T: TryFrom<u8, Error = mm::GxEnumError>,
{
    match T::try_from(value) {
        Ok(v) => Ok(v),
        Err(e) => bail!("{field}: {e}"),
    }
}

fn names<T: std::fmt::Display>(
    raw: [u8; 4],
    parse: impl Fn(u8) -> Result<T>,
) -> Result<[String; 4]> {
    Ok([
        parse(raw[0])?.to_string(),
        parse(raw[1])?.to_string(),
        parse(raw[2])?.to_string(),
        parse(raw[3])?.to_string(),
    ])
}

/// The manifest's colors are `Vec<Option<_>>`; a present slot in range or None.
fn flat<T: Copy>(slot: Option<&Option<T>>) -> Option<T> {
    slot.copied().flatten()
}

fn u8x4(v: [u8; 4]) -> UVec4 {
    UVec4::new(v[0] as u32, v[1] as u32, v[2] as u32, v[3] as u32)
}

fn rgba8(c: [u8; 4]) -> Vec4 {
    Vec4::new(
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
        c[3] as f32 / 255.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gx::model_manifest::{
        AttenuationFunction, ChannelState, CompareType, CullMode, DiffuseFunction, PixelEngineMode,
        SwapModeState, TevConfig, TevOrderState, TevStageState, TexGenState, TexMatrixState,
    };

    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn channel(lighting: bool, lit_mask: u8) -> ChannelState {
        ChannelState {
            lighting_enabled: lighting,
            mat_src: ColorSrc::Register,
            amb_src: ColorSrc::Register,
            diffuse: DiffuseFunction::Clamp,
            attenuation: AttenuationFunction::Spot,
            lit_mask,
        }
    }

    fn stage(color_in: [u8; 4], alpha_in: [u8; 4]) -> TevStageState {
        TevStageState {
            color_in,
            color_op: 0,
            color_bias: 0,
            color_scale: 0,
            color_clamp: true,
            color_reg: 0,
            alpha_in,
            alpha_op: 0,
            alpha_bias: 0,
            alpha_scale: 0,
            alpha_clamp: true,
            alpha_reg: 0,
        }
    }

    fn order(tex_coord: u8, tex_map: u8, channel: u8) -> Option<TevOrderState> {
        Some(TevOrderState {
            tex_coord,
            tex_map,
            channel,
        })
    }

    /// A one-stage, one-texgen material inside the subset. The slot-indexed
    /// lists are deliberately full-length (16 / 4) while `stages` and `texgens`
    /// are compacted, exactly as `output.rs` writes them.
    fn fixture() -> mm::MaterialEntry {
        mm::MaterialEntry {
            name: "fixture".into(),
            record: 0,
            pe_mode: PixelEngineMode::Opaque,
            cull: CullMode::Back,
            z_test: true,
            z_func: CompareType::LessEqual,
            z_write: true,
            z_compare_early: true,
            blend: None,
            alpha_compare: None,
            dither: false,
            num_tev_stages: 1,
            num_tex_gens: 1,
            num_color_chans: 1,
            texmaps: vec![Some(0), None],
            tev: TevConfig {
                stages: vec![stage([15, 8, 10, 15], [7, 4, 5, 7])],
                orders: {
                    let mut o = vec![None; 16];
                    o[0] = order(0, 0, 4);
                    o
                },
                konst_colors: vec![Some(WHITE); 4],
                reg_colors: vec![Some([0; 4]); 4],
                kcsels: vec![12; 16],
                kasels: vec![28; 16],
                swap_modes: {
                    let mut s = vec![None; 16];
                    s[0] = Some(SwapModeState {
                        ras_sel: 0,
                        tex_sel: 0,
                    });
                    s
                },
                swap_tables: {
                    let mut t = vec![None; 16];
                    t[0] = Some([0, 1, 2, 3]);
                    t
                },
            },
            texgens: vec![TexGenState {
                ty: 1,
                src: 4,
                matrix: 60,
            }],
            tex_matrices: vec![],
            channels: vec![channel(false, 2); 4],
            material_colors: vec![Some(WHITE), Some(WHITE)],
            ambient_colors: vec![Some([50; 4]), Some([0; 4])],
            light_colors: vec![None; 8],
        }
    }

    #[test]
    fn packs_stage_selectors_and_ops() {
        let mut m = fixture();
        m.num_tev_stages = 2;
        m.tev.stages = vec![
            stage([2, 14, 8, 15], [7, 7, 7, 7]),
            TevStageState {
                color_bias: 1,  // ADDHALF
                color_scale: 2, // SCALE_4
                color_clamp: false,
                color_reg: 1, // REG0
                alpha_op: 1,  // SUB
                alpha_reg: 3, // REG2
                ..stage([15, 8, 0, 15], [0, 6, 4, 7])
            },
        ];
        m.tev.orders[1] = order(0, 0, 255);

        let p = pack(&m).unwrap();
        assert_eq!(p.control, UVec4::new(2, 1, 1, 0));
        assert_eq!(p.stage_color_in[0], UVec4::new(2, 14, 8, 15));
        assert_eq!(p.stage_alpha_in[0], UVec4::new(7, 7, 7, 7));
        assert_eq!(p.stage_color_in[1], UVec4::new(15, 8, 0, 15));
        // op, bias, scale, clamp
        assert_eq!(p.stage_color_op[0], UVec4::new(0, 0, 0, 1));
        assert_eq!(p.stage_color_op[1], UVec4::new(0, 1, 2, 0));
        assert_eq!(p.stage_alpha_op[1], UVec4::new(1, 0, 0, 1));
        // colorReg, alphaReg, kcsel, kasel
        assert_eq!(p.stage_dest[1], UVec4::new(1, 3, 12, 28));
        assert_eq!(p.stage_order[1], UVec4::new(0, 0, 255, 0));

        // slots past num_tev_stages stay null rather than aliasing stage 0
        for i in 2..MAX_STAGES {
            assert_eq!(p.stage_order[i], UVec4::new(255, 255, 255, 0), "slot {i}");
        }
    }

    #[test]
    fn walks_by_count_not_len() {
        // The hazard: `stages` is compacted while orders/kcsels/kasels keep all
        // 16 slots. Index 1 of the compacted list must still pair with index 1
        // of the slot-indexed ones.
        let mut m = fixture();
        m.num_tev_stages = 2;
        m.tev.stages = vec![stage([15, 8, 10, 15], [7, 4, 5, 7]); 2];
        m.tev.orders[1] = order(0, 0, 4);
        m.tev.kcsels[1] = 13;
        m.tev.kasels[1] = 31;

        let p = pack(&m).unwrap();
        assert_eq!(p.stage_dest[0].z, 12);
        assert_eq!(p.stage_dest[0].w, 28);
        assert_eq!(p.stage_dest[1].z, 13);
        assert_eq!(p.stage_dest[1].w, 31);
    }

    #[test]
    fn compacted_length_mismatch_is_an_error() {
        let mut m = fixture();
        m.num_tev_stages = 2; // but `stages` still has one entry
        let err = pack(&m).unwrap_err().to_string();
        assert!(
            err.contains("packing TEV for material \"fixture\""),
            "{err}"
        );
        let chain = format!("{:#}", pack(&m).unwrap_err());
        assert!(chain.contains("compacted stages"), "{chain}");
    }

    #[test]
    fn register_colors_shift_by_one() {
        let mut m = fixture();
        m.tev.reg_colors = vec![
            Some([128, 128, 128, 255]),
            Some([255, 0, 0, 255]),
            Some([0, 255, 0, 255]),
            // never loaded by MAT3; must not appear anywhere in reg[]
            Some([7, 7, 7, 7]),
        ];
        let p = pack(&m).unwrap();
        // PREV gets no MAT3 value
        assert_eq!(p.reg[0], Vec4::ZERO);
        assert_eq!(p.reg[1], Vec4::splat(128.0 / 255.0).with_w(1.0));
        assert_eq!(p.reg[2], Vec4::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(p.reg[3], Vec4::new(0.0, 1.0, 0.0, 1.0));
        assert!(
            p.reg.iter().all(|r| r.x != 7.0 / 255.0),
            "reg_colors[3] must never be loaded"
        );
    }

    #[test]
    fn register_colors_are_not_clamped() {
        // S10 registers legitimately hold values outside [0, 255].
        let mut m = fixture();
        m.tev.reg_colors = vec![
            Some([0; 4]),
            Some([0; 4]),
            Some([-1024, 1023, 0, 0]),
            Some([0; 4]),
        ];
        let p = pack(&m).unwrap();
        assert_eq!(p.reg[3].x, -1024.0 / 255.0);
        assert_eq!(p.reg[3].y, 1023.0 / 255.0);
    }

    #[test]
    fn konst_colors_map_without_shift() {
        let mut m = fixture();
        m.tev.konst_colors = vec![
            Some([1, 2, 3, 4]),
            Some([160, 90, 0, 255]),
            Some(WHITE),
            Some(WHITE),
        ];
        let p = pack(&m).unwrap();
        assert_eq!(p.konst[0], rgba8([1, 2, 3, 4]));
        assert_eq!(p.konst[1], rgba8([160, 90, 0, 255]));
    }

    #[test]
    fn swap_tables_default_to_identity() {
        let mut m = fixture();
        m.tev.swap_tables[1] = Some([0, 0, 0, 3]);
        m.tev.swap_tables[2] = Some([1, 1, 1, 3]);
        let p = pack(&m).unwrap();
        assert_eq!(p.swap_table[0], UVec4::new(0, 1, 2, 3));
        assert_eq!(p.swap_table[1], UVec4::new(0, 0, 0, 3));
        assert_eq!(p.swap_table[2], UVec4::new(1, 1, 1, 3));
        // absent slot 3 falls back to the identity, not to zeros
        assert_eq!(p.swap_table[3], UVec4::new(0, 1, 2, 3));
    }

    #[test]
    fn texgen_matrix_code_to_rows() {
        // identity: raw code preserved, rows left as the identity
        let p = pack(&fixture()).unwrap();
        assert_eq!(p.texgen[0], UVec4::new(1, 4, 60, 0));
        assert_eq!(p.texgen_mtx[0], Vec4::new(1.0, 0.0, 0.0, 0.0));
        assert_eq!(p.texgen_mtx[1], Vec4::new(0.0, 1.0, 0.0, 0.0));

        // TEXMTX1 == 33 -> slot (33 - 30) / 3 == 1
        let mut m = fixture();
        m.num_tex_gens = 2;
        m.texgens.push(TexGenState {
            ty: 1,
            src: 4,
            matrix: 33,
        });
        m.tex_matrices = vec![TexMatrixState {
            slot: 1,
            center: [0.5, 0.5, 0.5],
            scale: [1.0, 1.0],
            rotation: 0,
            translation: [-0.05, 0.0],
            effect_matrix: [0.0; 16],
        }];
        let p = pack(&m).unwrap();
        assert_eq!(p.texgen[1], UVec4::new(1, 4, 33, 0));
        // the center term cancels under unit scale and zero rotation
        assert_eq!(p.texgen_mtx[2], Vec4::new(1.0, 0.0, -0.05, 0.0));
        assert_eq!(p.texgen_mtx[3], Vec4::new(0.0, 1.0, 0.0, 0.0));

        // a selected slot that is not present is an error, not a silent identity
        let mut missing = m.clone();
        missing.tex_matrices.clear();
        let err = format!("{:#}", pack(&missing).unwrap_err());
        assert!(err.contains("whose slot 1 is absent"), "{err}");

        // PNMTX1 (3) is a valid GXTexMtx but a *position* matrix, so it parses
        // and then fails the "is it 30 + 3n" check.
        let mut pnmtx = m.clone();
        pnmtx.texgens[1].matrix = 3;
        let err = format!("{:#}", pack(&pnmtx).unwrap_err());
        assert!(
            err.contains("selects PNMTX1, which is not a texture matrix"),
            "{err}"
        );

        // 31 is not a GXTexMtx value at all, so the typed parse rejects it first.
        let mut bad = m;
        bad.texgens[1].matrix = 31;
        let err = format!("{:#}", pack(&bad).unwrap_err());
        assert!(err.contains("invalid TexGenMatrix value 0x1f"), "{err}");
    }

    #[test]
    fn texmatrix_general_composition() {
        // Gate-unreachable on cl.bdl (non-unit scale, non-zero rotation), so this
        // pins the algebra rather than any observed behavior.
        // A full turn is 65536, so 16384 is +90 degrees.
        let tm = TexMatrixState {
            slot: 0,
            center: [0.5, 0.25, 0.0],
            scale: [2.0, 3.0],
            rotation: 16384,
            translation: [0.1, 0.2],
            effect_matrix: [0.0; 16],
        };
        let rows = texmtx_rows(&tm);
        // cos == 0, sin == 1 -> [[0, -3], [2, 0]]
        assert!((rows[0].x - 0.0).abs() < 1e-6, "{rows:?}");
        assert!((rows[0].y - -3.0).abs() < 1e-6, "{rows:?}");
        assert!((rows[1].x - 2.0).abs() < 1e-6, "{rows:?}");
        assert!((rows[1].y - 0.0).abs() < 1e-6, "{rows:?}");
        // tx = 0.1 + 0.5 - (0*0.5 + -3*0.25) = 1.35
        assert!((rows[0].z - 1.35).abs() < 1e-6, "{rows:?}");
        // ty = 0.2 + 0.25 - (2*0.5 + 0*0.25) = -0.55
        assert!((rows[1].z - -0.55).abs() < 1e-6, "{rows:?}");

        // 8192 is 45 degrees, not 90 — the unit is pi/32768 per step.
        let mut half = tm;
        half.rotation = 8192;
        half.scale = [1.0, 1.0];
        let rows = texmtx_rows(&half);
        let r2 = std::f32::consts::FRAC_1_SQRT_2;
        assert!((rows[0].x - r2).abs() < 1e-6, "{rows:?}");
        assert!((rows[1].x - r2).abs() < 1e-6, "{rows:?}");
    }

    #[test]
    fn identity_linear_part_keeps_the_translation_exact() {
        // Unit scale and zero rotation is the only branch cl.bdl takes, and the
        // center term cancels algebraically — so it must cancel in f32 too,
        // rather than shipping eyeL's -0.05 as -0.050000012.
        let tm = TexMatrixState {
            slot: 1,
            center: [0.5, 0.5, 0.5],
            scale: [1.0, 1.0],
            rotation: 0,
            translation: [-0.05, 0.0],
            effect_matrix: [0.0; 16],
        };
        let rows = texmtx_rows(&tm);
        assert_eq!(rows[0].z, -0.05_f32);
        assert_eq!(rows[1].z, 0.0_f32);
    }

    #[test]
    fn channel_pair_is_color0_and_alpha0() {
        let mut m = fixture();
        m.channels[0] = channel(true, 3);
        m.channels[1] = channel(false, 2);
        let p = pack(&m).unwrap();
        // lighting, diffuseFn (Clamp == 2), attnFn (Spot == 1), litMask
        assert_eq!(p.chan_control[0], UVec4::new(1, 2, 1, 3));
        assert_eq!(p.chan_control[1], UVec4::new(0, 2, 1, 2));
        // one register per pair
        assert_eq!(p.chan_mat_color, Vec4::ONE);
        assert_eq!(p.chan_amb_color, rgba8([50; 4]));
    }

    #[test]
    fn channel_rejections() {
        let mut m = fixture();
        m.num_color_chans = 2;
        assert!(pack(&m).is_err());

        let mut m = fixture();
        m.channels[0].mat_src = ColorSrc::Vertex;
        let err = format!("{:#}", pack(&m).unwrap_err());
        assert!(
            err.contains("channel 0: material color source Vertex"),
            "{err}"
        );

        let mut m = fixture();
        m.channels[1].lit_mask = 0x0F;
        let err = format!("{:#}", pack(&m).unwrap_err());
        assert!(err.contains("channel 1: lit mask 0x0f"), "{err}");

        let mut m = fixture();
        m.light_colors[0] = Some(WHITE);
        let err = format!("{:#}", pack(&m).unwrap_err());
        assert!(err.contains("per-light colors"), "{err}");
    }

    #[test]
    fn stage_rejections() {
        let mut m = fixture();
        m.tev.stages[0].color_op = 0xE; // COMP_RGB8_GT
        let err = format!("{:#}", pack(&m).unwrap_err());
        assert!(err.contains("TEV comparison op COMP_RGB8_GT"), "{err}");

        let mut m = fixture();
        m.tev.orders[0] = order(1, 0, 4); // only one texgen exists
        let err = format!("{:#}", pack(&m).unwrap_err());
        assert!(
            err.contains("reads TEXCOORD1 but only 1 texgens exist"),
            "{err}"
        );

        let mut m = fixture();
        m.tev.orders[0] = order(0, 2, 4);
        let err = format!("{:#}", pack(&m).unwrap_err());
        assert!(err.contains("only TEXMAP0 and TEXMAP1 are bound"), "{err}");

        let mut m = fixture();
        m.tev.stages[0].color_in[0] = 0x40; // not a GXTevColorArg
        let err = format!("{:#}", pack(&m).unwrap_err());
        assert!(err.contains("color_in: invalid CombineColor"), "{err}");
    }

    /// `ear`'s real values, inlined. The converted assets are gitignored and CI
    /// has none, so reading them from disk would make this test vacuous there.
    /// Cross-check against `assets/link/converted/mat3_dump.txt`'s
    /// "=== material 0 ear (record 0) ===" block.
    fn ear() -> mm::MaterialEntry {
        let mut m = fixture();
        m.name = "ear".into();
        m.num_tev_stages = 3;
        m.num_tex_gens = 2;
        m.texmaps = vec![Some(34), Some(35)];
        m.tev.stages = vec![
            stage([2, 14, 8, 15], [7, 7, 7, 7]),
            stage([15, 8, 0, 15], [7, 6, 4, 7]),
            stage([15, 14, 8, 0], [0, 7, 7, 7]),
        ];
        m.tev.orders[0] = order(1, 1, 255);
        m.tev.orders[1] = order(0, 0, 255);
        m.tev.orders[2] = order(1, 1, 255);
        m.tev.kcsels[0] = 12;
        m.tev.kcsels[1] = 12;
        m.tev.kcsels[2] = 13;
        m.tev.kasels[0] = 28;
        m.tev.kasels[1] = 31;
        m.tev.kasels[2] = 28;
        m.tev.swap_modes[0] = Some(SwapModeState {
            ras_sel: 0,
            tex_sel: 1,
        });
        m.tev.swap_modes[1] = Some(SwapModeState {
            ras_sel: 0,
            tex_sel: 0,
        });
        m.tev.swap_modes[2] = Some(SwapModeState {
            ras_sel: 0,
            tex_sel: 2,
        });
        m.tev.swap_tables[0] = Some([0, 1, 2, 3]);
        m.tev.swap_tables[1] = Some([0, 0, 0, 3]);
        m.tev.swap_tables[2] = Some([1, 1, 1, 3]);
        m.tev.konst_colors = vec![
            Some(WHITE),
            Some([160, 90, 0, 255]),
            Some(WHITE),
            Some(WHITE),
        ];
        m.tev.reg_colors = vec![
            Some([128, 128, 128, 255]),
            Some([255, 255, 255, 255]),
            Some([255, 255, 255, 255]),
            Some([0, 0, 0, 0]),
        ];
        m.texgens = vec![
            TexGenState {
                ty: 1,
                src: 4,
                matrix: 60,
            },
            // SRTG from COLOR0, identity matrix — the ramp lookup
            TexGenState {
                ty: 10,
                src: 19,
                matrix: 60,
            },
        ];
        m.channels[0] = channel(true, 3);
        m.channels[1] = channel(false, 2);
        m
    }

    #[test]
    fn ear_end_to_end() {
        let p = pack(&ear()).unwrap();
        assert_eq!(p.control, UVec4::new(3, 2, 1, 0));

        assert_eq!(
            [
                p.stage_color_in[0],
                p.stage_color_in[1],
                p.stage_color_in[2]
            ],
            [
                UVec4::new(2, 14, 8, 15),
                UVec4::new(15, 8, 0, 15),
                UVec4::new(15, 14, 8, 0)
            ]
        );
        // colorReg, alphaReg, kcsel, kasel. Stage 1's kasel is 31 = K3_A, not
        // K0_A: phase_08.md's worked table had it wrong. konst[3] is white so the
        // *value* is identical, which is exactly why only the selector catches it.
        assert_eq!(
            [p.stage_dest[0], p.stage_dest[1], p.stage_dest[2]],
            [
                UVec4::new(0, 0, 12, 28),
                UVec4::new(0, 0, 12, 31),
                UVec4::new(0, 0, 13, 28)
            ]
        );
        // the ramp stages read through the RRR and GGG swap tables
        assert_eq!(
            [p.stage_swap[0].y, p.stage_swap[1].y, p.stage_swap[2].y],
            [1, 0, 2]
        );
        assert_eq!(
            p.swap_table,
            [
                UVec4::new(0, 1, 2, 3),
                UVec4::new(0, 0, 0, 3),
                UVec4::new(1, 1, 1, 3),
                UVec4::new(0, 1, 2, 3),
            ]
        );
        // stage 0 selects C0, which is REG0, which is reg_colors[0] = mid-gray.
        // Unshifted it would be white, making stage 0 lerp(white, white, ramp) —
        // a no-op, and the whole cel effect would silently vanish.
        assert_eq!(p.reg[1], Vec4::splat(128.0 / 255.0).with_w(1.0));
        assert_eq!(p.konst[1], rgba8([160, 90, 0, 255]));
        assert_eq!(p.chan_amb_color, rgba8([50; 4]));
        assert_eq!(p.chan_control[0], UVec4::new(1, 2, 1, 3));
        // texgen 1 is SRTG from COLOR0 with no matrix
        assert_eq!(p.texgen[1], UVec4::new(10, 19, 60, 0));
    }

    #[test]
    fn ear_equations_match_the_dump() {
        // Byte-for-byte from assets/link/converted/mat3_dump.txt, minus its
        // two-space indent. If mat3_dump's renderer ever changes, this fails.
        assert_eq!(
            stage_equations(&ear()).unwrap(),
            [
                "stage0 C: PREV = clamp(ZERO + mix(C0, KONST, TEXC))",
                "stage0 A: PREV = clamp(ZERO + mix(ZERO, ZERO, ZERO))",
                "stage1 C: PREV = clamp(ZERO + mix(ZERO, TEXC, CPREV))",
                "stage1 A: PREV = clamp(ZERO + mix(ZERO, KONST, TEXA))",
                "stage2 C: PREV = clamp(CPREV + mix(ZERO, KONST, TEXC))",
                "stage2 A: PREV = clamp(ZERO + mix(APREV, ZERO, ZERO))",
            ]
        );
        // The konst selectors are what the dump's bare "KONST" hides.
        assert_eq!(
            stage_konst_selects(&ear(), 1).unwrap(),
            ("K0".to_string(), "K3_A".to_string())
        );
        assert_eq!(
            stage_konst_selects(&ear(), 2).unwrap(),
            ("K1".to_string(), "K0_A".to_string())
        );
    }

    #[test]
    fn eye_l_stage1_is_unclamped() {
        // The only stages in the model with the clamp bit off, so the S10 branch
        // in tev.slang is genuinely reachable.
        let mut m = fixture();
        m.name = "eyeL".into();
        m.num_tev_stages = 2;
        m.num_tex_gens = 2;
        m.tev.stages = vec![
            stage([8, 15, 15, 15], [7, 7, 7, 7]),
            TevStageState {
                color_clamp: false,
                alpha_clamp: false,
                ..stage([15, 8, 0, 15], [4, 7, 7, 7])
            },
        ];
        m.tev.orders[0] = order(1, 1, 255);
        m.tev.orders[1] = order(0, 0, 255);
        m.texgens.push(TexGenState {
            ty: 1,
            src: 4,
            matrix: 33,
        });
        m.tex_matrices = vec![TexMatrixState {
            slot: 1,
            center: [0.5, 0.5, 0.5],
            scale: [1.0, 1.0],
            rotation: 0,
            translation: [-0.05, 0.0],
            effect_matrix: [0.0; 16],
        }];

        let p = pack(&m).unwrap();
        assert_eq!(p.stage_color_op[1].w, 0, "clamp bit must survive as 0");
        assert_eq!(p.stage_alpha_op[1].w, 0);
        assert_eq!(p.stage_color_op[0].w, 1);
        // and the pupil offset rides on texgen 1
        assert_eq!(p.texgen_mtx[2], Vec4::new(1.0, 0.0, -0.05, 0.0));
    }
}
