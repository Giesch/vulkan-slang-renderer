# Phase 9 (eyes): the eye/brow write-mask multi-pass

Detailed plan for the **eye/brow decal portion only** of P9 in
[`../link_rendering.md`](../link_rendering.md) §6 (renderer §4.3/§4.5, example
§5). The other two items on that row — `--casual` clothes and the BCK-sampled
pose — are independent and get their own doc. Estimated: 1 day. Builds on P8
(`bd571fc`); line numbers below verified at that commit. Consumes the same
converted assets, unchanged (`just extract-link && just convert-link`).

**Goal**: after this phase, `just dev toon_link` renders Link's eyes and
eyebrows the way the hardware does — no opaque black rectangle, the pupil
visible inside the lash silhouette, and the eyes compositing *through* the hair
via destination alpha, which is the entire point of the technique.

**Deliverables**

1. `src/renderer/pipeline.rs` + `src/renderer.rs` — `BlendMode::DstAlpha`, and
   the color-blend factors driven by the blend mode instead of hardcoded
2. `examples/toon_link.rs` — decal role classification, per-role write masks,
   the five-group draw order, and the `Key::M` mask view
3. `src/game/traits.rs` — one new `Key` variant for the mask view
3a. `shaders/source/toon_link.shader.slang` — `DEBUG_WHITE`, the mask view's
   fragment output. *Added after the fact; the phase as planned changed no
   shader (see Step 5).*
4. Doc reconciliation (§Doc reconciliation below) — this phase **retires two
   recorded risks and corrects one misdiagnosis**
5. Recorded facts below filled in

*Superseded by the merge with `main`, which had meanwhile moved this example's
debug controls into the egui window and taught shader reflection to emit enums.
Deliverables 2, 3 and 3a landed as written and were then reworked: `DEBUG_WHITE`
is now the `MaskWhite` variant of a slang `enum DebugMode`, selecting it in the
debug window is what switches to `RenderMode::MaskWhite`, and deliverable 3 was
reverted — `Key::M`, and keyboard input in this example generally, is gone.
Everything below that says `Key::M` or `M` means "select `Mask White` in the
debug window". Nothing about the passes, the write masks or the draw order
changed.*

No converter change, no asset change, no codegen change. The golden hashes in
`scripts/link_converted.sha256` must not move.

*Corrected during implementation: this also said "no shader change" and required
`just test` snapshots to stay byte-identical. The mask view (Step 5) needs one
fragment-shader debug case, and its `discard` moves exactly one snapshot line —
`shader_branching_snapshots`, `toon_link.frag.spv: 80 → 81`. The
`ToonLinkParams` shape is untouched, so codegen and every other snapshot are
unmoved.*

## The correction this phase opens with

P7 traced the black quad (`phase_07.md:413-433`) and P8 restated the trace
(`phase_08.md:1014-1019`); both attribute it to "**missing BTP plus P9's
deferred `DstAlpha` pass**". Reading
`../tww/src/d/actor/d_a_player_main.cpp:1811-1868` (`daPy_lk_c::draw`) and
`:12150-12178` (`playerInit`) shows all three parts of that sentence are wrong,
and the corrections are what make this phase small:

1. **The 12 decal batches are 3 passes × 4 features, not 12 BTP frames.** The
   game draws all twelve every frame too. `playerInit` classifies them into
   three arrays of four and asserts
   `zon_cnt == 4 && zoff_none_cnt == 4 && zoff_blend_cnt == 4`. So
   `phase_07.md:600-608` risk 1 — "the game picks one frame per eye at runtime
   via BTP, which we don't implement, so all of them composite at once" — is a
   **misdiagnosis**. Nothing is stacking that the game does not also stack.
2. **BTP is not implicated.** All three passes of a feature sample the *same*
   default texture (38 `eyeh.1` for the eyes, 40 `mayuh.1` for the brows). BTP
   swaps that texture for blinking and expressions; it does not select between
   the three passes. The quad is fixable with no animation support whatsoever.
3. **The quad is a write-mask bug, not a blend bug.** The `*damB` materials run
   under `colorUpdate = 0, alphaUpdate = 1` — they only ever touch destination
   alpha. We draw them with color writes on, so their TEV output
   (`tex.rgb * ras.rgb`, which is black because `eyeh`/`mayuh` RGB is ~0) lands
   in the color buffer under an `Always` alpha compare that never discards.
   `BlendMode::DstAlpha` is genuinely required for the *composite* pass, but it
   is `color_write` that removes the rectangle.
4. **No per-pass shape-group toggling is needed.** `link_rendering.md:586`
   predicted the fix "needs `color_write` exercised + per-pass shape-group
   toggling". In fact each of the 24 materials is drawn exactly once, as it is
   today. The change is a **draw reordering plus per-material write masks** —
   still 24 draws.

## Measured facts this phase relies on

### The decal set

Read off `assets/link/converted/link.manifest.json` at `bd571fc`. The twelve
translucent batches are the entire translucent set; every other batch is
`Opaque`. Each is exactly 36 indices = 12 triangles.

| batch | mat | name | z_test | z_write | GX blend | alpha cmp | role |
|---|---|---|---|---|---|---|---|
| 11 | 2 | `eyeLdamA` | **true** | false | `Blend(SrcA, InvSrcA)` | `Always` | mask |
| 14 | 5 | `eyeRdamA` | **true** | false | `Blend(SrcA, InvSrcA)` | `Always` | mask |
| 18 | 9 | `mayuLdamA` | **true** | false | `Blend(SrcA, InvSrcA)` | `Always` | mask |
| 21 | 12 | `mayuRdamA` | **true** | false | `Blend(SrcA, InvSrcA)` | `Always` | mask |
| 10 | 1 | `eyeL` | false | false | **`Blend(DstA, InvDstA)`** | `Greater 0` | composite |
| 13 | 4 | `eyeR` | false | false | **`Blend(DstA, InvDstA)`** | `Greater 0` | composite |
| 17 | 8 | `mayuL` | false | false | **`Blend(DstA, InvDstA)`** | `Greater 0` | composite |
| 20 | 11 | `mayuR` | false | false | **`Blend(DstA, InvDstA)`** | `Greater 0` | composite |
| 12 | 3 | `eyeLdamB` | false | false | **`None_`** | `Always` | erase |
| 15 | 6 | `eyeRdamB` | false | false | **`None_`** | `Always` | erase |
| 19 | 10 | `mayuLdamB` | false | false | **`None_`** | `Always` | erase |
| 22 | 13 | `mayuRdamB` | false | false | **`None_`** | `Always` | erase |

**The three passes of a feature are byte-identical geometry.** Measured by
walking `link.idx.bin` → `link.vtx.bin` for each 36-index range: batches
10/11/12 have the same 12 unique positions and the same position+UV sequence in
the same order, and likewise 17/18/19 (and their R-side twins). This is the
structural proof of "3 passes × 4 features" — the three shapes *are* the same
quad, authored three times so the material state can differ. It also retires
the fringe-leak worry: the mask can never be empty where the composite's alpha
test passes, because both sample the same texel of the same texture through the
same identity texcoord 0.

TEV output alpha per role, from `mat3_dump.txt`:

- **mask** (`*damA`, 1 stage, `COLOR0A0`, unlit, `matColor` = white):
  `A = tex.a * ras.a = eyeh.a`
- **composite** (`eyeL`/`eyeR`, 2 stages, both `COLOR_NULL`):
  `rgb = eyeh.rgb * hitomi.rgb`, `A = eyeh.a`. `mayuL`/`mayuR` are 1 stage,
  `rgb = mayuh.rgb * ras.rgb` — and `mayuh.1`'s RGB is identically 0, so the
  brows are pure black by construction, independent of lighting.
- **erase** (`*damB`, 1 stage): `rgb = tex.rgb * ras.rgb`, **`A = 0`** — the
  alpha combiner is `mix(ZERO, ZERO, ZERO)`. That zero is the whole purpose of
  the material, and the black `rgb` is the byproduct we are currently drawing.

### What the game does

The write-mask packets are `mDoExt_offCupOnAupPacket` / `mDoExt_onCupOffAupPacket`
(`../tww/src/m_Do/m_Do_ext.cpp:1845-1853`), which call `GFSetBlendModeEtc` with
`colorUpdate`/`alphaUpdate` of `(0,1)` and `(1,0)`. They survive intervening
material loads because `J3DGDSetBlendMode`
(`../tww/include/JSystem/J3DGraphBase/J3DGD.h:106-129`) writes BP register
`0x41` through a mask (`0xFE001FE3`) that excludes bits 3 and 4 — exactly the
cup/aup bits.

`J3DDrawBuffer::entryImm`/`entryNonSort`
(`../tww/src/JSystem/J3DGraphBase/J3DDrawBuffer.cpp:182,193`) **prepend**, and
the buffer is walked head→tail, and Link's P0 list is a single non-sorted bucket
(`../tww/src/d/d_drawlist.cpp:2088,2107`). So GPU order is the *reverse* of the
source order in `draw()`. Resolved:

| # | pass | writes | our batches |
|---|---|---|---|
| 1 | **mask** — `mpZOnShape` (`*damA`) | **alpha only** | 11, 14, 18, 21 |
| 2 | **face + hair** — `face`, `ear(2)` | color + depth | 1, 4 |
| 3 | **composite** — `mpZOffBlendShape`, `DstA`/`InvDstA` | color only | 10, 13, 17, 20 |
| 4 | **erase** — `mpZOffNoneShape` (`*damB`) | **alpha only** | 12, 15, 19, 22 |
| 5 | rest of the model (P1) | color only | 0, 2, 3, 5, 6, 7, 8, 9, 16, 23 |

Step 1 deposits an eye/brow coverage mask in destination alpha, **z-tested**
against whatever was already drawn — that is what stops eyes appearing through
walls. Step 2 draws the bangs *without* touching alpha, so the mask survives
underneath them. Step 3 composites `out = eye·dstA + fb·(1−dstA)` with the depth
test off, which is how **the eyes read through the hair**. Step 4 zeroes the mask
so it cannot leak into later alpha-buffer effects.

`hideHatAndBackle` (`../tww/src/d/actor/d_a_player_main.cpp:1509-1531`) then
hides `face` and `ear(2)` for P1 so they draw exactly once — which is why those
two are pulled out of the opaque group and drawn early. Its comment names both
material strings verbatim (`:1512-1514`), so matching by manifest name is the
game's own contract, not our convention; `phase_06.md`'s per-batch isolation map
independently confirms they are batches 4 and 1.

The game leaves the four compositors *shown* for P1, where they redraw as a
no-op against the now-zero destination alpha. We simply do not draw them twice.

### Renderer affordances

- `BlendMode` (`src/renderer/pipeline.rs:181-187`) has exactly `Alpha` and
  `Opaque`. The Vulkan blend factors at `src/renderer.rs:3607-3610` are
  **hardcoded** to `SRC_ALPHA`/`ONE_MINUS_SRC_ALPHA`; `BlendMode` only toggles
  `blend_enable` (`:3604`).
- `color_write: [bool; 4]` is plumbed end-to-end and unit-tested
  (`vk_color_write_mask`, `src/renderer.rs:3524-3540`; test at `:5785-5804`) but
  **has never rendered anything** — `follow_up.md:146-147`. This phase is its
  first runtime exercise.
- The record loop (`src/renderer.rs:1825-1923`) preserves queue order verbatim
  with no state sorting, so a multi-pass sequence is just the order in which
  `queue_draw_index_range` is called.
- The color attachment is `B8G8R8A8_SRGB` (`src/renderer.rs:3218-3221`) with a
  real writable alpha channel, cleared to `[0,0,0,1.0]` (`:1754`), and the
  swapchain is created with `CompositeAlphaFlagsKHR::OPAQUE` (`:3343`) — so
  nothing outside the frame observes framebuffer alpha, and masking alpha writes
  globally is safe.
- `raster_state` cannot become a `PipelineConfigBuilder` field
  (`pipeline.rs:385-388`, the generated template emits a complete struct
  literal); everything flows through `with_raster_state`.

## Decisions

1. **Classify by state, not by name.** `playerInit` derives the three arrays
   from `(z_compare_enable, blend_type)` on the materials under the `CL_EYE` and
   `CL_MAYU` joints, never from `damA`/`damB` in the name. We do the same. The
   names then serve only as the assertion message.
2. **`face` and `ear(2)` are matched by name**, because the decomp identifies
   them by name and there is no state signature that distinguishes them from the
   other eight opaque materials.
3. **One write-mask rule**: alpha writes are enabled on exactly the mask and
   erase passes; every other material draws color-only. This is what the game
   does — `l_onCupOffAupPacket2` is the last P0 packet, so all of P1 runs with
   `alphaUpdate = 0` too — and it is one sentence instead of a list of
   exceptions.
4. **`DstAlpha` blends alpha with the same factors as color.** GX applies the
   blend expression to both. The alpha channel is masked off everywhere this
   blend is used, so the choice is unobservable; matching the hardware costs
   nothing and avoids a special case to explain later.
5. **The `Key::M` view is in scope**, as the phase's verification instrument
   rather than a convenience — see Step 5.
6. **Dolphin stays optional.** `tests.md:323-325` mandates it as P9's oracle,
   but the whole suite is unbuilt (`follow_up.md:172-186`). This phase verifies
   the pass *structure* against the decompiled source, which is stronger than a
   frame capture for structure and weaker for pixels, and says so rather than
   claiming a comparison it did not run.

## Step 1 — renderer: `BlendMode::DstAlpha`

- Add the variant to `src/renderer/pipeline.rs:181-187`, documented as GX's
  `GX_BL_DSTALPHA`/`GX_BL_INVDSTALPHA` and as only meaningful when something
  earlier in the same render pass has written destination alpha.
- `src/renderer.rs:3602-3611`: `blend_enable` becomes `raster_state.blend !=
  BlendMode::Opaque`, and the four factor calls become a match on the mode —
  `Alpha` keeps `SRC_ALPHA`/`ONE_MINUS_SRC_ALPHA`, `DstAlpha` uses
  `DST_ALPHA`/`ONE_MINUS_DST_ALPHA` for both color and alpha. `Opaque` keeps
  whatever the disabled path already passes.
- Extend the mapping tests at `src/renderer.rs:5754-5820` with the new variant,
  in the shape of the existing `color_write_mask_mapping` test.
- Nothing in `templates/` or `src/generated/` is touched, so the snapshot suite
  must come back byte-identical. Confirm rather than assume.

## Step 2 — example: classify the decals the way the game does

```rust
enum DecalRole { Mask, Composite, Erase }   // ZOn / ZOffBlend / ZOffNone

fn decal_role(material: &MaterialEntry) -> Option<DecalRole>
```

For `pe_mode == Translucent` only: `z_test` → `Mask`; else GX blend `Blend` →
`Composite`; else (`None_`) → `Erase`. `None` for everything opaque.

*Two corrections from implementation: the Rust variant is `mm::BlendMode::None`
— `None_` is only the serde/JSON spelling — and the return type must be
`anyhow::Result<Option<DecalRole>>`, not a bare `Option`, so `pe_mode`'s bail on
an unmapped GX mode propagates instead of collapsing into `None`.*

At setup, bail unless the counts are 4/4/4 and the translucent set is exactly
those twelve batches — the same assertion `playerInit` makes, and the thing that
will fire loudly if `--casual` or a future converter change perturbs the
material table.

## Step 3 — example: write masks and the real blend

In `raster_state` (`examples/toon_link.rs:470-511`), thread the role through and
set `color_write`:

- `Mask | Erase` → `[false, false, false, true]`
- everything else → `[true, true, true, false]`

In `blend_mode` (`:513-547`), map `(DestinationAlpha, InverseDestinationAlpha)`
to `BlendMode::DstAlpha`. **Delete the P7 approximation comment at `:526-535`**
— the precondition it documents (framebuffer alpha ≡ 1 at those pixels) is
exactly what this phase stops being true, on purpose. Replace it with a pointer
to this doc.

## Step 4 — example: the five-group draw order

Replace the `pe_mode` opaque/translucent partition (`:838-849`) with the
explicit five-group sequence from the table above, INF1 order preserved within
each group. Bail if `face` or `ear(2)` is missing rather than silently
degrading, and bail if any translucent batch was not consumed by a decal role.

Keep printing the resolved draw order to stdout the way `setup` already does
(`:857-870`); it is the cheapest regression witness this example has, and P7
used exactly that to confirm the two-pass reordering.

`pe_mode()` (`:119-126`) stays — it is still what separates group 5 from the
decals — but the `PeMode::Translucent` arm now feeds the role classifier.

## Step 5 — example: the mask view

`RenderMode::MaskWhite` draws **only the four `*damA` mask batches**, as solid
white on black.

*As merged: selected by picking `Mask White` in the debug window's `debug_mode`
radio group, not by a key. `ToonLink::mode` derives the `RenderMode` from the
debug mode rather than reading a control of its own, because
`render_unit_enum` renders every variant of a unit enum as a radio button with no
way to hide one — a separate toggle would leave `Mask White` selectable while the
draw order stayed `Hardware`, which paints the whole model white. Tying the two
to one control makes that state unreachable.*

The mask pass is invisible by construction under `Hardware` — decision 3 turns
its colour writes off and its entire product is destination alpha, which nothing
in this renderer can read back. So this is the only view that answers "what
coverage did the mask actually deposit", and that is the question every remaining
eye bug reduces to.

**What is white is exactly what composites.** The mask's TEV alpha is `eyeh.a` /
`mayuh.a` (see Measured facts), and the composite pass tests that same value with
`Greater 0` — same texel, same texture, same identity texcoord 0, because the
three passes of a feature are byte-identical geometry. So the white region is the
eye silhouette itself, not an approximation of it, and a mask/composite
disagreement is not expressible.

Mechanics:

- One extra pipeline per mask slot, **28 total**, held in a
  `Vec<Option<PipelineHandle>>` parallel to `pipelines` and indexed by
  `MaterialSlot` — `Some` only for `DecalRole::Mask`. It points at the *same*
  `UniformBufferHandle` as its `Hardware` twin, so the per-frame uniform write
  loop is unchanged.
- Its raster state deliberately drops the material's own: `Opaque` rather than
  the source-alpha blend (blending white *by* the coverage would put back the
  antialiased grey the discard exists to remove), and no depth test (nothing else
  is drawn, so an occluded mask would just be an invisible one).
- The white itself is `DebugMode.MaskWhite` in `toon_link.shader.slang` —
  `discard` where `tevOut.a <= 0`, else `float3(1.0)`. The discard is
  load-bearing: the `*damA` materials compare `Always`, so the GX alpha test
  never removes anything and a flat white would paint the decal *quad*, not the
  eye. It is the one case whose branch moves the `shader_branching_snapshots`
  count (`toon_link.frag.spv: 80 → 81`).

*Superseded during implementation.* This step originally built a second pipeline
for all 24 slots with the **pre-phase (P8) raster state** — default
`color_write`, dst-alpha → `Opaque` — plus a second `pe_mode`-ordered draw list,
48 pipelines, so `M` reproduced the black quad on demand. That A/B instrument
was removed once the phase landed: the misdiagnosis it existed to disprove is
settled in §"The correction this phase opens with", and reproducing a known-wrong
render is worth less than seeing the mask. Removing it also deleted the
`RenderMode` parameter that `blend_mode` carried purely to hold the deliberately
wrong `DstAlpha → Opaque` approximation, which was a standing invitation to read
it as a real mapping. The `pe_mode`-ordered list is gone; `pe_mode()` itself
stays, since `decal_role` calls it.

*(An earlier revision said "the 14 materials this change touches (the 12 decals
plus `face` and `ear(2)`)", 38 pipelines. That contradicted decision 3, which
states the write-mask rule globally — under it all 24 materials change
`color_write`, so 14 was never the touched set.)*

**No capture script exists.** `scripts/` has no capture tooling and the justfile
no recipe for it; `phase_08.md`'s capture path is recorded prose that must be
written from scratch before any of the numeric runtime gates below can run. This
phase does not ship it — the runtime items are judged by eye unless and until a
harness is committed. `phase_08.md`'s tooling note also records that the
screenshot portal closes the app's window between captures, which is why the mask
view is an in-app toggle rather than a second binary or a build flag.

## Test plan

Static:

- `cargo check --all-targets`, `just lint` (debug + release), `cargo fmt`
- `just test` — one snapshot line moves, `shader_branching_snapshots`
  `toon_link.frag.spv: 80 → 81` (the `DEBUG_WHITE` discard). No other snapshot
  and no codegen output changes: `ToonLinkParams` keeps its shape
- `just link-verify-p2` / `just link-verify-p3` green and
  `scripts/link_converted.sha256` unmoved (no converter or asset change)
- new unit test: `BlendMode::DstAlpha` → the expected Vulkan factors
- setup-time assertions: 4/4/4 decal roles; `face` and `ear(2)` both found

Runtime, on a real GPU with assets present:

1. **The quad is gone.** Whole-frame capture; sample the eye and brow regions.
   Before: exactly `(0,0,0)` over a rectangle. After: face skin tone everywhere
   the eye/brow silhouette does not cover. Numeric, the same method that pinned
   gamma to 0 LSB in P7 — not eyeballed.
2. **The eye reads correctly.** The `hitomi` pupil visible inside the `eyeh`
   lash silhouette; brows solid black exactly where `mayuh.1`'s alpha is nonzero
   and absent elsewhere. Side-by-side against noclip.
3. **The mask is load-bearing.** `M` shows four solid-white silhouettes on
   black — two eyes, two brows. Compare against debug mode 3 (final TEV alpha)
   under `Hardware`: same shape, same texel, same predicate. A white *rectangle*
   means the discard is not firing; a grey fringe means the blend is not
   `Opaque`.
4. **Eyes read through hair.** With `MODEL_SPIN` carrying the bangs across the
   eye, the eye stays visible. This is the functional point of the technique and
   the behavior `tests.md:323-325` names.
5. **Debt unblocked by the fix, closed here**: P7's `depth_write` visual
   isolation (`phase_07.md:443-447`, previously "masked by the eye-decal
   stacking") and P8's pupil `TEXMTX1` direction A/B via debug mode 9
   (`phase_08.md:870-880`, previously unobservable).
6. Validation sweep 16/16 with the layer confirmed loaded; **hot reload clean in
   each mode**; clean close via a real `WM_DELETE_WINDOW` with no VMA leak.

   Note only the *active* mode's 24 pipelines recompile on any given edit.
   `draw_frame` builds the recompile list from `pending_draws`
   (`src/renderer.rs:2184-2189`), and `ShaderChanges::events` drains the watcher
   channel (`src/shader_watcher.rs:18`, `try_iter`), so the edit event is
   consumed by whichever set was queued that frame. The inactive set keeps its
   existing SPIR-V — **not** until it is next drawn, but until an edit event
   arrives *while* it is being drawn.

   **No edit is ever lost.** `try_shader_recompile` compiles from the current
   source on disk (`create_from_atlas`, `:2646`) rather than replaying a queue,
   so a pipeline that sits out several edits catches up to all of them the first
   time it recompiles. Symptom is a stale render after toggling; the fix is one
   more save while in that mode. `raster_state` is preserved across the rebuild
   (`:2674-2676`), so a `MaskWhite` pipeline stays `MaskWhite`.

   This is pre-existing, not new here: `Q`/`E` isolation has always had it —
   isolate one batch and only that pipeline recompiles. P9 only made it
   reachable a second way, since before this phase all 24 pipelines were queued
   every frame. It bites hardest on the `DEBUG_WHITE` case itself: edit it while
   `Hardware` is live and the four mask pipelines do not see the edit until
   another save lands while `M` is held on.

   (The original "hot reload at 38 pipelines" was unachievable as written for
   two reasons: the count is 28, and no single edit ever rebuilds all of them.)

## Verification (exit checklist)

Static gates — **run and green**:

- [x] `cargo check --all-targets` / `just lint` (debug + release) / `cargo fmt` clean
- [x] `just test` green — 126 tests, **one snapshot line moved and reviewed**:
      `shader_branching_snapshots`, `toon_link.frag.spv: 80 → 81`, the
      `DEBUG_WHITE` discard. No template, converter or asset change; codegen
      output unmoved
- [x] Golden hashes unmoved — all 90 entries of `scripts/link_converted.sha256`
      verify. (There is no justfile recipe for this; run `sha256sum -c` from
      `assets/link/converted`.)
- [x] `BlendMode::DstAlpha` factor-mapping unit test passes
      (`blend_mode_mapping`, `src/renderer.rs`)
- [x] Draw order printed at startup matches the five-group table exactly:
      mask `[11, 14, 18, 21]`, face+hair `[1, 4]`, composite `[10, 13, 17, 20]`,
      erase `[12, 15, 19, 22]`, rest `[0, 2, 3, 5, 6, 7, 8, 9, 16, 23]` —
      derived purely from material state, with the 4/4/4 assertion live
- [x] 28 pipelines build; `just dev toon_link` runs with no Vulkan validation
      errors, and the startup legend prints the mask-view order `[11, 14, 18, 21]`

Runtime gates — **confirmed by eye 2026-08-28.** No capture harness exists
(see step 5), so these were judged at the window, not recorded:

- [x] Black rectangle gone — both eyes and both brows
- [x] Pupil visible; brows correct; noclip side-by-side recorded
- [x] `Mask White` shows four solid-white eye/brow silhouettes on black, matching
      the lash shape seen in `Tev Alpha` — no rectangle, no grey fringe, and
      **not** a white model (that would mean the draw order did not switch with
      the debug mode)
- [x] Eyes remain visible when the bangs cross them
- [x] The `batch` slider re-confirms `ear(2)` is the hair (risk 6). Note
      isolating any of the 8 mask/erase batches shows a **black frame by
      design** — colour writes are off; `dump_selection` says so
- [x] `depth_write` isolation and debug mode 9 pupil A/B recorded
- [x] Sweep 16/16, hot reload clean per mode, no VMA leak

## Risks / open questions

1. **Clear alpha is 1.0; GX's is 0.** `src/renderer.rs:1754` vs
   `../tww/src/JSystem/JFramework/JFWDisplay.cpp:41`, with the framebuffer at
   `GX_PF_RGBA6_Z24` (`:210`). The mask pass computes
   `A = a² + (1−a)·A_dst`, so our partial-coverage rim gets `1 − a(1−a)` where a
   zero-alpha background would give `a²` — a slightly stronger composite,
   confined to the antialiased rim of the lash silhouette. Fully transparent
   texels are unaffected: the composite's `Greater 0` alpha compare discards
   them before the blend. Note the game's own value there is whatever the
   *background* left in alpha, so there is no well-defined target to match.
   **Accepted and documented**; making the clear color configurable is not worth
   a renderer change for a rim.
2. ~~**Mask and composite footprints may differ.**~~ *Resolved before
   implementation*: the three passes of each feature are byte-identical
   geometry (same 12 positions, same UVs, same order — see Measured facts), so
   the mask covers the composite exactly and no fringe can leak.
3. **Alpha is per-sample under 8× MSAA**, and the color store op is `DONT_CARE`
   with an `AVERAGE` resolve (`src/renderer.rs:1770-1783`). All five groups run
   inside the one existing `cmd_begin_rendering`/`cmd_end_rendering`, so
   destination alpha lives exactly as long as it needs to. It is not readable
   across frames, and nothing here tries.
   *Refined during implementation*: `ENABLE_SAMPLE_SHADING = false`
   (`src/renderer.rs:63`), so the **alpha test runs per-pixel while blending and
   `color_write` apply per-sample**. The mask therefore deposits per-sample
   coverage and the composite reads it per sample — the antialiased edge we
   want, and *more* faithful than the console's per-pixel `GX_PF_RGBA6_Z24`. At
   the very silhouette edge a pixel can survive the discard while only some of
   its samples carry nonzero destination alpha. Not a bug; recorded so it is not
   rediscovered as one.

7. **The composite blends in linear space; GX blended in gamma space.** The
   attachment is `B8G8R8A8_SRGB`, so Vulkan linearizes the destination before
   blending, and the shader already `srgbDecode`s its source. `out = eye·dstA +
   fb·(1−dstA)` in linear differs slightly from the console's gamma-space blend
   at partial `dstA` — confined to the same antialiased rim as risk 1, and
   pre-existing (P8's mask pass already alpha-blended color). Alpha itself is
   untouched by sRGB encoding, so the mask→composite handoff is exact.

8. **The erase pass has no local observable.** `load_op(CLEAR)` runs every frame
   and nothing reads alpha after `cmd_end_rendering`, so *whether erase runs at
   all* is invisible here. What is observable is the write-mask fix: erase no
   longer paints black RGB. An honest gap alongside risk 4 — nobody should later
   claim to have "verified" the erase ordering.

9. **The composite's `Greater 0` alpha compare is load-bearing for
   correctness**, not an optimization. Because our clear is alpha 1.0, the mask
   leaves `A = a² + (1−a)·1 = 1 − a(1−a)` — destination alpha stays ≥ 0.75 over
   the *entire* 12-triangle quad, including where the texture is fully
   transparent. Only the shader-side discard
   (`shaders/source/toon_link.shader.slang:146-152`, which runs in every debug
   mode) stops the composite painting an opaque rectangle there: the black quad
   again, by a different route. Commented at `DecalRole::Composite` so a future
   "skip the alpha test in debug mode N" change trips over it.
4. **The mask's z-test has nothing to test against.** In-game it is z-tested
   against the background, which is what keeps eyes from showing through walls;
   our scene is Link alone on a cleared depth buffer, so step 1 always passes.
   The structure is faithful but that specific guarantee goes locally untested —
   an honest gap, not a defect. It would become testable if the example ever
   grew an occluder.
5. **BTP is still absent**, so the eyes never blink and the pupil never tracks;
   34 texture entries stay unloaded. Unchanged by this phase — and, per the
   correction above, no longer implicated in the black quad. It has no landing
   phase and stays in [`follow_up.md`](follow_up.md).
6. **`ear(2)` really is the hair.** Rests on the decomp comment at
   `d_a_player_main.cpp:1512-1514` plus P6's isolation map. Cheap to re-confirm
   with `Q`/`E` during bring-up; do it, because getting it wrong moves the wrong
   batch into the early group and the symptom (eyes compositing over the wrong
   surface) is subtle.

## Doc reconciliation

- `phase_07.md`: mark risk 1 **misdiagnosed** (the twelve batches are not BTP
  frames), and mark decision 2 + risk 3 (the dst-alpha → `Opaque` approximation
  and its precondition) **retired here**.
- `phase_08.md`: annotate the "Known and not a bug" note at `:1014-1019`, and
  close out the two outstanding items this phase unblocks.
- `link_rendering.md`: point the P9 row's eye-trick item at this doc; drop
  "missing BTP plus" from the black-quad explanation in the P7 row (`:649`);
  update §4.3's `BlendMode::DstAlpha` note and §4.5's "needs `color_write`
  exercised + per-pass shape-group toggling", which overestimated the work.
- `follow_up.md`: strike the `color_write`-has-no-runtime-test-object entry (§4)
  and the `BlendMode::DstAlpha` entry (§2).
- `risks.md`: add the eye/decal entry it has never had, or state that these
  risks live here.
- Settle `phase_05.md:162` ("P8") against `:566` ("P9") in favor of P9.

## Out of scope

BTP/BTK animation (blinking, expressions, pupil tracking), the `yamu` mask
model, the `checkFreezeState` and camera-angle paths that hide all twelve decals
outright, stencil, `--casual` clothes, the BCK-sampled pose.
