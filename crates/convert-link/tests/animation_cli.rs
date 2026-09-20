//! CLI integration tests for `convert_link_animations` against synthetic
//! raw inventories (no game assets; every fixture is built here in code).
//! Invokes the real binary via `CARGO_BIN_EXE_convert_link_animations`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

const BIN: &str = env!("CARGO_BIN_EXE_convert_link_animations");

// --- tiny sha256 (same construction as the binary's, kept independent) ------

fn sha256_hex(data: &[u8]) -> String {
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
    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
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

// --- fixture builders ------------------------------------------------------------

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}
fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}
fn bef32(v: f32) -> [u8; 4] {
    v.to_bits().to_be_bytes()
}

fn pad32(mut v: Vec<u8>) -> Vec<u8> {
    while !v.len().is_multiple_of(0x20) {
        v.push(0);
    }
    v
}

fn j3d_file(file_type: &[u8; 4], mut chunk: Vec<u8>) -> Vec<u8> {
    let chunk_len = chunk.len();
    chunk[4..8].copy_from_slice(&be32(chunk_len as u32));
    let mut file = vec![0u8; 0x20];
    file[0..4].copy_from_slice(b"J3D1");
    file[4..8].copy_from_slice(file_type);
    file[8..12].copy_from_slice(&be32((0x20 + chunk.len()) as u32));
    file[12..16].copy_from_slice(&be32(1));
    file[0x1C..0x20].copy_from_slice(&be32(0xFFFF_FFFF));
    file.extend_from_slice(&chunk);
    file
}

fn name_table(names: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&be16(names.len() as u16));
    out.extend_from_slice(&be16(0xFFFF));
    let mut data_off = 4 + 4 * names.len();
    let mut entries = Vec::new();
    let mut strings = Vec::new();
    for n in names {
        entries.extend_from_slice(&be16(0));
        entries.extend_from_slice(&be16(data_off as u16));
        strings.extend_from_slice(n.as_bytes());
        strings.push(0);
        data_off += n.len() + 1;
    }
    out.extend_from_slice(&entries);
    out.extend_from_slice(&strings);
    out
}

fn build_bck() -> Vec<u8> {
    let mut header = vec![0u8; 0x24];
    header[0..4].copy_from_slice(b"ANK1");
    header[8] = 2;
    header[0x0A..0x0C].copy_from_slice(&be16(6));
    header[0x0C..0x0E].copy_from_slice(&be16(1)); // one joint
    header[0x10..0x12].copy_from_slice(&be16(1)); // rot count
    header[0x12..0x14].copy_from_slice(&be16(1)); // trans count
    let mut tables = Vec::new();
    for _ in 0..3 {
        // S default, R constant idx 0, T constant idx 0
        for (count, index) in [(0u16, 0u16), (1, 0), (1, 0)] {
            tables.extend_from_slice(&be16(count));
            tables.extend_from_slice(&be16(index));
            tables.extend_from_slice(&be16(0));
        }
    }
    header[0x14..0x18].copy_from_slice(&be32(0x24));
    let rot_off = 0x24 + tables.len();
    header[0x1C..0x20].copy_from_slice(&be32(rot_off as u32));
    let trans_off = rot_off + 2;
    header[0x20..0x24].copy_from_slice(&be32(trans_off as u32));
    let mut chunk = header;
    chunk.extend_from_slice(&tables);
    chunk.extend_from_slice(&be16(0x4000));
    chunk.extend_from_slice(&bef32(5.5));
    j3d_file(b"bck1", pad32(chunk))
}

fn build_btp() -> Vec<u8> {
    let rows: [(&str, u16, u8, [u16; 2]); 2] = [("mouth", 14, 0, [27, 7]), ("eyeL", 1, 0, [0, 0])];
    let mut header = vec![0u8; 0x20];
    header[0..4].copy_from_slice(b"TPT1");
    header[8] = 2;
    header[9] = 0xFF;
    header[0x0A..0x0C].copy_from_slice(&be16(10));
    header[0x0C..0x0E].copy_from_slice(&be16(rows.len() as u16));
    header[0x0E..0x10].copy_from_slice(&be16(4));
    let mut table = Vec::new();
    let mut values = Vec::new();
    for (_, _, texno, samples) in rows.iter() {
        let index = values.len() as u16;
        values.extend_from_slice(samples);
        table.extend_from_slice(&be16(samples.len() as u16));
        table.extend_from_slice(&be16(index));
        table.push(*texno);
        table.push(0);
        table.extend_from_slice(&be16(0));
    }
    let values_off = 0x20 + table.len();
    let remap_off = values_off + 2 * values.len();
    let names_off = remap_off + 2 * rows.len();
    header[0x10..0x14].copy_from_slice(&be32(0x20));
    header[0x14..0x18].copy_from_slice(&be32(values_off as u32));
    header[0x18..0x1C].copy_from_slice(&be32(remap_off as u32));
    header[0x1C..0x20].copy_from_slice(&be32(names_off as u32));
    let mut chunk = header;
    chunk.extend_from_slice(&table);
    for v in &values {
        chunk.extend_from_slice(&be16(*v));
    }
    for (_, remap, _, _) in &rows {
        chunk.extend_from_slice(&be16(*remap));
    }
    chunk.extend_from_slice(&name_table(&["mouth", "eyeL"]));
    j3d_file(b"btp1", pad32(chunk))
}

fn build_btk() -> Vec<u8> {
    let mut header = vec![0u8; 0x60];
    header[0..4].copy_from_slice(b"TTK1");
    header[8] = 2;
    header[9] = 1;
    header[0x0A..0x0C].copy_from_slice(&be16(20));
    header[0x0C..0x0E].copy_from_slice(&be16(3));
    let mut tables = Vec::new();
    for _ in 0..3 {
        for (count, index, tangent) in [(0u16, 0u16, 0u16), (0, 0, 0), (0, 0, 0)] {
            tables.extend_from_slice(&be16(count));
            tables.extend_from_slice(&be16(index));
            tables.extend_from_slice(&be16(tangent));
        }
    }
    let remap_off = 0x60 + tables.len();
    let names_off = remap_off + 2;
    let names = name_table(&["eyeL"]);
    let selectors_off = names_off + names.len();
    let centers_off = selectors_off + 1;
    header[0x14..0x18].copy_from_slice(&be32(0x60));
    header[0x18..0x1C].copy_from_slice(&be32(remap_off as u32));
    header[0x1C..0x20].copy_from_slice(&be32(names_off as u32));
    header[0x20..0x24].copy_from_slice(&be32(selectors_off as u32));
    header[0x24..0x28].copy_from_slice(&be32(centers_off as u32));
    header[0x28..0x2C].copy_from_slice(&be32(centers_off as u32 + 12));
    header[0x2C..0x30].copy_from_slice(&be32(centers_off as u32 + 12));
    header[0x30..0x34].copy_from_slice(&be32(centers_off as u32 + 12));
    header[0x5C..0x60].copy_from_slice(&be32(0));
    let mut chunk = header;
    chunk.extend_from_slice(&tables);
    chunk.extend_from_slice(&be16(7));
    chunk.extend_from_slice(&names);
    chunk.push(2);
    chunk.extend_from_slice(&bef32(0.5));
    chunk.extend_from_slice(&bef32(-0.5));
    chunk.extend_from_slice(&bef32(0.25));
    j3d_file(b"btk1", pad32(chunk))
}

// --- harness ----------------------------------------------------------------------

struct RawDir {
    root: PathBuf,
}

impl RawDir {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "anim-cli-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn add(&self, archive: &str, member: &str, format: &str, data: &[u8]) -> serde_json::Value {
        let path = self.root.join(archive).join(member);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        let ordinal = self.next_ordinal();
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

    fn next_ordinal(&self) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static ORDINAL: AtomicU64 = AtomicU64::new(1);
        ORDINAL.fetch_add(1, Ordering::SeqCst)
    }

    fn write_inventory(&self, entries: &[serde_json::Value]) {
        let doc = json!({"version": 1, "disc": "GZLE01", "entries": entries});
        fs::write(self.root.join("inventory.json"), doc.to_string()).unwrap();
    }

    fn out(&self, label: &str) -> PathBuf {
        self.root.parent().unwrap().join(format!(
            "anim-out-{label}-{}",
            self.root.file_name().unwrap().to_str().unwrap()
        ))
    }
}

impl Drop for RawDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn full_raw() -> RawDir {
    let raw = RawDir::new("full");
    let bck = raw.add("LkAnm", "bcks/a.bck", "bck", &build_bck());
    let btp = raw.add("LkAnm", "btp/f.btp", "btp", &build_btp());
    let btk = raw.add("LkD01", "btk/s.btk", "btk", &build_btk());
    raw.write_inventory(&[bck, btp, btk]);
    raw
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN).args(args).output().expect("binary runs")
}

fn tree_hashes(root: &Path) -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            (rel, sha256_hex(&fs::read(&p).unwrap()))
        })
        .collect()
}

#[test]
fn animation_cli_synthetic_end_to_end() {
    let raw = full_raw();
    let out = raw.out("e2e");
    let status = run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let catalog: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("catalog.json")).unwrap()).unwrap();
    assert_eq!(catalog["version"], 1);
    assert_eq!(catalog["clips"].as_array().unwrap().len(), 3);
    assert!(out.join("clips/LkAnm/bcks/a.bck.json").is_file());
    assert!(out.join("clips/LkAnm/btp/f.btp.json").is_file());
    assert!(out.join("clips/LkD01/btk/s.btk.json").is_file());

    // The documents round-trip through the validated public reader.
    let clip_text = fs::read_to_string(out.join("clips/LkAnm/btp/f.btp.json")).unwrap();
    let clip = gx::animation_manifest::read_clip(&clip_text).unwrap();
    match clip.data {
        gx::animation_manifest::ClipData::Btp(ref b) => {
            assert_eq!(b.targets[0].material_remap, 14);
        }
        _ => panic!("wrong payload"),
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn animation_cli_deterministic_output() {
    let raw = full_raw();
    let out1 = raw.out("det1");
    let out2 = raw.out("det2");
    assert!(
        run(&[raw.root.to_str().unwrap(), out1.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        run(&[raw.root.to_str().unwrap(), out2.to_str().unwrap()])
            .status
            .success()
    );
    assert_eq!(tree_hashes(&out1), tree_hashes(&out2));
    let _ = fs::remove_dir_all(&out1);
    let _ = fs::remove_dir_all(&out2);
}

#[test]
fn animation_cli_invalid_input_does_not_publish() {
    // Hash mismatch.
    let raw = full_raw();
    let member = raw.root.join("LkAnm/bcks/a.bck");
    let mut bytes = fs::read(&member).unwrap();
    let last = bytes.len() - 2;
    bytes[last] ^= 0xFF;
    fs::write(&member, bytes).unwrap();
    let out = raw.out("bad1");
    let status = run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(!status.status.success());
    assert!(!out.exists(), "failed run must not publish output");
    let _ = fs::remove_dir_all(&out);

    // Extra raw file not in the inventory.
    let raw = full_raw();
    fs::write(raw.root.join("LkAnm/bcks/extra.bck"), build_bck()).unwrap();
    let out = raw.out("bad2");
    let status = run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(!status.status.success());
    assert!(String::from_utf8_lossy(&status.stderr).contains("not in inventory"));
    let _ = fs::remove_dir_all(&out);

    // Corrupt clip (bad chunk magic) with a matching hash.
    let raw = full_raw();
    let mut broken = build_bck();
    broken[0x20..0x24].copy_from_slice(b"TPT1");
    let entry = raw.add("LkAnm", "bcks/broken.bck", "bck", &broken);
    let btp = raw.add("LkAnm", "btp/f.btp", "btp", &build_btp());
    raw.write_inventory(&[entry, btp]);
    let out = raw.out("bad3");
    let status = run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(!status.status.success());
    assert!(!out.exists());
    let _ = fs::remove_dir_all(&out);

    // Previous valid output survives a failed rerun: the tampered member no
    // longer matches its inventory hash, so verification fails before any
    // publication step.
    let raw = full_raw();
    let out = raw.out("keep");
    assert!(
        run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()])
            .status
            .success()
    );
    let before = tree_hashes(&out);
    fs::write(raw.root.join("LkAnm/bcks/a.bck"), build_btp()).unwrap();
    let status = run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(!status.status.success());
    assert_eq!(
        tree_hashes(&out),
        before,
        "failed rerun must not touch published output"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn animation_cli_duplicate_inventory_rows_rejected() {
    // Same archive/member listed twice: rejected even though the file
    // itself is valid.
    let raw = RawDir::new("dup");
    let bck = raw.add("LkAnm", "bcks/a.bck", "bck", &build_bck());
    raw.write_inventory(&[bck.clone(), bck]);
    let out = raw.out("dup");
    let status = run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(!status.status.success());
    assert!(String::from_utf8_lossy(&status.stderr).contains("more than once"));
    assert!(!out.exists());
    let _ = fs::remove_dir_all(&out);

    // Reused entry index within one archive: rejected too.
    let raw = RawDir::new("dupidx");
    let a = raw.add("LkAnm", "bcks/a.bck", "bck", &build_bck());
    let mut b = raw.add("LkAnm", "btp/b.btp", "btp", &build_btp());
    b["entry_index"] = json!(a["entry_index"]);
    raw.write_inventory(&[a, b]);
    let out = raw.out("dupidx");
    let status = run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(!status.status.success());
    assert!(String::from_utf8_lossy(&status.stderr).contains("reuses entry index"));
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn animation_cli_rejects_overlapping_raw_and_out_dirs() {
    let raw = full_raw();
    // out == raw
    let status = run(&[raw.root.to_str().unwrap(), raw.root.to_str().unwrap()]);
    assert!(!status.status.success());
    assert!(String::from_utf8_lossy(&status.stderr).contains("overlap"));
    // out inside raw
    let inner = raw.root.join("converted");
    let status = run(&[raw.root.to_str().unwrap(), inner.to_str().unwrap()]);
    assert!(!status.status.success());
    assert!(String::from_utf8_lossy(&status.stderr).contains("overlap"));
    // and the raw tree was not damaged
    assert!(raw.root.join("inventory.json").is_file());
}

#[test]
fn animation_cli_help_and_errors() {
    let status = run(&["--help"]);
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("usage"));

    let status = run(&[]);
    assert_eq!(status.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&status.stderr).contains("usage"));

    let status = run(&["--nope"]);
    assert_eq!(status.status.code(), Some(2));

    // Missing inventory fails clearly.
    let raw = RawDir::new("empty");
    let out = raw.out("empty");
    let status = run(&[raw.root.to_str().unwrap(), out.to_str().unwrap()]);
    assert!(!status.status.success());
    assert!(String::from_utf8_lossy(&status.stderr).contains("inventory"));
}

#[test]
fn animation_cli_dump_canonical() {
    let raw = full_raw();
    let out = raw.out("dump");
    let status = run(&[
        raw.root.to_str().unwrap(),
        out.to_str().unwrap(),
        "--dump-canonical",
    ]);
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let text = String::from_utf8(status.stdout).unwrap();
    assert!(text.contains("clip LkAnm/bcks/a.bck bck sha256="));
    assert!(text.contains("remap=14 texno=0 samples=2:27,7"));
    assert!(text.contains("matrix_calc=0"));
    // Canonical mode writes nothing.
    assert!(!out.exists());
}
