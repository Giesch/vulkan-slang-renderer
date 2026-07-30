# Phase 8: the TEV interpreter — lighting channel, SRTG ramp, full stage math

Detailed plan for P8 of [`../link_rendering.md`](../link_rendering.md) §6
(shader §3, converter §2.2, example §5). Estimated: 3–5 days. Verification
follows [`tests.md`](tests.md) §P8. Builds directly on P7
([`phase_07.md`](phase_07.md)), consuming the same converted assets
(`just extract-link && just convert-link`). Line numbers and every manifest
measurement below verified at `00b8d59` against the
`assets/link/converted/link.manifest.json` on disk.

**Goal**: after P8, `just dev toon_link` renders Link with **cel shading** — a
real GX color channel driving the `ZBtoonEX` ramp through an SRTG texgen, the
full TEV stage pipeline (inputs, ops, bias/scale/clamp, registers, konst
selects, swap tables), and the two non-identity texture matrices on the pupils.
Rotating the light sweeps the terminator and the bands stay banded. The frozen
TEV subset finally has a **gate that fails loudly** instead of a doc paragraph.
Still out: BTP eye/brow frame animation, `BlendMode::DstAlpha` + the eye
write-mask multi-pass, `--casual`, BCK poses (all P9 or deferred).

**Deliverables**

1. `src/bin/convert_link/tev_ir.rs` — typed `TevMaterialDesc` IR + the subset
   gate, run on every conversion; validation-only, so the manifest bytes and
   `scripts/link_converted.sha256` are untouched
2. `shaders/source/tev.slang` — new shared module: `TevParams` plus the GX
   interpreter (texgen, color channel, stage loop, swap tables, konst selects)
3. `shaders/source/toon_link.shader.slang` — per-vertex channel + texgen,
   per-fragment TEV; new generated bindings including
   `src/generated/shader_atlas/tev.rs`
4. `src/tev_pack.rs` — manifest → `TevParams`, unit-tested under `just test`
5. `examples/toon_link.rs` — per-material TEV uniforms, light controls,
   expanded debug modes, isolation printout with stage equations
6. Doc edits: master plan §6 P8 row + risks #5/#6/#8;
   [`tests.md`](tests.md) §P8; [`follow_up.md`](follow_up.md) §5/§6;
   the wrong `reg_colors` doc comment in `src/model_manifest.rs:341`
7. Recorded facts below filled in

## Measured facts this phase relies on

Everything here was read off the shipped manifest and the source tree at
`00b8d59`, not from the master plan's older sketches. Where they disagree,
these win.

### The material set splits cleanly in two

All 24 materials fall into exactly two groups, and the split is total — the
same 12 names every time:

| | lit + SRTG group | unlit decal group |
|---|---|---|
| materials | `ear`, `face`, `mouth`, `podA`, `sleeve`, `ear(2)`…`ear(8)` | `eyeL/R`, `eyeL/RdamA`, `eyeL/RdamB`, `mayuL/R`, `mayuL/RdamA`, `mayuL/RdamB` |
| `channels[0].lighting_enabled` | `true`, `lit_mask: 3` (lights 0+1) | `false`, `lit_mask: 2` |
| SRTG texgen | yes | no |
| `tev.orders[*].channel` | `255` (`COLOR_NULL`) on every stage | `4` (`COLOR0A0`) |
| RASC / RASA in stage inputs | never | 10 use RASC, 6 use RASA |

**Consequence, and it is the shape of the whole phase**: on the lit group the
color channel never reaches TEV as a raster color — it reaches it *only* as the
SRTG texcoord that indexes the ramp. On the unlit group `lighting_enabled` is
false, so `COLOR0A0` collapses to the material register color (white, alpha
255) and RASC/RASA are constants. So **light direction can only ever move the
12 lit materials, and only through the ramp**. If rotating the light changes
the eyes or brows, something is wrong by construction.

### The TEV subset, re-measured

- **All 24 materials**: `color_op`/`alpha_op` = `0` (ADD) only, bias `0`
  (ZERO), scale `0` (SCALE_1), destination register `0` (PREV). Clamp is the
  only stage-op field that varies (`true` and `false` both occur).
- **Color inputs in use**: `0` CPREV, `2` C0, `8` TEXC, `10` RASC, `14` KONST,
  `15` ZERO. **Alpha inputs**: `0` APREV, `4` TEXA, `5` RASA, `6` KONST,
  `7` ZERO.
- **Konst selects in use** (stage-indexed, first `num_tev_stages` only) —
  three pairs: `(kcsel 12 = K0, kasel 28 = K0_A)`, `(13 = K1, 28 = K0_A)`,
  `(12 = K0, 31 = K3_A)`.
- **Swap modes**: `ras_sel` is always `0`; `tex_sel` ∈ {0, 1, 2}. Tables:
  slot 0 `[0,1,2,3]` identity, slot 1 `[0,0,0,3]` RRR+A, slot 2 `[1,1,1,3]`
  GGG+A. 12 materials carry slots 1 and 2; the other 12 carry slot 0 only.
- **Texgens** — exactly three distinct configs across the model:
  `{ty 1 MTX2x4, src 4 TEX0, mtx 60 IDENTITY}` ×24 (every material's texcoord
  0), `{ty 10 SRTG, src 19 COLOR0, mtx 60 IDENTITY}` ×12 (the lit group's
  texcoord 1), `{ty 1 MTX2x4, src 4 TEX0, mtx 33 TEXMTX1}` ×2
  (`eyeL`/`eyeR` texcoord 1, the `hitomi` pupil).
- **`num_tex_gens` is 1 (×10) or 2 (×14). Maximum 2** — so 2 texcoord
  interpolants suffice and 4 is generous headroom.
- **Texture matrices**: 3 distinct entries. Every material emits slot 0 with
  `scale [1,1]`, `rotation 0`, `translation [0,0]` (identity, and unreferenced
  since texcoord 0 selects `IDENTITY` not `TEXMTX0`). `eyeL`/`eyeR`
  additionally emit slot 1: `scale [1,1]`, `rotation 0`,
  `translation [-0.05, 0]`, `center [0.5, 0.5, 0.5]`. **With unit scale and
  zero rotation the `center` term and the Maya-vs-standard composition
  convention both cancel** — the matrix is a pure translate. That is why the
  gate can *reject* non-unit scale / non-zero rotation rather than this
  document having to settle a convention it cannot verify.
- **Channel colors**: `material_colors[0]` = `[255,255,255,255]` and
  `ambient_colors[0]` = `[50,50,50,50]` on every material;
  `mat_src`/`amb_src` are both `Register` everywhere (cl.bdl has no vertex
  colors — phase_03); `diffuse` is `Clamp` and `attenuation` is `Spot`
  everywhere.
- **`light_colors` is 8× `null` on every material.** The light color has no
  manifest source; the game writes it per frame from `dKy_tevstr_c` (master
  plan risk #8). It must come from an example-supplied uniform.

### `reg_colors` is REG0/REG1/REG2 — *not* PREV/REG0/REG1/REG2

The doc comment on `src/model_manifest.rs:341` says the four `reg_colors` slots
are `(PREV/REG0/REG1/REG2)`. **That is wrong**, and getting it wrong silently
makes the toon band vanish. From the decomp
(`../tww/src/JSystem/J3DGraphBase/J3DMatBlock.cpp:810-811`, and the same
shift in the `loadTevColor` helper at :42-44):

```cpp
for (u32 i = 0; i < ARRAY_SIZE(mTevColor) - 1; i++)
    J3DGDSetTevColorS10((GXTevRegID)(i + 1), mTevColor[i].mColor);
```

`GXTevRegID` is `PREV=0, REG0=1, REG1=2, REG2=3`, so entry *i* loads into
register *i+1*: **`reg_colors[0] → REG0` (the `C0`/`A0` selectors),
`[1] → REG1`, `[2] → REG2`, and `[3] is never loaded at all`** (the loop stops
one short). PREV gets no initial value from MAT3. Consistent with the data:
`reg_colors[3]` is `[0,0,0,0]` on all 24 materials. The konst path has **no**
such shift — `loadTevKColor` (:46-48) is `J3DGDSetTevKColor(GXTevKColorID(reg),
…)`, so `konst_colors[i] → K{i}` directly.

Worked check on `ear`, the canonical toon material (3 stages, 2 texgens,
texmaps `[34 linktexS3TC, 35 ZBtoonEX]`, `konst = [white, (160,90,0,255), …]`,
`reg_colors = [(128,128,128,255), white, white, (0,0,0,0)]`):

| stage | order | swap | equation |
|---|---|---|---|
| 0 | tc1 / tm1 (the ramp, via SRTG) | tex_sel 1 → RRR+A | `PREV = lerp(C0, K0, TEXC)` |
| 1 | tc0 / tm0 (the albedo) | tex_sel 0 → identity | `PREV = TEXC · CPREV`; `PREV.a = K0_A · TEXA` |
| 2 | tc1 / tm1 (the ramp again) | tex_sel 2 → GGG+A | `PREV = CPREV + K1 · TEXC` |

Under the corrected mapping `C0 = reg_colors[0] = mid-gray 128`, so stage 0 is
a band between mid-gray and white selected by the ramp's red channel, stage 1
modulates by the albedo, and stage 2 adds a warm `(160,90,0)` highlight
weighted by the ramp's green channel. That is cel shading. Under the *comment's*
mapping `C0` would be `reg_colors[1] = white`, making stage 0 `lerp(white,
white, ramp)` — a no-op, and the phase would ship with no bands and no obvious
culprit. Fix the comment as part of this phase; it is a comment only, so no
serialized bytes and no golden-hash change.

### Converter state

- **Nothing on the TEV path is gated today.** `tev_ir.rs` does not exist. MAT3
  parses into fully typed enums (`bmd/mat3.rs:166-197`, `Material`) and
  `output.rs:134-257` writes it straight through. The only existing gate is
  the GX enum *vocabulary* (`bmd/mat3.rs:269-278`, `Ctx::gx` — an unknown byte
  is a parse error), which catches bad values, not unsupported features.
- **The manifest already carries everything the shader needs**
  (`src/model_manifest.rs:283-412`), so **no converter output change is
  required** and `scripts/link_converted.sha256` must come out of this phase
  byte-identical.
- **Fields parsed but dropped before the manifest, hence gateable only
  converter-side**: `TexMatrix::projection` / `map_mode` / `is_maya`
  (`bmd/mat3.rs:509-513`), `fog` (:442), `indirect` (:240),
  `post_tex_coord_gens` / `post_tex_matrices` (:395, :397),
  `TevStage::tev_mode` (:544). This is the concrete reason the gate belongs in
  the converter rather than the example.
- **Index-compaction hazard**: `output.rs:166`, `:201`, `:226` build
  `tev.stages`, `texgens` and `channels` with `.iter().flatten()`, which drops
  `None` slots and destroys slot-index correspondence with the sibling
  slot-indexed lists (`orders`, `swap_modes`, `kcsels`/`kasels`). It happens to
  hold today (`stages.len() == num_tev_stages` and
  `texgens.len() == num_tex_gens` on all 24) but nothing enforces it, and the
  interpreter indexes `stages[i]` alongside `kcsels[i]`.
- **`channels.len() == 4` on every material while `num_color_chans == 1`** —
  slots 1–3 are live JSON but junk; iterate by count, never by length.
- The existing `#[ignore]`d integration test convention
  (`bmd/mat3.rs:751`, `real_mat3_expectations`) is picked up automatically by
  every `just link-verify-*` recipe, all of which run
  `cargo test --bin convert_link -- --include-ignored`.

### Renderer / codegen affordances

- **Non-`.shader.slang` files in `shaders/source/` are auto-discovered as
  importable modules** (`src/shaders/build_tasks.rs:1322-1349`, which skips the
  two entry-point suffixes and reflects everything else) and each gets its own
  generated Rust file via `collect_shared_modules` (:1374). `mvp.slang` →
  `src/generated/shader_atlas/mvp.rs` is the worked example. Shared structs and
  functions must be declared `public`. **Only structs generate Rust** — free
  functions produce nothing, which is fine and expected.
- **Nested structs in uniform blocks are proven**: `ToonLinkParams.mvp` is a
  `MVPMatrices`.
- **`float4[N]` / `uint4[N]` / `int4[N]` array fields** are supported with
  compile-time offset/size proofs since the vec4-array mini-phase (`0d08a7d`,
  [`vec4_array_support.md`](vec4_array_support.md)). `toon_link` will be the
  **first production shader** to use them; the test shaders
  `shaders/test/std140_arrays.shader.slang` are the only prior users.
- **`Key` is a closed 13-variant enum** (`src/game/traits.rs:188-203`:
  W A S D Q E R F Space Num1–Num4). `toon_link` uses Num1–4 + Q/E/Space today,
  leaving W/A/S/D/R/F free — enough for P8 with no core-library change. The
  held-key intent pattern to copy is `examples/ray_marching.rs:100-122`.
- `examples/toon_link.rs` today: `alpha_compare_codes` at :127, `ToonLink` at
  :375 with `alpha_compares: Vec<AlphaCompareCodes>` at :388, isolation
  printout at :415, setup at :437, the pipeline loop building `alpha_compares`
  at :511-525, the startup legend at :541-553 (marked "NOTE keep in sync with
  the module doc comment", which is :1-16), `draw` at :567 with the uniform
  loop at :590, `input` at :605.
- `bmd/mat3_dump.rs::equation` (:409) already renders exactly the
  `reg = clamp?((d op lerp(a,b,c)) + bias) * scale` form the interpreter must
  implement, and `human_report` (:287) writes it into `mat3_dump.txt`. That
  file is the cross-check for every stage.

## Decisions (settled in planning, user-approved)

1. **The subset gate lives in the converter** (`src/bin/convert_link/tev_ir.rs`),
   as master plan §3 says, and it is **validation-only**: it builds a typed
   `TevMaterialDesc` from the parsed `Material`, hard-errors on anything
   outside the implemented set, and changes no output. This closes
   [`follow_up.md`](follow_up.md) §6's open question ("phase_02 says the IR
   lands in P6, the master plan says P8") — **P8 is the answer**. It runs on
   every `just convert-link`, with no new flag, so an unsupported feature can
   never reach the shader silently. Its own correctness gate is that
   `scripts/link_converted.sha256` comes out unchanged.
2. **`tev.slang` declares `public struct TevParams`; `ToonLinkParams` embeds it
   as a field.** The codegen then emits `src/generated/shader_atlas/tev.rs`
   containing a Rust `TevParams`, which is exactly the type `src/tev_pack.rs`
   produces — one struct, no hand-maintained mirror, and the compile-time
   offset asserts cover the whole layout. Rejected alternative: a lib-local
   `TevUniforms` mirror, i.e. two things to keep in sync. **Consequence:
   `src/tev_pack.rs` cannot compile until the shader exists and `just shaders`
   has run**, which is why Step 2 precedes Step 3. Contingency if the codegen
   turns out not to handle *arrays inside a nested struct* (proven separately,
   never together): declare the arrays directly in `ToonLinkParams`, keep
   `tev.slang` for functions only, and have `tev_pack` return a lib-local
   struct the example spreads. Decide at the first `just shaders` run, record
   which way it went.
3. **No bit-packing anywhere in `TevParams`.** One GX value per vector
   component even where two would fit. The entire risk of this phase is
   misencoding an interpreter's configuration; a packed field is unreadable in
   a debugger and unreadable against `mat3_dump.txt`. Uniform space is
   irrelevant — ~1.5 KB against the 16 KB guaranteed `maxUniformBufferRange`.
4. **The color channel and all texgens are evaluated in the vertex shader**;
   the fragment shader receives `COLOR0` and the texcoords as interpolants and
   runs only the TEV stages and alpha compare. This is what GX does — lighting
   and texgen are XF-unit, per-vertex, and noclip matches it — and on a
   1754-vertex model the difference is visible: per-fragment evaluation gives
   rounder, smoother bands than the hardware produces. Debug mode 8 recomputes
   the channel per-fragment so the two can be A/B'd against noclip cheaply.
5. **Implement the full GX vocabulary where it is a table lookup; gate only
   structurally distinct features.** Implemented in full even though cl.bdl
   uses a fraction of it: all 16 `GXTevColorArg` and 8 `GXTevAlphaArg`
   selectors, all `GXTevKColorSel`/`GXTevKAlphaSel` including the eight
   constant fractions, both `SUB` and `ADD`, all three biases, all four scales,
   all four swap channels, all three diffuse functions, all four TEV
   registers. Rejected by the gate with an actionable error: indirect stages,
   fog enabled, TEV comparison ops (`op >= 8`), texgen types other than
   `MTX2x4`/`SRTG`, texgen sources other than `TEX0`/`COLOR0`, texmatrix
   `projection != MTX2x4` or `map_mode != None` or non-unit scale or non-zero
   rotation, `mat_src`/`amb_src != Register`, any post-texgen or
   post-texmatrix present, `num_tev_stages > 8`, `num_tex_gens > 4`,
   `num_color_chans > 1`. Error format from master plan §3:
   `material {name}: unsupported {feature} — extend tev.slang + tev_ir.rs`.
6. **Batch isolation is reused as material isolation.** phase_07.md:89-91
   established that batches and material slots are bijective (24 ↔ 24, merely
   permuted), so P7's existing Q/E/Space keys already satisfy tests.md §P8's
   "single-material isolation" ask. P8 does not add a parallel
   `Option<MaterialSlot>`; it grows the printout to include the material's
   stage equations.
7. **Dolphin is out of scope.** P8's oracle is per-feature comparison against
   noclip plus the internal consistency checks below. The savestate/`.dff`
   capture, `just link-dolphin-refs`, the FIFO analyzer and the
   software-renderer replay move to [`follow_up.md`](follow_up.md) as an
   **optional** escalation, invoked only if a specific feature is genuinely in
   dispute. The honest cost: the S10 clamp edge cases (master plan risk #6) and
   the exact `dKy_tevstr_c` light values (risk #8) ship **reasoned, not
   measured**, and the Recorded facts must say so.

## Step 1 — converter: the subset gate

New `src/bin/convert_link/tev_ir.rs`, called from `main.rs` on every conversion
(before `output::build`, so a rejected model produces no files).

- `pub struct TevMaterialDesc` with a typed field per feature — stages, orders,
  texgens, texmatrices, channel, swap tables, konst/register colors — where
  every value is one of this phase's implemented enum variants. Construction is
  `TryFrom<&Material>`; the conversion *is* the gate.
- Error type carries the material name and the offending feature, formatted per
  decision 5. Reuse `BmdError`'s style; do not invent a second error idiom.
- **Also assert the dense-prefix invariant** that `output.rs`'s `.flatten()`
  silently depends on: for each material, the `Some` slots of `tev_stages`,
  `tex_coord_gens` and `color_channels` form a dense prefix of length
  `num_tev_stages` / `num_tex_gens` / `num_color_chans`. Without this the
  manifest's compacted `stages[i]` can drift out of step with its
  slot-indexed `orders[i]` / `kcsels[i]` and nothing would notice.
- Test `real_tev_subset_accepted`, `#[ignore]`d with the same message as
  `bmd/mat3.rs:751`, asserting all 24 real materials pass the gate and spot-
  checking `ear`'s three stage equations against the table above. It is picked
  up automatically by the existing `--include-ignored` verify recipes.

**Gate:** `just convert-link` succeeds and reports the accepted material count;
`git diff scripts/link_converted.sha256` and `git diff assets/link/converted`
both empty; `just link-verify-p2` and `just link-verify-p3` green; `just test`
unchanged; `just lint`.

## Step 2 — shader: `tev.slang` + rewritten `toon_link.shader.slang`

New `shaders/source/tev.slang`, module `tev`, everything shared declared
`public` (follow `mvp.slang`). Names must be unique across all of
`shaders/source/` — see risk 6.

```slang
public struct TevParams {
    uint4  stageColorIn[8];   // a, b, c, d              GXTevColorArg
    uint4  stageColorOp[8];   // op, bias, scale, clamp  GXTevOp/GXTevBias/GXTevScale
    uint4  stageAlphaIn[8];   // a, b, c, d              GXTevAlphaArg
    uint4  stageAlphaOp[8];   // op, bias, scale, clamp
    uint4  stageDest[8];      // colorReg, alphaReg, kcsel, kasel
    uint4  stageOrder[8];     // texcoord, texmap, rasChannel, 0   (0xFF = null)
    uint4  stageSwap[8];      // rasSel, texSel, 0, 0
    uint4  texgen[4];         // type, src, mtxSlot (0xFF = identity), 0
    uint4  swapTable[4];      // r, g, b, a channel selects        GX_TEV_SWAP0..3
    float4 konst[4];          // K0..K3, /255
    float4 reg[4];            // PREV, REG0, REG1, REG2 — see the reg_colors note
    float4 texMtxRows[8];     // 4 slots x 2 rows, MTX2x4
    float4 chanMatColor;      // material_colors[0], /255
    float4 chanAmbColor;      // ambient_colors[0], /255
    float4 lightDir;          // world space, example-supplied
    float4 lightColor;        // example-supplied (light_colors is null in the manifest)
    uint4  chanControl;       // lightingEnabled, diffuseFn, attnFn, litMask
    uint4  control;           // numStages, numTexgens, numChans, 0
}
```

`reg[0]` is **PREV's** initial value and the packer must leave it at the GX
default rather than reading `reg_colors[0]` — the manifest's four entries load
into REG0/REG1/REG2 and one unused slot, per the measured-facts note.

Interpreter functions, in evaluation order:

1. `evalChannel(...)` — GX color channel 0.
   `matColor` / `ambColor` from `chanMatColor` / `chanAmbColor` (both sources
   are `Register`; the gate rejects `Vertex`). If `lightingEnabled` is false
   the result is `matColor` unchanged. Otherwise
   `illum = ambColor + Σ_lights attn · diffuse · lightColor`, then
   `color = matColor · saturate(illum)`. Diffuse follows the GX function —
   `None → 1`, `Signed → dot(N, L)`, `Clamp → max(dot(N, L), 0)`. **Attenuation
   is forced to 1.0** for both `Spot` and `Specular`: our light is a hardcoded
   directional, so there is no position to attenuate from. That is a deliberate
   approximation, not an oversight — say so at the call site.
   `lit_mask` is 3 on every lit material, so **two** lights are needed; the
   example supplies both.
2. `evalTexGen(...)` — per texgen `i < numTexgens`, by type:
   `MTX2x4` takes `(u, v, 1, 1)` from `TEX0` through `texMtxRows[2·slot]` /
   `[2·slot+1]`, or passes it through when `mtxSlot == 0xFF`; `SRTG` produces
   the texcoord from the rasterized channel color as **`(color.r, color.g)`**.
3. `evalStage(...)` ×`numStages` — sample `texmap` at `texcoord` and apply
   `swapTable[texSel]`; take the ras channel and apply `swapTable[rasSel]`;
   resolve konst via `kcsel`/`kasel`; select `a`/`b`/`c`/`d`; compute
   `out = clamp ? saturate(v) : v` where
   `v = ((d op lerp(a, b, c)) + bias) · scale`; write to
   `stageDest.x`/`.y`. Null texmap/texcoord/channel (`0xFF`) must yield the GX
   defaults, not an out-of-bounds read.

`toon_link.shader.slang` then keeps `tex0`/`tex1`/`mvp`/`alphaCompare`/
`alphaCompareOp`/`debugMode` exactly as P7 left them — **the P7 alpha-compare
code and `srgbDecode` carry over unchanged** — and gains `TevParams tev`. The
descriptor shape does not change (P7 decision 1 paying off), but the uniform
shape does. `FragVertex` gains `float4 color0` and `float2 texcoord[4]`,
keeping `normal` and `uv0` for the debug modes.

**Gate:** `just shaders` succeeds and emits
`src/generated/shader_atlas/tev.rs`; `just test` green with snapshot churn
confined to `toon_link` plus the new `tev` module files, every other
per-shader snapshot byte-identical; `just lint`. Record the reflected
`TevParams` / `ToonLinkParams` sizes and offsets in Recorded facts, and record
whether decision 2's nested-array contingency was needed.

## Step 3 — lib: `src/tev_pack.rs`

New module registered in `src/lib.rs` beside `model_manifest`.
`pub fn pack(material: &mm::MaterialEntry) -> anyhow::Result<TevParams>`.

Things it has to get right, each of which is a test:

- **Compacted vs slot-indexed lists.** `stages`, `texgens` and `channels` are
  compacted (`output.rs:166`, `:201`, `:226`); `orders`, `swap_modes`,
  `kcsels`, `kasels` are slot-indexed. Walk by `num_tev_stages` /
  `num_tex_gens` / `num_color_chans`, never by `.len()`.
- **`reg_colors` → registers**: `reg[1..=3] = reg_colors[0..=2] / 255.0`,
  `reg[0]` (PREV) left at the GX default. Values are `i16` and may fall outside
  `[0, 255]`, so no clamping on the way in.
- **Texgen matrix code → slot**: `TEXMTXn = 30 + 3n`, `IDENTITY = 60` → `0xFF`.
- **Texmatrix composition** from `center` / `scale` / `rotation` /
  `translation` into two MTX2x4 rows. With the gate guaranteeing unit scale and
  zero rotation this reduces to a translate, but write the general composition
  and let the gate keep the untested branches unreachable.
- **Swap tables**: slots 0–3 into `swapTable[4]`, absent slots → identity
  `[0,1,2,3]`.
- The unit tests: one synthetic `MaterialEntry` fixture per field group, plus
  one test that packs `ear`'s real values (inlined as a fixture, not read from
  disk — the assets are gitignored and CI has none) and asserts the resulting
  selectors reproduce the three stage equations in the measured-facts table.

**Gate:** `just test` green with the new tests actually running — unlike
anything in `examples/`, which `cargo test` builds but never executes;
`cargo check --all-targets`; `just lint`.

## Step 4 — example: uniforms, light controls, debug modes

`examples/toon_link.rs`.

- Replace `alpha_compares: Vec<AlphaCompareCodes>` (:388) with a
  `Vec<TevParams>` built in the setup pipeline loop (:511-525) via
  `tev_pack::pack`, keeping the alpha-compare codes folded into the same
  per-material struct. The per-frame `write_uniform` loop (:590) fills in
  `mvp`, `lightDir`, `lightColor` and `debugMode`; everything else is static.
- **Light controls**, held-key intent per `examples/ray_marching.rs:100-122`:
  A/D rotate azimuth, W/S rotate elevation, integrated in `update`. Seed from
  master plan §5 — direction up-forward-left, `lightColor ≈ (1.0, 0.98, 0.92)`,
  the second light (mask bit 1) dimmer and from the opposite side, ambient
  already comes from the manifest's `[50,50,50,50]`. These are hand-tuned
  seeds; risk #8's ground-truth extraction stays deferred.
- **Debug modes** cycled with R/F, with Num1–Num4 as jump-to presets:

  | mode | view |
  |---|---|
  | 0 | final TEV output (default, Num1) |
  | 1 | world-space normals (Num2) |
  | 2 | uv0 (Num3) |
  | 3 | final alpha as grayscale (Num4) |
  | 4 | rasterized channel color `COLOR0` — the lighting before TEV |
  | 5 | SRTG texcoord as red/green |
  | 6 | raw `tex0` sample |
  | 7 | raw `tex1` sample at its own texgen coord |
  | 8 | channel recomputed per-fragment (decision 4's A/B) |

- **Isolation printout** (:415) grows to print the isolated material's stage
  equations in `mat3_dump.txt`'s notation, so a wrong material can be compared
  against the dump without leaving the window.
- Update the module doc comment (:1-16) and the startup legend (:541-553)
  together — the file already carries the "NOTE keep in sync" marker.

**Gate:** `timeout 3 just dev toon_link` renders with no validation output;
every mode and every control responds; isolation prints equations that match
`mat3_dump.txt`.

## Test plan

**Automated:**

- `just shaders`; `just test` (new `tev_pack` tests running, churn confined to
  `toon_link` + `tev`); `cargo check --all-targets`; `just lint` debug and
  release; `cargo fmt`.
- `just link-verify-p2` and `just link-verify-p3` green (they pick up the new
  `#[ignore]`d gate test automatically); `git diff
  scripts/link_converted.sha256` empty.

**Validation sweep** (the documented P4–P7 loop, not a recipe):

```sh
for e in $(ls examples | sed 's/\.rs$//'); do
  timeout 3 just dev "$e" 2>&1 | grep -iE "validation|VUID" && { echo "FAIL: $e"; exit 1; }
done; echo "sweep clean"
```

**Eyeball** ([`tests.md`](tests.md) §P8, per feature rather than gestalt —
results go into Recorded facts):

1. **noclip side-by-side** at P6's canonical angles, feature by feature: skin
   tone, the tunic's two-band boundary, the hair highlight, eye whites.
2. **Light rotation** — A/D/W/S sweeps the terminator. The bands must move
   smoothly and stay *banded*, never becoming a gradient. This is the sharpest
   test of the SRTG ramp path (master plan risk #5).
3. **Only the 12 lit materials may respond to the light.** Per the measured
   split, the eye and brow decals are `lighting_enabled: false` with no SRTG,
   so if they change under light rotation the channel is leaking somewhere it
   should not.
4. **Per-material isolation** — Q/E through all 24, each inspected alone with
   its equations printed and cross-checked against `mat3_dump.txt`. `ear` is
   the one to check first and in most detail; it exercises every mechanism in
   the phase (SRTG, two swap tables, three konst selects, three stages).
5. **Debug-mode triage before judging the final image**: mode 4 (`COLOR0`)
   must be smoothly shaded, mode 5 (SRTG texcoord) must be a plausible ramp
   coordinate, mode 8 must differ from mode 0 only in band smoothness.
6. **`eyeL`/`eyeR` pupils** — the `TEXMTX1` translate of `[-0.05, 0]` must
   offset the `hitomi` sample visibly and in the right direction; isolate those
   two batches and toggle the matrix off to confirm the direction rather than
   assuming it.
7. **Hot reload** of a `tev.slang` *body* edit across all 24 pipelines, raster
   state preserved; clean exit via a real window close with **no VMA leak**
   (`timeout`'s SIGTERM skips `Drop`, so this needs a manual close).
8. Known and not a bug: the eye/brow decals still stack, because BTP is not
   implemented (phase_07 risk 1). P8 does not fix it.

## Verification (exit checklist)

- [ ] `tev_ir.rs` gate runs on every conversion, rejects with the master-plan
      error format, and asserts the dense-prefix invariant
- [ ] `just link-verify-p2` / `-p3` green; `scripts/link_converted.sha256` and
      all of `assets/link/converted/` byte-identical
- [ ] `just shaders` green; `src/generated/shader_atlas/tev.rs` emitted;
      `just test` churn = `toon_link` + `tev` only
- [ ] `src/tev_pack.rs` unit tests run under `just test` and cover the
      compacted-vs-slot-indexed lists, the register shift, and `ear`'s equations
- [ ] `src/model_manifest.rs:341` `reg_colors` comment corrected
- [ ] `cargo check --all-targets`, `just lint` (debug + release), `cargo fmt` clean
- [ ] Cel bands visible and stable over a full orbit; noclip per-feature
      comparison recorded
- [ ] Light rotation sweeps the terminator; bands stay banded; only the 12 lit
      materials respond
- [ ] All 24 materials isolated and compared against `mat3_dump.txt`
- [ ] Pupil `TEXMTX1` offset confirmed by toggling, not assumed
- [ ] Validation sweep clean (16/16); hot reload of a `tev.slang` body edit
      clean; no VMA leak on a real window close
- [ ] Docs updated: master plan §6 P8 row ✅ + hash, risks #5/#6/#8;
      `tests.md` §P8; `follow_up.md` §5 (Dolphin optional) and §6 (`tev_ir.rs`
      reconciliation closed)
- [ ] Recorded facts filled in, explicitly naming what shipped
      reasoned-rather-than-measured

## Recorded facts

```
commit:

gate:                     (materials accepted; anything the gate rejected and
                          why; whether the dense-prefix invariant held)

reflection:               (TevParams size/offsets, ToonLinkParams size, whether
                          decision 2's nested-array contingency was needed,
                          frag branch count)

reg_colors mapping:       (confirmed in the render? the band should vanish if
                          the shift is wrong — say which way it was verified)

cel bands vs noclip:      (per feature: skin, tunic boundary, hair highlight,
                          eye whites)

light rotation:           (terminator sweep; bands stayed banded?; did any
                          unlit material respond?)

SRTG (r,g) read:          (did the diagonal read of the 256x256 ramp behave? if
                          it needed changing, what to)

per-vertex channel:       (mode 0 vs mode 8 difference; which matched noclip)

pupil TEXMTX1:            (offset direction confirmed by toggling)

isolation pass:           (all 24 vs mat3_dump.txt)

sweep / hot reload / VMA:

reasoned, not measured:   (S10 clamp semantics, risk #6; dKy_tevstr_c light
                          values, risk #8 — both would need the Dolphin
                          escalation in follow_up.md)

deviations discovered:
```

## Out of scope for P8

- **The Dolphin oracle suite** — savestate + `.dff` capture,
  `just link-dolphin-refs`, FIFO analyzer TEV state, software-renderer replay,
  dolphin-memory-engine lighting extraction. Moved to
  [`follow_up.md`](follow_up.md) as an optional escalation (decision 7).
- **BTP eye/brow frame animation** — the 34 unreferenced texture entries stay
  unloaded, and the decals keep stacking.
- **`BlendMode::DstAlpha` and the eye write-mask multi-pass**, **`--casual`
  clothes**, **BCK-sampled pose** (P9, master plan §4.5).
- **Indirect stages, fog, post-texgens, post-texmatrices, TEV comparison ops,
  vertex-sourced channel colors, >1 color channel** — all gate-rejected, all
  absent from cl.bdl.
- **Runtime skinning** — the pose stays baked (master plan settled decision 4).
- **CPU reference TEV evaluator** — tests.md lists it as optional. The
  `tev_pack` unit tests cover the packing half of it; write the evaluating half
  only if pixel-chasing actually gets hard, and record that it was needed.

## Risks / open questions

1. **SRTG channel → texcoord semantics.** The interpreter reads
   `(color.r, color.g)`, and since `matColor` is white the two are equal, so
   the 256×256 `ZBtoonEX` ramp is sampled along its diagonal. This is the
   least-verified assumption in the phase and the **first suspect** if the
   bands are wrong, doubled, or absent. Debug mode 5 exists specifically to
   triage it.
2. **`reg_colors` register shift.** Documented above and traced to
   `J3DMatBlock.cpp:810-811`, but the wrong reading is *silent* — stage 0
   degenerates to a no-op and the model just looks flat. The `tev_pack` test on
   `ear` is what keeps it honest.
3. **S10 clamp semantics** (master plan risk #6). Two stages run with the clamp
   bit off, and with the software-renderer tiebreaker out of scope the
   implementation is reasoned, not measured. Low practical risk — every op is
   ADD at scale 1, so values stay near range — but record it as unmeasured
   rather than implying it was checked.
4. **Lighting ground truth** (risk #8). Hand-tuned daytime seeds, two lights,
   attenuation forced to 1. Any noclip color mismatch could be our TEV math
   *or* our light values, and P8 has no way to tell them apart. Prefer
   adjudicating on band *structure* (which the light values do not affect)
   over band *color* (which they do).
5. **`ToonLinkParams` shape change** → `assert_shader_interface_unchanged`
   panics if the struct is edited while `just dev` runs. `just shaders` +
   restart. Body edits in `tev.slang` still hot-reload, which is most of the
   iteration in this phase.
6. **Two shared-module hazards.** `collect_shared_modules`
   (`src/shaders/build_tasks.rs:1374`) panics if two shaders disagree on a
   shared struct's layout, and `reflect_shared_module_types` is silent
   last-write-wins on duplicate struct names across modules
   ([`follow_up.md`](follow_up.md) §5b's neighbour, `llm_notes/tech_debt.md`
   §4). So `TevParams` and every helper struct name in `tev.slang` must be
   unique across all of `shaders/source/`.
7. **First production use of the vec4-array codegen.** Mitigated by the
   compile-time offset/size proofs — a layout error is a build failure, not
   silent GPU-data corruption. The untested combination is *arrays inside a
   nested struct*; decision 2 records the contingency.
8. **Assets stay machine-local**, so `toon_link` is not CI-verifiable
   ([`follow_up.md`](follow_up.md) §5). Every runtime gate here means something
   only on a machine where `just convert-link` has run.
