//! `convert_link_animations <raw-dir> <out-dir> [--dump-canonical]`
//!
//! Converts the extracted Link animation raw tree (see
//! `extract_link_animations.py`) into the shared `gx::animation_manifest`
//! documents. Requires the raw tree's validated `inventory.json`: every
//! entry is hash-checked, missing or extra raw files are rejected, and all
//! clips parse and validate *before* any output is published. Conversion
//! needs neither `cl.bdl` nor the model conversion output.
//!
//! `--dump-canonical` prints the complete normalized semantic dump for every
//! clip (sorted by archive/member) without writing converted assets; its
//! format is byte-identical to the independent Python oracle's dump.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use gx::animation_manifest::{AnimationClip, AnimationFormat, ClipIdentity};

#[path = "../animation/mod.rs"]
mod animation;
#[path = "../be.rs"]
mod be;

use animation::output;

const USAGE: &str = "usage: convert_link_animations <raw-dir> <out-dir> [--dump-canonical]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(2),
        Err(err) => {
            eprintln!("convert_link_animations: error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// One inventory row.
#[derive(serde::Deserialize)]
struct InventoryEntry {
    archive: String,
    member: String,
    entry_index: u32,
    resource_id: u16,
    format: AnimationFormat,
    size: u64,
    sha256: String,
}

#[derive(serde::Deserialize)]
struct Inventory {
    version: u32,
    #[serde(default)]
    #[allow(dead_code)]
    disc: Option<String>,
    entries: Vec<InventoryEntry>,
}

fn run(args: &[String]) -> Result<bool> {
    let mut positional: Vec<String> = Vec::new();
    let mut dump_canonical = false;
    for arg in &args[1..] {
        match arg.as_str() {
            "--dump-canonical" => dump_canonical = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                println!();
                println!("  <raw-dir>     extracted raw tree with inventory.json");
                println!("  <out-dir>     converted output tree (catalog.json + clips/)");
                println!("  --dump-canonical  print the semantic dump instead of converting");
                return Ok(true);
            }
            other if other.starts_with('-') => {
                eprintln!("unknown option {other}");
                eprintln!("{USAGE}");
                return Ok(false);
            }
            other => positional.push(other.to_string()),
        }
    }
    if positional.len() != 2 {
        eprintln!("{USAGE}");
        return Ok(false);
    }
    let raw_dir = Path::new(&positional[0]);
    let out_dir = Path::new(&positional[1]);
    // Reject overlapping raw/out trees: publishing into (or inside) the raw
    // tree would destroy the verified inputs, and reading the out tree as
    // raw input is always a mistake.
    let raw_canon = raw_dir
        .canonicalize()
        .with_context(|| format!("raw dir {} is not readable", raw_dir.display()))?;
    let out_parent_canon = out_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .with_context(|| format!("out dir {} is not writable", out_dir.display()))?;
    let out_canon = out_parent_canon.join(out_dir.file_name().unwrap_or_default());
    if raw_canon == out_canon
        || raw_canon.starts_with(&out_canon)
        || out_canon.starts_with(&raw_canon)
    {
        bail!(
            "raw dir ({}) and out dir ({}) overlap; conversion would destroy its own input",
            raw_dir.display(),
            out_dir.display()
        );
    }

    let clips = load_and_verify(raw_dir)?;
    if dump_canonical {
        print!("{}", output::canonical_dump(&clips));
        return Ok(true);
    }

    write_outputs(&clips, out_dir)
        .with_context(|| format!("writing converted animations to {}", out_dir.display()))?;
    println!(
        "convert_link_animations: OK: {} clips (catalog + documents) in {}",
        clips.len(),
        out_dir.display()
    );
    Ok(true)
}

/// Read inventory.json, hash-check every entry, reject missing/extra files,
/// and parse every clip. The whole raw tree must validate before any output.
fn load_and_verify(raw_dir: &Path) -> Result<Vec<AnimationClip>> {
    let inventory_path = raw_dir.join("inventory.json");
    let inventory_text = fs::read_to_string(&inventory_path)
        .with_context(|| format!("reading {} (extract first)", inventory_path.display()))?;
    let inventory: Inventory = serde_json::from_str(&inventory_text)
        .with_context(|| format!("parsing {}", inventory_path.display()))?;
    if inventory.version != 1 {
        bail!(
            "inventory version {} is not supported (expected 1)",
            inventory.version
        );
    }
    if inventory.entries.is_empty() {
        bail!("inventory lists no clips");
    }

    let mut expected: BTreeSet<PathBuf> = BTreeSet::new();
    let mut entry_ids: BTreeSet<(String, u32)> = BTreeSet::new();
    for entry in &inventory.entries {
        let rel = PathBuf::from(format!("{}/{}", entry.archive, entry.member));
        if !expected.insert(rel) {
            bail!(
                "inventory lists {}/{} more than once",
                entry.archive,
                entry.member
            );
        }
        if !entry_ids.insert((entry.archive.clone(), entry.entry_index)) {
            bail!(
                "inventory reuses entry index {} in {}",
                entry.entry_index,
                entry.archive
            );
        }
    }
    // Reject extra raw files: the set of files on disk must be exactly the
    // inventory plus inventory.json itself.
    let mut actual: BTreeSet<PathBuf> = BTreeSet::new();
    collect_files(raw_dir, raw_dir, &mut actual)?;
    actual.remove(Path::new("inventory.json"));
    if actual != expected {
        for extra in actual.difference(&expected) {
            eprintln!(
                "convert_link_animations: raw file not in inventory: {}",
                extra.display()
            );
        }
        for missing in expected.difference(&actual) {
            eprintln!(
                "convert_link_animations: inventory file missing from raw tree: {}",
                missing.display()
            );
        }
        bail!("raw tree membership does not match the inventory");
    }

    let mut clips = Vec::with_capacity(inventory.entries.len());
    for entry in &inventory.entries {
        let rel = format!("{}/{}", entry.archive, entry.member);
        let path = raw_dir.join(&rel);
        let data = fs::read(&path).with_context(|| format!("reading {rel}"))?;
        let sha = sha256_hex(&data);
        if sha != entry.sha256 {
            bail!(
                "{rel}: sha256 {sha} does not match inventory entry {}",
                entry.sha256
            );
        }
        if data.len() as u64 != entry.size {
            bail!(
                "{rel}: size {} does not match inventory entry {}",
                data.len(),
                entry.size
            );
        }
        let identity = ClipIdentity {
            archive: entry.archive.clone(),
            member: entry.member.clone(),
            entry_index: entry.entry_index,
            resource_id: entry.resource_id,
            sha256: sha,
        };
        let clip =
            animation::parse_clip(entry.format, &data, identity).with_context(|| rel.clone())?;
        clips.push(clip);
    }
    Ok(clips)
}

fn collect_files(root: &Path, dir: &Path, out: &mut BTreeSet<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("listing {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .expect("child of root")
                .to_path_buf();
            out.insert(rel);
        }
    }
    Ok(())
}

/// Write catalog + clip documents into a staging dir next to `out_dir`, then
/// swap it in. The previous output is moved aside first and restored if the
/// swap fails, so a failure never destroys the last valid tree and never
/// publishes partial output.
fn write_outputs(clips: &[AnimationClip], out_dir: &Path) -> Result<()> {
    let parent = out_dir.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".staging-converted-{}", std::process::id()));
    let backup = parent.join(format!(".backup-converted-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    let result = (|| -> Result<()> {
        fs::create_dir_all(&staging)?;
        let catalog = output::build_catalog(clips);
        fs::write(
            staging.join(output::CATALOG_FILE),
            output::json_bytes(&catalog)?,
        )?;
        for clip in clips {
            let rel = output::clip_output_file(&clip.identity.archive, &clip.identity.member);
            let path = staging.join(&rel);
            fs::create_dir_all(path.parent().expect("clips/… has a parent"))?;
            fs::write(&path, output::json_bytes(clip)?)?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            // Swap with rollback: previous tree aside, new tree in, previous
            // deleted only after the new tree is in place.
            if out_dir.exists() {
                fs::rename(out_dir, &backup).with_context(|| {
                    format!("moving previous output aside from {}", out_dir.display())
                })?;
            }
            match fs::rename(&staging, out_dir) {
                Ok(()) => {
                    let _ = fs::remove_dir_all(&backup);
                    Ok(())
                }
                Err(err) => {
                    // Restore the previous tree; the staged tree is cleaned
                    // up below either way.
                    if backup.exists() && !out_dir.exists() {
                        fs::rename(&backup, out_dir).with_context(|| {
                            format!("restoring previous output into {}", out_dir.display())
                        })?;
                    }
                    Err(err).with_context(|| {
                        format!("publishing new output into {}", out_dir.display())
                    })
                }
            }
        }
        Err(err) => {
            let _ = fs::remove_dir_all(&staging);
            Err(err)
        }
    }
    .inspect_err(|_| {
        // Best-effort cleanup of leftovers; a surviving backup is kept (it
        // may be the only copy of the previous output).
        let _ = fs::remove_dir_all(&staging);
    })
}

pub fn sha256_hex(data: &[u8]) -> String {
    // Minimal SHA-256 (FIPS 180-4) so the binary has no extra dependencies;
    // the extraction script uses hashlib and both sides are cross-checked by
    // the shared manifests.
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = vec![b'a'; 1_000];
        assert_eq!(
            sha256_hex(&long),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }
}
