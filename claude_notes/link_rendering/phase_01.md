# Phase 1: converter skeleton — chunk walk

Detailed plan for P1 of [`../link_rendering.md`](../link_rendering.md) §6.
Estimated: 1 day. Verification strategy follows [`tests.md`](tests.md) §P1.
Depends on P0 ([`phase_00.md`](phase_00.md)): `just extract-link` must have
populated `assets/link/raw/cl.bdl` (364,544 bytes, hash in
`scripts/link_assets.sha256`).

**Goal**: a `convert_link` binary that opens `cl.bdl`, validates the J3D2
header, walks the 9-block chunk table with hard internal invariants, and
prints a canonical `--info` table that diffs byte-for-byte against an
independent gclib oracle script — establishing the parsing foundation (BE
reader + typed errors + dispatch skeleton) that P2/P3 grow into, without
parsing any chunk interiors.

**Deliverables**

1. `src/bin/convert_link/main.rs` — CLI + orchestration (dir-style bin,
   auto-discovered by cargo; **no `Cargo.toml` change**)
2. `src/bin/convert_link/be.rs` — hand-rolled big-endian reader (~60 lines,
   no new deps) + inline unit tests
3. `src/bin/convert_link/bmd/mod.rs` — header validation, chunk-table walk,
   dispatch stubs + inline unit tests on synthetic buffers
4. `scripts/link_chunk_table.py` — gclib oracle printing the same canonical
   table (uv PEP-723 script; dev-only dependency)
5. `just convert-link` and `just link-verify-p1` recipes
6. One `#[ignore]`d real-file test (`just test` stays green on a clean
   checkout without extracted assets)

## File-format facts this phase relies on

Verified against `../tww/include/JSystem/JUtility/JUTDataHeader.h` and
`../tww/src/JSystem/J3DGraphLoader/J3DModelLoader.cpp`:

- File header: `0x00` u32 magic `'J3D2'`, `0x04` u32 type `'bdl4'`, `0x08`
  u32 fileSize, `0x0C` u32 blockNum, `0x10..0x20` padding; first block at
  `0x20`.
- Block header: u32 FourCC + u32 size; size **includes** the 8-byte header;
  `next = this + size`. The game's loader iterates `blockNum` blocks and
  switches on the FourCC.
- Expected blocks in `cl.bdl`, in order: INF1, VTX1, EVP1, DRW1, JNT1, SHP1,
  MAT3, MDL3, TEX1. JNT1 must report 42 joints.
- **Count-peek hypothesis**: a BE u16 count sits at block offset +8 for
  {EVP1, DRW1, JNT1, SHP1, MAT3, TEX1}. INF1 has flags there (its counts are
  u32s deeper in), VTX1 has a format-table offset, MDL3 we skip — those three
  print no count. This is a pattern, not a spec: **confirm each offset
  against the corresponding `J3D*Factory` source during implementation** and
  cite the lines in Recorded facts below.

## Step 1 — `be.rs`: big-endian reader

```rust
pub struct BeReader<'a> { data: &'a [u8], pos: usize }

#[derive(Debug, Clone, PartialEq)]
pub struct BeError { pub offset: usize, pub wanted: usize, pub len: usize }
// Display: "read of {wanted} bytes at {offset:#x} past end ({len} bytes)"
// + impl std::error::Error, so anyhow context chains work

pub type BeResult<T> = Result<T, BeError>;

impl<'a> BeReader<'a> {
    pub fn new(data: &'a [u8]) -> Self;
    pub fn pos(&self) -> usize;
    pub fn seek(&mut self, pos: usize) -> BeResult<()>;   // past-end = error
    pub fn skip(&mut self, n: usize) -> BeResult<()>;
    pub fn u8(&mut self) -> BeResult<u8>;
    pub fn u16(&mut self) -> BeResult<u16>;
    pub fn i16(&mut self) -> BeResult<i16>;
    pub fn u32(&mut self) -> BeResult<u32>;
    pub fn f32(&mut self) -> BeResult<f32>;                // f32::from_bits
    pub fn bytes(&mut self, n: usize) -> BeResult<&'a [u8]>;
    pub fn str_fixed(&mut self, n: usize) -> BeResult<&'a str>; // non-UTF8 = error
    pub fn at(&self, pos: usize) -> BeReader<'a>;          // sub-reader; parent pos untouched
}
```

Design rules, stated once here and binding for all later phases:

- **Result-not-panic everywhere.** Out-of-bounds is an error carrying the
  byte offset — the debugging breadcrumb for every P2/P3 parse bug. No
  truncated reads, no `unwrap` in parse paths.
- Implementation is `u16::from_be_bytes` over checked slices — no `byteorder`
  crate, no `unsafe`, no new dependencies.
- `at()` returns a cheap sub-reader so count-peeking (and later, offset-table
  chasing in MAT3) never disturbs a parent cursor.
- **Deviation from house style, on purpose**: existing bins use
  `fn main()` + `.expect()`; the converter's `main` returns
  `anyhow::Result<()>` with `.context()` because a parser is error-dense and
  anyhow is already a dependency. Typed errors (`BeError`, `BmdError`) stay
  concrete underneath; anyhow appears only at the `main.rs` boundary.

## Step 2 — `bmd/mod.rs`: header walk, chunk table, dispatch stubs

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FourCc(pub [u8; 4]);        // Display prints ASCII

pub struct ChunkEntry { pub fourcc: FourCc, pub offset: usize, pub size: usize, pub count: Option<u16> }
pub struct ChunkTable { pub file_size: usize, pub block_num: u32, pub chunks: Vec<ChunkEntry> }

#[derive(Debug)]
pub enum BmdError {
    BadMagic { found: [u8; 4] },                    // != b"J3D2"
    BadType { found: [u8; 4] },                     // != b"bdl4"
    SizeMismatch { header: usize, actual: usize },  // header fileSize vs bytes on disk
    BadBlockCount { found: u32 },                   // != 9
    UnknownFourCc { fourcc: [u8; 4], offset: usize },
    BlockOverrun { fourcc: FourCc, offset: usize, size: usize, file_size: usize },
    TrailingBytes { covered: usize, file_size: usize }, // 0x20 + Σ sizes != file_size
    Read(BeError),                                  // From<BeError>
}

pub fn parse_chunk_table(data: &[u8]) -> Result<ChunkTable, BmdError>;
```

- **Every invariant is a hard error on every parse** (tests.md §P1): exact
  magic/type, header fileSize == actual length, blockNum == 9, every FourCC
  in the expected set, every block in bounds, blocks contiguous from 0x20
  with sizes summing exactly to fileSize, JNT1 present with count == 42.
  The `blockNum == 9` and `JNT1 == 42` gates are cl.bdl-specific by design —
  this is a one-model converter; generalize only if another model is ever fed
  in.
- **`Expectations` seam**: `parse_chunk_table` delegates to an internal
  `parse_chunk_table_with(&Expectations)` where the expected FourCC set,
  block count, and joint count live. Synthetic unit tests construct tiny
  2-block files with relaxed expectations; the public path stays strict.
- Count peeking: `reader.at(chunk.offset + 8).u16()` for the six
  count-bearing FourCCs only.
- **Dispatch skeleton** — the growth point for P2/P3. Each later chunk gets
  its own `bmd/<chunk>.rs` with
  `pub fn parse(r: BeReader, chunk: &ChunkEntry) -> Result<X, BmdError>`;
  `mod.rs` only ever grows match arms:

```rust
for chunk in &table.chunks {
    match &chunk.fourcc.0 {
        b"MDL3" => {}                    // skipped by design: MAT3 is authoritative
        b"TEX1" | b"MAT3" => {}          // P2
        b"INF1" | b"VTX1" | b"EVP1" | b"DRW1" | b"JNT1" | b"SHP1" => {} // P3
        _ => unreachable!("validated by parse_chunk_table"),
    }
}
```

## Step 3 — `main.rs`: CLI

- Hand-rolled `std::env::args` (house convention, no CLI crate): two
  positional args + one flag. Anything else → usage on stderr, exit 2.

  ```
  usage: convert_link <raw-dir> <out-dir> [--info]
  ```
- P1 behavior: read `<raw-dir>/cl.bdl` (missing → anyhow context: "run `just
  extract-link` first"); `parse_chunk_table` (invariants always run, even
  without `--info`); `create_dir_all(out_dir)` so the recipe shape is final;
  print the canonical table iff `--info`. Nothing is written to `out_dir`
  yet.
- Stdout is reserved for the canonical table; all diagnostics go to stderr
  (the verification diff depends on this).

## Step 4 — the canonical `--info` format

One line format, printed identically by the Rust binary and the python
oracle, so verification is a literal `diff`. Both sides implement this spec —
neither imitates the other's output, which insulates the gate from gclib API
drift.

```
J3D2 bdl4 size=364544 blocks=9
INF1 0x000020 <size> -
VTX1 0x...... <size> -
EVP1 0x...... <size> <count>
DRW1 0x...... <size> <count>
JNT1 0x...... <size> 42
SHP1 0x...... <size> <count>
MAT3 0x...... <size> <count>
MDL3 0x...... <size> -
TEX1 0x...... <size> <count>
```

Rules: one header line; one line per block in file order; single-space
separated fields; offset lowercase hex, zero-padded to 6 digits, `0x`
prefix; size decimal; count decimal or literal `-` for INF1/VTX1/MDL3;
trailing newline; nothing else on stdout.

## Step 5 — the gclib oracle script

`scripts/link_chunk_table.py`, executable, uv PEP-723 header per the
`scripts/extract_beats.py` precedent:

```python
#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["gclib @ git+https://github.com/LagoLunatic/gclib"]
# ///
```

Prints exactly the canonical format for `sys.argv[1]` using gclib's J3D
support for the chunk list and counts (~20 lines). **gclib's class/attribute
names are unverified** — adjust to its real API at implementation time; if
its parsed model doesn't expose raw offsets/sizes, fall back to gclib for
counts plus an independent 8-byte header walk in the script for
offsets/sizes (still fully independent of our Rust code). Dev-only oracle:
never required for `just test` or builds.

## Step 6 — just recipes

```just
# parse Link's BDL and emit converted assets (P1: chunk walk only)
[unix]
convert-link *args:
    cargo run --bin convert_link -- assets/link/raw assets/link/converted {{args}}

# P1 gate: diff our --info chunk table against the gclib oracle, then run ignored tests
[unix]
link-verify-p1:
    #!/usr/bin/env bash
    set -euo pipefail
    diff <(just convert-link --info) <(./scripts/link_chunk_table.py assets/link/raw/cl.bdl)
    cargo test --bin convert_link -- --include-ignored
    echo "P1 VERIFIED"
```

Notes: `*args` makes `just convert-link --info` work; `link-verify-p1` uses
a shebang recipe because process substitution needs bash (just's default
recipe shell is `sh`); cargo's build noise goes to stderr so the diff sees
only the canonical stdout.

## Test plan

All inline `#[cfg(test)] mod tests` at file bottoms (house convention). No
insta in P1 — nothing structured to snapshot yet; insta enters with P2's
tile decoders.

**`be.rs` unit tests** (synthetic byte arrays, no assets):

- `u16_is_big_endian` — `[0x12, 0x34]` → `0x1234`
- `i16_sign` — `[0xFF, 0xFE]` → `-2`; `0x8000` → `-32768` (the JNT1 rotation
  edge case for P3)
- `u32_and_f32` — `0x3F80_0000` → `1.0f32`, plus a negative float
- `str_fixed_reads_fourcc` — and non-UTF8 bytes → error, not panic
- `seek_skip_pos_roundtrip`
- `out_of_bounds_is_error_with_offset` — read past end, seek past end,
  `bytes(n)` overrun, empty buffer; assert the `BeError` fields
- `at_subreader_does_not_move_parent`

**`bmd/mod.rs` unit tests** (hand-built ~100-byte synthetic files as
committed literals, via the `Expectations` seam):

- `parses_minimal_two_block_file`
- one test per `BmdError` variant, asserting the variant (not string
  matching): `bad_magic`, `bad_type`, `file_size_mismatch`,
  `bad_block_count`, `unknown_fourcc`, `block_overrun_on_truncated_file`,
  `trailing_bytes`
- `count_peek_reads_u16_at_plus_8`

**Real-file test** (`#[ignore]` *and* skip-if-missing — belt and suspenders;
`just test` must pass on clean checkouts):

- `real_cl_bdl_invariants`: full parse succeeds; 9 blocks in expected order;
  JNT1 count == 42; once Recorded facts are filled in, assert the exact
  offsets/sizes/counts table. Run via `just link-verify-p1`
  (`--include-ignored`).

## Verification (exit checklist)

- [x] `just convert-link --info` prints the 10-line canonical table; JNT1
      shows 42
- [x] `just link-verify-p1` passes: zero-line diff vs the gclib oracle +
      real-file test green
- [x] Count-at-+8 confirmed against `J3D*Factory` sources for all six
      count-bearing chunks; citations recorded below
- [x] Tamper tests: truncated copy of `cl.bdl` → `SizeMismatch`/`BlockOverrun`
      (never a panic); byte-flipped FourCC → `UnknownFourCc` with the right
      offset (`unknown chunk [58, 4e, 54, 31] at offset 0xb460`)
- [x] `just test` green on a checkout **without** `assets/` present (verified
      by temporarily moving `assets/` aside; the real-file test self-skips
      even under `--include-ignored`)
- [x] `just lint` clean (clippy `-D warnings`, debug + release)
- [x] No `Cargo.toml` diff (bin auto-discovery confirmed)
- [x] `git status` clean of stray files; nothing under `assets/` staged
- [x] Recorded facts filled in

## Recorded facts

```
canonical table for cl.bdl (paste verbatim from --info):
J3D2 bdl4 size=364544 blocks=9
INF1 0x000020 992 -
VTX1 0x000400 40576 -
EVP1 0x00a280 3744 120
DRW1 0x00b120 832 270
JNT1 0x00b460 3392 42
SHP1 0x00c1a0 31424 24
MAT3 0x013c60 12352 24
MDL3 0x016ca0 13344 -
TEX1 0x01a0c0 257856 41

count-offset confirmations (chunk → tww source file:line):
EVP1 → include/JSystem/J3DGraphLoader/J3DModelLoader.h:28  (J3DEnvelopBlock:  /* 0x08 */ u16 mWEvlpMtxNum)
DRW1 → include/JSystem/J3DGraphLoader/J3DModelLoader.h:36  (J3DDrawBlock:     /* 0x08 */ u16 mMtxNum)
JNT1 → include/JSystem/J3DGraphLoader/J3DJointFactory.h:23 (J3DJointBlock:    /* 0x08 */ u16 mJointNum)
SHP1 → include/JSystem/J3DGraphLoader/J3DShapeFactory.h:38 (J3DShapeBlock:    /* 0x08 */ u16 mShapeNum)
MAT3 → include/JSystem/J3DGraphLoader/J3DModelLoader.h:44  (J3DMaterialBlock: /* 0x08 */ u16 mMaterialNum)
TEX1 → include/JSystem/J3DGraphLoader/J3DModelLoader.h:122 (J3DTextureBlock:  /* 0x08 */ u16 mTextureNum)
non-counts also confirmed: INF1 has u16 mFlags at +8 (J3DModelLoader.h:12),
VTX1 has the format-table pointer at +8 (J3DModelLoader.h:19).

gclib version/commit used by the oracle: 1.0.0 @ 64127742467acb633d51685b9b1798ab45bb4034
(gclib leaves EVP1/DRW1 unparsed; the oracle reads their u16 count from
gclib's chunk data at +8 — see the note in scripts/link_chunk_table.py)
```

## Out of scope for P1

- Chunk-interior parsing beyond the +8 count peek (INF1 hierarchy, VTX1
  formats, MAT3, TEX1 → P2/P3)
- Output files in `assets/link/converted/` (manifest/PNGs/bins → P2/P3);
  directory creation only
- MDL3 (permanently skipped — master-plan decision)
- `src/model_manifest.rs` lib module (first needed in P3)
- BTI parsing (P2); Windows recipe variants (P0 precedent)

## Risks / open questions

1. **gclib API uncertainty** — mitigated by canonical-format discipline
   (Step 4) and the header-walk fallback (Step 5); the diff stays trivial
   however the script obtains its numbers.
2. **Count-at-+8 is a hypothesis** — gated by the exit-checklist confirmation
   against `J3D*Factory` sources.
3. **Strict gates vs synthetic tests** — `blockNum == 9` / `JNT1 == 42` would
   make tiny test files impossible; resolved by the internal `Expectations`
   seam while the public path stays strict.
4. **Process substitution in just** — default recipe shell is `sh`; the
   shebang recipe (Step 6) avoids debugging that blind.
