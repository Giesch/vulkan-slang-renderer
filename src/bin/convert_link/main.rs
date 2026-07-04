//! BDL → renderer-native asset converter for the toon_link example.
//! P1 scope: header/chunk-table validation and the canonical `--info` table.
//! Plan: claude_notes/link_rendering/phase_01.md

mod be;
mod bmd;

use std::path::PathBuf;

use anyhow::{Context, Result};

const USAGE: &str = "usage: convert_link <raw-dir> <out-dir> [--info]";

fn main() -> Result<()> {
    let mut info = false;
    let mut positional = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--info" => info = true,
            flag if flag.starts_with('-') => usage_exit(&format!("unknown flag: {flag}")),
            _ => positional.push(PathBuf::from(arg)),
        }
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
    let table =
        bmd::parse_chunk_table(&data).with_context(|| format!("parsing {}", bdl_path.display()))?;
    bmd::walk_chunks(&table);

    std::fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    if info {
        // stdout carries only the canonical table; everything else is stderr
        print!("{}", bmd::canonical_table(&table));
    }
    Ok(())
}

fn usage_exit(message: &str) -> ! {
    eprintln!("convert_link: {message}");
    eprintln!("{USAGE}");
    std::process::exit(2);
}
