# Phase 2: texture decode + MAT3 dump

Detailed plan for P2 of [`../link_rendering.md`](../link_rendering.md) §6.
Estimated: 2–3 days. Verification strategy follows [`tests.md`](tests.md) §P2,
with one deliberate re-weighting of oracles (see Step 7). Depends on P1
([`phase_01.md`](phase_01.md), committed as `6431f0a`): chunk table, `BeReader`,
dispatch skeleton, `just link-verify-p1` all in place.

**Goal**: decode every texture Link needs (all 41 TEX1 entries + the 3
standalone `.bti` files) to PNGs that pixel-match an independent gclib decode
of the same bytes, and parse MAT3 completely into typed structs, emitting a
canonical dump that diffs byte-for-byte against a gclib oracle — then **freeze
the TEV subset** from that dump so P6–P8 shader work has a fixed, verified
scope. After P2, every input to the renderer except geometry is ground truth.

**Deliverables**

1. `src/bin/convert_link/gx/types.rs` — typed enums for every GX byte we read
   (image/palette formats, wrap, filter, cull, blend, TEV selectors, …),
   `TryFrom<u8>` with typed errors; parse-don't-validate
2. `src/bin/convert_link/bti.rs` — ResTIMG (0x20-byte BTI header) parse,
   shared by TEX1 entries and standalone `.bti` files; standalone re-emit
3. `src/bin/convert_link/gx/texture.rs` — tile decoders: I4, I8, IA4, IA8,
   RGB565, RGB5A3, RGBA8, CMPR, C4/C8 + palettes (IA8/RGB565/RGB5A3) →
   `image::RgbaImage`
4. `src/bin/convert_link/bmd/tex1.rs` — TEX1 chunk parse; emits
   `tex/NN_<name>.png` + `tex/NN_<name>.bti` per entry
5. `src/bin/convert_link/bmd/mat3.rs` — full MAT3 parse via its offset tables
   into typed material structs; cross-chunk invariants
6. `--dump-mat3` flag: canonical table on stdout (diff gate) +
   `mat3_dump.txt` human report (stage equations + subset summary) in out-dir
7. `scripts/link_texture_diff.py` + `scripts/link_mat3_table.py` — gclib
   oracles (uv PEP-723, dev-only, pinned to the P1-recorded gclib commit)
8. `just link-verify-textures`, `just link-verify-mat3`, umbrella
   `just link-verify-p2`
9. Recorded facts below filled in, including the **frozen TEV subset**

## File-format facts this phase relies on

**BTI header is tww's `ResTIMG`** — 0x20 bytes, layout verified at
`../tww/include/JSystem/JUtility/JUTTexture.h:14–37`:

```
0x00 u8  format        0x08 u8  paletteEnabled   0x10 u8  mipmapEnabled   0x18 u8  mipmapCount
0x01 u8  alphaEnabled  0x09 u8  paletteFormat    0x11 u8  doEdgeLOD       0x19 u8  (unknown)
0x02 u16 width         0x0A u16 numColors        0x12 u8  biasClamp       0x1A s16 LODBias
0x04 u16 height        0x0C u32 paletteOffset    0x13 u8  maxAnisotropy   0x1C u32 imageOffset
0x06 u8  wrapS         0x14 u8  minFilter        0x16 s8  minLOD
0x07 u8  wrapT         0x15 u8  magFilter        0x17 s8  maxLOD
```

**TEX1 layout** (`J3DModelLoader.h:121–125` + gclib `TEX1.read`): u16 count at
+8, u32 header-list offset at +0x0C, u32 name-table offset at +0x10 (both
chunk-relative). ResTIMG headers are packed 0x20 apart. **`paletteOffset` and
`imageOffset` are relative to each entry's own header start**, not to the
chunk (verified in gclib `BTI.read`: `header_offset + image_data_offset`).
A standalone `.bti` file is exactly one ResTIMG at offset 0 + data, same
relative-offset convention.

**Toon-ramp injection rule** (`../tww/src/d/d_resorce.cpp:70–82`, `setToonTex`):
texture names starting `"ZA"` get the runtime toon image, `"ZB"` the toonEX
image. `cl.bdl` contains exactly one such entry — `ZBtoonEX` (8×8 I4
placeholder) — and **no `ZA*` entry**, so `toon.bti` is not used by Link's body
model at all; only `toonex.bti` is. (Both still get decoded/verified in P2.)

**Actual texture inventory** (probed via gclib; our parser must reproduce it —
verbatim table becomes a Recorded fact):

- 41 TEX1 entries. Formats: CMPR ×14 (mouth/body/scabbard S3TC sets), I4 ×11
  (8×8 eye/brow "closed" frames + the ZBtoonEX placeholder), IA8 ×8 (96×96
  `eyeh.*`), IA4 ×7 (64×64 `mayuh.*` brows), C8+RGB565 ×1 (`hitomi`, 96×96
  pupils, 64-color palette).
- Standalone: `toon.bti` I4 256×8, `toonex.bti` CMPR 256×256,
  `linktexbci4.bti` C4+RGB565 160×96.
- **Every** texture: `mipmapCount == 1`, wrap Clamp/Clamp, filters
  Linear/Linear. No mipmapped or repeating texture exists in this model.
- **Names repeat**: `eyeh.1`, `linktexS3TC`, `mouthS3TC.1`, `podAS3TC`,
  `mayuh.1` each appear twice in TEX1 (distinct entries, shared name). Output
  filenames must be index-prefixed (`tex/34_linktexS3TC.png`), never
  name-keyed.

**GX tile layouts** (implementation reference; exact bit orders confirmed by
the pixel gate — YAGCD §17 and gclib `texture_utils.py` as written specs):

| format | tile   | bytes/tile | notes |
|--------|--------|-----------|-------|
| I4     | 8×8    | 32 | 2 px/byte, high nibble first; A = I |
| I8     | 8×4    | 32 | A = I |
| IA4    | 8×4    | 32 | per byte: high nibble A, low I (confirm) |
| IA8    | 4×4    | 32 | u16/px, A byte then I byte (confirm) |
| RGB565 | 4×4    | 32 | BE u16 |
| RGB5A3 | 4×4    | 32 | BE u16; top bit 1 → RGB555 opaque, 0 → A3+RGB444 |
| RGBA8  | 4×4    | 64 | two 32-byte planes per tile: AR then GB |
| C4     | 8×8    | 32 | palette index, 2 px/byte high-nibble first |
| C8     | 8×4    | 32 | palette index |
| CMPR   | 8×8    | 32 | 4 DXT1-style 4×4 sub-blocks, 8 bytes each |

Tiles are stored row-major over a width/height rounded **up** to tile
dimensions; decoders must clip padding pixels. Bit expansion is replication
(`x5 → (x<<3)|(x>>2)`), not scaling. CMPR per sub-block (gclib arithmetic,
which the gate enforces): BE u16 `color0`,`color1`; if `c0 > c1` the two
intermediates are floor((2a+b)/3); else intermediate = a/2+b/2 and index 3 =
transparent black `(0,0,0,0)`. Index bits: u32, 2 bits/px, MSB-first.
Palette entries are BE u16 in one of IA8/RGB565/RGB5A3.

**MAT3**: 24 materials. Offset-table structure per `J3DMaterialBlock`
(`J3DModelLoader.h:43–75`, confirmed in P1): u16 count at +8, then ~30 u32
chunk-relative offsets to per-property lists; each material's init data is
indices into those lists. Semantic references, in precedence order:
`../tww/src/JSystem/J3DGraphLoader/J3DMaterialFactory.cpp` (the loader itself),
`../tww/tools/converters/matDL_dis.py` (register meanings), noclip's
`J3DLoader.ts`/`gx_material.ts`. gclib parses all of it into typed objects
(verified by probe: `tev_stages`, `tev_orders`, konst colors/selects, texgens,
tex matrices, blend/z/alpha-compare/fog, channels — with `asdict()`), which is
what makes it a full MAT3 oracle. Material names show a wrinkle to
investigate: the name table yields `ear`, `eyeL`, …, `sleeve`, then
`ear(2)`…`ear(8)` — either genuine duplicate names or a remap-table effect
(J3D material *instances* sharing init data). Resolve during implementation
and record.

## Step 1 — `gx/types.rs`: typed enums

One enum per GX field consumed anywhere in P2 (`ImageFormat`, `PaletteFormat`,
`WrapMode`, `FilterMode`, `CullMode`, `PixelEngineMode`, `CompareType`,
`BlendMode`/`BlendFactor`/`LogicOp`, `TexGenType`/`TexGenSrc`/`TexGenMatrix`,
`TevColorIn`/`TevAlphaIn`/`TevOp`/`TevBias`/`TevScale`/`TevReg`,
`KonstColorSel`/`KonstAlphaSel`, `RasChannelId`, `ColorSrc`, `DiffuseFn`,
`AttnFn`, `FogType`). Each:

```rust
impl TryFrom<u8> for ImageFormat {
    type Error = GxEnumError; // { kind: &'static str, value: u8 }
}
```

Numeric values from GX headers (`../tww/include/dolphin/gx/GXEnum.h`) — cite
in code comments. Running the converter is then a fuzz-by-real-data test:
every byte in the real file must map to a known variant or the parse fails
with the field name and value (tests.md §P2 "parse-don't-validate").

## Step 2 — `bti.rs`: ResTIMG parse + standalone re-emit

```rust
pub struct BtiHeader { /* every ResTIMG field, enums from gx/types */ }
pub struct BtiTexture<'a> { pub header: BtiHeader, pub image: &'a [u8], pub palette: &'a [u8] }

pub fn parse(r: &BeReader, header_pos: usize) -> Result<BtiTexture, BmdError>;
pub fn image_byte_len(fmt: ImageFormat, w: u16, h: u16) -> usize; // tile-rounded
pub fn write_standalone(tex: &BtiTexture) -> Vec<u8>;             // header + data, offsets rebased
```

- Image slice length is computed from format/dims via the tile table (there is
  no explicit length field); `mipmapCount != 1` is a hard error for now (no
  such texture exists in our inputs — Recorded facts will re-confirm).
- `write_standalone` copies the 0x20 header verbatim (including
  `alphaEnabled`, the 0x19 unknown byte, LOD fields — no interpretation),
  rewriting only `imageOffset`/`paletteOffset` to the standalone layout. Image
  and palette bytes are **copied verbatim from the source file, never
  re-encoded** — so the pixel gate compares two independent decoders of
  byte-identical GX data (ours → PNG, gclib reading our `.bti` → PNG), and the
  only converter-authored bytes an oracle depends on are the trivial header
  fields.

## Step 3 — `gx/texture.rs`: decoders

```rust
pub fn decode(header: &BtiHeader, image: &[u8], palette: &[u8]) -> Result<image::RgbaImage, BmdError>;
```

Internally: `decode_tiles(w, h, tile_w, tile_h, |tile_bytes, put_pixel|)` —
one generic tiling walk (the classic bug source, written once), one small
per-format sub-block decoder. Palette formats decode indices first, then look
up through a decoded `Vec<[u8; 4]>` palette; out-of-range index (≥ numColors)
is a hard error. Implement the full format list even though `cl.bdl` uses six
— each decoder is ~20 lines, and P9's casual texture (C4) plus synthetic
tests cover the rest. `image` crate (already a dependency, v0.25.6) writes
the PNGs; **no new crates, no `Cargo.toml` diff**.

Intensity formats set A = I (GX samples I into alpha); if gclib disagrees the
pixel gate will show it on all 26 I4/IA4/IA8 textures at once — adjudicate
against Dolphin's `TextureDecoder` and record.

## Step 4 — `bmd/tex1.rs`: chunk parse + emission

- Parse count/offsets, the name table (P1's `str` handling; names are
  null-terminated in a JUTNameTab — hash u16 + offset u16 pairs; confirm
  layout against `../tww/include/JSystem/JUtility/JUTNameTab.h`), then each
  ResTIMG via `bti::parse`.
- Emit per entry: `tex/{i:02}_{name}.png` (decoded) and
  `tex/{i:02}_{name}.bti` (standalone re-emit). Also decode the three
  standalone raw `.bti` inputs to `tex/raw_{stem}.png` (their `.bti` originals
  already live in `assets/link/raw/` for the oracle).
- Invariants: name count == texture count; header list + image/palette spans
  all within chunk bounds; every name unique **after** index prefixing (free).
- No injection yet: substituting `toonex.bti` pixels for the `ZBtoonEX`
  placeholder happens when the manifest is assembled (P3) — P2 only proves
  both sides of that substitution decode correctly and confirms the names.

## Step 5 — `bmd/mat3.rs`: full parse

```rust
pub struct Mat3 { pub materials: Vec<Material>, pub names: Vec<String>, /* remap tables as parsed */ }
pub fn parse(r: &BeReader, chunk: &ChunkEntry, tex_count: u16) -> Result<Mat3, BmdError>;
```

- Read the ~30-offset header, then each material's init data, chasing every
  index through its list via `BeReader::at` sub-readers (this is why P1 built
  `at()`). Every byte lands in a `gx/types` enum or a typed struct — no raw
  `u8` escapes the module.
- `Material` carries everything the master plan's manifest sketch needs:
  cull, pixel-engine mode, z-mode, dither, material/ambient colors, channel
  controls, texgens, tex matrices, texture indices, TEV reg + konst colors,
  konst selects, orders, stages (a/b/c/d, op, bias, scale, clamp, dest — color
  and alpha), swap modes/tables, fog, alpha compare, blend, NBT scale.
- Cross-chunk invariants (hard errors): every texture index < TEX1 count
  (passed in); stage/texgen counts ≤ 8; every remap/init index within its
  list's bounds; material count == 24 for cl.bdl (Expectations-style, like P1).
- Resolve the `ear(2)` naming question here; if the remap table makes several
  of the 24 "materials" instances of one init-data record, parse and record
  that structure — it changes how many distinct pipelines P6 builds.

## Step 6 — `--dump-mat3` output

Two artifacts from one parse:

1. **Canonical table on stdout** (the diff gate, same discipline as P1 §4):
   deterministic line format, one `material N <name>` block per material in
   MAT3 order, `key=value` lines in fixed order covering every parsed field.
   Enums print as canonical GX names (spec'd in the doc comment, implemented
   independently by both sides — neither imitates the other's runtime output).
   Floats print as `%.6f`; colors as `r,g,b,a` integers (u8) or `%.6f` × 4 for
   the s10 TEV regs. Stdout stays reserved: diagnostics to stderr.
2. **`mat3_dump.txt`** in out-dir (human report): per-material stage equations
   rendered as `C = (d op ((1-c)·a + c·b) + bias) · scale` text in the spirit
   of `matDL_dis.py`, plus a **subset summary** — the distinct values used
   across all 24 materials for each dimension: TEV input selectors, ops,
   bias/scale, dest regs, konst selects, ras channels, texgen (type, src,
   matrix) tuples, blend modes, z modes, alpha-compare configs, fog types,
   cull modes, channel-control configs, non-identity tex matrices (if any).
   That summary, pasted into Recorded facts, **is the frozen TEV subset** —
   the exact contract for `tev.slang` (P8) and the `tev_ir.rs` gate (P6),
   and the resolver of master-plan risk #5's open questions (which channel
   feeds SRTG, whether tex matrices are identity).

## Step 7 — oracle scripts

Both uv PEP-723, executable, **pinned to the gclib commit recorded in P1**
(`gclib @ git+https://github.com/LagoLunatic/gclib@6412774...`) so the gates
don't drift; update the P1 script to the same pin while at it (re-run
`link-verify-p1` after).

1. `scripts/link_texture_diff.py <raw-dir> <tex-dir>` — for every
   `NN_*.bti` in tex-dir plus the three raw `.bti`s: decode with gclib
   (`BTI(path).render()`, returns PIL RGBA), load our corresponding PNG,
   compare **RGBA pixel buffers** (never PNG bytes — encoders differ), print
   one `OK name` / `FAIL name (N pixels differ)` line each, exit nonzero on
   any FAIL. 44 comparisons total.
2. `scripts/link_mat3_table.py <file.bdl>` — print the canonical MAT3 table
   from gclib's parsed materials (`asdict()` verified to expose every needed
   field), implementing the same format spec from this doc. The verify recipe
   is a literal `diff` like P1.

**Deviation from tests.md §P2, on purpose**: tests.md names SuperBMD's
materials-JSON as the MAT3 diff oracle. Probing showed gclib parses MAT3
completely (SuperBMD's parse and gclib's are independent codebases, but
gclib's is the one already wired into our toolchain, scriptable without mono,
and pinnable). So: **gclib is the automated MAT3 gate; SuperBMD (RenolY2
fork, mono is installed) is the manual second opinion** for any field where
we and gclib disagree, with `J3DMaterialFactory.cpp` as the final authority.
Same for textures: Dolphin's texture-dump replay (tests.md §Dolphin) is the
third vote if we and gclib ever disagree pixel-wise, and GCFT's GUI preview
is the quick visual sanity check.

## Step 8 — just recipes

```just
# P2 texture gate: pixel-diff every decoded texture against gclib
[unix]
link-verify-textures:
    #!/usr/bin/env bash
    set -euo pipefail
    just convert-link >/dev/null
    ./scripts/link_texture_diff.py assets/link/raw assets/link/converted/tex

# P2 MAT3 gate: diff our canonical dump against the gclib oracle
[unix]
link-verify-mat3:
    #!/usr/bin/env bash
    set -euo pipefail
    diff <(just convert-link --dump-mat3) <(./scripts/link_mat3_table.py assets/link/raw/cl.bdl)

# P2 gate: textures + MAT3 + ignored real-file tests
[unix]
link-verify-p2: link-verify-textures link-verify-mat3
    cargo test --bin convert_link -- --include-ignored
```

`convert-link` (P1 recipe) already forwards `*args`; P2 makes the plain run
emit textures into `assets/link/converted/` and `--dump-mat3` print the
canonical table. Once the gates pass, commit SHA256 golden hashes of
`assets/link/converted/tex/*` (hashes of derived data, not the data —
tests.md toolbox) as `scripts/link_converted.sha256` for free regression
detection thereafter.

## Test plan

Inline `#[cfg(test)]` per house convention; insta enters this phase
(dev-dependency already present with `json`+`glob`; snapshots land in
`src/bin/convert_link/**/snapshots/`, committed).

**`gx/texture.rs` — synthetic tile snapshots** (the committable, asset-free
core; tests.md §P2): for each format, one hand-authored tile exercising its
edge cases, decoded and snapshotted as a hex pixel grid:

- I4/I8/IA4/IA8: nibble/byte order, A=I semantics
- RGB565: bit replication (0x1F → 0xFF, not 0xF8)
- RGB5A3: one pixel per mode (top bit set/clear), A3 expansion
- RGBA8: the two-plane AR/GB split
- C4/C8: index unpack + each palette format (IA8/RGB565/RGB5A3)
- CMPR: one sub-block with c0 > c1 (thirds rounding) and one with c0 ≤ c1
  (transparent index 3); sub-block arrangement within the 8×8 tile
- non-tile-aligned dims (e.g. 5×3 RGB565): padding clipped, output exact size

**`gx/types.rs`**: every enum rejects an out-of-range byte with the right
`kind`; spot-check known values (e.g. CMPR == 14).

**`bti.rs`**: parse a synthetic ResTIMG (all fields distinct values) and
assert field-for-field; `write_standalone` → `parse` round-trips; image
length table vs hand-computed sizes; mipmapCount == 2 → typed error.

**`bmd/mat3.rs`**: synthetic MAT3 is too much structure to hand-build
wholesale — unit-test the pieces (offset-header read, one index-list chase,
one packed TevStage decode against hand-laid bytes) and let the real-file
gate carry the integration burden (that's what the oracle diff is for).

**Real-file tests** (`#[ignore]` + skip-if-missing, run via
`link-verify-p2`):

- `real_tex1_inventory`: 41 entries; formats/dims/mips match the Recorded
  facts table verbatim; `ZBtoonEX` present, no `ZA*` name; the five
  duplicate-name pairs are distinct entries.
- `real_mat3_parses`: full parse succeeds (every byte through typed enums);
  24 materials; all cross-chunk invariants hold; fog/texgen expectations from
  the frozen subset re-asserted once recorded.

**Tamper tests** (manual, like P1): flip a format byte in a copied `.bdl` →
typed `GxEnum` error naming the field; truncate mid-image-data →
`OutOfBounds` with offset, never a panic.

## Verification (exit checklist)

- [x] `just link-verify-textures`: 44/44 `OK`, zero pixels different
- [x] `just link-verify-mat3`: zero-line diff vs the gclib oracle
- [x] `just link-verify-p2` green end-to-end (includes real-file tests;
      48 tests total)
- [x] `mat3_dump.txt` subset summary reviewed; **frozen TEV subset** pasted
      into Recorded facts; SRTG channel + tex-matrix questions (risk #5)
      answered from it
- [x] Ramp names confirmed and recorded (`ZBtoonEX` only; no `ZA*`)
- [x] `ear(2)`-style material naming explained and recorded (literal names
      in the table; 11 distinct records via remap)
- [x] All three oracle scripts pinned to the recorded gclib commit; P1 gate
      re-verified after pinning
- [x] Golden hashes committed (`scripts/link_converted.sha256`, includes
      `mat3_dump.txt`)
- [x] Tamper tests pass (typed errors, no panics): flipped TEX1 format byte
      → `invalid ImageFormat value 0x7` naming the texture; flipped MAT3
      pe_mode → `material ear: pixelEngineMode: invalid ... 0x3`; truncation
      → P1's `SizeMismatch` (fires before any interior parse)
- [x] `just test` green without extracted assets (real-file tests are
      `#[ignore]` + skip-if-missing); `just lint` clean; no `Cargo.toml`
      diff; nothing under `assets/` staged
- [x] Recorded facts filled in

## Recorded facts

```
texture inventory (asserted in real_tex1_inventory; pixel gate 44/44 OK):
41 TEX1 entries: CMPR ×14, I4 ×11, IA8 ×8, IA4 ×7, C8+RGB565 ×1 (hitomi, 64
colors). All mipmapCount==1 (raw byte 1, not 0), all wrap Clamp/Clamp, all
filters Linear/Linear. Duplicate names (two entries each): eyeh.1,
linktexS3TC, mouthS3TC.1, podAS3TC, mayuh.1. Z-prefixed ramp slots: only
ZBtoonEX (8×8 I4, entry 35) — no ZA* entry, so toon.bti is unused by cl.bdl.
Standalone: toon.bti I4 256×8, toonex.bti CMPR 256×256, linktexbci4.bti
C4+RGB565 160×96. Full per-entry table: sha-pinned PNGs/BTIs in
scripts/link_converted.sha256.

frozen TEV subset (verbatim subset summary from mat3_dump.txt):
== TEV subset summary (active slots only) ==
pe_modes: Opaque, Translucent
cull_modes: Cull_Back, Cull_None
stage_counts: {1, 2, 3}
color_inputs: C0, CPREV, KONST, RASC, TEXC, ZERO
alpha_inputs: APREV, KONST, RASA, TEXA, ZERO
color_ops: ADD
alpha_ops: ADD
biases: ZERO
scales: SCALE_1
dest_regs: PREV
stages_with_clamp_off: 2
konst_color_sels: K0, K1
konst_alpha_sels: K0_A, K3_A
ras_channels: COLOR0A0, COLOR_NULL
texgens: (MTX2x4, TEX0, IDENTITY), (MTX2x4, TEX0, TEXMTX1), (SRTG, COLOR0, IDENTITY)
non_identity_tex_matrices: 2
channel_controls: (enable=false, mat=Register, amb=Register, diffuse=Clamp, attn=Spot, mask=0x02), (enable=false, mat=Register, amb=Register, diffuse=None_, attn=None_, mask=0x00), (enable=true, mat=Register, amb=Register, diffuse=Clamp, attn=Spot, mask=0x03), (enable=true, mat=Register, amb=Register, diffuse=Signed, attn=Specular, mask=0x00)
z_modes: (test=false, func=Less_Equal, write=false), (test=true, func=Less_Equal, write=false), (test=true, func=Less_Equal, write=true)
z_compare_loc: true
blend_modes: (Blend, Destination_Alpha, Inverse_Destination_Alpha, COPY), (Blend, Source_Alpha, Inverse_Source_Alpha, COPY), (None_, One, Zero, COPY), (None_, Source_Alpha, Inverse_Source_Alpha, COPY)
alpha_compares: (Always 0, OR, Always 0), (Greater 0, OR, Greater 0)
fog_types: LINEAR (enabled on 0 materials)
swap_modes_non_default: 24
indirect_enabled: 0

SRTG texgen answer (risk #5): SRTG from COLOR0 via IDENTITY — no texture
matrix on the ramp path. One MTX2x4 texgen uses TEXMTX1 (2 non-identity
tex matrices exist); inspect mat3_dump.txt when that material is wired.
swap-table usage (in-subset): ras_sel always 0; tex_sel ∈ {0,1,2} selecting
tables (0,1,2,3) identity, (0,0,0,3) RRR+A, (1,1,1,3) GGG+A — channel
broadcasts for reading intensity-texture channels; 12 materials use them.
fog-enabled materials: none (types declared LINEAR, all disabled).
material remap / duplicate-name explanation: the MAT3 name table literally
contains ear(2)..ear(8); the remap table is
[0,1,2,3,1,4,3,0,5,6,7,5,6,7,8,9,10,0,0,0,0,0,0,0] — only 11 distinct
0x14C records; face and all ear(N) slots share record 0, R-side eye/brow
slots share the L-side records (J3D material instancing).
intensity-format alpha semantics adjudication: none needed — A=I matched
gclib on all 26 I4/IA4/IA8 textures (44/44 pixel gate).
gclib pin used by all three oracle scripts (P1's too):
1.0.0 @ 64127742467acb633d51685b9b1798ab45bb4034
golden hashes: scripts/link_converted.sha256 (86 files: 85 tex/* + mat3_dump.txt)
```

## Out of scope for P2

- Geometry chunks (INF1/VTX1/EVP1/DRW1/JNT1/SHP1 → P3); manifest JSON and
  `src/model_manifest.rs` (P3)
- Ramp-pixel injection into the texture set (P3 wiring; P2 only verifies both
  decodes + names)
- `tev_ir.rs` / `TevMaterialDesc` uniform packing (P6) — P2 freezes the
  subset; P6 builds the IR against it
- Mipmap decode (hard error on `mipmapCount != 1`; none exist in our inputs),
  C14X2, texture *encoding*, `--casual` (P9)
- Renderer changes of any kind (P4/P5)

## Risks / open questions

1. **Tile/bit-order minutiae** (the classic GX decode bugs) — fully gated:
   synthetic snapshots catch regressions, the 44-texture pixel diff catches
   wrongness, Dolphin texture dumps break ties.
2. **CMPR arithmetic variants** — thirds-rounding and the c0 ≤ c1 rule differ
   across decoders in the wild; we implement the gclib arithmetic recorded
   above. 14 CMPR textures make the gate sensitive to any deviation.
3. **MAT3 canonical-format scope** — ~30 field families to spec and print
   identically from two codebases. Bounded by doing exactly the field set in
   Step 6 and nothing else; ambiguities adjudicated against
   `J3DMaterialFactory.cpp`, not negotiated between the two printers.
4. **Oracle-script fidelity** — the python side must map gclib's enum names to
   the canonical spellings without peeking at our output (P1 discipline: both
   implement the spec). Field-name typos show up as gate diffs, which is the
   point.
5. **Duplicate texture names / material-name wrinkle** — handled by index
   prefixing; the `ear(N)` question is an investigate-and-record item, not a
   blocker.
6. **JUTNameTab layout** (hash+offset pairs) — small, but a wrong read
   scrambles every name; verified implicitly by the inventory test and the
   MAT3 name column in the canonical diff.
7. **Float formatting drift** between Rust and Python — avoided by fixed
   `%.6f` on both sides (values are f32-exact in the file; six decimals is
   presentation, not precision).
