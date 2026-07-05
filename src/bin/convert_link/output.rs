//! Manifest + flat-binary emission and the `--obj` debug export. Converts the
//! parsed `bmd::Model` and baked `pose::BakedModel` into
//! `vulkan_slang_renderer::model_manifest` types plus `link.{vtx,idx,skin}.bin`.
//!
//! Enum→value mapping: renderer-facing raster state serializes as the GX
//! `Display` names; TEV interpreter fields serialize as the raw GX byte values
//! (`enum as u8`) the P6 shader packs into `uint4` uniforms.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use vulkan_slang_renderer::model_manifest as mm;

use crate::bmd::Model;
use crate::bmd::mat3::{self, Material};
use crate::pose::BakedModel;

/// The two ramp name-prefixes the game injects at runtime (`setToonTex`):
/// `ZA*` ← toon, `ZB*` ← toonEX. cl.bdl has only `ZBtoonEX`.
const RAMP_PREFIXES: [(&str, &str, &str); 2] = [
    ("ZA", "toon", "tex/raw_toon.png"),
    ("ZB", "toonex", "tex/raw_toonex.png"),
];

pub struct Converted {
    pub manifest: mm::Manifest,
    /// The shared index buffer (INF1 draw order), referenced by the batches.
    pub indices: Vec<u32>,
}

pub fn build(model: &Model, baked: &BakedModel) -> Converted {
    let textures = build_textures(model);
    let materials = build_materials(model);

    let mut indices = Vec::new();
    let mut batches = Vec::new();
    for &(material, shape) in &model.inf1.draw {
        let shape_indices = &baked.indices_per_shape[shape as usize];
        let first_index = indices.len() as u32;
        indices.extend_from_slice(shape_indices);
        batches.push(mm::Batch {
            material,
            shape,
            first_index,
            index_count: shape_indices.len() as u32,
        });
    }

    let skeleton = mm::Skeleton {
        joints: model
            .jnt1
            .joints
            .iter()
            .enumerate()
            .map(|(i, j)| mm::SkeletonJoint {
                name: j.name.clone(),
                parent: model.inf1.parents[i].map(|p| p as i32).unwrap_or(-1),
                t: j.translation,
                r_s16: j.rotation_s16,
                s: j.scale,
            })
            .collect(),
    };

    let manifest = mm::Manifest {
        version: 1,
        buffers: mm::Buffers {
            vertices: "link.vtx.bin".into(),
            indices: "link.idx.bin".into(),
            skinning: "link.skin.bin".into(),
            vertex_layout: vec!["position3f".into(), "normal3f".into(), "uv02f".into()],
            vertex_count: baked.vertices.len() as u32,
            index_count: indices.len() as u32,
        },
        textures,
        materials,
        batches,
        skeleton,
    };

    Converted { manifest, indices }
}

fn build_textures(model: &Model) -> Vec<mm::TextureEntry> {
    model
        .tex1
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let h = &e.texture.header;
            // Ramp slots point at the runtime-injected image, not the TEX1
            // placeholder.
            let (file, substitution) = RAMP_PREFIXES
                .iter()
                .find(|(prefix, ..)| e.name.starts_with(prefix))
                .map(|(_, name, path)| (path.to_string(), Some(name.to_string())))
                .unwrap_or_else(|| (format!("tex/{i:02}_{}.png", e.name), None));
            mm::TextureEntry {
                name: e.name.clone(),
                file,
                wrap_u: h.wrap_s.to_string(),
                wrap_v: h.wrap_t.to_string(),
                filter: h.min_filter.to_string(),
                mipmaps: h.mipmap_count > 1,
                runtime_substitution: substitution,
            }
        })
        .collect()
}

fn build_materials(model: &Model) -> Vec<mm::MaterialEntry> {
    model
        .mat3
        .materials
        .iter()
        .enumerate()
        .map(|(i, m)| material_entry(&model.mat3.names[i], model.mat3.remap[i], m))
        .collect()
}

fn material_entry(name: &str, record: u16, m: &Material) -> mm::MaterialEntry {
    let z = m.z_mode.clone();
    let colors = |slots: &[Option<mat3::Rgba8>]| slots.to_vec();

    mm::MaterialEntry {
        name: name.to_string(),
        record,
        pe_mode: m.pe_mode.to_string(),
        cull: m
            .cull_mode
            .map(|c| c.to_string())
            .unwrap_or_else(|| "Cull_Back".into()),
        z_test: z.as_ref().map(|z| z.test).unwrap_or(true),
        z_func: z
            .as_ref()
            .map(|z| z.func.to_string())
            .unwrap_or_else(|| "Less_Equal".into()),
        z_write: z.as_ref().map(|z| z.write).unwrap_or(true),
        z_compare_early: m.z_compare_loc.unwrap_or(true),
        blend: m.blend.as_ref().map(|b| mm::BlendState {
            mode: b.mode.to_string(),
            src: b.src.to_string(),
            dst: b.dst.to_string(),
            logic: b.logic.to_string(),
        }),
        alpha_compare: m.alpha_compare.as_ref().map(|a| mm::AlphaCompareState {
            comp0: a.comp0.to_string(),
            ref0: a.ref0,
            op: a.op.to_string(),
            comp1: a.comp1.to_string(),
            ref1: a.ref1,
        }),
        dither: m.dither.unwrap_or(false),
        num_tev_stages: m.num_tev_stages.unwrap_or(0),
        num_tex_gens: m.num_tex_gens.unwrap_or(0),
        num_color_chans: m.num_color_chans.unwrap_or(0),
        texmaps: m.texture_indices.to_vec(),
        tev: mm::TevConfig {
            stages: m.tev_stages.iter().flatten().map(tev_stage).collect(),
            orders: m
                .tev_orders
                .iter()
                .map(|o| {
                    o.as_ref().map(|o| mm::TevOrderState {
                        tex_coord: o.tex_coord as u8,
                        tex_map: o.tex_map as u8,
                        channel: o.channel as u8,
                    })
                })
                .collect(),
            konst_colors: colors(&m.konst_colors),
            reg_colors: m.tev_colors.to_vec(),
            kcsels: m.kcsels.iter().map(|k| *k as u8).collect(),
            kasels: m.kasels.iter().map(|k| *k as u8).collect(),
            swap_modes: m
                .swap_modes
                .iter()
                .map(|s| {
                    s.as_ref().map(|s| mm::SwapModeState {
                        ras_sel: s.ras_sel,
                        tex_sel: s.tex_sel,
                    })
                })
                .collect(),
            swap_tables: m
                .swap_tables
                .iter()
                .map(|s| s.as_ref().map(|s| s.rgba))
                .collect(),
        },
        texgens: m
            .tex_coord_gens
            .iter()
            .flatten()
            .map(|g| mm::TexGenState {
                ty: g.ty as u8,
                src: g.src as u8,
                matrix: g.matrix as u8,
            })
            .collect(),
        tex_matrices: m
            .tex_matrices
            .iter()
            .enumerate()
            .filter_map(|(slot, tm)| {
                tm.as_ref().map(|tm| mm::TexMatrixState {
                    slot: slot as u8,
                    center: tm.center,
                    scale: tm.scale,
                    rotation: tm.rotation,
                    translation: tm.translation,
                    effect_matrix: tm.effect_matrix,
                })
            })
            .collect(),
        channels: m
            .color_channels
            .iter()
            .flatten()
            .map(|c| mm::ChannelState {
                lighting_enabled: c.lighting_enabled,
                mat_src: c.mat_src.to_string(),
                amb_src: c.amb_src.to_string(),
                diffuse: c.diffuse.to_string(),
                attenuation: c.attenuation.to_string(),
                lit_mask: c.lit_mask,
            })
            .collect(),
        material_colors: m.material_colors.to_vec(),
        ambient_colors: m.ambient_colors.to_vec(),
        light_colors: m.light_colors.to_vec(),
    }
}

fn tev_stage(s: &mat3::TevStage) -> mm::TevStageState {
    mm::TevStageState {
        color_in: s.color_in.map(|c| c as u8),
        color_op: s.color_op as u8,
        color_bias: s.color_bias as u8,
        color_scale: s.color_scale as u8,
        color_clamp: s.color_clamp,
        color_reg: s.color_reg as u8,
        alpha_in: s.alpha_in.map(|a| a as u8),
        alpha_op: s.alpha_op as u8,
        alpha_bias: s.alpha_bias as u8,
        alpha_scale: s.alpha_scale as u8,
        alpha_clamp: s.alpha_clamp,
        alpha_reg: s.alpha_reg as u8,
    }
}

pub fn write_files(converted: &Converted, baked: &BakedModel, out_dir: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(&converted.manifest).context("serializing manifest")?;
    let json_path = out_dir.join("link.manifest.json");
    std::fs::write(&json_path, json).with_context(|| format!("writing {}", json_path.display()))?;

    // vtx.bin: interleaved LE f32 pos[3] nrm[3] uv[2].
    let mut vtx = Vec::with_capacity(baked.vertices.len() * 32);
    for v in &baked.vertices {
        for f in v.pos.iter().chain(&v.nrm).chain(&v.uv) {
            vtx.extend_from_slice(&f.to_le_bytes());
        }
    }
    write_bin(out_dir, "link.vtx.bin", &vtx)?;

    // idx.bin: LE u32 triangle list.
    let mut idx = Vec::with_capacity(converted.indices.len() * 4);
    for &i in &converted.indices {
        idx.extend_from_slice(&i.to_le_bytes());
    }
    write_bin(out_dir, "link.idx.bin", &idx)?;

    // skin.bin: per vertex 4×(u8 joint + LE f32 weight).
    let mut skin = Vec::with_capacity(baked.skin.len() * 20);
    for infl in &baked.skin {
        for &(joint, weight) in infl {
            skin.push(joint);
            skin.extend_from_slice(&weight.to_le_bytes());
        }
    }
    write_bin(out_dir, "link.skin.bin", &skin)?;

    Ok(())
}

fn write_bin(out_dir: &Path, name: &str, data: &[u8]) -> Result<()> {
    let path = out_dir.join(name);
    std::fs::write(&path, data).with_context(|| format!("writing {}", path.display()))
}

/// Debug-only Wavefront OBJ + MTL: one `g`/`usemtl` group per batch, textured
/// via each material's texmap slot 0. Excluded from golden hashes.
pub fn write_obj(
    model: &Model,
    baked: &BakedModel,
    converted: &Converted,
    out_dir: &Path,
) -> Result<()> {
    let mut obj = String::new();
    writeln!(obj, "# Toon Link bind pose (debug export)").unwrap();
    writeln!(obj, "mtllib link.mtl").unwrap();
    for v in &baked.vertices {
        writeln!(obj, "v {} {} {}", v.pos[0], v.pos[1], v.pos[2]).unwrap();
    }
    for v in &baked.vertices {
        // OBJ V origin is bottom; our PNGs are top-down.
        writeln!(obj, "vt {} {}", v.uv[0], 1.0 - v.uv[1]).unwrap();
    }
    for v in &baked.vertices {
        writeln!(obj, "vn {} {} {}", v.nrm[0], v.nrm[1], v.nrm[2]).unwrap();
    }

    for (b, batch) in converted.manifest.batches.iter().enumerate() {
        let mat_name = &converted.manifest.materials[batch.material as usize].name;
        writeln!(obj, "g batch{b}_{mat_name}").unwrap();
        writeln!(obj, "usemtl {mat_name}").unwrap();
        let range = batch.first_index as usize..(batch.first_index + batch.index_count) as usize;
        for tri in converted.indices[range].chunks_exact(3) {
            let (a, b, c) = (tri[0] + 1, tri[1] + 1, tri[2] + 1);
            writeln!(obj, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}").unwrap();
        }
    }
    write_bin(out_dir, "link.obj", obj.as_bytes())?;

    // MTL: reference each material's slot-0 texture PNG.
    let mut mtl = String::new();
    let mut seen = std::collections::HashSet::new();
    for mat in &converted.manifest.materials {
        if !seen.insert(mat.name.clone()) {
            continue;
        }
        writeln!(mtl, "newmtl {}", mat.name).unwrap();
        writeln!(mtl, "Kd 0.8 0.8 0.8").unwrap();
        if let Some(Some(tex)) = mat.texmaps.first()
            && let Some(entry) = converted.manifest.textures.get(*tex as usize)
        {
            writeln!(mtl, "map_Kd {}", entry.file).unwrap();
        }
        writeln!(mtl).unwrap();
    }
    write_bin(out_dir, "link.mtl", mtl.as_bytes())?;
    let _ = model; // reserved for future per-joint debug output
    Ok(())
}
