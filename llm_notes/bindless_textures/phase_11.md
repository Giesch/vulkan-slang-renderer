# Phase 11 — watercolor: the spike and the reads-only collapses

Detailed plan for Phase 11 of [../bindless_textures.md](../bindless_textures.md).
**Status: done, 2026-08-18.** Written against `718486e`; the line numbers below
are that snapshot. The spike gate passed on all five criteria, so Phase 11b is
viable. See [§9](#9-outcome) for the measurements and for the four places this
plan was measured wrong. This doc covers the storage-image **spike** and the **unconditional
read-side collapses** (25 pipelines → 20). The storage half — a second heap
binding, two feature bits, a new handle shape, and the per-pass migration to
12 — is **Phase 11b**, split off the way 8b was from 8, and its doc gets written
only if [§2's gate](#2-the-decision-gate) passes. If the gate fails, this phase
ends at 20, the evidence is recorded here, and storage-image handles stay a
non-goal.

The planning survey behind this doc measured watercolor pass-by-pass and found
the parent doc's Phase 11 sketch wrong in four load-bearing ways — pipeline
count, reads-only payoff, the "no per-draw channel needed" claim, and what the
storage spike actually has to answer. [§8](#8-corrections-to-bindless_texturesmd)
lists the corrections; the numbers below are the measured ones.

## Why this phase exists

Watercolor runs **25** pipelines, not the parent doc's 22: 18
`create_compute_pipeline` call sites, one of which sits in a 2×2 loop
(main.rs:655-692), give 21 compute pipelines, plus 4 graphics display pipelines
from a second 2×2 loop (main.rs:721-746). The parent doc counted call sites.

The duplicates are not all the same kind, and the collapse comes in three rungs:

| rung | what collapses | pipelines |
|---|---|---|
| today | — | 25 |
| **Phase 11** (this doc) | duplicates that vary by *sampled reads alone*: display 4→1, divergence 2→1, blur_h 2→1 | **20** |
| Phase 11b (gated on §2) | duplicates that also vary by *storage writes*: brush, update_velocity, project_velocity, flow_outward, capillary_flow 2→1 each, advect 4→1 | **12** |
| Phase 12 (out of scope) | duplicates that vary *per dispatch within a frame*: jacobi 2→1, blur h/v 2→1 | **10** |

Rung one needs nothing the heap doesn't already serve — every varying slot is a
combined-image-sampler read, which is binding 1, the one binding
`DescriptorHeap` creates. Rung two needs storage-image handles, which are
unmeasured; §1 measures them. Rung three needs a per-dispatch data channel that
does not exist — uniform buffers here are ringed and written once per frame in
the draw closure (main.rs:998-1155), and a different uniform buffer means a
different descriptor set means a different pipeline. That channel is compute
push constants, i.e. Phase 12, and jacobi/blur are its honest trigger
([§6](#6-the-remainder-is-phase-12s)).

Parity is CPU state: `sim_parity` and `deposit_parity` flip once per frame
(main.rs:972-973), so for every rung-one and rung-two pass the shader reads
*the* handle the app wrote, selecting nothing. The uniformity hazards in
[../../docs/bindless.md](../../docs/bindless.md) don't apply. The exception is
`pressure_parity`, which flips per Jacobi dispatch *inside* a frame
(main.rs:923-927, `JACOBI_ITERATIONS = 2`) — that is what makes jacobi rung
three.

## 1. The spike — storage-image handles, compile-only

Method per [phase_0_spike.md](phase_0_spike.md): a throwaway `#[cfg(test)]` mod,
this time in `crates/slang-reflection/src/lib.rs` beside
`prepare_reflected_compute_shader` (:186) — the entry points moved there in the
workspace split. Compile ad-hoc compute sources, walk the reflection printing
`kind` / declared `full_name()` / categories / offsets, dump SPIR-V to
`/tmp/bindless_spike_11/`, disassemble with `spirv-dis`. Nothing committed,
scaffolding deleted afterwards. Use the vendored slang fork pinned in the root
`Cargo.toml`, not a system `slangc`.

Phase 0 established the heap binding map — 0 sampler, 1 combined image sampler,
2 sampled image — and explicitly never exercised storage images ("Binding 3
'unknown' was not exercised"). This spike closes that, plus one hole in the
*read* story nobody has measured. Variants:

- **(a) Control, no compilation needed:** `spirv-dis` the committed
  `examples/watercolor/shaders/compiled/paint_brush.comp.spv` (all-`RWTexture2D`).
  Records the classic path's `OpTypeImage` format operands for
  `RWTexture2D<float>` / `<float4>` and its capability list. This is the parity
  baseline for criterion 3 below — the renderer requests no
  `shaderStorageImage{Read,Write}WithoutFormat` feature anywhere, and the
  classic path runs validation-clean, so whatever the control shows is the
  budget the handle path must stay inside.
- **(b)** `RWTexture2D<float>.Handle` in a param block, accessed with `[]`
  read-modify-write (`tex[id] = tex[id] + x` — the flow_outward/project pattern).
- **(c)** `RWTexture2D<float4>.Handle`, same shape.
- **(d) The architecture question:** both element types in **one** shader.
  `RWTexture2D<float>` and `<float4>` are different `OpTypeImage`s, so Slang
  either emits one storage runtime array at one binding (descriptor-aliased) or
  one array *per format* at several bindings. Count the storage
  `OpTypeRuntimeArray`s and their `Binding` decorations.
- **(e)** Mixed `Sampler2D<float>.Handle` + `RWTexture2D<float>.Handle` in one
  param block — combined stays at binding 1, storage binding number recorded.
- **(f)** `Sampler2D<float>.Handle` + `Sampler2D<float4>.Handle` in one shader.
  This gates the *read* work in §4, not Phase 11b: Phase 0 and every fixture
  since only ever measured the `float4` element type, and watercolor's reads are
  `<float>`. The reflection gate accepts any `Sampler2D<` prefix
  (crates/slang-reflection/src/reflection/parameters.rs:659-666) without
  inspecting the element type — this variant is what proves that acceptance
  sound, i.e. both element types share the one combined array at binding 1.
- **(g)** A handle-typed texture passed to a helper function taking a
  `Sampler2D<float>` / `RWTexture2D<float>` parameter — the
  `watercolor_common.slang:7` `bilinearSampleR` pattern and advect's
  `advectAndTransferGroup`.

Per variant, record: the reflection view (a `uint2` in the `Uniform` category,
size/align 8, identity on the declared `full_name()` —
`DescriptorHandle<RWTexture2D<...>>`), the SPIR-V view (set/binding of every
heap array, `OpTypeImage` operands including the format, the capability list),
and whether it compiles at all.

## 2. The decision gate

Phase 11b gets written only if **all** of:

1. Every `RWTexture2D<T>.Handle` variant compiles and reflects exactly like the
   sampler handles do — `uint2`, `Uniform`, size 8, identity on the declared
   `full_name()` — so `DescriptorHandleShape` generalizes to a second variant.
2. Variant (d) emits **one** storage runtime array at **one** binding, and that
   binding number is identical across (b)-(e) — fixed by Slang the way
   0/1/2 are. Per-element-type or per-format arrays at distinct bindings →
   **fail**: the one-binding-per-descriptor-type heap design breaks, and no
   renderer-side work fixes a shape the compiler won't emit.
3. Format operands through the handle match control (a) for the same declared
   type, and no capability appears that the control doesn't already carry
   (`RuntimeDescriptorArray` excepted — requested since Phase 2). ~~A divergence —
   e.g. `Unknown` where the control says `R32f` — pulls
   `shaderStorageImage{Read,Write}WithoutFormat` feature work into Phase 11b's
   scope~~ — **there is no divergence, and the feature work is the wrong fix
   anyway** ([§9.3](#93-what-the-spike-corrected-in-this-doc)). Both sides say
   `Unknown`, and under Vulkan 1.3 that is governed per format, not per device
   feature.
4. The `[]` read-modify-write in (b) lowers to a heap `OpAccessChain` +
   `OpImageRead`/`OpImageWrite`, and the helper-parameter variant (g) compiles.
5. Variant (f): both `Sampler2D` element types share the one combined array at
   binding 1.

Criteria 1-4 failing closes the storage half: this phase ends at 20 pipelines,
the disassembly goes in [§9](#9-outcome), and the parent doc's non-goal stands
with evidence instead of a question mark. ~~Criterion 5 failing is different — it
blocks §4 too, so stop before converting anything and re-plan; nothing else in
this doc survives that result unchanged.~~ **Overstated, and moot.** Criterion 5
passed. Had it failed, §4.1 would have been untouched (it is all `float4`), and
§4.2/§4.3 had a fallback the plan missed: declare the reads as untyped
`Sampler2D.Handle` and take `.x`, which is what `paint_display` already does to
the R32_SFLOAT `wet_mask`.

## 3. Verification scaffolding — deterministic strokes and the baseline

The parent doc's "compare frames against the pre-migration build" is impossible
as written: the sim is all-zero without brush input, and a wrong ping-pong
handle is invisible on a blank canvas. So, before any conversion, temporary
scaffolding (reverted at the end, the Phase 3-8 precedent — measurement, not a
deliverable):

- A scripted stroke path replacing the mouse state in `update`, as a pure
  function of the frame index — a diagonal drag over a few dozen frames is
  enough to light every pass, including brush and advect.
- ~~**Fix `dt` to a constant.** Wall-clock `dt` makes every run unique — the same
  class of correction phase_08 §6 records for `elapsed`.~~ **Already true:**
  `DT` is a `const` (main.rs:56). The wall-clock input that did break the A/B
  was the FPS label ([§9.4](#94-what-the-scaffolding-actually-needed)).
- Run a fixed frame count; capture at three checkpoints — mid-stroke (~frame
  30), post-stroke (~120), late-sim (~300) — so an early-pass error isn't
  washed out by diffusion and a slow-pass error isn't missed by an early
  capture. Capture via the Phase 6 route: real GPU, `SDL_VIDEODRIVER=x11`,
  `import -window` against `xwininfo -root -tree` (the portal screenshot recipe
  fails in a non-interactive session).

Capture the baseline from the **unconverted** build with the scaffolding in.
Every later A/B compares against these three images; the scaffolding stays in
the tree until teardown so both sides of every comparison run the same input.

## 4. The reads-only collapses

One sub-item per pass, in this order, each A/B'd against the baseline before
the next starts. The shared recipe: change the *varying* `Texture2D<float>`
declarations to `Sampler2D<float>` (constant slots stay put), replace the
per-pipeline binding with a `Sampler2D.Handle` field in the param block,
`just shaders watercolor`, delete the duplicate `create_compute_pipeline` call
and the array indexing in main.rs, write the handle in the draw closure — then
the A/B plus the poison control (§7).

**The parity-ordering footgun, once for all three:** compute dispatches select
pipelines with the *pre-flip* parity (`sim`, captured at main.rs:906), the
display with the *post-flip* value (main.rs:997), and the flips sit between
them (main.rs:972-973) — but every uniform write runs later, inside the draw
closure. When handles move into the uniforms, the closure must write
compute-pass handles from the captured pre-flip `sim` and display handles from
the post-flip fields, or every pass reads its own output. The A/B catches this
mistake; this paragraph exists so it doesn't have to.

`[]` indexing lowers to `OpImageFetch` and ignores the sampler, so the
`Texture2D<float>` → `Sampler2D<float>` conversions below change no filtering
anywhere. The samplers already exist: `storage_texture_as_sampled`
(renderer.rs:816-852) creates one per alias and routes through
`register_texture` (:577-589), so all 26 of watercolor's sampled aliases hold
heap slots today and `TextureHandle::bindless_handle()` already answers for
each.

### 4.1 display, 4 → 1

First, because it is the *safest* consumer in the whole example, not just this
phase: it is graphics — the path toon_link already proved end to end — and all
five of its textures are already `Sampler2D`, accessed only with `.Sample`
(paint_display.shader.slang:20-24), so the shader change is **five field
declarations and zero access sites**. What it isolates is the one new thing:
handle plumbing through watercolor's uniform writes.

The 2×2 loop at main.rs:721-746 becomes one `create_pipeline` call; `wet_mask`
and the three `deposit_*` fields become handles written per frame from the
post-flip parity (`wet_mask.sampled[parity]`, `deposit_*_sampled[!deposit]` —
the same selections the loop bakes in today); `paper_height` can stay
descriptor-bound or move too — move it, so one pipeline has one binding style.
`display_idx` (main.rs:997) and the `display_pipelines` array (:159) go away.

### 4.2 divergence, 2 → 1

The **first compute-stage handle in the workspace**. The compute side is
already wired — `cmd_bind_texture_heap` runs before every dispatch
(renderer.rs:1670-1675), and compute reflection threads `has_bindless_handle`
symmetrically (crates/slang-reflection/src/reflection.rs:40-46) — but no
fixture, example or test has ever exercised it end to end. Divergence is the
smallest possible first: two varying sampled reads (`uIn`, `vIn`,
wc_divergence.compute.slang:10-11), one constant storage write, no helpers, no
uniforms beyond `gridSize`.

Same item, close the fixture gap for good: add
`crates/cli/fixtures/alignment/handle_params.compute.slang` — a
`Sampler2D<float>.Handle` in a compute param block, modeled on
`pointer_params.compute.slang`, snapshot-reviewed via `just insta`. The
compute-handle codegen path currently has **no** fixture; this is the permanent
regression pin, independent of watercolor.

### 4.3 blur_h, 2 → 1

~~The first helper-signature change: `bilinearSampleR`
(watercolor_common.slang:7-37) takes `Texture2D<float>` and must take
`Sampler2D<float>` … if it turns out separable, it lands
here anyway as the smallest carrier.~~ **It is separable, and it was left
alone.** `wc_gaussian_blur` reads `params.inputTex[...]` directly and never
calls `bilinearSampleR`. Only `wc_update_velocity` calls it, and that pass keeps
2 pipelines either way, because its varying slots include the storage writes
`u_out`/`v_out`. Changing the signature would convert four more declarations
plus `interpolateU`/`interpolateV` for no collapse. It is recorded as a
[follow-up](#follow-up) instead. blur_h's one varying slot is `inputTex`
(wc_gaussian_blur.compute.slang:13, bound to `wet_mask.read_sampled(parity)`),
so the pair at main.rs:603-610 collapses; `blur_v_pipeline` (main.rs:621) keeps
its own pipeline — same shader, different `direction` uniform, which is §6's
territory, not a texture.

End state of §4: **20 pipelines**, every remaining duplicate blocked on either
storage handles (Phase 11b) or a per-dispatch channel (Phase 12).

## 5. What Phase 11b would contain (sketch, not plan)

Recorded so the gate has a visible other side; the real plan is written as
`phase_11b.md` after the spike passes, with the measured binding number in
hand.

- **Heap:** a second binding in `descriptor_heap.rs` at the spike-measured
  number — the `bindings` / `binding_flags` / `pool_sizes` arrays (:47-52,
  :56-58, :68-70) each gain a parallel entry (Vulkan requires
  `bindingCount == pBindingFlags.len()`), plus a storage insert path (image
  view + `GENERAL`, no sampler) with its **own** monotonic slot counter — slots
  in a different binding are an independent index space.
- **Device:** `shader_storage_image_array_dynamic_indexing` on the core
  features builder (renderer.rs:3758) and
  `descriptor_binding_storage_image_update_after_bind` on the 1.2 builder
  (:3783), both mirrored in the gate list (:3403-3438); `undersized_limits`
  (:3547-3583) gains `maxPerStageDescriptorUpdateAfterBindStorageImages` and
  `maxDescriptorSetUpdateAfterBindStorageImages`; plus whatever criterion 3
  measured about format features.
- **Reflection/codegen:** a second `DescriptorHandleShape` variant, the shape
  gate (parameters.rs:659-666) accepting `RWTexture2D<`, the codegen arm
  (build_tasks.rs:1010-1020), a marker in `renderer/bindless.rs` **and the
  check_crate stub** — phase_08 §2.2 called the stub the easiest thing in the
  whole phase to miss, and it recurs here — and fixtures with named-snapshot
  expectations.
- **Migration**, one pass per A/B cycle against the §3 baseline, easiest mixed
  pass first: project_velocity → update_velocity → flow_outward →
  capillary_flow → brush (all-storage — the pass the read work could never
  touch) → advect last (15 varying slots, the `SampleLevel` reads already
  `Sampler2D<float4>`, both helper signatures). **Jacobi is deliberately not
  converted**: with handles it would still need two per-parity uniform buffers,
  which is still two descriptor sets, which is still two pipelines — conversion
  churns the one pass whose parity flips per dispatch and buys nothing.

End state: **12 pipelines**.

## 6. The remainder is Phase 12's

Record, don't do. The last two duplicates need data that varies *between
dispatches of the same pipeline in one frame*: jacobi's `pressure_parity` flips
per dispatch (main.rs:923-927), and the two `wc_gaussian_blur` dispatches
differ by the `direction` uniform (main.rs:940-950). That is verbatim the
parent doc's "honest trigger" for Phase 12 — and its current claim that
watercolor's duplicates "a param-block handle already collapses with no
per-dispatch channel at all" is false for exactly these two
([§8](#8-corrections-to-bindless_texturesmd)). Note also that Phase 12's
push-clobber hazard is live here, not theoretical: watercolor records draws and
dispatches into one command buffer, so once both bind points push, interleaving
matters. Compute push constants are a full reflection + codegen + renderer
phase with their own verification burden; pulling them in as a freebie is the
thing Phase 12's own text warns against.

## 7. Verification

Evidentiary weight, strongest first — a green sweep proves nearly nothing here,
per the parent doc:

1. **A/B frame identity plus the poison control.** Against the §3 baseline,
   `compare -metric AE` with a target of **0**: the migration changes how
   descriptors are reached, not one arithmetic op, so bit-identity is the
   honest target. If presentation effects make 0 unattainable, fall back to a
   bounded diff and record it as weaker evidence. And the control that carries
   the real weight: identity alone cannot distinguish "handles work" from
   "handles never read, stale binding still doing the work" — so after each
   collapse, temporarily write a *wrong* slot into one handle field and confirm
   the frame **changes**. A wrong heap index reads a valid, different texture;
   validation is structurally silent on it. This is the Phase 6 decoy-slot
   check, per pass.
2. **Validation layers** (via `just sweep` and live runs): catch layout, type
   and unbound-set errors. Cannot catch a wrong slot.
3. **`just sweep` green**: the regression floor. Watercolor is a simulation, so
   this mostly proves the code ran, not that it ran right.
4. **`just shaders watercolor` / `just test` / `just lint`** plus snapshot
   review: the §4.2 fixture's new snapshots reviewed via `just insta`, never
   blind-accepted; the only pre-existing snapshot expected to change is the
   alignment-tests atlas (one new fixture = additive lines), the phase_08
   precedent.

**Teardown:** revert the §3 scaffolding, re-run `just shaders watercolor` and
confirm the committed artifacts are byte-identical, final `just sweep`.

## 8. Corrections to `../bindless_textures.md`

Made in the parent doc's Phase 11/12 sections as part of landing this plan, in
the house strikethrough style:

1. ~~"22 pipelines → 10"~~ — wrong at both ends. 25 runtime pipelines (a
   creation site sits in a 2×2 loop; call sites ≠ pipelines; +4 graphics), and
   the ladder is 25 → 20 (this phase) → 12 (11b) → 10 (12).
2. ~~"exactly 2×" / "every duplicate is pure descriptor duplication"~~ — advect
   is ×4, blur_v ×1, and the jacobi/blur duplicates are per-dispatch *data*
   duplication the heap cannot erase.
3. ~~"needs no per-draw channel"~~ — true for the once-per-frame parities,
   false for jacobi/blur; without a per-dispatch channel the floor is 12.
4. ~~reads-only "collapses the pipelines that vary by read target, which is
   most of them"~~ — exactly three sites vary by reads alone; brush's varying
   slots are all storage, so it gains nothing from reads-only.
5. Add: the best first consumer is the **display quad** — graphics, already
   `Sampler2D` / `.Sample`-only, zero access-site changes — not a compute pass.
6. Re-scope the storage unknown: the central question is
   one-array-vs-per-format-arrays (§2 criterion 2), an architecture breaker,
   not merely "which binding number".
7. The frame-comparison instruction needs the §3 scaffolding to mean anything:
   the sim is all-zero without input, and no capture facility exists in the
   repo.
8. Phase 12's trigger paragraph: ~~"its duplicates are parity variants that a
   param-block handle already collapses with no per-dispatch channel at
   all"~~ — jacobi and blur are per-dispatch, i.e. the trigger Phase 12 says
   does not exist yet.

## 9. Outcome

**Done, 2026-08-18. 25 pipelines → 20, with pixel-identical output.** The spike
gate passed on all five criteria, so Phase 11b is viable and its plan can be
written against the numbers below.

### 9.1 The spike

Method as [§1](#1-the-spike--storage-image-handles-compile-only), with one
deviation: production sets no `bindless_space_index` target option, so the
harness set none either. Phase 0's harness did set it. Every variant reported
`bindless_space_index = 1`.

Three variants were added to §1's list:

- **(h)** `[]` on a `Sampler2D<float>.Handle`, both direct and through a local.
  §4.2 and §4.3 read with `[]`, and no shipping shader indexed a handle that
  way — the proven idioms were `.Sample` and the local deref at
  `toon_link.shader.slang:111-112`.
- **(i)** `Texture2D<float>.Handle` alone, to read its binding number.
- **(j)** `Texture2D<float>.Handle` together with `RWTexture2D<float>.Handle`,
  to test whether the two collide.

**Reflection.** Every handle shape reflects the same way: `TypeKind::Vector`,
category `Uniform`, size 8, stride 8, align 8, with the identity surviving only
on the declared name.

| declared `full_name()` | kind | category | size / align |
|---|---|---|---|
| `DescriptorHandle<Sampler2D<float>>` | Vector | Uniform | 8 / 8 |
| `DescriptorHandle<Sampler2D<vector<float,4>>>` | Vector | Uniform | 8 / 8 |
| `DescriptorHandle<Texture2D<float>>` | Vector | Uniform | 8 / 8 |
| `DescriptorHandle<RWTexture2D<float>>` | Vector | Uniform | 8 / 8 |
| `DescriptorHandle<RWTexture2D<vector<float,4>>>` | Vector | Uniform | 8 / 8 |

The shape gate (parameters.rs:659-666) rejects every `RWTexture2D` form with the
message it is written to give. Nothing degrades silently.

**SPIR-V.** Every heap array lands in descriptor set 1:

| handle shape | heap binding | array element type |
|---|---|---|
| `Sampler2D<T>.Handle` | **1** | `OpTypeSampledImage` of `OpTypeImage %float 2D 2 0 0 1 Unknown` |
| `Texture2D<T>.Handle` | **2** | `OpTypeImage %float 2D 2 0 0 1 Unknown` |
| `RWTexture2D<T>.Handle` | **2** | `OpTypeImage %float 2D 2 0 0 2 Unknown` |

Those are the numbers under Slang's default preset,
`BindlessDescriptorOptions.VkMutable`. They are a *choice*, not a fact —
see [§9.3](#93-what-the-spike-corrected-in-this-doc) item 3.

The element type never reaches the `OpTypeImage`. Slang emits a scalar-`%float`
image for both `float` and `float4` and truncates the fetch result, so
`Sampler2D<float>` and `Sampler2D<float4>` are the same type at one binding, and
so are `RWTexture2D<float>` and `RWTexture2D<float4>`.

**Access lowering.** The `[]` read-modify-write in variant (b):

```
%32 = OpCompositeExtract %uint %31 0          ; low half of the uint2 handle
%33 = OpAccessChain %_ptr_UniformConstant_20 %__slang_resource_heap %32
%35 = OpLoad %20 %33
%sampled = OpImageRead %v4float %35 %34
...
%41 = OpLoad %20 %33
       OpImageWrite %41 %29 %42
```

Variant (h), both idioms, lower identically — heap `OpAccessChain`, `OpLoad`,
`OpImage` to drop the sampler half, then `OpImageFetch`. Reading a handle with
`[]` needs no local deref.

### 9.2 Gate verdict — all five criteria pass

1. **Pass.** Every `RWTexture2D<T>.Handle` reflects exactly like the sampler
   handles, so `DescriptorHandleShape` generalizes to a second variant.
2. **Pass.** Variant (d) emits **one** storage runtime-array *type* at **one**
   binding. Two SPIR-V variables (`%__slang_resource_heap`,
   `%__slang_resource_heap_0`) alias it, both decorated `Binding 2` /
   `DescriptorSet 1`. The binding number is 2 across (b), (c), (d) and (e).
3. **Pass.** The format operand is `Unknown` in the control and in the handle
   path alike, and the only capability the handle path adds is
   `RuntimeDescriptorArray`.
4. **Pass.** See the disassembly above; variant (g) compiles.
5. **Pass.** Variant (f) puts both `Sampler2D` element types in **one** array at
   binding 1, which makes the existing `Sampler2D<` prefix gate sound.

### 9.3 What the spike corrected in this doc

1. **The control already carries the "without format" capabilities, and that is
   correct.** `paint_brush.comp.spv` declares `StorageImageReadWithoutFormat`
   and `StorageImageWriteWithoutFormat`, and its one `OpTypeImage` has format
   `Unknown` — one image type serves all six `RWTexture2D<float>` / `<float4>`
   bindings. The renderer requests neither
   `shaderStorageImageReadWithoutFormat` nor the write twin.

   That is legal, not an oversight. `VUID-RuntimeSpirv-apiVersion-07954` and
   `-07955` make the *device feature* govern only when
   `VkPhysicalDeviceProperties::apiVersion` is below 1.3. The renderer requires
   1.3 and bails below it (renderer.rs:3365), so the governing rule is the
   per-format one instead: `VK_FORMAT_FEATURE_2_STORAGE_READ_WITHOUT_FORMAT_BIT`
   and its write twin, which Vulkan 1.3 requires for `R32_SFLOAT`.

   Measured: watercolor runs validation-clean on this machine's Intel Iris Xe,
   which reports `shaderStorageImageReadWithoutFormat = false`. If the feature
   path applied, that device could not run watercolor at all, and the renderer
   could not enable the feature there to fix it.

   **The actionable half is a warning for Phase 11b:** do not add
   `shaderStorageImageRead/WriteWithoutFormat` to the required-features list
   (renderer.rs:3403-3441). Requiring the read bit would fail the suitability
   gate on a device that runs the classic path today. §5's feature list is
   right to name only `shaderStorageImageArrayDynamicIndexing` and
   `descriptorBindingStorageImageUpdateAfterBind`.
2. **Criterion 2 asked the wrong question, and got a better answer.** The
   worry was one array versus one array *per format*. Slang emits one image type
   per *access class* — sampled-and-combined, sampled-separate, storage — and
   the format never enters it. So a single storage binding serves every element
   type.
3. **Binding 2 is the mutable-descriptor binding, not a collision.** Variant (j)
   puts `Texture2D.Handle` and `RWTexture2D.Handle` at `Binding 2` together,
   which no ordinary `VkDescriptorSetLayout` can express. That is not a Slang
   defect: the default preset is
   `BindlessDescriptorOptions.VkMutable`, whose table aliases *every*
   non-sampler, non-combined descriptor type onto binding 2 on purpose, because
   that binding is meant to be `VK_DESCRIPTOR_TYPE_MUTABLE_EXT`
   (`VK_EXT_mutable_descriptor_type`). The repo does not implement that
   contract and does not need to today, because both presets agree that a
   combined image sampler is binding 1.

   `hlsl.meta.slang:27529-27556` holds both tables:

   | descriptor type | `None` | `VkMutable` (default) |
   |---|---|---|
   | sampler | 0 | 0 |
   | combined image sampler | **1** | **1** |
   | sampled image | 2 | 2 |
   | storage image | **3** | 2 |
   | uniform/storage texel buffer | 4 / 5 | 2 |
   | uniform buffer | 6 | 2 |
   | storage buffer | 7 | 2 |

   So Phase 11b has a choice, and it does **not** need
   `VK_EXT_mutable_descriptor_type`. Overriding `getDescriptorFromHandle` to
   call `defaultGetDescriptorFromHandle(handle, BindlessDescriptorOptions.None)`
   moves storage images to **binding 3** and leaves binding 1 untouched.
   Measured: with the override, one shader holding all three handle shapes emits
   combined at 1, sampled at 2, storage at 3, one descriptor type per binding.
   Slang pins the same table in its own test,
   `tests/language-feature/descriptor-handle/desc-handle-default.slang`.

   **This was adopted, ahead of Phase 11b.** `load_bindless_options_module`
   (`crates/slang-reflection/src/lib.rs`) injects the override as a component of
   the SPIR-V link, so no shader imports it — which matters, because 12 of 37
   example shaders do not `import mltrs;`, including eight of watercolor's nine
   compute shaders. All 71 committed artifacts stayed byte-identical, since
   every handle in the tree is a combined image sampler and that is binding 1
   under both presets. That identity is also why the switch needs a positive
   control: `storage_image_handles_land_on_their_own_heap_binding` compiles a
   storage handle and asserts binding 3, and it fails with binding 2 if the
   override is ever dropped.
4. **Criterion 5's failure branch was overstated.** See the strikethrough in
   [§2](#2-the-decision-gate).

### 9.4 What the scaffolding actually needed

`DT` is already a `const`, so §3's fixed-`dt` item was satisfied before the
phase started. Three things it did not anticipate each broke the A/B once:

1. **The FPS label.** `update` writes a wall-clock FPS string into the egui
   label, which put ~50 differing pixels into every capture. Frozen for the run.
2. **A frame-counted hold does not hold.** A paused frame skips ten dispatches
   and costs almost nothing, so a 20-frame hold ended before `import` had
   grabbed the window, and the capture showed the *previous* checkpoint. Two
   fixes together: the hold is wall-clock, and the app prints its
   `CHECKPOINT <n>` marker only after drawing five frames of the held state, so
   a capture that starts on the marker can never grab the pre-hold image. The
   capture script also grabs twice and requires the two grabs to match.
3. **The window size is not stable.** The tiling WM ignores
   `initial_window_size` and sizes by what else is on screen, and
   `windowAspect` feeds the display shader, so a stray instance from an earlier
   run changed every pixel. The script kills strays first and records the
   geometry with each capture.

The stroke is generated in canvas space and bypasses `window_to_canvas`, so the
comparison does not depend on window size at all.

Checkpoints landed at frames 30 (mid-stroke), 60 (post-stroke) and 120
(late-sim). The self-test — two runs of the unconverted build — was 0 differing
pixels at all three, and the three checkpoints differ from each other by 53,289
and 53,838 pixels, so the captures are distinct sim states rather than a
converged image compared with itself.

### 9.5 The collapses

25 → **20** runtime pipelines:

| pass | before | after |
|---|---|---|
| display | 4 | **1** |
| divergence | 2 | **1** |
| blur_h | 2 | **1** |
| everything else | 17 | 17 |

- **display** cost five declarations and zero access-site edits, as predicted.
  `paint_display`'s `ParameterBlock` now holds no texture descriptor at all.
- **divergence** is the first compute-stage handle in the workspace, and it
  needed no renderer change: `cmd_bind_texture_heap` already runs before every
  dispatch, and compute reflection already threads `has_bindless_handle`.
- **blur_h** collapses on its own. `blur_v` keeps its own pipeline: same shader,
  different storage write and a different `direction` uniform.
- `crates/cli/fixtures/alignment/handle_params.compute.slang` pins the compute
  codegen path. It declares both `Sampler2D.Handle` and `Sampler2D<float>.Handle`
  so the element-type acceptance that §4.2 and §4.3 rely on is pinned too.
  The `check_crate` stub needed no change — this phase adds no shape marker.

### 9.6 Verification

| check | result |
|---|---|
| A/B at three checkpoints, `compare -metric AE` | **0, 0, 0** |
| poison — display `deposit_0_3` → `paper_height` | 913,052 px changed |
| poison — divergence `u_in` → wrong ping-pong side | 1,376 / 2,463 px changed |
| poison — blur_h `input_tex` → wrong ping-pong side | 761 / 2,934 / 3,813 px changed |
| `just sweep` | 16 ok / 0 skip / 0 fail, self-test detected the injected fault |
| `just test` | green; 1 snapshot changed (the alignment atlas, additive) + 2 new |
| `just lint`, `cargo check --workspace --all-targets` | clean |
| artifacts re-generate byte-identically after teardown | yes |

The two compute poisons read the *wrong side of the ping-pong* rather than an
unrelated texture. That is the failure this phase could actually cause, and it
is the one a green sweep cannot see. Both changed the frame, so the handle is
read and the parity is right.

### 9.7 Deviations from the plan

1. `bilinearSampleR` and `wc_update_velocity` were left alone. See the
   strikethrough in [§4.3](#43-blur_h-2--1) and the [follow-up](#follow-up).
2. Spike variants (h), (i) and (j) were added ([§9.1](#91-the-spike)).
3. `phase_11b.md` is not written here. §5 says it is authored after the spike;
   the gate has now passed, so it is unblocked.

## Follow-up

**Done by phase_11b §5.7.** No non-`RW` `Texture2D<` declaration remains in
`examples/watercolor/shaders/source/`.

**Convert watercolor's remaining `Texture2D<T>` reads to `Sampler2D<T>`, so the
example has one read-descriptor style.** `wc_update_velocity`, `wc_advect_and_
transfer_pigment`, `wc_flow_outward`, `wc_capillary_flow` and
`wc_pressure_jacobi` still declare `Texture2D<T>` for reads that are neither
handles nor filtered.

- It is behaviour-neutral. Every one of those reads uses `[]`, and Slang's
  subscript emits `OpImageFetch` off the extracted image for both declarations
  (`hlsl.meta.slang:4766-4795`). The sampler half is never consulted.
- It costs nothing at runtime. `storage_texture_as_sampled` already creates a
  sampler and a heap slot for all 26 sampled aliases.
- It collapses no pipeline on its own, which is why it is not in Phase 11.
- Phase 11b's migration touches `wc_update_velocity` anyway, so doing it there
  would fold the `bilinearSampleR` / `interpolateU` / `interpolateV` signature
  change into work already happening.

## Out of scope

- **Phase 12** (compute push constants) — see §6.
- **The two-handles-one-image ownership refactor.** The parent doc already
  carves it out: `StorageTexture` is an ownership type, and collapsing the
  `StorageTextureHandle` + aliased `TextureHandle` pair is a refactor bindless
  neither requires nor provides.
- **`getDescriptorFromHandle` / `NonUniformEXT`.** Every handle this phase
  writes is CPU state in a uniform — the shader reads *the* handle and indexes
  nothing, so the dynamic-uniformity lever stays untouched.
