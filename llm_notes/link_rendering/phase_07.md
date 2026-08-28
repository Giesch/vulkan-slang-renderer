# Phase 7: `toon_link` albedo — real textures, alpha cutout, full raster state

Detailed plan for P7 of [`../link_rendering.md`](../link_rendering.md) §6
(shader §3, example §5, texture options §4.4). Estimated: 1 day. Verification
follows [`tests.md`](tests.md) §P7. Builds directly on P6
([`phase_06.md`](phase_06.md), `9508563`), consuming the same converted assets
(`just extract-link && just convert-link`). Line numbers below verified at
`fd06085`.

**Goal**: after P7, `just dev toon_link` renders Link with his **real albedo
textures**, clean **alpha-cutout** brows and lashes, **complete per-material
raster state** (cull + the exact `Less_Equal` depth function + honest
`z_write` + blend modes), **J3D two-pass draw ordering**, and **gamma-correct
output**. Still no TEV stages, no lighting channel, no ramp sampling — those
are P8.

Everything in this phase is **example + shader only**. No renderer change, no
converter change, no template change: `RasterState`, `TextureOptions`,
`DepthCompare::LessEqual` and the multi-sampler `ParameterBlock` pattern all
already exist (P4/P5, `paint_display`).

**Deliverables**

1. `shaders/source/toon_link.shader.slang` — two `Sampler2D` slots, GX
   alpha-compare → `discard`, four debug modes, inverse-sRGB output; new
   generated bindings, snapshot churn confined to `toon_link`
2. `examples/toon_link.rs` — texture loading from the manifest (7 referenced
   PNGs + a 1×1 white dummy), `tex0`/`tex1` bound per material, completed
   `raster_state()`, `pe_mode` draw ordering, per-material alpha-compare
   uniforms, renumbered debug keys
3. Master-plan edits: §6 P7 row ✅ + hash, §4.3's `BlendMode::DstAlpha` note
   refined with P7's measured justification, risk list updated
4. Recorded facts below filled in

## Measured facts this phase relies on

Read off `assets/link/converted/link.manifest.json` and the converted PNGs on
disk at `fd06085` — not from the master plan's older sketches. Where the two
disagree, the numbers here win (§2.3's `"Back"`/`"None"` cull spelling and §3's
four-sampler `ToonLinkParams` are both superseded).

### Textures

- **41 texture entries, only 7 referenced by any material**: 34 `linktexS3TC`,
  35 `ZBtoonEX` (the ramp; its `file` is already `tex/raw_toonex.png` and its
  `runtime_substitution` is `toonex`), 36 `mouthS3TC.1`, 37 `podAS3TC`,
  38 `eyeh.1`, 39 `hitomi`, 40 `mayuh.1`. The other 34 entries are BTP
  eye/brow animation frames, unreachable without BTP (deferred,
  [`follow_up.md`](follow_up.md)).
- Every entry is `ClampToEdge`/`ClampToEdge`, `Linear`, `mipmaps: false` —
  P2's inventory finding, still true.
- **Alpha channels are real exactly where the cutout needs them**:
  `mayuh.1` is I4 and `gx/texture.rs::i4_color` sets `R=G=B=A=I`, so its alpha
  spans 0–255; `eyeh.1` is IA8 with a genuine alpha channel, also 0–255. All
  three albedos used by opaque materials — `linktexS3TC`, `mouthS3TC.1`,
  `podAS3TC` — are **alpha 255 at every texel** (verified with `identify`).
  That last fact is load-bearing for the blend decision below.

### Materials (24 slots)

- **At most 2 texmaps per material**, always slots 0 and 1, never higher.
  Distribution: `[34,35]` ×10, `[40]` ×6, `[38]` ×4, `[38,39]` ×2, `[36,35]`
  ×1, `[37,35]` ×1.
- **texmap slot 0 is the albedo**, confirmed by the TEV order tables rather
  than assumed: the `ear` family's stage 1 is `tc0/tm0` → `linktexS3TC`,
  `eyeL`'s stage 1 is `tc0/tm0` → `eyeh.1`. Slot 1 is the ramp (`ZBtoonEX`) or
  the `hitomi` pupil.
- **texcoord 0 is the identity everywhere**: every material's texgen 0 is
  `ty1/src4/mtx60` = `MTX2x4 · TEX0 · GX_IDENTITY`. So P7 samples `tex0` with
  the raw interpolated `uv0` and needs no texture matrix at all. The two
  non-identity matrices flagged in master-plan risk #5 (`mtx33` = `TEXMTX1`)
  sit on `eyeL`/`eyeR`'s **texcoord 1**, which P7 never evaluates.
- **Alpha compare**: exactly two configs. 20 materials are
  `Always/0 OR Always/0` (a no-op), and 4 — `eyeL`, `eyeR`, `mayuL`, `mayuR` —
  are `Greater/0 OR Greater/0`, i.e. "keep iff alpha > 0".
- **Blend**: 12 × `None_(One, Zero)`, 4 × `None_(Source_Alpha, …)` (mode
  `None_` means blending is off regardless of the factors), 4 ×
  `Blend(Source_Alpha, Inverse_Source_Alpha)`, 4 ×
  `Blend(Destination_Alpha, Inverse_Destination_Alpha)`.
- **Depth**: all 24 have `z_func: Less_Equal`. `z_test` is false on 8;
  `z_write` is false on 12 — **including 4 with `z_test: true, z_write: false`**
  (`eyeLdamA`, `eyeRdamA`, `mayuLdamA`, `mayuRdamA`). P6's mapping forces
  `depth_write: true` whenever `z_test` is set, so those 4 are wrong today.
- **Cull**: 23 × `Cull_Back`, 1 × `Cull_None` (`sleeve`).
- **pe_mode**: 12 Opaque / 12 Translucent.

### Draw order

Batches ↔ material slots are bijective but permuted (already handled by the
`MaterialSlot` / `BatchIndex` newtypes, examples/toon_link.rs:45–91). In INF1
batch order the `pe_mode` sequence is:

```
O O O O O O O O O O  T T T T T T  O  T T T T T T  O
0 1 2 3 4 5 6 7 8 9  10 … 15     16  17 … 22     23
```

Nearly two-pass already, but batch 16 (slot 22, `ear(7)`) and batch 23 (slot
23, `ear(8)`) are opaque batches drawn after translucent ones.

### Renderer / codegen affordances (all shipped)

- `Renderer::create_texture_with_options` (src/renderer.rs:511) +
  `TextureOptions { filter, wrap_u, wrap_v, mipmaps, color_space }`
  (src/renderer.rs:2915). `TextureWrap` / `TextureFilter` /
  `TextureColorSpace` variant names line up 1:1 with the manifest's strings.
- `RasterState` (src/renderer/pipeline.rs:216) with `DepthCompare::LessEqual`
  already present (pipeline.rs:203); `BlendMode` has only `Alpha` and `Opaque`
  (pipeline.rs:182).
- Multi-sampler `ParameterBlock`: `shaders/source/paint_display.shader.slang`
  declares five `Sampler2D`s as struct fields ahead of the scalars; they land
  in the generated `Resources` in declaration order and do **not** appear in
  the uniform struct.
- `uint4` uniform fields are proven since the vec4-array mini-phase
  (`0d08a7d`, [`vec4_array_support.md`](vec4_array_support.md)).
- Color clear is `[0.0, 0.0, 0.0, 1.0]` (src/renderer.rs:1744).
- `examples/multi_mesh.rs:353-378` is the worked example for
  create-all-textures-then-build-pipelines.

## Decisions (settled in planning, user-approved)

1. **Two sampler slots, both bound in P7.** The data says 2 is the terminal
   count, so declare `tex0` and `tex1`, bind the real ramp/`hitomi` into slot
   1 now, and sample only `tex0`. P8 then adds ramp sampling with **no
   descriptor-shape change** — no `just shaders` + restart mid-P8. Materials
   with a single texmap get a 1×1 white dummy in slot 1. Setup asserts
   `texmaps[2..]` are all `None` and bails otherwise, so a future model that
   breaks the assumption says so instead of silently dropping a texture.
2. **`Destination_Alpha` blend maps to `BlendMode::Opaque`, not `Alpha`.**
   GX computes `dst_alpha·src + (1 − dst_alpha)·dst`, which reduces *exactly*
   to `src` wherever the framebuffer alpha is 1. It is 1 at those pixels:
   the clear is alpha 1.0, every opaque albedo is alpha-255 at every texel, and
   the 4 dst-alpha materials (`eyeL`, `eyeR`, `mayuL`, `mayuR`) are the **first
   translucent batches drawn**, immediately after the opaque group, so nothing
   has yet written a non-1 alpha over the face. This keeps P7 free of renderer
   changes; real `BlendMode::DstAlpha` still lands with the eye write-mask
   multi-pass in P9 (master plan §4.3/§4.5), where it finally buys something.
   *This is an approximation with a precondition — see risk 3.*
   **RETIRED by P9** ([`phase_09_eyes.md`](phase_09_eyes.md)): `BlendMode::DstAlpha`
   now exists and these four materials use it. The precondition this decision
   rested on — framebuffer alpha ≡ 1 at those pixels — is exactly what P9's mask
   pass stops being true, on purpose. The `Opaque` mapping survives only behind
   the example's `M` toggle, as the instrument that reproduces the old artifact.
3. **Opaque-before-translucent draw ordering lands in P7.** A stable partition
   of the batch list by `pe_mode`, INF1 order preserved within each group.
   It only becomes observable now that blending is real, and it is ~10 lines.
4. **Inverse-sRGB output is pulled forward from P8 into P7.** P7's entire
   verification story is per-feature comparison against noclip; doing that
   through a known systematic brightness error wastes the phase. ~6 lines.

## Step 1 — shader: samplers, alpha compare, gamma

`shaders/source/toon_link.shader.slang`. Samplers first, matching the
`paint_display` convention:

```slang
ParameterBlock<ToonLinkParams> params;

struct ToonLinkParams {
    // texmap slot 0 — the albedo. Slot 1 is the ZBtoonEX ramp (or the hitomi
    // pupil on eyeL/eyeR): bound now so P8 needs no descriptor-shape change,
    // but not sampled until the TEV interpreter exists.
    Sampler2D tex0;
    Sampler2D tex1;

    MVPMatrices mvp;

    // raw GX codes, straight from MAT3 via the manifest.
    // GXCompare: NEVER 0, LESS 1, EQUAL 2, LEQUAL 3,
    //            GREATER 4, NEQUAL 5, GEQUAL 6, ALWAYS 7
    uint4 alphaCompare;      // comp0, ref0, comp1, ref1
    uint  alphaCompareOp;    // GXAlphaOp: AND 0, OR 1, XOR 2, XNOR 3
    // 0 albedo, 1 world normals, 2 uv0, 3 albedo alpha as gray
    uint  debugMode;
}
```

Explicit `uint4` + a scalar rather than bit-packing five values into one
`uint`: vector uniform fields are proven, and the packed form is unreadable in
a debugger for no space that matters.

Fragment shader:

1. `let texel = params.tex0.Sample(fragVertex.uv0);`
2. **Alpha compare**, on the 8-bit value GX actually compares
   (`uint(round(saturate(texel.a) * 255.0))`): implement all 8 `GXCompare`
   functions and all 4 `GXAlphaOp` combiners as plain `switch`es, then
   `discard` when the predicate fails. Only 2 of the 32 combinations occur in
   `cl.bdl`, but the full table is ~20 lines and P8 inherits it unchanged.
   The compare runs in **every** debug mode, so the cutout stays visible while
   inspecting normals or UVs.
3. **Debug modes** as listed above; mode 0 (albedo) becomes the default.
4. **Output**: return the color through an `srgbDecode` helper. The color
   target is `_SRGB`, so the hardware applies linear→sRGB *encode* on store;
   we hold raw GX values that must survive to the stored bits unchanged, so
   the shader must apply the *decode* (`sRGB → linear`) first. Standard
   piecewise transfer: `c ≤ 0.04045 ? c/12.92 : pow((c + 0.055)/1.055, 2.4)`.
   Applied to RGB only, never alpha.

**The direction of that transfer is the single easiest thing in this phase to
get backwards, and both directions look "plausibly a bit off".** Do not
eyeball it — Step 5's numeric texel check is the gate.

Then `just shaders` → `just test`.

**Gate:** `just shaders` + `just test` green with churn confined to
`toon_link`'s `.rs` and `.json` snapshots (this is a shape change: two new
samplers plus two new uniform fields); every other per-shader snapshot
byte-identical. `just lint` clean.

## Step 2 — example: texture loading

`examples/toon_link.rs`, setup.

- Build a `Vec<Option<TextureHandle>>` indexed by **manifest texture index**,
  populating only the entries some material's `texmaps` actually references (7
  of 41). Loading all 41 would work but would quietly claim we understand the
  BTP frames; loading the referenced set and printing the count is honest.
- `util::load_image` hardcodes the `textures/` directory, so read directly:
  `ImageReader::open(dir.join(&entry.file))?.decode()?` (`image` is already a
  dependency, and `entry.file` is manifest-relative).
- `TextureOptions` from the manifest strings, each mapped explicitly with a
  bail on anything unrecognized:
  `wrap_u`/`wrap_v` → `TextureWrap`, `filter` → `TextureFilter`, `mipmaps`
  straight through. `color_space` is **hardcoded `Unorm`** — the manifest has
  no such field because GX has no sRGB anywhere (master plan §3); leave a
  comment saying so at the call site, since `Unorm` looks like a mistake to
  anyone who hasn't read it.
- **1×1 white dummy** for unused texmap slots: `RgbaImage::from_pixel(1, 1,
  Rgba([255; 4]))`, same options (`Unorm`, `ClampToEdge`, no mips, `Linear`).
- **Borrow ordering matters.** `create_texture_with_options` takes
  `&mut Renderer` while `Resources` holds `&'a TextureHandle`, so every
  texture must be created *before* the pipeline loop starts. Same shape as
  `examples/multi_mesh.rs:353-378`.
- Assert `texmaps[2..]` are all `None` across all 24 materials (decision 1).

Pipeline loop gains `tex0` / `tex1` fields resolved through
`texmaps[0]` / `texmaps[1]`, falling back to the dummy.

**Gate:** `cargo check --all-targets` clean; running with assets reaches end
of setup and prints the texture count; running without assets still bails
helpfully.

## Step 3 — example: complete `raster_state()`

Replace the deliberately partial P6 mapping (examples/toon_link.rs:145–170),
keeping its shape and the `CULL_OVERRIDE` knob:

- **Cull** — unchanged, still the literal `Cull_Back → Back` mapping. Master
  plan risk #2: winding and cull are one decision and it is already settled;
  do not touch either.
- **Depth test**: `z_test ? map(z_func) : Always`, mapping `"Less_Equal" →
  LessEqual`, `"Less" → Less`, `"Always" → Always`, bail otherwise. All 24
  materials are `Less_Equal` in practice, so this is the P6 `Less` placeholder
  becoming correct.
- **Depth write**: honor `z_write` directly instead of tying it to `z_test`.
  **This is a real behavior fix**, not bookkeeping — it flips
  `eyeLdamA`/`eyeRdamA`/`mayuLdamA`/`mayuRdamA` from writing depth to not
  writing it, which is what lets the layered eye decals composite at all.
- **Blend**:
  - `blend: None` or `mode == "None_"` (whatever the factors say) → `Opaque`
  - `Blend(Source_Alpha, Inverse_Source_Alpha)` → `Alpha`
  - `Blend(Destination_Alpha, Inverse_Destination_Alpha)` → `Opaque`, with the
    decision-2 justification as a comment at the mapping site
  - anything else bails with an actionable message naming the material and the
    unmapped mode — the subset-gate spirit from `tev_ir.rs`.

## Step 4 — example: draw order, uniforms, debug keys

- **Draw order**: compute `draw_order: Vec<BatchIndex>` once in setup — the
  opaque batches in INF1 order, then the translucent ones in INF1 order — and
  iterate that in `draw` instead of `manifest.batches`. Bail on an
  unrecognized `pe_mode`. Print the resulting order once at startup beside the
  existing control legend.
  The batch-tiling asserts (examples/toon_link.rs:244–271) validate the
  *manifest*, not the draw order, so they stay exactly as they are.
- **Per-material uniforms**: precompute each material's alpha-compare codes in
  setup (manifest strings → the raw GX numbers listed in Step 1, bail on
  unknown). Uniforms are per-pipeline and the pipeline vector is already in
  `MaterialSlot` order, so `submit_draws`'s existing loop only needs to walk
  the materials alongside the buffers.
- **Debug keys** renumbered, since albedo is now the interesting default:

  | key | mode |
  |---|---|
  | Num1 | albedo (default) |
  | Num2 | world-space normals |
  | Num3 | uv0 |
  | Num4 | albedo alpha as grayscale |

  Q / E / Space isolation is unchanged. Update the startup legend and the
  module doc comment.

**Gate:** `timeout 3 just dev toon_link` renders a textured Link; all four
modes and the isolation keys work; no validation output.

## Test plan

**Automated:**

- `just shaders`; `just test` with churn confined to `toon_link`;
  `cargo check --all-targets`; `just lint`; `cargo fmt`.
- No converter change, so no `just link-verify-*` run is required — but the
  golden hashes must be untouched (`git diff scripts/link_converted.sha256`
  empty) as proof.

**Validation sweep** (P4/P5/P6 convention, documented loop not a recipe):

```sh
for e in $(ls examples | sed 's/\.rs$//'); do
  timeout 3 just dev "$e" 2>&1 | grep -iE "validation|VUID" && { echo "FAIL: $e"; exit 1; }
done; echo "sweep clean"
```

**Eyeball** ([`tests.md`](tests.md) §P7 — results go into Recorded facts):

1. **UV correctness vs noclip, feature by feature** — face decals, eye
   placement, belt buckle (`podA`), tunic pattern. Misaligned UVs or a V-flip
   are instantly visible on a character; this is the phase's primary check.
   Compare at the same 2–3 canonical angles used in P6.
2. **Alpha cutout** — clean edges on brows and lashes, no rectangular halos.
   Num4 (alpha view) plus Q/E isolation on the `mayuL` / `eyeL` batches is the
   tool for triaging a bad edge.
3. **Per-material raster state on real data** — no missing body parts (wrong
   cull), hair-over-face correct at this phase's depth settings, `sleeve`
   still double-sided.
4. **Draw order** — the two relocated opaque `ear` batches must not punch
   holes in, or disappear behind, the eye/brow decals.
5. **Gamma, numerically.** Screenshot a flat interior texel of the tunic and
   compare its RGB against the same texel's raw value in
   `tex/34_linktexS3TC.png`. They must agree within a couple of LSBs. **This
   is the gate for Step 1's transfer direction — not a visual impression.**
6. **Hot reload** — a fragment-body edit recompiles across all 24 pipelines
   with per-material raster state preserved (P6 proved this at 24-pipeline
   scale; re-confirm now that textures are bound).
7. Clean exit, **no VMA leak report**. No manual close needed: `timeout`'s
   SIGTERM becomes `SDL_QUIT`, so `Drop` runs and leaks report themselves —
   `scripts/headless-sweep.sh toon_link` covers this (`build_reproducibility.md`
   §7.4 corrects the earlier claim that it needed a real window close).

## Verification (exit checklist)

Static / code gates — **done**:

- [x] `just shaders` green; `just test` churn = `toon_link` only (the two
      `shader_atlas.rs` atlas-order failures are pre-existing on this machine
      and unrelated — see Recorded facts deviation 2)
- [x] `cargo check --all-targets`, `just lint` (debug + release), `cargo fmt` clean
- [x] `scripts/link_converted.sha256` and the whole converter untouched
- [x] Texture loader populates only texmap-referenced entries + 1×1 white
      dummy; `texmaps[2..]` assert in place
- [x] `tex0` sampled with raw `uv0`; `tex1` bound but unread
- [x] Alpha compare implemented in full (8 compares × 4 ops) and discarding
- [x] `raster_state()` maps cull, `z_func`, `z_write` and all four blend
      configurations, bailing on anything unmapped
- [x] Draw order partitions opaque before translucent, INF1 order within
- [x] Debug keys Num1–4 + Q/E/Space wired; legend and module doc updated
- [x] Recorded facts filled in

Runtime / visual gates — **run 2026-07-27** at the start of P8 (Pop!_OS/COSMIC
Wayland, RTX 3070 Ti + Intel Xe, converted assets present). See the second
Recorded-facts block:

- [x] 7 referenced textures + 1 dummy actually load
- [x] Gamma verified **numerically** against the source PNG — 0 LSB on four
      distinct colors; the transfer direction is now measured, not reasoned
- [x] Nothing missing (per-material cull correct front and back)
- [~] Cutout edges: the alpha compare is correct and the body silhouette is
      clean, but an opaque black rectangle surrounds each eye and brow. Traced
      to the missing BTP + the deferred `DstAlpha` pass, **not** a P7 defect —
      see the Recorded-facts entry. Persists through P8.
- [x] UV features vs noclip: everything observed is correctly placed; the
      per-feature noclip side-by-side folded into P8 was confirmed by eye
      2026-08-28
- [x] Draw-order effect observed; `depth_write` honored per material (code-level)
- [x] Validation sweep clean (16/16), validation layer confirmed loaded
- [x] Hot reload preserves per-material raster state; no VMA leak on real close
- [x] Master plan §6 P7 row ✅ + hash

## Recorded facts — runtime gates, run 2026-07-27

Run at the start of P8 on the development machine (Pop!_OS 22.04 / COSMIC
Wayland, `DISPLAY=:1`, RTX 3070 Ti Laptop + Intel Iris Xe, Vulkan 1.4.318,
converted assets present and matching all 90 golden hashes). **No code change
was needed** — every gate passed on the committed P7 code, with one gate blocked
by a documented deferred feature.

```
texture load:             PASS, exactly as predicted. "toon_link: loaded 7 of 41
                          textures (34 unreferenced BTP frames skipped)".

gamma check:              PASS — and this is the one that most needed running,
                          since it is the gate for Step 1's transfer direction.
                          MEASURED, not eyeballed: a screenshot of the running
                          window was decoded and the flat tunic interior reads
                          (90, 178, 74), which is byte-identical to
                          tex/34_linktexS3TC.png's most common color
                          (90, 178, 74) -- 0 LSB error. Three more colors match
                          to 0 LSB in the same frame: (247,219,156) skin,
                          (255,239,16) hair/hat yellow, (33,158,57) dark green.
                          The two wrong hypotheses are nowhere close: "no
                          transform in the shader" implies a source color of
                          (26,114,17) (nearest real texture color 674 squared
                          away) and "encode instead of decode" implies (3,43,1)
                          (9290 away). So Step 1's reasoning was right: textures
                          load R8G8B8A8_UNORM (renderer.rs:4121), the shader
                          applies the sRGB *decode*, and the B8G8R8A8_SRGB
                          swapchain (renderer.rs:3219) encodes on store; the two
                          transfers cancel exactly.

uv / noclip per-feature:  Everything observed is correctly placed, front and
                          back: hat, tunic pattern, belt, boots, white leggings,
                          shield on the back, face decals, ear tips. Clearly
                          recognizable Toon Link with no V-flip and no
                          misregistration. The *formal* per-feature noclip
                          side-by-side is folded into P8's, which uses the same
                          P6 camera angles -- doing it once for both is cheaper
                          and P8 is where a color discrepancy actually matters.

alpha cutout:             The compare works; the visual "no rectangular halos"
                          check is BLOCKED by a deferred feature, and this is
                          worth recording precisely because it looks like a bug.
                          Observed: an opaque black rectangle around each eye and
                          each brow. Fully traced:
                            - tex/38_eyeh.1.png is literally (0,0,0,0) over 7703
                              of its 9216 pixels -- black with alpha 0.
                            - eyeLdamB has alpha_compare = Always 0 (never
                              discards) and blend = None_, which maps to
                              BlendMode::Opaque (correct: GX_BM_NONE ignores the
                              factors), and it draws LAST of the eyeL group
                              (draw order ... 10 eyeL, 11 eyeLdamA, 12 eyeLdamB).
                              So it paints its whole quad opaque black.
                            - the mayu* group is the same shape with texture 40.
                          The materials that DO have a cutout compare
                          (eyeL/eyeR/mayuL/mayuR, Greater 0) discard correctly:
                          in debug mode 3 the body silhouette is exactly the mesh
                          with clean edges. So this is phase_07 risk 1 (BTP not
                          implemented) plus decision 2's deferred DstAlpha pass
                          (P9's eye write-mask trick), not a P7 alpha-compare
                          defect. It will still be present after P8.
                          CORRECTION (P9, phase_09_eyes.md): the diagnosis in
                          the last sentence is wrong. BTP is not implicated at
                          all -- the twelve batches are 3 passes x 4 features,
                          all of which the game also draws every frame. The
                          observation above is accurate; the cause is that
                          eyeLdamB runs colorUpdate=0 on hardware (it only ever
                          touches destination alpha) and we drew it with color
                          writes ON. It is a write-mask bug, not a blend bug and
                          not a missing-animation artifact. Fixed in P9.

cull / missing parts:     PASS. Full front and back views show a complete model,
                          nothing inside-out, nothing missing.

draw-order effect:        PASS, exactly as predicted. Printed order is
                          [0,1,2,3,4,5,6,7,8,9,16,23,10,...] -- batches 16 and 23
                          (materials ear(7) and ear(8), the two opaque ear
                          batches) relocated ahead of the translucent group.

depth_write fix effect:   Honored per material (raster_state reads z_write
                          directly); the manifest confirms eyeLdamA / eyeRdamA /
                          mayuLdamA / mayuRdamA are z_write=false. Not isolated
                          visually -- the effect is masked by the eye-decal
                          stacking above.
                          UNBLOCKED by P9 (phase_09_eyes.md): the black quad is
                          gone and the four *damA masks now draw color-free in
                          their own pass, so the depth_write effect is finally
                          observable in isolation. Note Q/E on a mask or erase
                          batch yields a black frame by design (color writes
                          off); print_isolation says so.

sweep:                    PASS 16/16. Every example ran 3s with zero lines
                          matching /validation|VUID/. VK_LAYER_KHRONOS_validation
                          confirmed loaded in the log, so the silence is
                          meaningful rather than the layer being absent.

hot reload / VMA:         PASS both. A body-only edit to toon_link.shader.slang
                          (debug mode 2's blue channel) produced
                          "recompiling shaders..." followed by 24 x "finished
                          recompiling shaders" -- one per pipeline, confirming
                          24-pipeline scale -- with no errors, no panic, no
                          interface assert, and the app surviving; a screenshot
                          after the reload shows the model and its raster state
                          intact. Clean close was done properly rather than with
                          SIGTERM: the app was run under SDL_VIDEODRIVER=x11 and
                          sent a real WM_DELETE_WINDOW ClientMessage via
                          python-xlib, which SDL turns into Event::Quit -- the
                          same path as clicking the window's X. Exit code 0, no
                          VMA leak report, no validation error at device destroy,
                          so Drop ran fully.

debug keys:               PASS. Num1/2/3/4 all switch (albedo, normals with
                          smooth gradients, uv0, alpha-gray) and Q/E/Space
                          isolate and clear, printing the expected
                          "batch N: shape S material M "name" [first..+count]".

tooling note (reusable):  This machine is Wayland/COSMIC, so the X11 root is
                          black and ffmpeg x11grab captures nothing. What works,
                          and what P8 reuses: `cosmic-screenshot
                          --interactive=false --modal=false --notify=false -s DIR`
                          for frames, and python-xlib XTEST fake_input against a
                          window found by WM_NAME (run the example with
                          SDL_VIDEODRIVER=x11) for keystrokes. Together these
                          make the per-material isolation sweep and the debug-mode
                          sweep scriptable instead of manual.
```

## Recorded facts — implementation (original, headless)

**Implementation landed; every runtime/visual gate is still outstanding.** The
code was written and all *static* gates pass, but it was implemented in a
headless container with **no video device** (`Error: No available video device`
from SDL — every example bails, not just `toon_link`) and **without the
converted assets** (machine-local, gitignored, needs the disc image — risk 5).
So nothing below that requires a window or `assets/link/converted/` has been
observed. Those checks must be run on a machine with a GPU and converted assets
before P7 can be called done. **They were, on 2026-07-27 — see the block
above.**

```
commit:                   f415612 ("implement phase 7: toon_link albedo
                          textures, alpha cutout, raster state"; authored on
                          branch claude/link-rendering-phase-7-2k9d5z)

static gates:             PASS. shaders regenerated; churn confined to
                          toon_link (.slang, .json, both .spv, generated
                          toon_link.rs, and 3 snapshots: toon_link.rs,
                          toon_link.json, shader_branching). cargo check
                          --all-targets clean; clippy --all-targets clean in
                          both debug and release; cargo fmt clean;
                          scripts/link_converted.sha256 untouched (no converter
                          change).

reflection (verified):    ToonLinkParams 208 → 224 bytes; alphaCompare at
                          offset 192 (uint4), alphaCompareOp at 208,
                          debugMode at 212, _padding_0 [u8; 8]. Resources
                          gained tex0/tex1 at descriptor slots 1 and 2 ahead
                          of the uniform buffer, matching the paint_display
                          convention. toon_link.frag.spv branch count 1 → 7
                          (the two compare/op switches, the debug switch and
                          the discard).

texture load:             NOT RUN — no assets on this machine. Code loads only
                          texmap-referenced entries and prints
                          "loaded N of M textures (K unreferenced BTP frames
                          skipped)"; expect 7 of 41 per the measured facts.

uv / noclip per-feature:  NOT RUN (headless, no assets).

alpha cutout:             NOT RUN (headless, no assets).

gamma check:              NOT RUN — and this is the one that most needs
                          running, since it is the gate for Step 1's transfer
                          direction and the plan explicitly says not to
                          eyeball it. The shader applies sRGB *decode*
                          (c <= 0.04045 ? c/12.92 : pow((c+0.055)/1.055, 2.4))
                          to RGB only, reasoning that the _SRGB color target
                          encodes on store so the two cancel. That reasoning
                          is unverified against a real screenshot.

draw-order effect:        NOT RUN. The partition is implemented and printed at
                          startup; per the measured INF1 sequence it should
                          move exactly batches 16 and 23 (the two opaque `ear`
                          batches) ahead of the translucent group.

depth_write fix effect:   NOT RUN. raster_state now honors z_write directly,
                          which should flip eyeLdamA/eyeRdamA/mayuLdamA/
                          mayuRdamA from writing depth to not writing it.

eye/brow decal stacking:  NOT RUN (expected muddled per risk 1 — a
                          missing-BTP artifact, not a P7 bug).

sweep:                    NOT RUN — no video device, so the validation sweep
                          is meaningless here (every example bails at SDL
                          init, before Vulkan).

hot reload / VMA:         NOT RUN (headless).

deviations discovered:    1. `BatchIndex::raw` takes `self` by value, so
                             `.map(BatchIndex::raw)` over `iter()` does not
                             typecheck; used a closure.
                          2. **Pre-existing, unrelated snapshot failure on
                             this machine**: `generated_files` and
                             `alignment_tests` both fail on the
                             `src/generated/shader_atlas.rs` snapshot because
                             `write_precompiled_shaders` builds the atlas from
                             `std::fs::read_dir` (build_tasks.rs:28) without
                             sorting, so the module order follows filesystem
                             order and differs from the committer's. Verified
                             pre-existing by stashing every P7 change and
                             re-running on a pristine tree — both still fail,
                             with a pure-reordering diff (every line present on
                             both sides). Those two atlas `.snap.new` files
                             were therefore **discarded, not accepted**, and
                             `src/generated/shader_atlas.rs` was reverted to
                             the committed order. Sorting the read_dir results
                             would make this deterministic — worth a follow-up,
                             out of scope for P7.
                             **Fixed 2026-07-26** — the read_dir walks are now
                             sorted and the atlas snapshots re-recorded; see
                             `follow_up.md` §5b. This entry stands as the
                             discovery record.
                          3. Environment setup needed before anything built:
                             libasound2-dev + libvulkan-dev, the slang
                             submodule, and a from-source slang build
                             configured with -DSLANG_ENABLE_SLANG_RHI=OFF
                             (slang-rhi's CMake tries to fetch OptiX and fails
                             behind the proxy).
```

## Out of scope for P7

- **TEV stages, lighting channel, SRTG ramp sampling, konst/register colors**
  (P8). `tex1` is bound but never read.
- **Texture matrices** — `TEXMTX1` on `eyeL`/`eyeR` texcoord 1 (P8, with the
  `hitomi` pupil).
- **`BlendMode::DstAlpha` and the eye write-mask multi-pass** (P9, master plan
  §4.5).
- **BTP eye/brow frame animation** — the 34 unreferenced texture entries stay
  unloaded ([`follow_up.md`](follow_up.md)).
- **`--casual` clothes, BCK poses, runtime skinning** (P9 / deferred).

## Risks / open questions

1. ~~**Eye and brow decals stack.**~~ **MISDIAGNOSED — corrected by P9**
   ([`phase_09_eyes.md`](phase_09_eyes.md)). The original text read: "Twelve
   36-index decal batches (`eyeL`, `eyeLdamA`, `eyeLdamB` and the R / `mayu`
   equivalents) all draw over the same patch of face. The game picks one frame
   per eye at runtime via BTP, which we don't implement, so all of them
   composite at once."

   Every clause of that is wrong. The twelve batches are **3 passes × 4
   features**, not 12 BTP frames, and `playerInit`
   (`../tww/src/d/actor/d_a_player_main.cpp:12150-12178`) classifies them into
   three arrays of four and asserts `zon_cnt == 4 && zoff_none_cnt == 4 &&
   zoff_blend_cnt == 4` — **the game draws all twelve every frame too**. All
   three passes of a feature sample the *same* default texture; BTP swaps that
   texture for blinking, it does not select between the passes. Nothing was
   stacking that the game does not also stack. The real defect was a write-mask
   bug (the `*damB` materials run `colorUpdate = 0` and we drew them with color
   writes on), fixed in P9 with no animation support whatsoever.
2. **Gamma transfer direction.** Covered above; the mitigation is that the
   gate is numeric.
3. **RETIRED by P9** — see decision 2. The precondition below is precisely what
   P9's mask pass invalidates on purpose, and `BlendMode::DstAlpha` now exists,
   so there is no approximation left to guard. Original text follows.

   **The dst-alpha → `Opaque` mapping has a precondition.** It is exact only
   while the framebuffer alpha at those pixels is 1. That holds today because
   the clear is alpha 1.0, all opaque albedos are alpha-255, and the four
   dst-alpha materials draw first among the translucent group. Any of the
   three changing — a new albedo with alpha < 255, a different draw order, a
   `--casual` texture — silently degrades it. Put the precondition in the
   comment at the mapping site so the next reader can check it, rather than
   rediscovering it.
4. **Descriptor-shape change.** Two new samplers plus two new uniform fields
   change the reflected interface, so `assert_shader_interface_unchanged`
   panics if the struct is edited while `just dev` is running. `just shaders`
   + restart. Fragment-body edits still hot-reload; the existing comment on
   `ToonLinkParams` already says this and should stay.
5. **Assets are machine-local.** CI and other machines bail on `toon_link`;
   the sweep line for it only means something where `just convert-link` has
   run.
6. **The noclip comparison is still partly subjective**, but much less so than
   P6's silhouette check: misaligned UVs on a character are unambiguous. If a
   feature is genuinely in dispute, defer it to P8's Dolphin golden frames
   rather than tuning toward a screenshot.
