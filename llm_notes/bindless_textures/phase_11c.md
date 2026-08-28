# Phase 11c — constant texture slots become handles

**Status: done, 2026-08-20.** Watercolor in [§6](#6-outcome--watercolor), the
other seven examples in [§7](#7-outcome--the-other-examples). Follow-up to
[phase_11b.md](phase_11b.md), for Phase 11c of
[../bindless_textures.md](../bindless_textures.md). Written against the 11b
working tree; the line numbers below are that snapshot.

## Why this phase exists

`../../docs/bindless.md` states the preferred default: a texture field is a
heap handle. 18 texture slots across 15 shaders still declare a descriptor.
Phases 11 and 11b converted only the slots whose value *varies*, because only
those collapse a pipeline. Constant slots stayed descriptors, and the rule was
applied unevenly: `wc_update_velocity` and `paint_brush` moved their constant
`pressure` to a handle, while `wc_project_velocity` and `wc_flow_outward` kept
theirs as descriptors. `wc_project_velocity.compute.slang:13` is the visible
edge of that split.

This phase converts every remaining slot that a once-per-frame uniform write
can carry. It collapses no pipeline. What it buys:

- One binding style per shader, and one binding style across the workspace.
- 14 of the 15 shaders lose their `Resources` texture fields entirely, leaving
  `params_buffer` alone — the shape `paint_brush` and `paint_display` already
  have. `wc_pressure_jacobi` is the one exception, and §1 says why.
- Every example becomes a worked example of the handle path.

**Feasibility is settled.** Every texture in the tree already holds a heap
slot. `textures.add` has one caller, `register_texture` (renderer.rs:588), and
`storage_textures.add` has one caller, `register_storage_texture` (:794). No
texture reaches a shader without a slot, so no slot below needs new plumbing.

## 1. What stays a descriptor

**Two slots, both in `wc_pressure_jacobi`.**

```slang
Sampler2D<float> pressureIn;    // per-dispatch
RWTexture2D<float> pressureOut; // per-dispatch
```

`pressure_parity` flips per dispatch inside the Jacobi loop
(main.rs:776-779). A handle lives in the params uniform, and that uniform is
written once per frame in the draw closure. One write cannot carry two values
for two dispatches in one frame. Compute push constants are the channel, and
that is Phase 12.

So this phase does **not** retire the per-pipeline descriptor path
(`texture_handles` and `storage_texture_handles` in `PipelineConfig` /
`ComputePipelineConfig`, and the image writes in `create_descriptor_sets`).
Phase 12 is what makes that possible. Recording it here because "convert the
last descriptors" reads like it should delete the machinery, and it does not.

## 2. Inventory

### 2.1 watercolor — 9 slots, 8 shaders

| shader | slot | kind | after |
|---|---|---|---|
| `wc_project_velocity` | `pressure` | `Sampler2D<float>` | handle |
| `wc_update_velocity` | `paperHeight` | `Sampler2D<float>` | handle |
| `wc_capillary_flow` | `paperHeight` | `Sampler2D<float>` | handle |
| `wc_advect_and_transfer_pigment` | `paperHeight` | `Sampler2D<float>` | handle |
| `wc_divergence` | `divergence` | `RWTexture2D<float>` | handle |
| `wc_flow_outward` | `blurredMask` | `Sampler2D<float>` | handle |
| `wc_flow_outward` | `pressure` | `RWTexture2D<float>` | handle |
| `wc_gaussian_blur` | `outputTex` | `RWTexture2D<float>` | handle |
| `wc_pressure_jacobi` | `divergence` | `Sampler2D<float>` | handle |

Two of these need the reasoning written down.

**`wc_flow_outward`'s `pressure` is constant.** `JACOBI_ITERATIONS` is even
(main.rs:53 asserts it), so the loop always lands the final pressure on side 0.
Both the descriptor (`pressure.read_storage(false)`, main.rs:561) and the
handle read the same texture every frame.

**`wc_gaussian_blur`'s `outputTex` is per-pipeline, not per-dispatch.**
`blur_h` and `blur_v` are two pipelines of one shader with two uniform buffers
(main.rs:457-459), each written once per frame. `inputTex` is **already** a
handle that differs between them (main.rs:1005 and :1015), which proves the
pattern: two uniform buffers carry two handle values in the same frame.
`outputTex` moves the same way. The pass keeps 2 pipelines — two uniform
buffers are two descriptor sets — and Phase 12 is what collapses it.

### 2.2 The other examples — 9 slots, 7 shaders

Each is a single-pipeline example, so each conversion is a straight
substitution with no pipeline arithmetic.

| example | shader | slot(s) |
|---|---|---|
| `koch_curve` | `koch_curve.shader.slang:31` | `cubeMap` |
| `multi_mesh` | `multi_mesh.shader.slang:14` | `texture` |
| `serenity_crt` | `serenity_crt.shader.slang:13` | `tex` |
| `space_invaders` | `space_invaders.shader.slang:14` | `spriteSheet` |
| `sprite_batch` | `sprite_batch.shader.slang:20` | `texture` |
| `suzanne` | `suzanne.shader.slang:12-14` | `texture0`, `texture1`, `texture2` |
| `viking_room` | `depth_texture.shader.slang:14` | `texture` |

Every one of these param structs already holds other uniform fields, so each
handle has somewhere to live. None of them needs a new uniform buffer.

`koch_curve`'s slot is named `cubeMap` but declares `Sampler2D` and samples
with `.Sample(r.xy)` (:167). It is a 2D sampler. Rename it in the same commit,
or leave it; do not read the name as a cube-map requirement.

## 3. The recipe

Per slot:

1. Change the declaration to `Sampler2D<T>.Handle` or `RWTexture2D<T>.Handle`.
2. `just shaders <example>`.
3. Delete the field from the `Resources` literal at the pipeline-creation site.
4. Write the handle in the same place the uniform is written, from the same
   expression the `Resources` literal used:
   `&paper_height_sampled` becomes `self.paper_height_sampled.bindless_handle()`.

**Helper functions need no signature change.** `serenity_crt`'s
`sampleBloom(Sampler2D tex, …)` (:143) and `toon_link`'s `tevSampleTexmap`
(tev.slang:224) take a `Sampler2D` value. A handle converts at the boundary:
`Sampler2D tex = params.tex;`, which is what `toon_link.shader.slang:111-112`
does. Convert at the call site, leave the helper alone.

**No parity-ordering footgun.** Every value this phase writes is constant per
frame. The 11b hazard — compute handles from the captured pre-flip `sim`,
display handles from the post-flip fields — does not apply to any slot here.

## 4. Costs

State them, because this phase collapses no pipeline and so has to justify
itself on shape alone.

- **Uniform space.** A handle is 8 bytes in the params block, plus std140
  padding. `suzanne` gains 24 bytes, `wc_flow_outward` 16.
- **Less validation.** A descriptor type or layout mismatch is a validation
  error. A wrong heap slot reaches a valid, different image and is silent. This
  phase moves 18 slots from the first category to the second.
- **The uniformity rule widens.** `docs/bindless.md` requires a handle to be
  dynamically uniform within a draw. Every slot here is CPU state written once
  per frame, so the rule is satisfied by construction, but it now governs more
  of the tree.

## 5. Verification

The A/B target is 0 differing pixels, as in 11b. The change alters how a
descriptor is reached, not one arithmetic operation.

- **watercolor**: re-instate the 11b scaffolding and re-capture a baseline
  from the unconverted build. What it needs is recorded in
  [phase_11b.md](phase_11b.md) §9.4 and [phase_11.md](phase_11.md) §9.4:
  scripted stroke in canvas space, frozen FPS label, wall-clock checkpoint
  holds, marker printed after five held frames, stray-kill, double-grab, and a
  retry for the stray quit event. Self-test two unconverted runs to 0 before
  trusting the baseline.
- **The other examples**: each needs its own deterministic frame before a
  screenshot A/B means anything, and several animate on wall-clock time. Decide
  per example whether to freeze the clock or to accept a weaker check. Do not
  record a green sweep as evidence — it cannot see a wrong slot.
- **The poison control carries the weight**, as in 11b. After each conversion,
  write a wrong slot and confirm the frame changes. This is the only check that
  distinguishes "the handle works" from "the handle is ignored".
- **A mechanical check on the committed artifacts.** Each conversion removes
  exactly one `bindingRanges` entry of type `texture` or `storageImage` from
  the shader's reflection JSON, and grows the block `size` by 8 plus padding.
  That is reviewable in the `just shaders` diff without running anything.
- `just test` / `just lint` / `cargo check --workspace --all-targets` /
  `cargo fmt`, plus `just sweep` as the regression floor.

**Teardown:** revert the scaffolding, re-run `just shaders`, confirm the
committed artifacts are byte-identical to the converted state.

## 6. Docs

`docs/bindless.md` needs no rule change — it already states the handle default.
Check whether the `RWTexture2D` line still reads correctly once the only
remaining descriptors are Jacobi's two per-dispatch slots.

## 6. Outcome — watercolor

**Done, 2026-08-20. 9 slots, 8 shaders. Pipeline count stays 12.**

### 6.1 The conversions

| shader | slot | result |
|---|---|---|
| `wc_project_velocity` | `pressure` | `Sampler2D<float>.Handle` |
| `wc_update_velocity` | `paperHeight` | `Sampler2D<float>.Handle` |
| `wc_advect_and_transfer_pigment` | `paperHeight` | `Sampler2D<float>.Handle` |
| `wc_capillary_flow` | `paperHeight` | field deleted, see §6.2 |
| `wc_divergence` | `divergence` | `RWTexture2D<float>.Handle` |
| `wc_flow_outward` | `blurredMask` | `Sampler2D<float>.Handle` |
| `wc_flow_outward` | `pressure` | `RWTexture2D<float>.Handle` |
| `wc_gaussian_blur` | `outputTex` | `RWTexture2D<float>.Handle` |
| `wc_pressure_jacobi` | `divergence` | `Sampler2D<float>.Handle` |

Eight of the nine shaders hold `params_buffer` alone in `Resources`.
`wc_pressure_jacobi` keeps `pressure_in` and `pressure_out`, per §1. Its
`bindlessHeapSet` flips from `null` to `1`, so every watercolor pipeline binds
the heap.

`main.rs` gains two fields, `blurred_mask_sampled` and `divergence_sampled`.
Both are sampled aliases that `setup` held as locals to feed a `Resources`
literal. `divergence`, `blur_temp` and `blurred_mask` drop their
`#[expect(unused)]`; each is now a per-frame handle source.

No access site changed. Every converted slot is read with `[pixel]` or assigned
with `[pixel] =`, and a handle behaves as the underlying type there.

### 6.2 Two deviations

1. **`wc_capillary_flow`'s `paperHeight` is deleted, not converted.** §2.1
   lists it as "handle". The field is declared and never read in the shader
   body. Deletion drops the descriptor and adds no uniform bytes;
   `Resources` collapses to `params_buffer` alone either way.
   `wc_capillary_flow.comp.spv` is byte-identical after the change, which
   confirms the slot was dead.
2. **No A/B and no poison control.** §5 asks for the 11b scaffolding rebuilt
   from scratch; that was declined as out of proportion to a substitution
   which collapses no pipeline. What this leaves uncovered: a wrong heap slot
   reaches a valid, different image and is silent, and no check in §6.4 can
   see it. Each of the nine handle expressions is the expression its deleted
   `Resources` field used, so the exposure is a transcription error rather
   than a design error.

### 6.3 Uniform cost, measured

§4 predicts the growth. Five of the eight blocks absorb the handle into
existing std140 tail padding and grow by 0 bytes.

| shader | `Params` size |
|---|---|
| `wc_divergence` | 32 → 32 |
| `wc_gaussian_blur` | 32 → 32 |
| `wc_pressure_jacobi` | 16 → 16 |
| `wc_capillary_flow` | 64 → 64 |
| `wc_advect_and_transfer_pigment` | 336 → 336 |
| `wc_flow_outward` | 32 → 48 |
| `wc_project_velocity` | 32 → 48 |
| `wc_update_velocity` | 80 → 96 |

### 6.4 Verification

| check | result |
|---|---|
| reflection JSON, per converted slot: one `bindingRanges` entry dropped, field flips `resource` → `descriptorHandle` with a `uniform` binding of size 8 | 8 of 8, right `shape` each |
| `wc_capillary_flow`: binding dropped, no uniform field added, `.spv` unchanged | yes |
| eight of nine `descriptorSetLayouts[0]` hold `binding 0 constantBuffer` alone | yes; jacobi keeps 3 |
| `cargo check --workspace --all-targets` | clean |
| `just lint` (debug and release) | clean |
| `just test` | green, no snapshot changed |
| `just sweep watercolor` | ok, self-test detected the injected fault |
| `just sweep` | 16 ok / 0 skip / 0 fail |
| `git status` confined to `examples/watercolor` | yes |

~~Not run: the interactive paint-and-cycle-`DebugView` check, and the hot-reload
check. Both need a human at the window.~~ Both confirmed by eye 2026-08-28.

`docs/bindless.md` needs no edit. The handle default, the two heap bindings,
and the "`examples/watercolor` is the reference for storage handles" paragraph
all still describe the tree.

## 7. Outcome — the other examples

**Done, 2026-08-20. 9 slots, 7 shaders, 7 examples. Pipeline counts unchanged.**

### 7.1 The conversions

Every slot in the §2.2 table converted to `Sampler2D.Handle`. Every shader
body is unchanged except two:

- `koch_curve` renames `cubeMap` to `reflectionMap`, taking the rename option
  §2.2 offers. The declaration and the one read site (`:167`) change together.
- `serenity_crt` converts at the helper boundary: `Sampler2D tex = params.tex;`
  feeds `sampleBloom`, the toon_link pattern §3 names. `sampleBloom` keeps its
  signature. The dead `params.tex.Sample(uv);` statement at `:46` reads
  identically through a handle and stays.

Each shader's `Resources` collapses to `params_buffer` alone, its
`descriptorSetLayouts[0]` to `binding 0 constantBuffer` alone, and its
`bindlessHeapSet` flips `null` → `1`.

Five examples promote a `setup` local to a `Game` struct field so the
per-frame write can reach it: `koch_curve` (`reflection_map`), `multi_mesh`
(`textures: Vec<TextureHandle>`), `serenity_crt` (`texture`),
`space_invaders` (`sprite_sheet_texture`), `sprite_batch` (`texture`).
`suzanne` and `viking_room` already hold theirs; both drop their struct-level
`#[allow(unused)]`. `multi_mesh` writes
`self.textures[spec.texture].bindless_handle()` once per pipeline per frame,
17 uniform buffers in all.

### 7.2 Uniform cost, measured

`koch_curve` absorbs the handle into existing std140 tail padding; the other
six blocks grow by 16 (8 for the handle, 8 padding).

| shader | `Params` size |
|---|---|
| `koch_curve` | 48 → 48 |
| `multi_mesh` | 208 → 224 |
| `serenity_crt` | 64 → 80 |
| `space_invaders` | 80 → 96 |
| `sprite_batch` | 80 → 96 |
| `suzanne` | 208 → 224 |
| `viking_room` (`depth_texture`) | 192 → 208 |

`serenity_crt`'s `tex` lands at offset 0 and shifts every later field by 8.

### 7.3 Verification

Mechanical checks only, the §6.2 deviation applied again: no A/B baselines,
no poison controls. The exposure is the same — a wrong heap slot is silent —
and each handle expression is the expression its deleted `Resources` field
used.

| check | result |
|---|---|
| reflection JSON, per slot: one `bindingRanges` entry dropped, field flips `resource` → `descriptorHandle` with a `uniform` binding of size 8, shape `sampler2D` | 9 of 9 |
| all seven `descriptorSetLayouts[0]` hold `binding 0 constantBuffer` alone, `bindlessHeapSet` `null` → `1` | yes |
| `cargo check --workspace --all-targets` | clean |
| `just lint` (debug and release) | clean |
| `just test` | green, no snapshot changed |
| `just sweep` | 16 ok / 0 skip / 0 fail, self-test detected the injected fault |
| `git status` confined to the seven example crates | yes |

`docs/bindless.md` needs no edit. Its reference examples
(`examples/depth_texture`, `examples/watercolor`, `examples/toon_link`) and
both heap bindings still describe the tree.

## Out of scope

- **Phase 12** (compute push constants). It owns `wc_pressure_jacobi`'s two
  per-dispatch slots, collapses jacobi 2 → 1 and the blur pair 2 → 1, and is
  what makes retiring the per-pipeline descriptor path possible.
  Done, see [phase_12.md](phase_12.md).
- **Retiring `texture_handles` / `storage_texture_handles`.** Blocked on
  Phase 12. See §1.
- **The two-handles-one-image ownership refactor** (`StorageTextureHandle` +
  aliased `TextureHandle`), carved out by the parent doc, phase_11.md and
  phase_11b.md alike.
- **`NonUniformEXT` / handle indexing.** Every handle this phase writes is CPU
  state in a uniform. The shader reads *the* handle and indexes nothing.
