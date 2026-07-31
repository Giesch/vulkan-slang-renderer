# Toon Link plan: per-phase correctness testing

How to verify each phase of [`../link_rendering.md`](../link_rendering.md) §6
*as we go*, without the full toon renderer / TEV interpreter in place — by
diffing intermediate outputs against independent implementations (oracles) or
by tests we own. Companion to [`risks.md`](risks.md).

## The oracle toolbox

Recurring external references, in rough order of automation strength:

- **GCFT / gclib** (LagoLunatic's GameCube File Tools + its library; Python,
  pip-installable from GitHub) — battle-tested BTI→PNG conversion, RARC, Yaz0,
  and J3D chunk parsing (it powers Wind Waker Randomizer's custom-player-model
  support, which reads and rewrites BDLs). Scriptable, so we can do *exact*
  diffs rather than eyeballing. *As implemented (P1/P2): gclib is the
  automated oracle for every gate so far — chunk table, 44/44 pixel-exact
  textures, canonical MAT3 diff — via uv PEP-723 scripts pinned to
  1.0.0 @ `6412774`. Its parse depth varies by chunk (deep for
  INF1/VTX1/JNT1/MAT3/TEX1; headers-only for SHP1, nothing for EVP1/DRW1),
  so geometry oracles supplement it with independent struct walks from the
  tww headers.* The GUI also opens BMD/BDL directly with MAT3 material
  property editing and a **real-time 3D preview** — a locally-runnable
  material inspector and a third visual reference besides noclip/Dolphin.
  Dev-only dependency, invoked from `just` recipes.
- **SuperBMD** (C#, runs under mono) — the modding community's standard
  BDL→COLLADA converter, both directions. Gives bind-pose geometry, a
  skeleton, and a materials JSON dump (**RenolY2's fork**,
  github.com/RenolY2/SuperBMD, has the JSON feature). *Demoted from
  automated oracle to manual second opinion (P2/P3 planning): gclib covers
  the MAT3 diff, and P3's skeleton verification uses the file's own inverse
  bind matrices, which is stronger than a cross-tool armature comparison.
  SuperBMD remains the Blender DAE-overlay tool and the tiebreaker if an
  intrinsic check fails ambiguously.* Dev-only, optional.
- **noclip.website** — final visual ground truth in the browser (it renders
  Link's `cl.bdl` with full TEV), and its source (`gx_material.ts`,
  `J3DLoader.ts`) is the semantic spec when dumps disagree.
- **Dolphin** — far more than a screenshot source; see [Dolphin as an
  automated oracle](#dolphin-as-an-automated-oracle) below: headless FIFO-log
  replay for golden reference frames, automated texture dumping, a
  software-renderer tiebreaker for TEV semantics, and runtime RAM extraction
  via dolphin-memory-engine + tww decomp symbols. *None of it has been set up,
  and after P8 the software-renderer tiebreaker (risk #6) is the only remaining
  thing it would buy.*
- **The `../tww` decomp itself** — added to this list after P8, where it did
  the work Dolphin was scoped for. Reading it settled the light model, the
  attenuation coefficients and the per-frame TEV register overrides directly,
  and it is grep-able rather than needing a running emulator. Reach for it
  before the emulator whenever the question is "what does the game *set*", as
  opposed to "what did this frame *compute*".
- **Our own repo machinery** — insta snapshot tests (already the house style);
  the converter's internal invariant checks, which run against the *real*
  356 KiB file on every conversion (tests with a fixture we never commit); and
  committed **golden hashes**: once a phase's output is verified, commit
  SHA256s of the converted outputs (hashes of derived data aren't copyrightable
  content). Any converter refactor that changes a hash is either an intended
  fix or a regression — free regression detection from then on.

Committed test fixtures must always be synthetic (hand-computed tiles, tiny
buffers) so `just test` runs in a clean checkout with no extracted assets.

## Dolphin as an automated oracle

> **Status: optional, and none of it has been set up.** No savestate, no
> `.dff`, and `just link-dolphin-refs` does not exist as a recipe. P8 —
> originally the phase that would have made this load-bearing — deliberately
> does not use it ([`phase_08.md`](phase_08.md) decision 7, §P8 below). Treat
> this section as a capability catalogue: what is available *if* a specific
> feature ends up genuinely in dispute. [`follow_up.md`](follow_up.md) §5 owns
> the decision to invoke it.

One-time manual setup: play to Outset at noon with Link framed, save a
**savestate**, and record a **FIFO log** (`.dff`) of one frame — a capture of
every command the game sent to the GPU that frame. Everything below is then
headless and deterministic. Mainline `dolphin-emu-nogui` supports `--batch`,
`--exec`, `--save_state`, `--movie`, `--video_backend`, `--user <dir>`
(isolated config/dump dirs, so scripts never touch the real Dolphin install),
and `-C System.Section.Key=Value` per-invocation config overrides — verified
against `Source/Core/UICommon/CommandLineParse.cpp`.

- **Golden reference frames**: `dolphin-emu -b -e link.dff` replays the log
  and exits; with frame dumping enabled the rendered output is stable across
  runs. This is exactly how the Dolphin project's own **FifoCI** does GPU
  regression testing on every commit. Gives P7/P8 a regenerable ground-truth
  image without ever re-playing the game. Recipe: `just link-dolphin-refs`
  (replay `.dff` with frame + texture dumping into an isolated `--user` dir).
- **Second texture oracle** (P2): the same replay with texture dumping on
  (`DumpTextures` in the GFX settings) writes every texture the frame uploads
  as PNG, with the GX format ID encoded in the filename — exactly Link's
  frame's textures, nothing else.
- **Runtime TEV state** (P2/P8): the FIFO Player GUI steps the log
  draw-call-by-draw-call, and its analyzer decodes each draw's BP/XF register
  writes — the actual TEV configuration *after* the engine fed in live
  values. Cross-checks the P2 frozen subset against reality, and reveals the
  real C0/K0/K1 colors the kankyo system wrote that frame.
- **Reference rasterizer** (P8): replaying under
  `--video_backend "Software Renderer"` uses Dolphin's most literal GX
  implementation (slow, per-pixel exact). The tiebreaker when our shader,
  noclip, and Dolphin's hardware backends disagree — e.g. the S10 clamping
  edge cases of risk #6.
- ~~**Ground-truth lighting values** (risk #8)~~ — **no longer needed; risk #8
  was closed without any of this on 2026-07-27.** The plan was:
  dolphin-memory-engine (pip-installable) reads emulated RAM from outside the
  process and the tww decomp gives exact symbol addresses, so a script attached
  to Dolphin on noon-Outset could read Link's live `dKy_tevstr_c` colors and
  even *write* the time-of-day variable to force noon first. It would have
  worked, and it was the harder route to values that were sitting in plain
  sight: the light colors are **constants in the decomp** (one channel per
  light — red diffuse, green eflight) and the stage-0 lerp endpoints are
  **static stage data on the disc**, read by `scripts/link_env_colors.py`
  through the same `dtk vfs cp` path `extract_link.sh` already used. Kept here
  as a live capability for anything genuinely runtime-only; see
  [`risks.md`](risks.md) §8 for the trace.

Not available: Dolphin has no per-TEV-stage intermediate dump in mainline
(checked `VideoConfig.h`) — stage-level debugging stays with our optional CPU
reference evaluator (P8).

## P0 — extraction ✅ (done as planned; `a76d0cb`)

Fully automatable in the script itself:

- Assert exact byte sizes against `dtk vfs ls` output.
- Assert `cl.bdl` begins with magic `J3D2bdl4`.
- Record and verify SHA256s — the disc image is fixed, so these never change
  (the one place golden hashes are *permanently* stable).
- Re-running must be idempotent.

## P1 — chunk walk ✅ (done as planned; `6431f0a`)

- **Internal invariants** (run on every convert): chunk magics ∈ {INF1, VTX1,
  EVP1, DRW1, JNT1, SHP1, MAT3, MDL3, TEX1}; chunk sizes sum to file size;
  JNT1 count == 42; INF1's joint/material/shape counts consistent with the
  other chunks.
- **Oracle**: `scripts/link_chunk_table.py` (gclib) printing the canonical
  chunk table; `just link-verify-p1` diffs it against `--info` — zero-line
  diff.
- **Unit tests**: `BeReader` on tiny synthetic buffers — endianness, string
  tables, seek behavior.

## P2 — texture decode + MAT3 dump ✅ *(biggest early-oracle win; `8a0a4af`)*

As run (details + recorded facts in [`phase_02.md`](phase_02.md)):

- **Pixel-exact texture gate** (`just link-verify-textures`): the converter
  emits each TEX1 entry as a standalone `.bti` with the GX bytes copied
  verbatim; `scripts/link_texture_diff.py` decodes every `.bti` with gclib
  (not the GCFT GUI — same decode code, scriptable) and pixel-diffs against
  our PNG. **Result: 44/44 zero pixels different** (41 TEX1 + 3 standalone).
  Because the `.bti` bytes are verbatim, the gate compares two independent
  decoders over identical data. Dolphin's texture dump remains the unused
  third vote; GCFT's J3D preview the visual sanity check.
- **Per-format unit tests**: synthetic hand-computed tiles as insta
  snapshots — committed, catch regressions with no extracted assets present.
- **MAT3 field-exact gate** (`just link-verify-mat3`) — *oracle swapped from
  the original plan*: probing showed gclib parses MAT3 completely
  (`asdict()` covers every field), so the gate became a **canonical-table
  byte diff** (P1 discipline: both sides implement a written spec, enum
  spellings shared) against `scripts/link_mat3_table.py`, instead of a
  SuperBMD-JSON field mapper under mono. **Result: zero-line diff** over all
  24 materials. Disagreements would be adjudicated against
  `J3DMaterialFactory.cpp` and noclip's loader; none arose. The TEV subset
  is frozen from this dump (phase_02.md Recorded facts) — notable catches
  vs the guessed subset: swap-table channel broadcasts and two non-identity
  texture matrices.
- **Parse-don't-validate as a test**: every selector byte in the real file
  maps to a typed enum variant or conversion errors — running the converter
  *is* a fuzz-by-real-data test. (It passed first try on the real file, and
  tamper tests produce typed errors naming the field.)

## P3 — geometry + pose baking *(second-biggest win)*

Gate recipes: `just link-verify-geometry` / `link-verify-p3`. Planned in
detail in [`phase_03.md`](phase_03.md); strategy re-weighted after probing
found the file carries its own answer key:

- **invBind identity (the skeleton oracle, upgraded)**: EVP1 stores every
  joint's inverse bind matrix, so at bind pose `world(j) · invBind(j) = I`
  must hold for all 42 joints — verifying our FK (composition order, INF1
  parent wiring, rotation conversion) against Nintendo's own exporter output
  with **no third-party tool in the loop**. Hard error with a max-residual
  report. This replaces the SuperBMD-armature comparison as the automated
  skeleton check.
- **Weighted-identity check**: EVP1-weighted verts must bake to ≈ their stored
  positions at bind pose. A hard converter error with a distance report, not a
  warning. Per risk #1, identity + verified skeleton leaves the SHP1
  matrix-table logic as the only remaining suspect for geometry weirdness.
- **Canonical geometry diff**: `--dump-geometry` (raw file data only — no
  computed floats) byte-diffed against `scripts/link_geometry_table.py`
  (gclib for INF1/VTX1/JNT1 + independent tww-struct walks for
  EVP1/DRW1/SHP1, including its own display-list decoder — prototype already
  ran clean against the real file).
- **Mesh metrics**: total triangle count must equal **exactly 2,874**
  (deterministic: Σ(len−2), 573 strips) — *confirmed*. *As implemented*, the
  baked-vs-stored AABB cross-check was **dropped as redundant**: the canonical
  oracle diff already verifies every stored SHP1 min/max byte-for-byte, and
  invBind + weighted identity verify the pose, so a baked-AABB comparison adds
  nothing. The overall model AABB (X 125, Y 124 tall, Z 89) is a sanity anchor.
  Vertex counts are dedup-dependent (1754 after dedup) — compare geometry-derived
  metrics, not arrays.
- **Property checks in the converter**: all indices in range, PNMTXIDX %3==0
  and slot-set, no degenerate triangles, normals unit length (where present
  — two eye shapes have none), every batch's material index valid.
- **Manual**: the 10-step Blender procedure in phase_03.md — textured
  `--obj`+`.mtl` import, scale/pose, rigid attachment, weighted regions,
  per-batch isolation, triangle count, UV placement, face orientation
  (early winding read), and the SuperBMD-DAE (or noclip) overlay.

## P4 — multi-draw + shared mesh (renderer; no Link assets needed)

- **A committed test/demo example, `examples/multi_mesh.rs`**: two or three
  distinct shapes with free textures, multiple pipelines, one shared mesh,
  index-range draws — including deliberately drawing *disjoint sub-ranges* so
  an off-by-one in `first_index` is visually obvious (gaps or overlaps).
  Permanent, asset-free regression coverage and documentation-by-example for
  the new API.
- **Regression sweep** (`just dev-all` or similar): loop
  `timeout 3 just dev <name>` over all examples, failing on any Vulkan
  validation output. The validation layers are the real test — they catch
  descriptor/binding mistakes in the recording loop immediately.
- `just test`: snapshots must be byte-identical (the change is codegen-
  invisible by design).

## P5 — raster state + texture options

- **Raster state**: extend `multi_mesh` — one object per state: cull front
  (inside-out on demand), blend opaque vs alpha, depth-write off (draw-order
  artifacts visible on demand). Each a visually unambiguous single-purpose
  check.
- **Texture options**: a quad rendering a tiny asymmetric test texture 4 ways
  (clamp/repeat × linear/nearest, sampled past [0,1]); and the decisive sRGB
  check: two quads showing the same 50%-gray texture as `Srgb` vs `Unorm` —
  they *must* differ visibly in brightness, with a solid-gray in-shader
  reference triangle to show which is correct.
- Same validation sweep; UNORM + no-mips + clamp is a new format/usage combo
  the layers will vet.

## P6 — debug-shaded Link

- **Uniform-array smoke test first** (risk #4): throwaway shader with a
  `uint4[8]` uniform, write a known pattern, render values as colors — before
  any TEV code exists.
- Normals-as-color is itself a diagnostic: smooth gradients = smooth normals;
  hard color seams = normal-transform bugs. Silhouette vs a noclip screenshot
  from the same angle.
- Run with culling off; then the winding check (risk #3): enable back-face
  culling, confirm nothing disappears.

## P7 — albedo-only

- UV correctness checked feature-by-feature against noclip: face decals, eye
  placement, belt buckle, tunic patterns — misaligned UVs or a V-flip are
  instantly visible on a character.
- Alpha-compare: clean cutout edges on eyebrows/eyelashes, no rectangular
  halos.
- Per-material raster state now live on real data: no missing body parts
  (wrong cull), hair-over-face correct at this stage's depth settings.

## P8 — full TEV *(the one phase where the final visual is the test)*

By here every input is independently verified, so remaining discrepancies are
TEV-interpreter bugs specifically — that was the point of the earlier gates.

**Dolphin is deliberately not P8's oracle** (user decision; see
[`phase_08.md`](phase_08.md) decision 7). The savestate/`.dff` capture,
`just link-dolphin-refs`, the FIFO analyzer and the software-renderer replay
are an **optional escalation** owned by [`follow_up.md`](follow_up.md) §5,
invoked only for a specific disputed feature. The cost was recorded honestly as
two items shipping reasoned rather than measured — ~~the S10 clamp edge cases
(risk #6) and the exact `dKy_tevstr_c` light values (risk #8)~~ — and **it
turned out to be one.** Risk #8 was closed from the decomp and the disc instead
(`risks.md` §8); risk #6 stands, and is the only thing here the software
renderer would still settle.

- **Structured side-by-side vs noclip**: same camera angles as P6; compare per
  feature (skin tone, tunic two-band boundary, hair highlight, eye whites)
  rather than gestalt.
- **Terminator sweep** in the example: the bands must sweep smoothly and stay
  *banded* — the sharpest test of the SRTG ramp path (risk #5). ~~Light
  rotation~~; as shipped the lights are fixed and the *model* spins under them,
  as in the game, so this needs no keypress.
  ~~Prefer adjudicating on band *structure*, which the hand-tuned light values
  do not affect, over band *color*, which they do.~~ **That advice is obsolete
  and now points the wrong way**: the light values are derived rather than
  tuned (risk #8), so band *color* is once again real evidence, not a
  known-unknown to be discounted.
- **Eflight A/B** (`T` in the example): a sharper test of the same mechanism
  than the sweep. Light 0 is red-only and light 1 green-only, so toggling the
  eflight must light stage 2's additive highlight *and nothing else* — with it
  off, the ramp's G axis must be dead.
- **Only the lit materials may respond**: the 12 `lighting_enabled: false`
  eye/brow decals have no SRTG texgen, so if they change as the model turns
  under the light, the channel is leaking (phase_08 measured facts).
- **Single-material isolation**: P7's Q/E/Space batch keys already are material
  isolation — batches and material slots are bijective — so a wrong material is
  inspected alone rather than through overdraw. P8 adds the stage equations to
  the printout for direct comparison against `mat3_dump.txt`.
  *As run, this became fully mechanical rather than a per-material eyeball: the
  window is driven through all 24 batches with synthetic keypresses, the
  printouts captured from stdout, and each material's equation / order / texgen
  lines diffed against the matching block of `mat3_dump.txt` — 24 compared, 0
  mismatched. Comparing **printouts** is what makes this reliable; see the
  screenshot caveat in [`follow_up.md`](follow_up.md) §5.*
- **`mat3_dump.txt` is not a complete oracle for the konst path.** It renders
  every konst input as a bare `KONST`, so it cannot distinguish K0 from K3_A —
  which is exactly where phase_08's own worked example was wrong. The isolation
  printout therefore annotates each stage with the *resolved* `kcsel`/`kasel`
  and the swap-table contents, and `tev_pack`'s `ear_end_to_end` test asserts
  the selectors rather than the resulting values (the two konst colors involved
  are both white, so only the selector distinguishes them).
- **Internal cross-checks that need no emulator**: the converter's subset gate
  (`tev_ir.rs`), the `tev_pack` unit tests, and the debug modes that expose
  `COLOR0` and the SRTG texcoord before the final image is judged.
- Optional, only if pixel-chasing gets hard: a tiny CPU reference evaluator of
  our own TEV IR (evaluate one stage config at a hand-picked N·L, compare
  against the shader's output for a flat-lit patch). Note that mainline Dolphin
  has no per-TEV-stage intermediate dump either, so this is the stage-level
  tool whether or not the Dolphin escalation is taken.

## P9 — polish

- Casual clothes: texture-swap only → P7-style UV/feature checks.
- Eye multi-pass: verify against the game's behavior in **Dolphin** (eyes
  reading through hair at grazing angles) — noclip may not implement this
  trick, so Dolphin is the reference here.
