//! CLI integration tests for `convert_link_animations` against synthetic raw
//! inventories (no game assets). The J3D fixtures come from the library's
//! `animation::*::fixtures` modules, the same ones the parser unit tests use;
//! these tests assert only what the CLI adds on top: inventory verification,
//! atomic publication, path guards, and argument handling.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::json;

use convert_link::animation::{bck, btk, btp};
use convert_link::sha256_hex;

const BIN: &str = env!("CARGO_BIN_EXE_convert_link_animations");

// --- harness ----------------------------------------------------------------------

/// A synthetic raw tree inside a private temp base directory.
///
/// Every output path this hands out is a sibling of `root` inside `base`, so
/// the converter's `.staging-*` and `.backup-*` scratch directories land in
/// `base` too. Tests run in parallel; without that isolation one test would
/// observe another's in-flight scratch directory. `Drop` removes `base`, so a
/// panicking test leaves nothing behind.
struct RawDir {
    base: PathBuf,
    root: PathBuf,
    next_ordinal: Cell<u64>,
}

impl RawDir {
    fn new(label: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "anim-cli-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = base.join("raw");
        fs::create_dir_all(&root).unwrap();
        Self {
            base,
            root,
            next_ordinal: Cell::new(1),
        }
    }

    /// Write one raw member and return the inventory row describing it.
    fn add(&self, archive: &str, member: &str, format: &str, data: &[u8]) -> serde_json::Value {
        let path = self.root.join(archive).join(member);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        let ordinal = self.next_ordinal.get();
        self.next_ordinal.set(ordinal + 1);
        json!({
            "archive": archive,
            "member": member,
            "entry_index": ordinal,
            "resource_id": ordinal + 1,
            "format": format,
            "size": data.len(),
            "sha256": sha256_hex(data),
        })
    }

    fn write_inventory(&self, entries: &[serde_json::Value]) {
        self.write_inventory_doc(&json!({"version": 1, "disc": "GZLE01", "entries": entries}));
    }

    fn write_inventory_doc(&self, doc: &serde_json::Value) {
        fs::write(self.root.join("inventory.json"), doc.to_string()).unwrap();
    }

    fn read_inventory(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.root.join("inventory.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    /// An output path beside the raw root, inside this fixture's base.
    fn out(&self, label: &str) -> PathBuf {
        self.base.join(format!("out-{label}"))
    }
}

impl Drop for RawDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// A raw tree with one clip of each format. The inventory lists them in
/// reverse sorted order, so the converter's sort changes the order.
fn full_raw() -> RawDir {
    let raw = RawDir::new("full");
    let btk_row = raw.add(
        "LkD01",
        "btk/s.btk",
        "btk",
        &btk::fixtures::build_btk(true, 1).file,
    );
    let btp_row = raw.add("LkAnm", "btp/f.btp", "btp", &btp::fixtures::build_btp());
    let bck_row = raw.add("LkAnm", "bcks/a.bck", "bck", &bck::fixtures::build_bck());
    raw.write_inventory(&[btk_row, btp_row, bck_row]);
    raw
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("binary runs")
}

fn convert(raw: &RawDir, out: &Path) -> Output {
    run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()])
}

fn run_ok(raw: &RawDir, out: &Path) {
    let result = convert(raw, out);
    assert!(
        result.status.success(),
        "conversion failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// Assert the conversion fails and return its stderr.
fn run_err(raw: &RawDir, out: &Path) -> String {
    let result = convert(raw, out);
    assert!(
        !result.status.success(),
        "conversion unexpectedly succeeded"
    );
    String::from_utf8_lossy(&result.stderr).into_owned()
}

/// Every file under `root`, by relative path, with its contents.
fn tree_bytes(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|p| {
            (
                p.strip_prefix(root).unwrap().to_path_buf(),
                fs::read(&p).unwrap(),
            )
        })
        .collect()
}

// --- successful conversion ---------------------------------------------------------

#[test]
fn writes_catalog_and_clip_documents() {
    let raw = full_raw();
    let out = raw.out("e2e");
    run_ok(&raw, &out);

    let catalog_text = fs::read_to_string(out.join("catalog.json")).unwrap();
    let catalog = gx::animation_manifest::read_catalog(&catalog_text).unwrap();
    assert_eq!(catalog.clips.len(), 3);
    for row in &catalog.clips {
        let clip_text = fs::read_to_string(out.join(&row.file))
            .unwrap_or_else(|e| panic!("catalog row {} points at no file: {e}", row.file));
        let clip = gx::animation_manifest::read_clip(&clip_text).unwrap();
        assert_eq!(clip.identity.archive, row.archive);
        assert_eq!(clip.identity.member, row.member);
        assert_eq!(clip.format(), row.format);
    }
}

#[test]
fn catalog_rows_are_sorted_by_archive_and_member() {
    let raw = full_raw();
    let out = raw.out("sorted");
    run_ok(&raw, &out);

    let catalog_text = fs::read_to_string(out.join("catalog.json")).unwrap();
    let catalog = gx::animation_manifest::read_catalog(&catalog_text).unwrap();
    let order: Vec<_> = catalog
        .clips
        .iter()
        .map(|c| format!("{}/{}", c.archive, c.member))
        .collect();
    assert_eq!(
        order,
        ["LkAnm/bcks/a.bck", "LkAnm/btp/f.btp", "LkD01/btk/s.btk"]
    );
}

#[test]
fn repeated_runs_produce_identical_trees() {
    let raw = full_raw();
    let first = raw.out("det1");
    let second = raw.out("det2");
    run_ok(&raw, &first);
    run_ok(&raw, &second);
    assert_eq!(tree_bytes(&first), tree_bytes(&second));
}

#[test]
fn rerun_replaces_previous_output() {
    let raw = full_raw();
    let out = raw.out("rerun");
    run_ok(&raw, &out);
    assert!(out.join("clips/LkD01/btk/s.btk.json").is_file());

    // Drop one clip from both the raw tree and the inventory, then rerun.
    fs::remove_file(raw.root.join("LkD01/btk/s.btk")).unwrap();
    let mut inventory = raw.read_inventory();
    let entries = inventory["entries"].as_array().unwrap().clone();
    inventory["entries"] = json!(
        entries
            .into_iter()
            .filter(|e| e["archive"] != "LkD01")
            .collect::<Vec<_>>()
    );
    raw.write_inventory_doc(&inventory);
    run_ok(&raw, &out);

    let catalog_text = fs::read_to_string(out.join("catalog.json")).unwrap();
    let catalog = gx::animation_manifest::read_catalog(&catalog_text).unwrap();
    assert_eq!(catalog.clips.len(), 2);
    assert!(
        !out.join("clips/LkD01").exists(),
        "the replaced tree must not keep the stale clip"
    );
}

#[test]
fn successful_run_leaves_no_scratch_directories() {
    let raw = full_raw();
    let out = raw.out("scratch");
    run_ok(&raw, &out);

    let leftovers: Vec<_> = fs::read_dir(&raw.base)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|name| {
            name.starts_with(".staging-converted-") || name.starts_with(".backup-converted-")
        })
        .collect();
    assert_eq!(leftovers, Vec::<String>::new());
}

#[test]
fn dump_canonical_prints_dump_and_writes_nothing() {
    let raw = full_raw();
    let out = raw.out("dump");
    let result = run(&[
        raw.root.to_str().unwrap(),
        out.to_str().unwrap(),
        "--dump-canonical",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!out.exists(), "canonical mode must not publish output");
    insta::assert_snapshot!(String::from_utf8(result.stdout).unwrap());
}

// --- rejected input ----------------------------------------------------------------

#[test]
fn rejects_hash_mismatch() {
    let raw = full_raw();
    let member = raw.root.join("LkAnm/bcks/a.bck");
    let mut bytes = fs::read(&member).unwrap();
    let last = bytes.len() - 2;
    bytes[last] ^= 0xFF;
    fs::write(&member, bytes).unwrap();

    let out = raw.out("hash");
    let stderr = run_err(&raw, &out);
    assert!(
        stderr.contains("does not match inventory entry"),
        "{stderr}"
    );
    assert!(!out.exists(), "failed run must not publish output");
}

#[test]
fn rejects_size_mismatch() {
    // The hash still matches; only the inventory's size field lies.
    let raw = full_raw();
    let mut inventory = raw.read_inventory();
    inventory["entries"][0]["size"] = json!(1);
    raw.write_inventory_doc(&inventory);

    let out = raw.out("size");
    let stderr = run_err(&raw, &out);
    assert!(stderr.contains("size"), "{stderr}");
    assert!(
        stderr.contains("does not match inventory entry 1"),
        "{stderr}"
    );
    assert!(!out.exists());
}

#[test]
fn rejects_raw_file_not_in_inventory() {
    let raw = full_raw();
    fs::write(
        raw.root.join("LkAnm/bcks/extra.bck"),
        bck::fixtures::build_bck(),
    )
    .unwrap();

    let out = raw.out("extra");
    let stderr = run_err(&raw, &out);
    assert!(stderr.contains("not in inventory"), "{stderr}");
    assert!(stderr.contains("LkAnm/bcks/extra.bck"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn rejects_inventory_file_missing_from_raw_tree() {
    let raw = full_raw();
    fs::remove_file(raw.root.join("LkAnm/btp/f.btp")).unwrap();

    let out = raw.out("missing");
    let stderr = run_err(&raw, &out);
    assert!(stderr.contains("missing from raw tree"), "{stderr}");
    assert!(stderr.contains("LkAnm/btp/f.btp"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn rejects_unparseable_clip() {
    // A hash-correct file whose chunk magic is wrong for its declared format.
    let raw = RawDir::new("unparseable");
    let mut broken = bck::fixtures::build_bck();
    broken[0x20..0x24].copy_from_slice(b"TPT1");
    let row = raw.add("LkAnm", "bcks/broken.bck", "bck", &broken);
    raw.write_inventory(&[row]);

    let out = raw.out("unparseable");
    let stderr = run_err(&raw, &out);
    assert!(stderr.contains("LkAnm/bcks/broken.bck"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn rejects_duplicate_inventory_rows() {
    let raw = RawDir::new("dup");
    let row = raw.add("LkAnm", "bcks/a.bck", "bck", &bck::fixtures::build_bck());
    raw.write_inventory(&[row.clone(), row]);

    let out = raw.out("dup");
    let stderr = run_err(&raw, &out);
    assert!(stderr.contains("more than once"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn rejects_reused_entry_index() {
    let raw = RawDir::new("dupidx");
    let first = raw.add("LkAnm", "bcks/a.bck", "bck", &bck::fixtures::build_bck());
    let mut second = raw.add("LkAnm", "btp/b.btp", "btp", &btp::fixtures::build_btp());
    second["entry_index"] = first["entry_index"].clone();
    raw.write_inventory(&[first, second]);

    let out = raw.out("dupidx");
    let stderr = run_err(&raw, &out);
    assert!(stderr.contains("reuses entry index"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn rejects_unsupported_inventory_version() {
    let raw = full_raw();
    let mut inventory = raw.read_inventory();
    inventory["version"] = json!(2);
    raw.write_inventory_doc(&inventory);

    let out = raw.out("version");
    let stderr = run_err(&raw, &out);
    assert!(
        stderr.contains("inventory version 2 is not supported"),
        "{stderr}"
    );
    assert!(!out.exists());
}

#[test]
fn rejects_empty_inventory() {
    let raw = RawDir::new("emptyinv");
    raw.write_inventory(&[]);

    let out = raw.out("emptyinv");
    let stderr = run_err(&raw, &out);
    assert!(stderr.contains("inventory lists no clips"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn reports_missing_inventory_clearly() {
    let raw = RawDir::new("noinv");
    let out = raw.out("noinv");
    let stderr = run_err(&raw, &out);
    assert!(stderr.contains("inventory.json"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn failed_rerun_leaves_published_output_intact() {
    let raw = full_raw();
    let out = raw.out("keep");
    run_ok(&raw, &out);
    let published = tree_bytes(&out);

    // Replace a member with different bytes: verification fails before any
    // publication step.
    fs::write(
        raw.root.join("LkAnm/bcks/a.bck"),
        btp::fixtures::build_btp(),
    )
    .unwrap();
    run_err(&raw, &out);

    assert_eq!(
        tree_bytes(&out),
        published,
        "failed rerun must not touch published output"
    );
}

// --- path guards -------------------------------------------------------------------

#[test]
fn rejects_out_dir_equal_to_raw_dir() {
    let raw = full_raw();
    let stderr = run_err(&raw, &raw.root);
    assert!(stderr.contains("overlap"), "{stderr}");
}

#[test]
fn rejects_out_dir_inside_raw_dir() {
    let raw = full_raw();
    let stderr = run_err(&raw, &raw.root.join("converted"));
    assert!(stderr.contains("overlap"), "{stderr}");
}

#[test]
fn overlap_rejection_leaves_raw_tree_intact() {
    let raw = full_raw();
    let before = tree_bytes(&raw.root);
    run_err(&raw, &raw.root);
    run_err(&raw, &raw.root.join("converted"));
    assert_eq!(tree_bytes(&raw.root), before);
}

// --- argument handling -------------------------------------------------------------

#[test]
fn help_flags_print_usage() {
    for flag in ["--help", "-h"] {
        let result = run(&[flag]);
        assert!(result.status.success(), "{flag}");
        assert!(
            String::from_utf8_lossy(&result.stdout).contains("usage"),
            "{flag}"
        );
    }
}

#[test]
fn no_arguments_exits_two() {
    let result = run(&[]);
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("usage"));
}

#[test]
fn unknown_option_exits_two() {
    let result = run(&["--nope"]);
    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("unknown option --nope"), "{stderr}");
}
