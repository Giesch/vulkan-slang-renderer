# Extracting the King of Red Lions: implementation plan

Goal: extract the King of Red Lions boat from *The Legend of Zelda: The Wind
Waker* and export it as a **Wavefront OBJ + MTL + PNG textures**, the way
[`link_rendering/phase_03.md`](link_rendering/phase_03.md) did for Toon Link's
`--obj` debug export. The deliverable stops at an OBJ you can open in Blender —
no renderer integration, no shader work.

Companion document: [`link_rendering.md`](link_rendering.md), whose P0–P3 phases
built the converter this plan generalizes.

**Status: implemented 2026-08-09.** Both OBJs export and verify — `--info`,
`--dump-mat3` and `--dump-geometry` all diff identical against the gclib oracles
for both models, every texture is pixel-identical (6/6 hull, 7/7 head), and
`fn_head_h`'s `world·invBind = I` holds at 1.97e-2 against `INVBIND_EPS` 0.02,
which is the acceptance criterion this plan sets for the whole geometry path.
Link stayed byte-identical throughout (90/90 golden hashes). Five claims below
turned out to be wrong and are annotated in place rather than deleted: the
deployed sail's location (§Scope), the `Expectations` FRU snippet and the
`--model NAME` flag form (§5), the ignored-test count and the fate of the
`parse_model` wrapper (§4), the missing `pose.rs:458` caller (§6), and the
oracle needing a second edit (§Verification).

One thing this plan does not mention that cost nothing in the end: `bti.rs`
hard-errors on `mip_levels() != 1`, and no ship texture has mips (all nine are
`mips=1`, formats C8/I4/CMPR, all already covered by `gx/texture.rs`). Worth
checking first for any future model, since it would be unscoped work.

## Context

`crates/convert-link` already turns `cl.bdl` into PNGs, a manifest, flat binaries,
and — behind `--obj` — an OBJ + MTL. Every stage of it was deliberately written to
*pin* `cl.bdl`'s exact shape and fail loudly on anything else: the chunk table
expects exactly 42 joints, VTX1 accepts only F32 positions/normals and S16 UVs,
and a TEV subset gate refuses to write output for a material the toon_link shader
cannot render.

That strictness was the right call for P0–P9, and it is what has to be selectively
loosened here. The OBJ-only goal is what makes it cheap: the TEV gate is
validation-only and writes nothing, so downgrading it to a warning cannot change
an output byte for any model.

**Hard constraint:** Toon Link's outputs must stay byte-identical. There are 90
golden sha256 hashes in `examples/toon_link/scripts/link_converted.sha256`.

## Scope

`Ship.arc` holds four models; `daShip_c::createHeap()`
(`../tww/src/d/actor/d_a_ship.cpp:4412-4470`) loads all four into one actor. This
plan covers the two that *are* the boat:

| | size | joints | mats | shapes | normals | EVP1 |
|---|---|---|---|---|---|---|
| `fn_body.bdl` | 38240 | 11 | 1 | 1 (278 strips) | F32 | **header-only, count 0** |
| `fn_head_h.bdl` | 77888 | 18 | 2 | 2 (380 strips) | **S16 shift 14** | 20 envelopes, 18 invbinds |

- **`fn_body.bdl`** — hull, mast and sail rig. Joints `j_fn_main1`, `j_fn_futa`
  (*futa* = lid, the deck hatch), `j_fn_gattai` (*gattai* = join, where the
  figurehead attaches), `j_fn_kaji` (rudder), `j_fn_mast`, `j_fn_sail1/2/_e`,
  `j_fn_steer1`.
- **`fn_head_h.bdl`** — the lion figurehead. Joints `j_fn_kubi1..6` (neck),
  `j_fn_atama` (head), `j_fn_ago1/2` (jaw), `j_fn_hoho` (cheek),
  `j_fn_mayu_l1/l2` + `j_fn_mayu_r1/r2` (eyebrows), `j_fn_me_l/r` (eyes).

**Out of scope: `vfncn.bdl` (cannon) and `vfncr.bdl` (salvage arm).** They are
equippable attachments drawn mutually exclusively (`d_a_ship.cpp:304-317`), and
they are the only two files in the archive carrying an `RGBA8 CLR0` attribute.
Excluding them removes all vertex-color work and, with it, every change to
`shp1.rs`. See "Deferred" below.

Also not in `Ship.arc`: ~~the deployed sail is a separate actor
(`VsaiL.arc:bdlm/vsail.bdl` + a `btk`)~~, and the salvage rope-end is
`Link.arc:ropeend.bdl`.

**The sail claim was wrong, corrected 2026-08-09.** There is no separate sail
model, and `VsaiL.arc` is not it. `daShip_c::createHeap()` loads five models
(`fn_body`, `vfncn`, `vfncr`, `Link:ropeend`, `fn_head_h`) and `daShip_c::draw()`
draws exactly those five; the class has no sail member. `vsail.bdl` is the sail
**item pickup** — 4 joints (`root_VsaiL`, `VsaiL_body`, `Vupy1_outside`,
`Vupy2_inside`), materials `SC_lupy_inside`/`SC_lupy_outside` (*upy*/*lupy* =
rupee) and textures `V_lupy_spc` + `toonMETAL02`, registered in
`d_item_data.cpp:132-133` alongside its `VsaiM.arc` sibling.

The sail is geometry *inside* `fn_body.bdl`, rigidly bound to `j_fn_sail1` (77
verts), `j_fn_sail2` (21), `j_fn_sail_e` (54) and `j_fn_mast` (151) — DRW1 slots
5/6/7 plus the mast — so it is already in this plan's OBJ export. Deployment is
animation, not a model swap: `Ship.arc:bck/fn_mast_on2.bck` is 11 frames scaling
`j_fn_mast.X` 0.05→1.12→1.00 and `j_fn_sail2.X` 0.05→0.94→0.81→0.77→0.80→0.88→1.00
(an unfurl with a billow-and-settle wobble), with `fn_mast_off2.bck` the reverse
and the actor rotating `j_fn_sail1` by `mSailAngle` each frame
(`d_a_ship.cpp:116-117`). The bind pose this plan exports sits at scale 1.0, the
deployed end of that animation.

Two nearby claims that *did* hold: `Ship.arc:tex/new_ho1.bti` really is
unreferenced (nothing in `d_a_ship.cpp` names it — `ho1` is a generic name used
by other actors, e.g. `d_a_dr.cpp:129`), and `ropeend` really does come from
`Link.arc`.

## Where things live

The ship rides along inside the existing example directory rather than getting a
new crate. `Cargo.toml` declares `members = ["crates/*", "examples/*"]`, so a new
`examples/` directory would need its own `Cargo.toml` and a `src/main.rs` that
`just sweep` runs headlessly — real cost for an OBJ-only goal.

```
examples/toon_link/
  assets/ship/raw/              gitignored by the existing /examples/toon_link/assets/ rule
  assets/ship/converted/body/   fn_body outputs
  assets/ship/converted/head/   fn_head_h outputs
  scripts/extract_ship.sh       new
  scripts/ship_assets.sha256    new, bootstrapped on first run
  justfile                      new recipes
```

**Separate out-dirs per model, not a shared one.** Both models write
`mat3_dump.txt` and index-prefixed textures (`tex/00_*.png`); a shared directory
would collide on both. Separate dirs also mean `mat3_dump.txt` never needs a
prefix, which keeps that golden hash untouched.

## Code changes

Six files, all small. Everything is additive — Link keeps the identical code path.

### 1. `crates/convert-link/src/bmd/evp1.rs` — the actual blocker

`fn_body` has a 32-byte header-only EVP1: count 0, all four offsets 0.
`evp1.rs:67` computes `inv_count = (32 - 0) / 0x30 = 0` and line 68 hard-errors
*"EVP1 has 0 inverse-bind matrices but 11 joints"*. No amount of flag-plumbing
fixes this.

Insert immediately after the header reads (after line 36), before the envelope
loop:

```rust
// A model with no skinning has a header-only EVP1: count 0 and all offsets 0.
if count == 0 && inv_bind_off == 0 {
    return Ok(Evp1 { envelopes: Vec::new(), inv_bind: Vec::new() });
}
```

Everything below stays verbatim, so `cl.bdl` takes the identical path.

### 2. `crates/convert-link/src/pose.rs` — guard the invBind gate

The invBind identity check (`pose.rs:64-75`) loops `0..world.len()` and indexes
`inv_bind[j]`, which panics once `inv_bind` is empty:

```rust
let invbind_max_residual = if inv_bind.is_empty() { 0.0 } else { /* existing loop */ };
```

Link's loop runs exactly as today, so `baked.invbind_max_residual` and every
emitted byte are unchanged. `WEIGHTED_EPS` needs nothing — with no envelopes,
`drw1::parse` can only produce `DrwSlot::Joint`, so `weighted_max` stays 0.0.
Leave both epsilons alone; `fn_head_h` is the one model here that exercises them
for real.

### 3. `crates/convert-link/src/bmd/vtx1.rs` — S16 normals

`fn_head_h` stores normals as S16 with shift 14. Two edits:

- The `Attr::Nrm` arm (`vtx1.rs:110-113`): relax to
  `fmt.comp_count == 0 && matches!(fmt.comp_type, ComponentType::F32 | ComponentType::S16)`.
- Split the normal decode out of the shared `decode_vec3` call (`vtx1.rs:140`)
  into `decode_nrm(&r, off, end, comp_type, shift)`, which **delegates to the
  existing `decode_vec3(.., 12)` for F32** — so Link's bytes come from the same
  code — and reads stride-6 `i16 * 1.0/(1 << shift)` for S16.

`Attr::Pos` and `Attr::Tex0` need no change (both models match `cl.bdl` exactly),
and the `other =>` catch-all stays as-is now that CLR0 is out of scope.

### 4. `crates/convert-link/src/bmd/mod.rs` — unpin "42 joints"

`Expectations` is threaded exactly one way today: `parse_chunk_table` hardcodes
`&CL_BDL` and calls the private `parse_chunk_table_with`; `parse_model` calls
`parse_chunk_table`. Nothing else sees it.

- Make `Expectations` `pub` + `#[derive(Clone, Copy)]`, export `pub const CL_BDL`.
- Make `parse_chunk_table_with` `pub`.
- ~~Add `pub fn parse_model_with(data, expect)` holding today's body; redefine
  `parse_model(data)` as `parse_model_with(data, &CL_BDL)`. This keeps the five
  ignored real-file tests compiling *and still meaning `cl.bdl`*~~ (`pose.rs:386`,
  `pose.rs:448`, `tev_ir.rs:1058`, `bmd/tex1.rs:146`, `bmd/mat3.rs:760`).
- `BmdError::BadJointCount` Displays a literal `expected 42` (`mod.rs:148`) —
  change the variant to `{ found: Option<u16>, expected: u16 }` and interpolate.
  Touches only the `bad_joint_count` unit test at `mod.rs:499`.

**Two corrections here, 2026-08-09.** *The test count was wrong*: there are
**six** ignored real-file tests, not five — this list omits `bmd/mod.rs:535`
`real_cl_bdl_invariants`, which calls `parse_chunk_table` (not `parse_model`)
and asserts a hardcoded chunk table including `size=364544`.

*And the `parse_model` wrapper did not survive.* Once `parse_model_with` existed,
`parse_model` had no production caller, and an unused `pub fn` in a **binary**
crate is a `dead_code` warning that `just lint` (`-D warnings`) rejects. The
wrapper is gone and all six tests say `parse_model_with(&data, &CL_BDL)`, which
is in any case more explicit about meaning Toon Link than the wrapper name was.
The same fate hit `parse_chunk_table` and the `Naming::LINK` constant §6 implies.
Anything that "exists only to keep tests compiling" is dead code here — this
crate has no external consumers to be a public API *for*.

**Do not touch `canonical_table`.** Its format is count/offset-driven and already
prints the ship files correctly; `--info` for Link must stay byte-identical.

Both ship files are `J3D2`/`bdl4`, exactly 9 blocks, exactly the
`EXPECTED_FOURCCS` set — so only `jnt1_count` varies.

### 5. `crates/convert-link/src/main.rs` — model specs + TEV gate mode

`fn_body`'s single material `m_fn_main_hashi` fails the gate three ways:
`num_tex_gens = 3` against `MAX_TEXGENS = 2`; a `MTX3x4`/`POS` texgen rejected by
the match at `tev_ir.rs:387`; and a `proj=MTX3x4 mode=Projmap` texture matrix
rejected at `tev_ir.rs:428` and `:434`. `fn_head_h` should pass.

**Change nothing inside `tev_ir.rs`** — only how `main.rs` consumes it. The gate
is validation-only, so `Warn` cannot alter an output byte for any model:

```rust
enum TevGate { Error, Warn, Off }   // Link stays Error

let tev_count = match spec.tev_gate {
    TevGate::Error => tev_ir::describe_all(&model.mat3).context("TEV subset gate")?.len(),
    TevGate::Warn  => match tev_ir::describe_all(&model.mat3) {
        Ok(d) => d.len(),
        Err(e) => { eprintln!("convert_link: WARNING: TEV subset gate: {e} \
                     (continuing; OBJ output does not use the TEV interpreter)"); 0 }
    },
    TevGate::Off => 0,
};
```

Note `describe_all` is a `collect::<Result<Vec<_>>>` (`tev_ir.rs:154`) and
short-circuits, so `Warn` reports one reason per run.

Replace the hardcoded `raw_dir.join("cl.bdl")` and `STANDALONE_BTIS` with a small
built-in spec table — this keeps the repo's "pin the expected shape, fail loudly"
style rather than degrading to free-form flags, and keeps the number 42 attached
to the thing that is actually 42:

```rust
struct ModelSpec {
    name: &'static str,        // --model value
    display: &'static str,     // OBJ header comment
    bdl: &'static str,         // file inside raw-dir
    prefix: &'static str,      // output basename
    expect: bmd::Expectations,
    standalone: &'static [&'static str],
    ramps: bool,
    tev_gate: TevGate,
}

const MODELS: &[ModelSpec] = &[
    ModelSpec { name: "link", display: "Toon Link", bdl: "cl.bdl", prefix: "link",
                expect: bmd::CL_BDL,                          // jnt1_count: Some(42)
                standalone: &["toon.bti", "toonex.bti", "linktexbci4.bti"],
                ramps: true, tev_gate: TevGate::Error },
    ModelSpec { name: "ship", display: "King of Red Lions (hull)",
                bdl: "fn_body.bdl", prefix: "ship",
                expect: bmd::Expectations { jnt1_count: Some(11), ..bmd::CL_BDL },
                standalone: &["toon.bti", "toonex.bti"],
                ramps: true, tev_gate: TevGate::Warn },
    ModelSpec { name: "ship-head", display: "King of Red Lions (figurehead)",
                bdl: "fn_head_h.bdl", prefix: "ship_head",
                expect: bmd::Expectations { jnt1_count: Some(18), ..bmd::CL_BDL },
                standalone: &["toon.bti", "toonex.bti"],
                ramps: true, tev_gate: TevGate::Error },  // it should pass; find out if it doesn't
];
```

**The `..bmd::CL_BDL` lines above do not compile, corrected 2026-08-09.**
Functional-record-update requires *every* field of the struct to be visible at
the use site, including the ones the update omits — so writing
`bmd::Expectations { jnt1_count: Some(11), ..bmd::CL_BDL }` in `main.rs` is
E0451 unless `fourccs` and `block_num` are also `pub`, which would make
`EXPECTED_FOURCCS`'s shape part of `bmd`'s public API. (FRU *inside* a `const`
is fine — privacy is the blocker, not const-eval. The `..TEST_EXPECT` use in
`mod.rs`'s own tests keeps working for exactly that reason.) What landed instead
keeps the fields private and adds a constructor:

```rust
impl Expectations {
    pub const fn bdl(jnt1_count: u16) -> Self {
        Expectations { fourccs: &EXPECTED_FOURCCS, block_num: 9, jnt1_count: Some(jnt1_count) }
    }
}
pub const CL_BDL: Expectations = Expectations::bdl(42);
```

so the table reads `expect: bmd::CL_BDL` / `bmd::Expectations::bdl(11)` /
`bmd::Expectations::bdl(18)`, and 42 still lives next to `CL_BDL`.

New CLI surface, `--model` defaulting to `link`:

```
usage: convert_link <raw-dir> <out-dir> [--model=NAME]
                    [--info | --dump-mat3 | --dump-geometry] [--obj]
                    [--tev-gate=error|warn|off] [--no-ramps]
```

**`--model NAME` (space-separated) was wrong, corrected 2026-08-09** — hence
`--model=NAME` above. `main.rs`'s hand-rolled loop pushes any non-`-` token into
`positional`, so `--model ship` would put `"ship"` there and fail the
`[PathBuf; 2]` destructure with "expected exactly two directory arguments" — a
confusing error for a correct-looking invocation. The `=` form also matches the
plan's own `--tev-gate=`.

Keep ramps **on** for the ship: `fn_body`'s TEX1 genuinely carries a `ZBtoonEX`
placeholder, so the `RAMP_PREFIXES` substitution in `output.rs` is semantically
correct — it just needs `toon.bti` / `toonex.bti` in the ship's raw dir.

### 6. `crates/convert-link/src/output.rs` — parameterize the `link.` prefix

Seven literals across three functions. `build()` writes the buffer names *into the
manifest JSON*, so the prefix has to reach `build`:

- `pub fn build(model, baked, prefix: &str) -> Converted` — lines 80-82.
- `write_files()` — lines 260, 270, 277, 287.
- `write_obj()` — line 306 (`# Toon Link bind pose` → `# {display} bind pose`),
  307 (`mtllib`), 329, 347.
- Thread `ramps: bool` into `build` → `build_textures` for the `--no-ramps`
  escape hatch.

**This list is missing a caller, noted 2026-08-09:** `pose.rs:458`
(`real_bake_and_manifest`) also calls `output::build`, so changing that signature
breaks the build. It is a compile error rather than a silent skip, so it cannot
be missed — but it is a fourth site, not "three functions". What landed groups
the three parameters into a `Naming { prefix, display, ramps }` struct rather
than threading a bare `&str` plus a `bool` through four signatures.

Leave `mat3_dump.txt` (written at `main.rs:96`) unprefixed — separate out-dirs
make it collision-free, and not touching it is zero-risk for the golden hash.

The existing MTL rule (material's texmap slot 0) picks the right image for both
models: `fn_body`'s material has `texture_indices = 1 2 3`, so slot 0 → TEX1
index 1, the hull diffuse.

### Deliberately not changed

- **`shp1.rs`** — both models use the identical descriptor set (`PNMTXIDX/DIRECT`,
  `POS/NRM/TEX0` `IDX16`), 100% `TRIANGLESTRIP`, no billboards. `fn_head_h`'s
  largest `use_mtx` table is exactly 10 entries, so the existing `(byte / 3) >= 10`
  guard passes with zero headroom — but it does pass.
- **`jnt1.rs`** — every joint in both models is exactly scale (1,1,1), so the
  non-unit-scale hard error never fires. **Keep it.** Correct J3D scale support
  needs `no_inherit_scale` cancellation against the parent's *accumulated* scale
  and the INF1 scaling rule (both models are Maya, flags 0x2, same as `cl.bdl`),
  and the only oracle for getting it right is the `world·invBind = I` check —
  implementing it against a model with no invBinds means working with the answer
  key removed. Relax it when a model that actually needs it *and* has invBinds
  shows up.
- **`gx/types.rs`** — `S16` is already a `ComponentType`; no new variants are
  needed without CLR0.
- **The crate name.** A rename to `convert-j3d` would touch the directory, the
  justfile, `CLAUDE.md`, and 10+ insta snapshot files that embed the crate name
  (`convert_link__gx__texture__tests__*.snap`) — a wide cosmetic diff landing at
  the same time as the substantive parser work, which is exactly when a
  golden-hash regression is hardest to attribute. Do it later as a
  renames-only commit. Update the `main.rs` module doc and `CLAUDE.md:28` to say
  "Wind Waker J3D asset converter (Link + Ship)" instead.

## Extraction

`scripts/extract_ship.sh` mirrors `extract_link.sh` exactly: same
`TWW_DIR`/`DISC`/`DTK` resolution, same `be_u16` / `be_u32` / `check_size` /
`check_bdl_header` / `check_bti_header` helpers, same two-tier structure
(structural checks that work on the first run, then golden hashes). `dtk vfs cp`
handles the Yaz0 + RARC layers, so no decompression code is needed here either.

```bash
RAW=assets/ship/raw
MANIFEST=scripts/ship_assets.sha256

extract "/files/res/Object/Ship.arc:bdl/fn_body.bdl"   fn_body.bdl
extract "/files/res/Object/Ship.arc:bdl/fn_head_h.bdl" fn_head_h.bdl
# fn_body's TEX1 carries a ZBtoonEX placeholder, so RAMP_PREFIXES needs the real ramps.
extract "/files/res/Object/System.arc:dat/toon.bti"    toon.bti
extract "/files/res/Object/System.arc:dat/toonex.bti"  toonex.bti

# tier 1 -- measured exact sizes: 38240 / 77888
check_bdl_header "$RAW/fn_body.bdl";   check_size "$RAW/fn_body.bdl"   32768 49152
check_bdl_header "$RAW/fn_head_h.bdl"; check_size "$RAW/fn_head_h.bdl" 65536 98304
check_bti_header "$RAW/toon.bti";      check_size "$RAW/toon.bti"        512  4096
check_bti_header "$RAW/toonex.bti";    check_size "$RAW/toonex.bti"    16384 65536

# tier 2 -- bootstrap scripts/ship_assets.sha256 on first run, gated by tier 1
```

`Ship.arc:tex/new_ho1.bti` is not referenced by either model's TEX1 — skip it.

The ~50 duplicated helper lines are worth factoring into a
`scripts/_j3d_checks.sh` sourced by both scripts, but only if `extract_link.sh`'s
behavior is provably unchanged afterward.

### `examples/toon_link/justfile`

```just
extract-ship:
    ./scripts/extract_ship.sh

convert-ship *args:
    cargo run -p convert-link --bin convert_link -- \
      assets/ship/raw assets/ship/converted/body --model ship --obj {{args}}

convert-ship-head *args:
    cargo run -p convert-link --bin convert_link -- \
      assets/ship/raw assets/ship/converted/head --model ship-head --obj {{args}}
```

`just toon_link convert-link` stays untouched.

## Verification

The python oracles in `examples/toon_link/scripts/` are already path-parameterized
and model-agnostic. `link_chunk_table.py`, `link_mat3_table.py` and
`link_texture_diff.py` run on the ship files **unmodified**; only
`link_geometry_table.py` needs an edit (S4).

| step | command | expects |
|---|---|---|
| S0 | `just toon_link extract-ship` | 4 files, tier-1 checks pass, `ship_assets.sha256` bootstraps |
| S1 | `diff <(just toon_link convert-ship --info) <(./scripts/link_chunk_table.py assets/ship/raw/fn_body.bdl)` | identical; repeat for `fn_head_h.bdl` |
| S2 | same with `--dump-mat3` / `link_mat3_table.py` | identical |
| S3 | `just toon_link convert-ship`, then `./scripts/link_texture_diff.py assets/ship/raw assets/ship/converted/body/tex` | 4 TEX1 re-emits + 2 raw ramps, zero differing pixels |
| S4 | same with `--dump-geometry` / `link_geometry_table.py` | identical, after the oracle edit below |
| S5 | `just toon_link convert-ship-head` | invBind residual well under `INVBIND_EPS = 0.02` |
| S6 | Blender import of both OBJs | see below |
| S7 | Link regression | all 90 hashes + `link-verify-p1/p2/p3` + `cargo test -p convert-link -- --include-ignored` |

**Run S2 before worrying about the TEV gate.** `--dump-mat3` returns at
`main.rs:75-78`, *before* the gate fires — by design, so the dump stays usable for
diagnosing whatever the gate rejected. It is also the strongest early signal: it
exercises the entire MAT3 parser including the `Projmap` texmtx and 3-texgen paths.

**S4 needs ~~one~~ two additive edits to `link_geometry_table.py`:** its `evp1_section`
asserts `inv_count >= joint_count` and reads 12 floats from offset 0, so it
crashes on a header-only EVP1. Add the branch — if `inv_off == 0`, emit
`EVP1 envelopes=0 invbinds=0` and skip both loops. Do S4 against `fn_body` first
(F32 normals, so it isolates the EVP1 change), then `fn_head_h` (S16 normals).

**The second edit, found 2026-08-09:** `vtx1_section` hardcodes
`count(offsets[1], 12)` for the normal-array stride, so with `fn_head_h`'s S16
normals it reports `nrm=469` where the converter says `938`. Take the stride from
the NRM format entry (`{"Float32": 12, "Signed16": 6}`, defaulting to 12 when
absent, mirroring `vtx1.rs`'s own fallback). Without this, S4 cannot pass for
`fn_head_h` no matter how correct the Rust side is — which is a good argument for
the plan's own advice to run `fn_body` first.

Minor: the crash mechanism above is right but the reason is not the assert. With
`inv_off == 0` the assert at the *end* of `evp1_section` never runs, because the
invbind loop hits `struct.unpack_from` past the end of the 32-byte chunk first.
Had the chunk been larger, the assert would have passed and the oracle would have
emitted `joint_count` lines of float garbage read from the chunk header — a
silent wrong answer rather than a traceback. Same fix either way.

**S5 is the real FK test.** `fn_body` has no invBind matrices, so its residual
prints `0.00e0` and tells you nothing. `fn_head_h` is the only model here carrying
the file's own answer key — run it early, and treat a residual under
`INVBIND_EPS` as the acceptance criterion for the whole geometry path. Its joint
chain is ~8 deep with translations up to ~32 units, comparable to Link's ~6 deep /
~30 units, so 0.02 should hold; if it trips, that is real signal about FK, not
noise.

**S6 — Blender.** Import with the settings `link_rendering/phase_03.md` documents
for Link (Wavefront import, −Z forward / Y up, Split by Group). Model-specific
checks: the hull should read as a single ~278-strip group; the mast should stand
**up** rather than lying along +X — that's the `j_fn_mast` rotation chain, and it
is the best visual FK check available for `fn_body` given it has no invBinds; the
sail booms `j_fn_sail1/2/_e` should extend to ~270 units. For the head, the six
`j_fn_kubi1..6` neck segments should read as a curve rather than a straight stack.

**S7 after every code change, not just at the end.** The EVP1 early-return, the
`pose.rs` guard, and the `output.rs` prefix threading are the three most likely to
silently perturb Link.

## Known limitations, recorded not solved

- **The two OBJs are in separate local spaces.** The figurehead attaches at
  `j_fn_gattai`, but `mpHeadAnm`'s base matrix is set in actor code, not in the
  files, and it has not been traced. Merging them into one placed model is
  follow-up work. For reference, the cannon and crane — out of scope here — are
  trivially placeable: `d_a_ship.cpp:118-125` intercepts `J_FN_MAST` and gives
  crane = mast joint × `Zrot(0xC000)`, cannon = that × `Yrot(-0x8000)`.
- **`fn_body` cannot be rendered by the existing `tev.slang`.** Three texgens, a
  `MTX3x4`/`POS` texgen and a `Projmap` texture matrix all fall outside the frozen
  subset. That is why the gate is downgraded rather than satisfied. Rendering the
  boat is separate, larger work.
- **No animation.** `Ship.arc` has 10 `bck` files (`fn_mast_on2`, `fn_mast_off2`,
  `fn_look_l/r`, `fn_talk_a/b`, `damage1`, `akibi1`, `fn_lose1`, `kyakkan1`). BCK
  parsing does not exist in this repo and stays out of scope, consistent with
  [`link_rendering/follow_up.md`](link_rendering/follow_up.md).
- **`vfncn` / `vfncr` deferred.** Picking them up later means adding `RGBA8` to
  `ComponentType` (GX overloads `GXCompType` for color attributes: 0..5 are
  RGB565/RGB8/RGBX8/RGBA4/RGBA6/RGBA8, and gclib aliases 0x00–0x04 onto the
  numeric names, so exactly one new variant keeps `--dump-geometry` diffable), a
  `colors` field on `Vtx1`, and — critically — the `Attr::Clr0` arms in **both**
  `shp1.rs:190` (descriptor list) and `shp1.rs:288` (display-list decode)
  *together*: adding the descriptor without the decode arm just moves the error,
  and adding the decode arm without the right width desyncs the index stream.
  Their CLR0 arrays hold a single white RGBA8 value padded with J3D's ASCII
  filler, so the visual payoff is nil.
