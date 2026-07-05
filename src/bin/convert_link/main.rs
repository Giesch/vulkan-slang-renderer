//! BDL → renderer-native asset converter for the toon_link example.
//! P1: header/chunk-table validation + the canonical `--info` table.
//! P2: TEX1/BTI texture decode → PNGs + standalone .bti re-emits, full MAT3
//! parse with the canonical `--dump-mat3` table and mat3_dump.txt report.
//! Plans: claude_notes/link_rendering/phase_01.md, phase_02.md

mod be;
mod bmd;
mod bti;
mod gx;
mod output;
mod pose;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const USAGE: &str =
    "usage: convert_link <raw-dir> <out-dir> [--info | --dump-mat3 | --dump-geometry] [--obj]";

/// The three textures extracted as standalone .bti files (P0): the two
/// runtime-injected toon ramps and the casual-clothes body texture.
const STANDALONE_BTIS: [&str; 3] = ["toon.bti", "toonex.bti", "linktexbci4.bti"];

fn main() -> Result<()> {
    let mut info = false;
    let mut dump_mat3 = false;
    let mut dump_geometry = false;
    let mut obj = false;
    let mut positional = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--info" => info = true,
            "--dump-mat3" => dump_mat3 = true,
            "--dump-geometry" => dump_geometry = true,
            "--obj" => obj = true,
            flag if flag.starts_with('-') => usage_exit(&format!("unknown flag: {flag}")),
            _ => positional.push(PathBuf::from(arg)),
        }
    }
    if [info, dump_mat3, dump_geometry]
        .iter()
        .filter(|&&f| f)
        .count()
        > 1
    {
        usage_exit("--info, --dump-mat3 and --dump-geometry are mutually exclusive");
    }
    let [raw_dir, out_dir]: [PathBuf; 2] = positional
        .try_into()
        .unwrap_or_else(|_| usage_exit("expected exactly two directory arguments"));

    let bdl_path = raw_dir.join("cl.bdl");
    let data = std::fs::read(&bdl_path).with_context(|| {
        format!(
            "reading {} (run `just extract-link` first)",
            bdl_path.display()
        )
    })?;
    // All structural invariants (chunk table, TEX1, MAT3) run on every mode.
    let model =
        bmd::parse_model(&data).with_context(|| format!("parsing {}", bdl_path.display()))?;

    std::fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    // stdout carries only canonical tables; everything else is stderr
    if info {
        print!("{}", bmd::canonical_table(&model.table));
        return Ok(());
    }
    if dump_mat3 {
        print!("{}", bmd::mat3_dump::canonical(&model.mat3));
        return Ok(());
    }
    if dump_geometry {
        print!("{}", bmd::geometry_dump::canonical(&model));
        return Ok(());
    }

    let tex_dir = out_dir.join("tex");
    bmd::tex1::emit(&model.tex1, &tex_dir)
        .with_context(|| format!("emitting textures to {}", tex_dir.display()))?;
    emit_standalone_btis(&raw_dir, &tex_dir)?;
    let report_path = out_dir.join("mat3_dump.txt");
    std::fs::write(&report_path, bmd::mat3_dump::human_report(&model.mat3))
        .with_context(|| format!("writing {}", report_path.display()))?;

    let baked = pose::bake(&model).with_context(|| "baking geometry")?;
    let converted = output::build(&model, &baked);
    output::write_files(&converted, &baked, &out_dir).with_context(|| "writing manifest")?;
    if obj {
        output::write_obj(&model, &baked, &converted, &out_dir).with_context(|| "writing OBJ")?;
    }

    let tris = converted.indices.len() / 3;
    eprintln!(
        "convert_link: {} TEX1 textures + {} standalone, {} materials",
        model.tex1.entries.len(),
        STANDALONE_BTIS.len(),
        model.mat3.materials.len(),
    );
    eprintln!(
        "convert_link: baked {} vertices, {} triangles, {} batches \
         (invBind residual {:.2e}, weighted dist {:.2e}) -> {}",
        baked.vertices.len(),
        tris,
        converted.manifest.batches.len(),
        baked.invbind_max_residual,
        baked.weighted_max_distance,
        out_dir.display(),
    );
    Ok(())
}

/// Decodes the P0-extracted standalone .bti files to `tex/raw_<stem>.png`.
/// Their originals stay in raw-dir for the pixel gate; no .bti re-emit.
fn emit_standalone_btis(raw_dir: &Path, tex_dir: &Path) -> Result<()> {
    for file in STANDALONE_BTIS {
        let path = raw_dir.join(file);
        let data = std::fs::read(&path).with_context(|| {
            format!("reading {} (run `just extract-link` first)", path.display())
        })?;
        let stem = file.strip_suffix(".bti").unwrap();
        let reader = be::BeReader::new(&data);
        let texture =
            bti::parse(&reader, 0, stem).with_context(|| format!("parsing {}", path.display()))?;
        let image = bti::decode(&texture, stem)?;
        let png_path = tex_dir.join(format!("raw_{stem}.png"));
        image
            .save(&png_path)
            .with_context(|| format!("writing {}", png_path.display()))?;
    }
    Ok(())
}

fn usage_exit(message: &str) -> ! {
    eprintln!("convert_link: {message}");
    eprintln!("{USAGE}");
    std::process::exit(2);
}
