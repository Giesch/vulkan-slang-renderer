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
  diffs rather than eyeballing. The GUI also opens BMD/BDL directly with MAT3
  material property editing and a **real-time 3D preview** — i.e. it contains
  its own working J3D renderer, giving us a locally-runnable material
  inspector and a third visual reference besides noclip/Dolphin. Dev-only
  dependency, invoked from `just` recipes.
- **SuperBMD** (C#, runs under mono) — the modding community's standard
  BDL→COLLADA converter, both directions (modders inject custom models into
  real games with it, a harder correctness bar than exporting). Gives us
  bind-pose geometry, a skeleton, *and* a materials JSON dump — an independent
  implementation of almost exactly our converter's job. **Use RenolY2's fork**
  (github.com/RenolY2/SuperBMD): it's the one with the materials-as-JSON
  dump/insert feature (format documented on the MKDD wiki, "SuperBMD JSON
  Files"). Dev-only dependency.
- **noclip.website** — final visual ground truth in the browser (it renders
  Link's `cl.bdl` with full TEV), and its source (`gx_material.ts`,
  `J3DLoader.ts`) is the semantic spec when dumps disagree.
- **Dolphin** — far more than a screenshot source; see [Dolphin as an
  automated oracle](#dolphin-as-an-automated-oracle) below: headless FIFO-log
  replay for golden reference frames, automated texture dumping, a
  software-renderer tiebreaker for TEV semantics, and runtime RAM extraction
  via dolphin-memory-engine + tww decomp symbols.
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
- **Ground-truth lighting values** (risk #8): **dolphin-memory-engine**
  (pip-installable Python module) reads/writes emulated RAM from outside the
  process, and the tww decomp provides exact symbol addresses — so a small
  script attached to Dolphin on noon-Outset reads Link's live
  `dKy_tevstr_c` light/ambient colors directly, replacing hand-tuned seeds
  with extracted constants. The same mechanism can *write* the time-of-day
  variable to force noon before capturing the savestate/FIFO log.

Not available: Dolphin has no per-TEV-stage intermediate dump in mainline
(checked `VideoConfig.h`) — stage-level debugging stays with our optional CPU
reference evaluator (P8).

## P0 — extraction

Fully automatable in the script itself:

- Assert exact byte sizes against `dtk vfs ls` output.
- Assert `cl.bdl` begins with magic `J3D2bdl4`.
- Record and verify SHA256s — the disc image is fixed, so these never change
  (the one place golden hashes are *permanently* stable).
- Re-running must be idempotent.

## P1 — chunk walk

- **Internal invariants** (run on every convert): chunk magics ∈ {INF1, VTX1,
  EVP1, DRW1, JNT1, SHP1, MAT3, MDL3, TEX1}; chunk sizes sum to file size;
  JNT1 count == 42; INF1's joint/material/shape counts consistent with the
  other chunks.
- **Oracle**: a five-line gclib script printing its chunk table and counts for
  `cl.bdl`; diff against our `--info` output.
- **Unit tests**: `BeReader` on tiny synthetic buffers — endianness, string
  tables, seek behavior.

## P2 — texture decode + MAT3 dump *(biggest early-oracle win)*

- **Pixel-exact texture gate** (`just link-verify-textures`): the converter
  also emits each TEX1 entry as a standalone `.bti` (header + data — they
  share the format); the recipe runs GCFT on every `.bti` and pixel-diffs
  GCFT's PNG against ours. **Zero pixels different, per format, across all of
  Link's textures.** This turns "eyeball the PNGs" into a hard pass/fail and
  pins down the tile-layout code (the classic bug source, risk #2's cousin)
  completely. If GCFT and we disagree, Dolphin's texture dump from the Link
  `.dff` replay is the third vote, and GCFT's built-in J3D preview a quick
  visual sanity check.
- **Per-format unit tests**: synthetic hand-computed tiles (one 8×8 CMPR tile,
  one I4 tile, …) as insta snapshots — committable, catch regressions with no
  extracted assets present.
- **MAT3 field-exact gate**: run SuperBMD (RenolY2 fork) on `cl.bdl` and diff
  its materials JSON against our `--dump-mat3` output field by field for
  every material —
  stage counts, selectors, konst colors, blend/Z modes, texgen types. A small
  comparison script, not manual reading; disagreements adjudicated against
  `J3DMaterialFactory.cpp` and noclip's loader. The TEV subset is frozen from
  this dump, so independently confirming the *parse* here de-risks everything
  downstream — a wrong shader is debuggable, a wrong parse poisons all later
  phases.
- **Parse-don't-validate as a test**: the typed-enum IR means every selector
  byte in the real file must map to a known variant or conversion errors —
  running the converter *is* a fuzz-by-real-data test of the parser.

## P3 — geometry + pose baking *(second-biggest win)*

Gate recipe: `just link-verify-geometry`.

- **Weighted-identity check**: EVP1-weighted verts must bake to ≈ their stored
  positions at bind pose. A hard converter error with a distance report, not a
  warning.
- **Skeleton oracle**: compare all 42 joint *world-space positions* against
  SuperBMD's exported armature (or a gclib script evaluating JNT1); exact
  match within float epsilon. This isolates the JNT1 walk from everything
  else — per risk #1, a verified skeleton plus the identity check leaves the
  SHP1 matrix-table logic as the only remaining suspect for geometry
  weirdness.
- **Mesh metrics vs SuperBMD's DAE**: total triangle count (must match
  *exactly* — strip expansion is deterministic: Σ(len−2) per strip),
  per-material triangle counts, per-material bounding boxes within epsilon,
  overall AABB (Link ≈ 100 units tall). Raw vertex counts may legitimately
  differ (different dedup strategies) — compare geometry-derived metrics, not
  arrays.
- **Property checks in the converter**: all indices in range, no degenerate
  triangles, normals unit length, every batch's material index valid.
- **Manual**: `--obj` export overlaid on SuperBMD's DAE in Blender — they
  should coincide visually vertex-for-vertex.

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

- **Structured side-by-side**: same camera angle as noclip and the golden
  Dolphin reference frames (from `just link-dolphin-refs`); compare per
  feature (skin tone, tunic two-band boundary, hair highlight, eye whites)
  rather than gestalt.
- **Semantic disputes adjudicated by Dolphin**: the FIFO analyzer shows the
  runtime BP/XF register state per draw (is our frozen TEV subset what the
  game actually configures?), and the software-renderer replay is the
  reference-rasterizer answer for any TEV math disagreement.
- **Light rotation** in the example: terminator bands must sweep smoothly and
  stay *banded* — the sharpest test of the SRTG ramp path (risk #5).
- **Single-material isolation**: a debug key in the example to draw only batch
  N, so a wrong material is inspected alone rather than through overdraw.
- Optional, only if pixel-chasing gets hard: a tiny CPU reference evaluator of
  our own TEV IR (evaluate one stage config at a hand-picked N·L, compare
  against the shader's output for a flat-lit patch).

## P9 — polish

- Casual clothes: texture-swap only → P7-style UV/feature checks.
- Eye multi-pass: verify against the game's behavior in **Dolphin** (eyes
  reading through hair at grazing angles) — noclip may not implement this
  trick, so Dolphin is the reference here.
