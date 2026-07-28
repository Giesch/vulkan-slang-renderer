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

> **Corrections applied during implementation.** Eleven claims below and in
> Step 2 turned out to be wrong; they are corrected **in place** in the sections
> that follow, and the full list with evidence is in
> [Recorded facts](#recorded-facts) under *deviations discovered*. The ones that
> would have been build failures or silent wrong renders: `float2 texcoord[4]`
> in a varying is rejected by the reflection; `texMtxRows[8]` indexed by
> texmatrix slot cannot address `TEXMTX9`; the `color_channels` dense-prefix
> assertion as specified rejects all 24 materials (`num_color_chans` counts
> channel *pairs*); the alpha compare cannot "carry over unchanged"; one
> `lightDir` is not enough for `lit_mask == 3`.

### The material set splits cleanly in two

All 24 materials fall into exactly two groups, and the split is total — the
same 12 names every time:

| | lit + SRTG group | unlit decal group |
|---|---|---|
| materials | `ear`, `face`, `mouth`, `podA`, `sleeve`, `ear(2)`…`ear(8)` | `eyeL/R`, `eyeL/RdamA`, `eyeL/RdamB`, `mayuL/R`, `mayuL/RdamA`, `mayuL/RdamB` |
| `channels[0].lighting_enabled` | `true`, `lit_mask: 3` (lights 0+1) | `false`, `lit_mask: 2` |
| SRTG texgen | yes | no |
| `tev.orders[*].channel` | `255` (`COLOR_NULL`) on every stage | `4` (`COLOR0A0`) on the 10 one-stage materials; **`255` on `eyeL`/`eyeR`** |
| RASC / RASA in stage inputs | never | 10 use RASC, 6 use RASA — all of them in the `channel: 4` group |

**Correction (measured):** the `channel: 4` row is not the whole unlit group.
`eyeL`/`eyeR` are unlit but use `COLOR_NULL` like the lit group, so the raster
channel is read by exactly the 10 one-stage materials. The conclusion below is
unaffected but its *reason* differs per material: `eyeL`/`eyeR` cannot respond to
the light because they never read the raster channel at all, while the brows
cannot respond because their channel is unlit.

**Also measured:** `channels[0]` is GX_COLOR0 and `channels[1]` is GX_ALPHA0 —
MAT3 stores the four slots as (color0, alpha0, color1, alpha1) *pairs*, which is
why `num_color_chans == 1` still means two live slots. The rasterized color TEV
sees is therefore `float4(COLOR0.rgb, ALPHA0.a)`. ALPHA0 is unlit on all 24, so
RASA is just the material alpha (1.0) and this makes no visible difference on
cl.bdl — but it is the correct model, it costs one extra call, and getting it
wrong would matter the moment a lit alpha channel appeared.

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
  `(12 = K0, 31 = K3_A)`. On the three-stage family the per-stage sequence is
  `kcsels [12, 12, 13]` / `kasels [28, 31, 28]` — so **stage 1's alpha selector
  is `K3_A`, not `K0_A`** as the worked example below originally said.
  `konst_colors[3]` is white, so the *value* is identical and only the selector
  distinguishes them; `tev_pack`'s `ear_end_to_end` test asserts the selector.
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
| 1 | tc0 / tm0 (the albedo) | tex_sel 0 → identity | `PREV = TEXC · CPREV`; `PREV.a = K3_A · TEXA` |
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
  `tex_coord_gens` and `color_channels` form a dense prefix. Without this the
  manifest's compacted `stages[i]` can drift out of step with its
  slot-indexed `orders[i]` / `kcsels[i]` and nothing would notice.

  **Correction — the three lists need two different policies.** As specified
  (prefix of length `num_color_chans`) this rejects **all 24 materials**:
  `channels.len()` is 4 on every one of them while `num_color_chans` is 1,
  because the count is of channel *pairs*. As shipped:

  - `tev_stages` / `tex_coord_gens`: dense prefix of `num_tev_stages` /
    `num_tex_gens`, **plus a tail check** that every later slot is `None`. The
    tail check is the half that actually matters — a populated slot past the
    count makes `output.rs`'s compacted list longer than the count and shifts
    every sibling index.
  - `color_channels`: dense prefix of `2 * num_color_chans`, **prefix only, no
    tail check**. Slots 1–3 being live is exactly what makes ALPHA0 readable.
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

**As shipped** (1328 bytes, every field 16-aligned so the codegen emits no
padding). Four fields differ from the original sketch; see the notes below.

```slang
public struct TevParams {
    uint4  stageColorIn[8];   // a, b, c, d              GXTevColorArg
    uint4  stageColorOp[8];   // op, bias, scale, clamp  GXTevOp/GXTevBias/GXTevScale
    uint4  stageAlphaIn[8];   // a, b, c, d              GXTevAlphaArg
    uint4  stageAlphaOp[8];   // op, bias, scale, clamp
    uint4  stageDest[8];      // colorReg, alphaReg, kcsel, kasel
    uint4  stageOrder[8];     // texcoord, texmap, rasChannel, 0   (0xFF = null)
    uint4  stageSwap[8];      // rasSel, texSel, 0, 0
    uint4  swapTable[4];      // r, g, b, a channel selects        GX_TEV_SWAP0..3
    uint4  texgen[2];         // type, src, raw GX matrix code (60 = IDENTITY), 0
    float4 texgenMtx[4];      // 2 composed MTX2x4 rows per texgen
    float4 konst[4];          // K0..K3, /255
    float4 reg[4];            // PREV, REG0, REG1, REG2 — see the reg_colors note
    float4 lightDir[2];       // world space, toward the light, example-supplied
    float4 lightColor[2];     // example-supplied (light_colors is null in the manifest)
    uint4  chanControl[2];    // [0] COLOR0, [1] ALPHA0: lit, diffuseFn, attnFn, litMask
    float4 chanMatColor;      // material_colors[0], /255 — one register per pair
    float4 chanAmbColor;      // ambient_colors[0], /255
    uint4  control;           // numStages, numTexgens, numChanPairs, 0
}
```

Four corrections to the sketch, all forced:

1. **`texgen[4]` → `texgen[2]`, and `texMtxRows[8]` → `texgenMtx[4]`.** The
   sketch indexed the matrix rows by texmatrix *slot*, but GX slots run
   `TEXMTX0..TEXMTX9`, so four slots cannot address `TEXMTX9` — the field was
   unusable as specified. Storing two composed rows **per texgen** removes the
   slot-index space entirely, moves the `(code − 30) / 3` arithmetic into
   unit-tested Rust, handles two texgens sharing a slot, and is smaller. The raw
   GX code stays in `texgen[i].z` so it is still readable against
   `mat3_dump.txt`.
2. **Two texgens, not four.** `FragVertex` cannot carry `float2 texcoord[4]`:
   `fragMain`'s parameter struct *is* reflected, and
   `src/shaders/reflection/parameters.rs`'s `TypeKind::Array` arm bails on any
   array field whose binding is not `Uniform`. The coords are packed into a
   single `float4 texcoord01` varying and the gate caps `num_tex_gens` at 2
   (measured max is 2). Widening means adding a `texcoord23` varying.
3. **Two lights.** `lit_mask` is 3, so one `lightDir`/`lightColor` cannot
   express the data. The gate rejects `lit_mask & !0x3 != 0`; going to GX's full
   eight is a one-line array resize plus the gate constant.
4. **Two channel controls.** `chanControl[0]` is COLOR0 and `[1]` is ALPHA0,
   per the channel-pair correction above. They share `chanMatColor` /
   `chanAmbColor` because `GXSetChanMatColor` takes `GX_COLOR0A0` — one RGBA
   register for the pair.

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
   `v = ((d op lerp(a, b, c)) + bias) · scale` and clamp it; write to
   `stageDest.x`/`.y`. Null texmap/texcoord/channel (`0xFF`) must yield the GX
   defaults, not an out-of-bounds read.

   **Correction — the clamp.** `out = clamp ? saturate(v) : v` is wrong:
   clearing GX's clamp bit does not mean "no clamp", it means clamp to the S10
   *register* range. As shipped: `clamp ? saturate(v) : clamp(v, -1024/255,
   1023/255)`, matching noclip. This branch is genuinely reachable —
   `eyeL`/`eyeR` stage 1 runs unclamped on both halves — though its values stay
   inside `[0,1]` anyway, so it is visually inert here. Still **unmeasured**
   (risk #6).

   **Correction — input ordering.** Both halves must be computed from the
   *pre-write* register file. GX latches every input at stage start, so writing
   the color result before evaluating the alpha would corrupt any stage whose
   alpha reads the register its color just overwrote. cl.bdl never hits this
   (every stage writes PREV and no stage's alpha reads CPREV in the same stage),
   but it is one line to get right and invisible to get wrong.

`toon_link.shader.slang` then keeps `tex0`/`tex1`/`mvp`/`alphaCompare`/
`alphaCompareOp`/`debugMode` and gains `TevParams tev`. The descriptor shape
does not change (P7 decision 1 paying off — still one uniform + two samplers),
but the uniform shape does. `FragVertex` gains `float4 color0` and
`float4 texcoord01`, keeping `normal` and `uv0` for the debug modes.

**Correction — the alpha compare does *not* carry over unchanged.** Two things
move:

- The test must run on the **TEV output alpha**, not on a raw `tex0` texel.
  That is GX's order, and it restructures `fragMain` into TEV → discard → debug
  switch. On cl.bdl the test *result* is unchanged (every alpha-tested material
  — `eyeL`, `eyeR`, `mayuL`, `mayuR`, all `Greater 0` — has a final alpha that
  reduces to `TEXA` from texmap 0), so this is behavior-preserving here but
  correct in general.
- The shader must **stop returning `1.0`** as its alpha. Four materials
  (`eyeLdamA`, `eyeRdamA`, `mayuLdamA`, `mayuRdamA`) are
  `Blend / Source_Alpha / Inverse_Source_Alpha`, so a hardcoded 1.0 silently
  turns their alpha blend into an opaque write. This one is a real behavior
  change.

`srgbDecode`, `gxCompare` and `gxAlphaOp` do carry over unchanged.

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

- [x] `tev_ir.rs` gate runs on every conversion, rejects with the master-plan
      error format, and asserts the dense-prefix invariant (under the corrected
      per-list policy — see the Step 1 note)
- [x] `just link-verify-p2` / `-p3` green; `scripts/link_converted.sha256` and
      all of `assets/link/converted/` byte-identical
- [x] `just shaders` green; `src/generated/shader_atlas/tev.rs` emitted;
      `just test` churn = the five predicted files (`toon_link` + `tev` plus
      `shader_atlas.rs` and the shared branching snapshot), each diff reviewed
      before accepting
- [x] `src/tev_pack.rs` unit tests run under `just test` and cover the
      compacted-vs-slot-indexed lists, the register shift, and `ear`'s equations
      (16 tests)
- [x] `src/model_manifest.rs` `reg_colors` comment corrected
- [x] `cargo check --all-targets`, `just lint` (debug + release), `cargo fmt` clean
- [~] Cel bands visible and stable over a full orbit — **yes, and measured**
      (shadow (45,89,37) vs lit (250,255,74) in one region, with the
      white-albedo shadow band at exactly REG0's (128,128,128)). The **noclip
      per-feature comparison is NOT done** and is the main outstanding item
- [x] Light rotation sweeps the terminator; bands stay banded; only the 12 lit
      materials respond (the last part structurally as well as observationally —
      `evalChannel` returns `matColor` before touching a light when lighting is
      off, and the gate test asserts SRTG and lighting coincide on all 24)
- [x] All 24 materials isolated and compared against `mat3_dump.txt` — driven
      mechanically, 24 compared, 0 mismatched
- [ ] Pupil `TEXMTX1` offset confirmed by toggling, not assumed — **still
      open.** The packing is proven numerically and debug mode 9 exists, but the
      on-screen direction was not observed (isolated-decal screenshots proved
      unreliable; see the tooling note in Recorded facts)
- [x] Validation sweep clean (16/16); hot reload of a `tev.slang` body edit
      clean (2 events × 24 pipelines); no VMA leak on a real window close (via a
      genuine `WM_DELETE_WINDOW`, not SIGTERM)
- [x] Docs updated: master plan §5 (stale `setLightTevColorType` sentence), §6
      P8 row, risks #5/#6/#8; `tests.md` §P8; `follow_up.md` §5 and §6
- [x] Recorded facts filled in, explicitly naming what shipped
      reasoned-rather-than-measured
- [ ] Mode 0 vs mode 8 (per-vertex vs per-fragment channel) compared —
      **still open**, needs the same side-by-side as the noclip pass

## Recorded facts

Implemented and verified on 2026-07-27, on the development machine (Pop!_OS /
COSMIC Wayland, RTX 3070 Ti + Intel Xe, converted assets present). P7's
outstanding runtime gates were closed out first — see
[`phase_07.md`](phase_07.md)'s second Recorded-facts block; the headline there is
that the **sRGB transfer direction is now measured** (0 LSB on four colors),
which is what any P8 color claim rests on.

```
commit:                   (this commit)

step 0b (new):            The 14 TEV/texgen gx_enum!s moved from
                          convert_link's gx/types.rs into the library's
                          model_manifest.rs (re-exported from the old path, so
                          every `crate::gx::types::` path still resolves).
                          Nothing serialized changed -- the manifest carries
                          these as raw u8 -- and `just link-verify-mat3` still
                          diffs zero lines against the gclib oracle. This is
                          what lets tev_pack parse-don't-validate the bytes on
                          their way to the GPU *and* print equations whose
                          spellings cannot drift from mat3_dump.txt.

gate:                     All 24 materials accepted; nothing rejected. The
                          dense-prefix invariant holds, but only under the
                          corrected policy: tev_stages and tex_coord_gens are
                          exact (dense prefix + empty tail), while
                          color_channels is prefix-only over 2*num_color_chans
                          -- all four slots are live on all 24 while
                          num_color_chans is 1. `just convert-link` reports
                          "24 materials (24 passed the TEV subset gate)".
                          scripts/link_converted.sha256: all 90 hashes match,
                          `git status assets/` empty, link-verify-p2 and -p3
                          both VERIFIED. 92 convert_link tests pass including
                          the ignored real-asset one.

reflection:               TevParams 1328 bytes, ToonLinkParams 1552. Offsets
                          landed exactly as designed and **no _padding_N field
                          was emitted** in TevParams (every field 16-aligned
                          and a multiple of 16); ToonLinkParams keeps its one
                          trailing 8-byte pad. Decision 2's nested-array
                          contingency was **not needed** -- arrays inside a
                          nested std140 struct work, as shaders/test's
                          `Nested { uint4 inner[2] }` already implied.
                          Branch counts: toon_link.frag.spv 7 -> 80,
                          toon_link.vert.spv 0 -> 11.
                          Descriptor shape unchanged at 1 constantBuffer + 2
                          combinedTextureSampler, so P7 decision 1 paid off
                          exactly as intended: no descriptor change in P8.
                          Snapshot churn was the five predicted files
                          (toon_link.json, toon_link.rs, new tev.rs,
                          shader_atlas.rs gaining `pub mod tev;`, and the shared
                          shader_branching snapshot) -- reviewed line by line
                          before accepting; the atlas diff is one added line.

reg_colors mapping:       Confirmed three ways, and the decomp trace is the
                          strongest. (1) `../tww/.../J3DMatBlock.cpp`:
                          `loadTevColor(reg, c)` is
                          `J3DGDSetTevColorS10(GXTevRegID(reg + 1), c)`, and
                          `patchTevReg`'s loop runs to ARRAY_SIZE - 1, with
                          `GXTevRegID { GX_TEVPREV=0, GX_TEVREG0=1, ... }` at
                          GXEnum.h:327 -- so entry i loads register i+1 and
                          entry 3 is never loaded. (2) The data agrees:
                          reg_colors[3] is [0,0,0,0] on all 24.
                          (3) In the render: `ear` stage 0 is
                          `mix(C0, K0, ramp_r)` with C0 = REG0 = mid-gray 128,
                          and the shadow band renders at exactly (128,128,128)
                          on white-albedo geometry (measured in a frame). Under
                          the unshifted reading C0 would be white and stage 0
                          would be `mix(white, white, ramp)` -- a no-op with no
                          other symptom. Pinned by tev_pack's
                          `register_colors_shift_by_one` and `ear_end_to_end`.

cel bands vs noclip:      Bands render, and they are unambiguously *banded*:
                          the tunic shows two discrete values with a sharp
                          terminator, not a gradient. Measured in one frame, the
                          same screen region reads (45,89,37) in shadow and
                          (250,255,74) lit, with the white leggings' shadow band
                          at exactly (128,128,128) = REG0.
                          **The per-feature noclip side-by-side is NOT done**
                          and is the main outstanding item -- see below.
                          One honest observation that the comparison will have
                          to adjudicate: the lit band is strongly *yellow*.
                          Traced, not guessed: stage 2 adds
                          konst1 = (160,90,0)/255 weighted by the ramp's G
                          channel, and because the ramp's two axes step at
                          nearly the same place (~0.49) and our light is
                          near-neutral, r ~= g, so G saturates wherever R does
                          and the warm add covers the whole lit band rather
                          than a sub-band. Debug mode 5 measures the ramp coord
                          on the lit tunic as (193, 190, 0) -- confirming
                          r ~= g. Two candidate explanations, neither settled
                          here: our light seeds are brighter/more neutral than
                          the game's, or `setLightTevColorType` overwrites K1
                          per frame with an environment tint (which is what
                          master plan §5 always claimed and P8 deliberately does
                          not do, following noclip). Adjudicating needs either
                          the noclip comparison or risk #8's ground truth.
                          **No konst or reg value was tuned to make the picture
                          look better** -- they are the manifest's, verbatim.

                          **RESOLVED 2026-07-27, by reading ../tww.** The first
                          explanation, and it is not a matter of degree: the two
                          GX lights are *single-channel by construction*.
                          Light 0 is red-only -- d_kankyo.cpp:1494-1499 sets
                          mColor.r and :1545-1547 hard-zero g and b, repeated in
                          dKy_tevstr_init at :3410-3412. Light 1 is green-only
                          and exists only near an "eflight" (torch, sword glow),
                          :2557-2559, gated by lightMask = 1 with no eflight vs
                          3 with one (:2527-2531). That is *why* the ramp is
                          separable: red carries the diffuse term, green carries
                          the eflight term, and SRTG's (color.r, color.g) is two
                          independent lookups. With ambient 50/255 on every
                          channel and no eflight, color.g == 0.196 < 0.49
                          forever, so ramp.G is 0 and stage 2 contributes
                          *exactly nothing*. The game belt-and-braces it:
                          setLightTevColorType_sub (:1764-1787) forces
                          setLightMask(1) and calls setTevStageNum to drop the
                          kcsel==13 stage outright unless mColorK1.a != 0.
                          The canonical C-source spelling of this same shader is
                          dCloth_packet_c::TevSetting, d_cloth_packet.cpp:395-437
                          -- SRTG from COLOR0, SWAP1 = RRRA on stage 0, SWAP2 =
                          GGGA on the optional stage 2, numStages 3 and lightMask
                          3 iff mColorK1.a != 0. It matches sleeve stage for
                          stage. The example now ships LIGHT0_COLOR = (1,0,0)
                          and LIGHT1_COLOR = (0,0,0), with T toggling the
                          eflight. Still no konst or reg value tuned by eye.

light rotation:           Terminator sweeps, bands stay banded. Verified by
                          driving the window with synthetic held keys and
                          diffing frames: holding A rotates the azimuth and the
                          same screen region flips from the shadow value
                          (45,89,37) to the lit value (250,255,74) -- two
                          discrete values, no intermediate gradient.
                          Only lit materials respond: `mayuL` isolated and
                          light-rotated is pixel-identical, and more strongly,
                          `tev.slang`'s evalChannel returns matColor before
                          touching any light when lightingEnabled is 0, while
                          tev_ir's real-asset test asserts that SRTG and
                          lighting coincide on all 24. (The isolated-decal
                          screenshot comparison is weak evidence on its own --
                          see the tooling caveat below.)

SRTG (r,g) read:          Correct and unchanged. The ramp turned out to be
                          separable (R along u, G along v, B = 0), which makes
                          the diagonal read exactly right rather than merely
                          adequate -- see risk #1, rewritten. Confirmed at
                          runtime by debug mode 5.

per-vertex channel:       Mode 4 (COLOR0) is smoothly shaded as required --
                          499 distinct values across the tunic, no banding
                          before TEV, which is the precondition for the ramp
                          doing the banding. Mode 8 (channel per-fragment) is
                          implemented and switches, but **mode 0 vs mode 8 was
                          not compared side by side**, and neither was compared
                          against noclip. Outstanding.

pupil TEXMTX1:            **NOT confirmed by toggling.** The packing is proven
                          numerically -- tev_pack's texgen_matrix_code_to_rows
                          and eye_l_stage1_is_unclamped assert
                          texgen_mtx[2] == (1, 0, -0.05, 0), and the exact -0.05
                          survives f32 (see the deviation below) -- and debug
                          mode 9 exists to force the matrix to identity. But the
                          on-screen *direction* was not observed, because
                          screenshotting an isolated 12-triangle decal proved
                          unreliable here (see the tooling caveat). The plan
                          asks for this to be confirmed rather than assumed, so
                          it stays open.

isolation pass:           **All 24, done mechanically rather than by eye.** The
                          window was driven through all 24 batches with
                          synthetic E keypresses, the printouts captured, and
                          every material's stage-equation, stage-order and
                          texgen lines diffed against the corresponding block of
                          assets/link/converted/mat3_dump.txt: 24 materials
                          compared, 0 mismatched. The printout adds the two
                          things the dump does not carry -- the resolved
                          kcsel/kasel (the dump prints a bare KONST) and the
                          swap-table contents -- which is precisely where this
                          plan's own worked example was wrong.

sweep / hot reload / VMA: Validation sweep 16/16 clean, with
                          VK_LAYER_KHRONOS_validation confirmed loaded so the
                          silence means something. Hot reload of a *tev.slang
                          body* edit (nudging TEV_S10_MIN and putting it back):
                          2 recompile events x 24 "finished recompiling shaders"
                          each, no errors, no interface assert, app survives.
                          Clean close via a real WM_DELETE_WINDOW ClientMessage
                          (not SIGTERM, so Drop actually runs): exit 0, no VMA
                          leak report, no validation error at device destroy.
                          just test / cargo check --all-targets / clippy debug
                          and release / cargo fmt all clean.

reasoned, not measured:   1. **S10 clamp semantics** (risk #6). Implemented as
                             clamp-to-[-1024/255, 1023/255] when the clamp bit
                             is clear, matching noclip; no software-renderer
                             capture was taken. Reachable but inert on cl.bdl:
                             eyeL/eyeR stage 1 is the only unclamped stage and
                             its values stay in [0,1].
                          2. **dKy_tevstr_c light values** (risk #8). Hand-tuned
                             seeds, two lights, attenuation forced to 1.0. They
                             are constrained rather than arbitrary -- the ramp's
                             ~0.49 terminator and the manifest's 0.196 ambient
                             pin roughly where light 0 has to sit -- but they are
                             not the game's values, and the yellow lit band above
                             is the visible consequence.
                          3. **The texmatrix rotation/scale branches.** The
                             general SRT composition is written and unit-tested,
                             but every cl.bdl matrix has unit scale and zero
                             rotation, so those branches are gate-unreachable
                             and unverified against the game. The rotation unit
                             (s16, pi/32768 per step) is the J3D/noclip
                             convention; gclib's `u16Rot` carries no conversion
                             to check it against.

tooling (reusable):       This machine is Wayland/COSMIC, so the X11 root is
                          black and ffmpeg x11grab captures nothing. What works:
                          `cosmic-screenshot --interactive=false --modal=false
                          --notify=false -s DIR`, plus python-xlib XTEST
                          fake_input against a window found by WM_NAME with the
                          example run under SDL_VIDEODRIVER=x11. Two caveats
                          learned the hard way: (a) the screenshot portal
                          appears to close the app's window, so a whole
                          capture sequence must run inside one script rather
                          than across several tool calls; (b) captures of an
                          *isolated* small decal came back as stale identical
                          frames, so this path is trustworthy for whole-model
                          numeric sampling and for stdout-driven checks, but not
                          for small-region visual diffs. The isolation sweep
                          above works because it compares *printouts*, not
                          pixels.

deviations discovered:    Eleven corrections to this document, all applied in
                          place above, plus three implementation findings.

                          Corrections that would have been build failures or
                          silent wrong renders:
                          1. `float2 texcoord[4]` in FragVertex is rejected by
                             the reflection (arrays need a uniform binding), so
                             the coords are packed into one float4 and the
                             texgen cap is 2, not 4.
                          2. `texMtxRows[8]` indexed by texmatrix slot cannot
                             address TEXMTX9; rows are stored per texgen.
                          3. The color_channels dense-prefix assertion as
                             written rejects all 24 materials --
                             num_color_chans counts channel *pairs*, so
                             channels[0] is COLOR0 and channels[1] is ALPHA0.
                          4. "The P7 alpha-compare code carries over unchanged"
                             is false: the test moves to the TEV output alpha
                             and the shader must stop writing 1.0, which
                             genuinely changes four materials' blending.
                          5. One lightDir/lightColor cannot express lit_mask 3.
                          6. `out = clamp ? saturate(v) : v` should clamp to the
                             S10 range in the else branch.

                          Smaller corrections:
                          7. `ear` stage 1's kasel is K3_A (31), not K0_A.
                          8. The unlit group's raster channel is not uniformly
                             COLOR0A0 -- eyeL/eyeR use COLOR_NULL.
                          9. `TryFrom<&Material>` cannot produce the mandated
                             `material {name}: ...` text, because Material has
                             no name field; the gate takes `(&str, &Material)`.
                         10. The gate has to run before `tex1::emit`, not merely
                             before `output::build` -- textures and
                             mat3_dump.txt are written earlier than the plan
                             assumed. (It still runs after the --dump-* early
                             returns, so a rejected model can still be dumped.)
                         11. Churn is five snapshots, not two: the shared
                             shader_branching snapshot and shader_atlas.rs also
                             move.

                          Implementation findings:
                         12. A validation-only typed IR trips `-D warnings`;
                             tev_ir.rs carries `#![allow(dead_code)]` with a
                             comment saying why.
                         13. `bmd::mat3_dump::equation` lives inside the
                             convert_link *binary*, so neither the library nor
                             an example can call it, and mat3_dump.txt is
                             covered by the golden hashes so it must not move.
                             The renderer is therefore re-implemented in
                             tev_pack, with tests pinning the literal strings
                             against the real dump.
                         14. The texmatrix composition loses f32 bits on the one
                             path that ships: `translation + center -
                             R*S*center` is algebraically exact when R*S = I but
                             not in floating point, so eyeL's -0.05 came out as
                             -0.050000012. The identity-linear case is now
                             special-cased so the shipped translate is exact.

outstanding:              The per-feature noclip side-by-side (skin, tunic
                          boundary, hair highlight, eye whites), the mode 0 vs
                          mode 8 comparison, and the pupil-direction toggle.
                          All three need a human looking at two images; the
                          numeric and structural gates around them are done.
                          Known and not a bug: the eye/brow decals still stack
                          and paint an opaque black quad -- traced in phase_07's
                          Recorded facts to eyeLdamB's `Always` alpha compare
                          plus `None_` blend over an all-(0,0,0,0) texture, i.e.
                          missing BTP plus P9's deferred DstAlpha pass. P8 does
                          not fix it and was never going to.
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

1. ~~**SRTG channel → texcoord semantics.**~~ — **largely resolved before
   implementation, by decoding the ramp.** `tex/raw_toonex.png` is 256×256 RGBA
   and **separable**: R varies only with u, G only with v, B is 0, A is 255
   (sampling every 3rd pixel, 58 of ~7000 deviate, all by ≤2 LSB inside the
   transition). Both channels are sharp steps — R goes 0→255 over x ∈ [117,137]
   and G over y ∈ [115,141], i.e. a terminator at ≈0.49 in each axis. So the
   `(color.r, color.g)` read is not just right, it is *robust*: stage 0's RRR
   swizzle reads f(color.r) and stage 2's GGG swizzle reads g(color.g)
   independently, and the sampler's `ClampToEdge` makes out-of-range channel
   values harmless.

   Confirmed at runtime by debug mode 5, which reads `(193, 190, 0)` on the lit
   tunic — r ≈ g as predicted, B exactly 0, both well past the 0.49 step.

   ~~**What this hands to risk #8 instead**~~ — **answered 2026-07-27.** The
   note here read: because the two axes have nearly identical thresholds and our
   light is near-neutral, r ≈ g, so both ramp channels saturate together and
   stage 2's warm `(160,90,0)` highlight fires over the *entire* lit band. It
   then guessed that "a 2D separable ramp only buys separate thresholds when the
   light color is distinctly non-neutral."

   That guess was too weak. The game's lights are not merely non-neutral, they
   are **one channel each** — light 0 red-only, light 1 green-only, so the two
   axes are wired to two *different lights* rather than to a hue. Separability is
   the whole design, not a happy accident of the texture. Full trace in the
   *cel bands vs noclip* entry in Recorded facts.
2. **`reg_colors` register shift.** Documented above and traced to
   `J3DMatBlock.cpp:810-811`, but the wrong reading is *silent* — stage 0
   degenerates to a no-op and the model just looks flat. The `tev_pack` test on
   `ear` is what keeps it honest.
3. **S10 clamp semantics** (master plan risk #6). Two stages run with the clamp
   bit off, and with the software-renderer tiebreaker out of scope the
   implementation is reasoned, not measured. Low practical risk — every op is
   ADD at scale 1, so values stay near range — but record it as unmeasured
   rather than implying it was checked.
4. ~~**Lighting ground truth**~~ (risk #8) — **largely closed 2026-07-27.** The
   entry read: hand-tuned daytime seeds, two lights, attenuation forced to 1;
   any noclip color mismatch could be our TEV math *or* our light values.

   Nothing is hand-tuned any more, and none of it needed emulated RAM:
   - **Light colors are traced constants**, `(1,0,0)` and `(0,0,0)` — see risk #1
     and the *cel bands vs noclip* entry.
   - **Attenuation ≡ 1 is now measured, not assumed.** `mCosAtten` and
     `mDistAtten` are both `(1,0,0)` (`d_kankyo.cpp:1548-1553`, `:3413-3418`), so
     `tev.slang` forcing 1.0 is exact rather than an approximation.
   - **The stage-0 lerp endpoints are read off the disc.** They are static stage
     data, not runtime state: `scripts/link_env_colors.py` walks a stage `.dzs`'s
     `EnvR → Colo → Pale` chain (`just link-env-colors`). The ocean stage's
     daytime plateau gives `Actor_C0 = (156,140,134)` (shadow end, → `GX_TEVREG0`)
     and `Actor_K0 = (255,255,255)` (lit end, → konst K0). The example patches
     both per frame, mirroring `setLightTevColorType_sub`
     (`d_kankyo.cpp:1817-1829` — note the sibling branch at `:1797-1816` swaps
     them, but it is gated on `toon_proc_check()`, which unconditionally returns
     false in retail, `:89-99`).

   Ambient is *not* patched by the game — MAT3's 50/255 is what the hardware
   uses, so `tev_pack` was already right.

   **What is still a choice, not a measurement:** which palette slot to render.
   The script defaults to the ocean stage's 150–270 schedule plateau, the widest
   daytime band and the only one whose two schedule endpoints name the same slot,
   so it alone needs no time-of-day blend. Any other time of day would need
   `setLight_actor`'s two-way palette lerp (`d_kankyo.cpp:1328-1353`).
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
