//! Wind Waker J3D (BDL) → renderer-native asset converter for the toon_link
//! example. Handles Toon Link (`cl.bdl`) and the King of Red Lions
//! (`fn_body.bdl`, `fn_head_h.bdl`); pick one with `--model=`.
//! P1: header/chunk-table validation + the canonical `--info` table.
//! P2: TEX1/BTI texture decode → PNGs + standalone .bti re-emits, full MAT3
//! parse with the canonical `--dump-mat3` table and mat3_dump.txt report.
//! P8: the TEV subset gate (`tev_ir`), which runs on every conversion and
//! changes no output — so a material the shader cannot render never reaches the
//! manifest.
//! Plans: llm_notes/link_rendering/phase_01.md, phase_02.md, phase_08.md,
//! llm_notes/ship_extraction.md

mod be;
mod bmd;
mod bti;
mod gx;
mod output;
mod pose;
mod tev_ir;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const USAGE: &str = "usage: convert_link <raw-dir> <out-dir> [--model=NAME]\n\
     \x20                  [--info | --dump-mat3 | --dump-geometry] [--obj]\n\
     \x20                  [--tev-gate=error|warn|off] [--no-ramps]";

/// What to do when a material falls outside the frozen TEV subset. The gate is
/// validation-only, so this cannot change an output byte for any model — it
/// only decides whether an unrenderable material stops the conversion.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TevGate {
    /// Refuse to write output. Link's setting: the manifest feeds tev.slang.
    Error,
    /// Report and continue. For OBJ-only models, where nothing consumes the
    /// TEV interpreter's view of the material.
    Warn,
    /// Skip the gate entirely.
    Off,
}

/// A model this converter knows how to read. Keeping these pinned in a table
/// rather than exposing free-form flags preserves the crate's "pin the expected
/// shape, fail loudly" style — and keeps the joint count attached to the file
/// that actually has that many joints.
struct ModelSpec {
    /// `--model=` value.
    name: &'static str,
    /// Human-readable name, used in the OBJ header comment.
    display: &'static str,
    /// Filename inside raw-dir.
    bdl: &'static str,
    /// Output basename.
    prefix: &'static str,
    expect: bmd::Expectations,
    /// Standalone .bti files decoded to `tex/raw_<stem>.png` alongside the
    /// TEX1 re-emits.
    standalone: &'static [&'static str],
    ramps: bool,
    tev_gate: TevGate,
}

impl ModelSpec {
    /// The justfile recipe that puts this model's inputs in raw-dir, named in
    /// the "you have not extracted yet" error.
    fn extract_recipe(&self) -> &'static str {
        if self.name == "link" {
            "extract-link"
        } else {
            "extract-ship"
        }
    }
}

/// The two runtime-injected toon ramps, which every model here references
/// through a `ZA*`/`ZB*` TEX1 placeholder.
const RAMP_BTIS: &[&str] = &["toon.bti", "toonex.bti"];

const MODELS: &[ModelSpec] = &[
    ModelSpec {
        name: "link",
        display: "Toon Link",
        bdl: "cl.bdl",
        prefix: "link",
        expect: bmd::CL_BDL,
        // the two ramps plus the casual-clothes body texture
        standalone: &["toon.bti", "toonex.bti", "linktexbci4.bti"],
        ramps: true,
        tev_gate: TevGate::Error,
    },
    ModelSpec {
        name: "ship",
        display: "King of Red Lions (hull)",
        bdl: "fn_body.bdl",
        prefix: "ship",
        expect: bmd::Expectations::bdl(11),
        standalone: RAMP_BTIS,
        // fn_body's TEX1 genuinely carries a ZBtoonEX placeholder, so the
        // substitution is semantically right even though nothing renders it.
        ramps: true,
        // Three texgens, a MTX3x4/POS texgen and a Projmap texture matrix all
        // fall outside the frozen subset. OBJ output does not use the TEV
        // interpreter, so this is recorded rather than satisfied.
        tev_gate: TevGate::Warn,
    },
    ModelSpec {
        name: "ship-head",
        display: "King of Red Lions (figurehead)",
        bdl: "fn_head_h.bdl",
        prefix: "ship_head",
        expect: bmd::Expectations::bdl(18),
        standalone: RAMP_BTIS,
        ramps: true,
        // Both its materials look like they should pass; find out if they don't.
        tev_gate: TevGate::Error,
    },
];

fn main() -> Result<()> {
    let mut info = false;
    let mut dump_mat3 = false;
    let mut dump_geometry = false;
    let mut obj = false;
    let mut no_ramps = false;
    let mut model_name = "link".to_string();
    let mut gate_override = None;
    let mut positional = Vec::new();
    for arg in std::env::args().skip(1) {
        // `--model=NAME`, not `--model NAME`: a bare value would land in
        // `positional` and fail the two-directory destructure below.
        if let Some(v) = arg.strip_prefix("--model=") {
            model_name = v.to_string();
            continue;
        }
        if let Some(v) = arg.strip_prefix("--tev-gate=") {
            gate_override = Some(match v {
                "error" => TevGate::Error,
                "warn" => TevGate::Warn,
                "off" => TevGate::Off,
                other => usage_exit(&format!("unknown --tev-gate value: {other}")),
            });
            continue;
        }
        match arg.as_str() {
            "--info" => info = true,
            "--dump-mat3" => dump_mat3 = true,
            "--dump-geometry" => dump_geometry = true,
            "--obj" => obj = true,
            "--no-ramps" => no_ramps = true,
            flag if flag.starts_with('-') => usage_exit(&format!("unknown flag: {flag}")),
            _ => positional.push(PathBuf::from(arg)),
        }
    }
    let spec = MODELS
        .iter()
        .find(|m| m.name == model_name)
        .unwrap_or_else(|| {
            let known: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
            usage_exit(&format!(
                "unknown --model={model_name} (known: {})",
                known.join(", ")
            ))
        });
    let tev_gate = gate_override.unwrap_or(spec.tev_gate);
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

    let bdl_path = raw_dir.join(spec.bdl);
    let data = std::fs::read(&bdl_path).with_context(|| {
        format!(
            "reading {} (run `just toon_link {}` first)",
            bdl_path.display(),
            spec.extract_recipe()
        )
    })?;
    // All structural invariants (chunk table, TEX1, MAT3) run on every mode.
    let model = bmd::parse_model_with(&data, &spec.expect)
        .with_context(|| format!("parsing {}", bdl_path.display()))?;

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

    // The TEV subset gate, before the first file is written: a material the
    // interpreter cannot render must never reach the manifest. Validation-only,
    // so no output byte depends on it —
    // `examples/toon_link/scripts/link_converted.sha256` staying unchanged is
    // the proof. It runs *after* the dump modes return, so
    // `--dump-mat3` remains usable for diagnosing whatever it rejected.
    //
    // The three outcomes are distinct in the summary line: a rejection under
    // Warn is not the same as never having run, and neither is "0 passed".
    // `describe_all` short-circuits, so Warn surfaces one rejection per run.
    let tev_summary = match tev_gate {
        TevGate::Error => {
            let n = tev_ir::describe_all(&model.mat3)
                .with_context(|| "TEV subset gate")?
                .len();
            format!("{n} passed the TEV subset gate")
        }
        TevGate::Warn => match tev_ir::describe_all(&model.mat3) {
            Ok(d) => format!("{} passed the TEV subset gate", d.len()),
            Err(e) => {
                eprintln!(
                    "convert_link: WARNING: {}: TEV subset gate: {e} \
                     (continuing; OBJ output does not use the TEV interpreter)",
                    spec.name
                );
                "TEV subset gate rejected this model, downgraded to a warning".to_string()
            }
        },
        TevGate::Off => "TEV subset gate skipped".to_string(),
    };

    let tex_dir = out_dir.join("tex");
    bmd::tex1::emit(&model.tex1, &tex_dir)
        .with_context(|| format!("emitting textures to {}", tex_dir.display()))?;
    emit_standalone_btis(&raw_dir, &tex_dir, spec)?;
    let report_path = out_dir.join("mat3_dump.txt");
    std::fs::write(&report_path, bmd::mat3_dump::human_report(&model.mat3))
        .with_context(|| format!("writing {}", report_path.display()))?;

    let baked = pose::bake(&model).with_context(|| "baking geometry")?;
    let naming = output::Naming {
        prefix: spec.prefix,
        display: spec.display,
        ramps: spec.ramps && !no_ramps,
    };
    let converted = output::build(&model, &baked, &naming);
    output::write_files(&converted, &baked, &out_dir, &naming)
        .with_context(|| "writing manifest")?;
    if obj {
        output::write_obj(&model, &baked, &converted, &out_dir, &naming)
            .with_context(|| "writing OBJ")?;
    }

    let tris = converted.indices.len() / 3;
    eprintln!(
        "convert_link: {}: {} TEX1 textures + {} standalone, {} materials ({})",
        spec.name,
        model.tex1.entries.len(),
        spec.standalone.len(),
        model.mat3.materials.len(),
        tev_summary,
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
fn emit_standalone_btis(raw_dir: &Path, tex_dir: &Path, spec: &ModelSpec) -> Result<()> {
    for file in spec.standalone {
        let path = raw_dir.join(file);
        let data = std::fs::read(&path).with_context(|| {
            format!(
                "reading {} (run `just toon_link {}` first)",
                path.display(),
                spec.extract_recipe()
            )
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
