//! MAT3 dump printers.
//!
//! `canonical` is the byte-exact diff-gate format, printed identically by
//! scripts/link_mat3_table.py from gclib's parse. The spec, implemented
//! independently by both sides:
//!
//! - header `MAT3 materials=N`, then per slot `material <i> <name>` followed
//!   by fixed field lines in the order below; no indentation; single spaces.
//! - enums print gclib's member spellings (the `Display` impls in gx/types).
//! - absent values (0xFF/0xFFFF index or zero list offset) print `-` — for
//!   single fields after the key (`fog -`), for array slots as the token.
//! - u8 colors `r,g,b,a`; s16 TEV colors likewise; floats `%.6f`; lit_mask
//!   as `0x%02x`; swap modes `ras:tex`; swap tables `r,g,b,a`.
//! - swap-table slots past the junk-index guard (see mat3.rs) print `-`.
//!
//! `human_report` (mat3_dump.txt) renders stage equations in the spirit of
//! tww's matDL_dis.py plus the subset summary that P2 freezes.

use std::collections::BTreeSet;
use std::fmt::Write;

use super::mat3::*;
use crate::gx::types::{Register, TevBias, TevOp, TevScale, TexMtxMapMode};

fn opt<T: std::fmt::Display>(v: &Option<T>) -> String {
    match v {
        Some(v) => v.to_string(),
        None => "-".into(),
    }
}

fn c8(c: &Rgba8) -> String {
    format!("{},{},{},{}", c[0], c[1], c[2], c[3])
}

fn c16(c: &RgbaS16) -> String {
    format!("{},{},{},{}", c[0], c[1], c[2], c[3])
}

fn floats(vals: &[f32]) -> String {
    vals.iter()
        .map(|v| format!("{v:.6}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn tokens<T>(slots: &[Option<T>], f: impl Fn(&T) -> String) -> String {
    slots
        .iter()
        .map(|s| s.as_ref().map(&f).unwrap_or_else(|| "-".into()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn texgen_line(g: &TexGen) -> String {
    format!("type={} src={} mtx={}", g.ty, g.src, g.matrix)
}

fn texmtx_line(m: &TexMatrix) -> String {
    format!(
        "proj={} mode={} maya={} center={} scale={} rot={} trans={} effect={}",
        m.projection,
        m.map_mode,
        m.is_maya,
        floats(&m.center),
        floats(&m.scale),
        m.rotation,
        floats(&m.translation),
        floats(&m.effect_matrix),
    )
}

fn stage_line(s: &TevStage) -> String {
    format!(
        "mode=0x{:02x} a={} b={} c={} d={} op={} bias={} scale={} clamp={} reg={} \
         aa={} ab={} ac={} ad={} aop={} abias={} ascale={} aclamp={} areg={}",
        s.tev_mode,
        s.color_in[0],
        s.color_in[1],
        s.color_in[2],
        s.color_in[3],
        s.color_op,
        s.color_bias,
        s.color_scale,
        s.color_clamp,
        s.color_reg,
        s.alpha_in[0],
        s.alpha_in[1],
        s.alpha_in[2],
        s.alpha_in[3],
        s.alpha_op,
        s.alpha_bias,
        s.alpha_scale,
        s.alpha_clamp,
        s.alpha_reg,
    )
}

pub fn canonical(mat3: &Mat3) -> String {
    let mut out = format!("MAT3 materials={}\n", mat3.materials.len());
    for (i, (m, name)) in mat3.materials.iter().zip(&mat3.names).enumerate() {
        let w = &mut out;
        writeln!(w, "material {i} {name}").unwrap();
        writeln!(w, "pe_mode {}", m.pe_mode).unwrap();
        writeln!(w, "cull {}", opt(&m.cull_mode)).unwrap();
        writeln!(w, "num_color_chans {}", opt(&m.num_color_chans)).unwrap();
        writeln!(w, "num_tex_gens {}", opt(&m.num_tex_gens)).unwrap();
        writeln!(w, "num_tev_stages {}", opt(&m.num_tev_stages)).unwrap();
        writeln!(w, "z_compare_loc {}", opt(&m.z_compare_loc)).unwrap();
        match &m.z_mode {
            Some(z) => writeln!(
                w,
                "z_mode test={} func={} write={}",
                z.test, z.func, z.write
            )
            .unwrap(),
            None => writeln!(w, "z_mode -").unwrap(),
        }
        writeln!(w, "dither {}", opt(&m.dither)).unwrap();
        writeln!(w, "mat_colors {}", tokens(&m.material_colors, c8)).unwrap();
        for (ci, chan) in m.color_channels.iter().enumerate() {
            match chan {
                Some(c) => writeln!(
                    w,
                    "chan {ci} enable={} mat_src={} amb_src={} diffuse={} attn={} lit_mask=0x{:02x}",
                    c.lighting_enabled, c.mat_src, c.amb_src, c.diffuse, c.attenuation, c.lit_mask
                )
                .unwrap(),
                None => writeln!(w, "chan {ci} -").unwrap(),
            }
        }
        writeln!(w, "amb_colors {}", tokens(&m.ambient_colors, c8)).unwrap();
        writeln!(w, "light_colors {}", tokens(&m.light_colors, c8)).unwrap();
        for (gi, texgen) in m.tex_coord_gens.iter().enumerate() {
            writeln!(
                w,
                "texgen {gi} {}",
                texgen.as_ref().map(texgen_line).unwrap_or("-".into())
            )
            .unwrap();
        }
        for (gi, texgen) in m.post_tex_coord_gens.iter().enumerate() {
            writeln!(
                w,
                "post_texgen {gi} {}",
                texgen.as_ref().map(texgen_line).unwrap_or("-".into())
            )
            .unwrap();
        }
        for (mi, mtx) in m.tex_matrices.iter().enumerate() {
            writeln!(
                w,
                "texmtx {mi} {}",
                mtx.as_ref().map(texmtx_line).unwrap_or("-".into())
            )
            .unwrap();
        }
        for (mi, mtx) in m.post_tex_matrices.iter().enumerate() {
            writeln!(
                w,
                "post_texmtx {mi} {}",
                mtx.as_ref().map(texmtx_line).unwrap_or("-".into())
            )
            .unwrap();
        }
        writeln!(
            w,
            "texture_indices {}",
            tokens(&m.texture_indices, u16::to_string)
        )
        .unwrap();
        writeln!(w, "konst_colors {}", tokens(&m.konst_colors, c8)).unwrap();
        writeln!(
            w,
            "kcsels {}",
            m.kcsels
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        )
        .unwrap();
        writeln!(
            w,
            "kasels {}",
            m.kasels
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        )
        .unwrap();
        for (oi, order) in m.tev_orders.iter().enumerate() {
            match order {
                Some(o) => writeln!(
                    w,
                    "tev_order {oi} coord={} map={} chan={}",
                    o.tex_coord, o.tex_map, o.channel
                )
                .unwrap(),
                None => writeln!(w, "tev_order {oi} -").unwrap(),
            }
        }
        writeln!(w, "tev_colors {}", tokens(&m.tev_colors, c16)).unwrap();
        for (si, stage) in m.tev_stages.iter().enumerate() {
            writeln!(
                w,
                "tev_stage {si} {}",
                stage.as_ref().map(stage_line).unwrap_or("-".into())
            )
            .unwrap();
        }
        writeln!(
            w,
            "swap_modes {}",
            tokens(&m.swap_modes, |s| format!("{}:{}", s.ras_sel, s.tex_sel))
        )
        .unwrap();
        writeln!(
            w,
            "swap_tables {}",
            tokens(&m.swap_tables, |t| {
                format!("{},{},{},{}", t.rgba[0], t.rgba[1], t.rgba[2], t.rgba[3])
            })
        )
        .unwrap();
        match &m.fog {
            Some(fog) => writeln!(
                w,
                "fog type={} enable={} center={} start={:.6} end={:.6} near={:.6} far={:.6} color={} ranges={}",
                fog.fog_type,
                fog.enable,
                fog.center,
                fog.start_z,
                fog.end_z,
                fog.near_z,
                fog.far_z,
                c8(&fog.color),
                fog.range_adjustments.map(|v| v.to_string()).join(","),
            )
            .unwrap(),
            None => writeln!(w, "fog -").unwrap(),
        }
        match &m.alpha_compare {
            Some(a) => writeln!(
                w,
                "alpha_comp comp0={} ref0={} op={} comp1={} ref1={}",
                a.comp0, a.ref0, a.op, a.comp1, a.ref1
            )
            .unwrap(),
            None => writeln!(w, "alpha_comp -").unwrap(),
        }
        match &m.blend {
            Some(b) => writeln!(
                w,
                "blend mode={} src={} dst={} logic={}",
                b.mode, b.src, b.dst, b.logic
            )
            .unwrap(),
            None => writeln!(w, "blend -").unwrap(),
        }
        match &m.nbt_scale {
            Some(n) => writeln!(
                w,
                "nbt_scale enable={} scale={}",
                n.enable,
                floats(&n.scale)
            )
            .unwrap(),
            None => writeln!(w, "nbt_scale -").unwrap(),
        }
        match &m.indirect {
            Some(ind) => writeln!(
                w,
                "indirect enable={} stages={}",
                ind.enable, ind.num_stages
            )
            .unwrap(),
            None => writeln!(w, "indirect -").unwrap(),
        }
    }
    out
}

/// Human-readable report: per-material stage equations plus the distinct-value
/// subset summary (active slots only) that P2 freezes as the TEV contract.
pub fn human_report(mat3: &Mat3) -> String {
    let mut out = String::new();
    let w = &mut out;
    writeln!(w, "MAT3 report: {} material slots", mat3.materials.len()).unwrap();
    writeln!(w, "remap table: {:?}", mat3.remap).unwrap();
    writeln!(w).unwrap();

    for (i, (m, name)) in mat3.materials.iter().zip(&mat3.names).enumerate() {
        writeln!(w, "=== material {i} {name} (record {}) ===", mat3.remap[i]).unwrap();
        writeln!(
            w,
            "  pe={} cull={} z_test={} z_write={} dither={}",
            m.pe_mode,
            opt(&m.cull_mode),
            opt(&m.z_mode.as_ref().map(|z| format!("{}({})", z.test, z.func))),
            opt(&m.z_mode.as_ref().map(|z| z.write)),
            opt(&m.dither),
        )
        .unwrap();
        if let Some(blend) = &m.blend {
            writeln!(
                w,
                "  blend {} {} -> {} (logic {})",
                blend.mode, blend.src, blend.dst, blend.logic
            )
            .unwrap();
        }
        if let Some(a) = &m.alpha_compare {
            writeln!(
                w,
                "  alpha discard unless: a {} {} {} a {} {}",
                a.comp0, a.ref0, a.op, a.comp1, a.ref1
            )
            .unwrap();
        }
        for gi in 0..active(m.num_tex_gens) {
            if let Some(g) = &m.tex_coord_gens[gi] {
                writeln!(w, "  texgen{gi}: {} from {} via {}", g.ty, g.src, g.matrix).unwrap();
            }
        }
        for (ci, chan) in m.color_channels.iter().enumerate() {
            if let Some(c) = chan
                && (c.lighting_enabled || c.lit_mask != 0)
            {
                writeln!(
                    w,
                    "  chan{ci}: lit={} mask=0x{:02x} diffuse={} attn={} mat={} amb={}",
                    c.lighting_enabled, c.lit_mask, c.diffuse, c.attenuation, c.mat_src, c.amb_src
                )
                .unwrap();
            }
        }
        for si in 0..active(m.num_tev_stages) {
            let (Some(stage), order) = (&m.tev_stages[si], &m.tev_orders[si]) else {
                continue;
            };
            if let Some(o) = order {
                writeln!(
                    w,
                    "  stage{si} order: coord={} map={} chan={}",
                    o.tex_coord, o.tex_map, o.channel
                )
                .unwrap();
            }
            writeln!(
                w,
                "  stage{si} C: {}",
                equation(
                    &stage.color_in.map(|c| c.to_string()),
                    stage.color_op,
                    stage.color_bias,
                    stage.color_scale,
                    stage.color_clamp,
                    stage.color_reg
                )
            )
            .unwrap();
            writeln!(
                w,
                "  stage{si} A: {}",
                equation(
                    &stage.alpha_in.map(|a| a.to_string()),
                    stage.alpha_op,
                    stage.alpha_bias,
                    stage.alpha_scale,
                    stage.alpha_clamp,
                    stage.alpha_reg
                )
            )
            .unwrap();
        }
        for (ki, k) in m.konst_colors.iter().enumerate() {
            if let Some(k) = k {
                writeln!(w, "  konst{ki} = {}", c8(k)).unwrap();
            }
        }
        for (ri, reg) in m.tev_colors.iter().enumerate() {
            if let Some(reg) = reg {
                writeln!(w, "  reg{ri} = {}", c16(reg)).unwrap();
            }
        }
        if let Some(fog) = &m.fog
            && fog.enable
        {
            writeln!(
                w,
                "  !! fog enabled: {} (converter renders without fog)",
                fog.fog_type
            )
            .unwrap();
        }
        writeln!(w).unwrap();
    }

    out.push_str(&subset_summary(mat3));
    out
}

fn active(n: Option<u8>) -> usize {
    n.unwrap_or(0) as usize
}

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

/// The distinct values used across all materials, active slots only —
/// pasted into phase_02.md as the frozen TEV subset.
pub fn subset_summary(mat3: &Mat3) -> String {
    let mut pe = BTreeSet::new();
    let mut cull = BTreeSet::new();
    let mut stage_counts = BTreeSet::new();
    let mut color_in = BTreeSet::new();
    let mut alpha_in = BTreeSet::new();
    let mut color_ops = BTreeSet::new();
    let mut alpha_ops = BTreeSet::new();
    let mut biases = BTreeSet::new();
    let mut scales = BTreeSet::new();
    let mut regs = BTreeSet::new();
    let mut unclamped = 0usize;
    let mut kcsels = BTreeSet::new();
    let mut kasels = BTreeSet::new();
    let mut ras_channels = BTreeSet::new();
    let mut texgens = BTreeSet::new();
    let mut nonidentity_texmtx = 0usize;
    let mut chans = BTreeSet::new();
    let mut z_modes = BTreeSet::new();
    let mut z_comp_locs = BTreeSet::new();
    let mut blends = BTreeSet::new();
    let mut alpha_comps = BTreeSet::new();
    let mut fog_types = BTreeSet::new();
    let mut fog_enabled = 0usize;
    let mut swap_nondefault = 0usize;
    let mut indirect_enabled = 0usize;

    for m in &mat3.materials {
        pe.insert(m.pe_mode.to_string());
        cull.insert(opt(&m.cull_mode));
        stage_counts.insert(active(m.num_tev_stages));
        for si in 0..active(m.num_tev_stages) {
            if let Some(s) = &m.tev_stages[si] {
                color_in.extend(s.color_in.iter().map(ToString::to_string));
                alpha_in.extend(s.alpha_in.iter().map(ToString::to_string));
                color_ops.insert(s.color_op.to_string());
                alpha_ops.insert(s.alpha_op.to_string());
                biases.insert(s.color_bias.to_string());
                biases.insert(s.alpha_bias.to_string());
                scales.insert(s.color_scale.to_string());
                scales.insert(s.alpha_scale.to_string());
                regs.insert(s.color_reg.to_string());
                regs.insert(s.alpha_reg.to_string());
                if !s.color_clamp || !s.alpha_clamp {
                    unclamped += 1;
                }
            }
            kcsels.insert(m.kcsels[si].to_string());
            kasels.insert(m.kasels[si].to_string());
            if let Some(o) = &m.tev_orders[si] {
                ras_channels.insert(o.channel.to_string());
            }
            if let Some(sm) = &m.swap_modes[si]
                && (sm.ras_sel != 0 || sm.tex_sel != 0)
            {
                swap_nondefault += 1;
            }
        }
        for gi in 0..active(m.num_tex_gens) {
            if let Some(g) = &m.tex_coord_gens[gi] {
                texgens.insert(format!("({}, {}, {})", g.ty, g.src, g.matrix));
            }
        }
        for mtx in m.tex_matrices.iter().flatten() {
            let identity = mtx.scale == [1.0, 1.0]
                && mtx.rotation == 0
                && mtx.translation == [0.0, 0.0]
                && mtx.map_mode == TexMtxMapMode::None;
            if !identity {
                nonidentity_texmtx += 1;
            }
        }
        for c in m.color_channels.iter().flatten() {
            chans.insert(format!(
                "(enable={}, mat={}, amb={}, diffuse={}, attn={}, mask=0x{:02x})",
                c.lighting_enabled, c.mat_src, c.amb_src, c.diffuse, c.attenuation, c.lit_mask
            ));
        }
        if let Some(z) = &m.z_mode {
            z_modes.insert(format!(
                "(test={}, func={}, write={})",
                z.test, z.func, z.write
            ));
        }
        z_comp_locs.insert(opt(&m.z_compare_loc));
        if let Some(b) = &m.blend {
            blends.insert(format!("({}, {}, {}, {})", b.mode, b.src, b.dst, b.logic));
        }
        if let Some(a) = &m.alpha_compare {
            alpha_comps.insert(format!(
                "({} {}, {}, {} {})",
                a.comp0, a.ref0, a.op, a.comp1, a.ref1
            ));
        }
        if let Some(fog) = &m.fog {
            fog_types.insert(fog.fog_type.to_string());
            if fog.enable {
                fog_enabled += 1;
            }
        }
        if let Some(ind) = &m.indirect
            && ind.enable
        {
            indirect_enabled += 1;
        }
    }

    let join = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>().join(", ");
    let mut out = String::new();
    let w = &mut out;
    writeln!(w, "== TEV subset summary (active slots only) ==").unwrap();
    writeln!(w, "pe_modes: {}", join(&pe)).unwrap();
    writeln!(w, "cull_modes: {}", join(&cull)).unwrap();
    writeln!(w, "stage_counts: {:?}", stage_counts).unwrap();
    writeln!(w, "color_inputs: {}", join(&color_in)).unwrap();
    writeln!(w, "alpha_inputs: {}", join(&alpha_in)).unwrap();
    writeln!(w, "color_ops: {}", join(&color_ops)).unwrap();
    writeln!(w, "alpha_ops: {}", join(&alpha_ops)).unwrap();
    writeln!(w, "biases: {}", join(&biases)).unwrap();
    writeln!(w, "scales: {}", join(&scales)).unwrap();
    writeln!(w, "dest_regs: {}", join(&regs)).unwrap();
    writeln!(w, "stages_with_clamp_off: {unclamped}").unwrap();
    writeln!(w, "konst_color_sels: {}", join(&kcsels)).unwrap();
    writeln!(w, "konst_alpha_sels: {}", join(&kasels)).unwrap();
    writeln!(w, "ras_channels: {}", join(&ras_channels)).unwrap();
    writeln!(w, "texgens: {}", join(&texgens)).unwrap();
    writeln!(w, "non_identity_tex_matrices: {nonidentity_texmtx}").unwrap();
    writeln!(w, "channel_controls: {}", join(&chans)).unwrap();
    writeln!(w, "z_modes: {}", join(&z_modes)).unwrap();
    writeln!(w, "z_compare_loc: {}", join(&z_comp_locs)).unwrap();
    writeln!(w, "blend_modes: {}", join(&blends)).unwrap();
    writeln!(w, "alpha_compares: {}", join(&alpha_comps)).unwrap();
    writeln!(
        w,
        "fog_types: {} (enabled on {fog_enabled} materials)",
        join(&fog_types)
    )
    .unwrap();
    writeln!(w, "swap_modes_non_default: {swap_nondefault}").unwrap();
    writeln!(w, "indirect_enabled: {indirect_enabled}").unwrap();
    out
}
