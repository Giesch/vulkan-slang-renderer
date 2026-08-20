# Phase 11b — watercolor: storage-image handles

**Status: done, 2026-08-20. 20 pipelines → 12, with pixel-identical output.**
Detailed plan for Phase 11b of [../bindless_textures.md](../bindless_textures.md).
Written against `79ce02b`; the line numbers below are that snapshot. The
measurements this plan rests on are in [phase_11.md](phase_11.md) §9; the sketch
it expands is §5 there. See [§9](#9-outcome) for the results and for the three
places this plan was measured wrong.

## Why this phase exists

Watercolor runs 20 pipelines after Phase 11. Six of its eleven passes are
duplicated only because a per-parity `RWTexture2D<T>` write target is a
descriptor, and a descriptor is welded to a pipeline. `RWTexture2D<T>.Handle`
moves the write target into the params uniform, where the draw closure already
writes the read-side handles every frame. The ladder:

| pass             | now | after 11b | duplicated by                      |
| ---------------- | --- | --------- | ---------------------------------- |
| brush            | 2   | 1         | sim parity (all-storage)            |
| update_velocity  | 2   | 1         | velocity parity                     |
| divergence       | 1   | 1         | — (Phase 11)                        |
| pressure_jacobi  | 2   | 2         | per-dispatch parity → Phase 12      |
| project_velocity | 2   | 1         | velocity parity                     |
| blur_h + blur_v  | 2   | 2         | per-dispatch `direction` → Phase 12 |
| flow_outward     | 2   | 1         | wet-mask parity                     |
| advect           | 4   | 1         | sim × deposit parity                |
| capillary_flow   | 2   | 1         | saturation parity                   |
| display          | 1   | 1         | — (Phase 11)                        |
| **total**        | **20** | **12** |                                     |

The gate (phase_11.md §2) passed on all five criteria. What the spike measured,
and this plan builds on (phase_11.md §9.1, §9.3):

- `RWTexture2D<T>.Handle` reflects exactly like a sampler handle: a `uint2` in
  the `Uniform` category, size/align 8, identity only on the declared
  `full_name()`.
- Under the pinned `None` preset the storage heap array is **binding 3 of the
  bindless set**. `storage_image_handles_land_on_their_own_heap_binding`
  (crates/slang-reflection/src/lib.rs:561) pins it.
- One array serves every element type: the element type never reaches the
  `OpTypeImage`, and the format operand is `Unknown` in the classic path and
  the handle path alike.
- The handle path adds no capability beyond `RuntimeDescriptorArray`, requested
  since Phase 2.
- **`shaderStorageImage{Read,Write}WithoutFormat` must stay off the required
  features.** The classic path already emits both WithoutFormat capabilities
  while the renderer requests neither feature — legal at API 1.3, where the
  per-format bits govern. Requiring the read bit would fail the suitability
  gate on the Intel Iris Xe that runs the classic path validation-clean today
  (phase_11.md §9.3 item 1).

Synchronization is not at risk, and it is worth stating because losing the
storage-texture fields from `Resources` looks like it could lose a barrier. No
barrier derives from descriptor bindings: `Gpu::dispatch` inserts a global
compute→compute memory barrier between consecutive dispatches
(renderer.rs:5750-5773), and the cross-frame and compute→graphics barriers are
global too (renderer.rs:1742-1775).

## 1. Heap: the storage-image binding

`crates/renderer/src/renderer/descriptor_heap.rs`. The bindless set gains a
second binding. It is the same set, so `cmd_bind_texture_heap`
(renderer.rs:2427), the heap-layout append in `vk_create`, and the five
`descriptor_heap.layout()` call sites need no change.

- `const STORAGE_IMAGE_BINDING: u32 = 3;` next to
  `COMBINED_IMAGE_SAMPLER_BINDING` (:23). The number is Slang's, fixed by the
  pinned `None` preset (the map at :20-22).
- The `bindings` (:49-56), `binding_flags` (:59-62) and `pool_sizes` (:71-73)
  arrays each gain a parallel `STORAGE_IMAGE` entry — Vulkan requires
  `bindingCount == pBindingFlags.len()`, so the arrays must stay parallel.
  Same flags as binding 1:
  `PARTIALLY_BOUND | UPDATE_AFTER_BIND | UPDATE_UNUSED_WHILE_PENDING`, stage
  `ALL`, count `MAX_BINDLESS_TEXTURES` — the constant bounds each binding's
  count and does not need a storage twin.
- `insert_storage_image(&mut self, device, image_view: vk::ImageView) ->
  anyhow::Result<BindlessIndex>`: a write to binding 3 with the view and
  `vk::ImageLayout::GENERAL`, no sampler. `GENERAL` is hardcoded because a
  `StorageTexture` is created in `GENERAL` and every path returns it there
  (renderer.rs:788-797, :907-960). The slot counter is a **second** monotonic
  field — slots in a different binding are an independent index space, and
  `next_slot` stays untouched.

## 2. Device features and limits

Two new bits. Request and gate land together, in the unconditional parts of
the builders (the Phase 2 rule), keeping the requested set equal to the gated
set:

- `shader_storage_image_array_dynamic_indexing` on the core features builder
  (renderer.rs:3758), beside its sampled twin and for the same reason: the
  heap index is loaded from a buffer.
- `descriptor_binding_storage_image_update_after_bind` on `vulkan_12_features`
  (:3774-3784).
- Both mirrored into `missing_features` (:3403-3442).
- `undersized_limits` (:3547) gains
  `max_per_stage_descriptor_update_after_bind_storage_images` and
  `max_descriptor_set_update_after_bind_storage_images` against
  `MAX_BINDLESS_TEXTURES`, in the existing formatted-reason style.

Deliberately absent, recorded because both look like omissions:

- `shader_storage_image_{read,write}_without_format` — see above; per-format
  bits govern at API 1.3.
- `shader_storage_image_array_non_uniform_indexing` — nothing emits
  `NonUniformEXT` (Phase 0); the sampled twin was omitted in Phase 2 for the
  same reason.

## 3. The slot on `StorageTextureHandle`

The mirror of what Phase 3 did for `TextureHandle`.

- `StorageTextureHandle` (storage_texture.rs:4-6) gains
  `bindless_slot: BindlessIndex`; `StorageTextureStorage::add` (:15) takes it
  as an argument, like `TextureStorage::add`.
- `StorageTextureHandle::bindless_handle() -> BindlessHandle<RwTexture2D>`,
  mirroring `TextureHandle::bindless_handle` (texture.rs:19). The slot lives
  on the handle for the Phase 3 reason: nothing else ever needs to look it up.
- New marker `pub enum RwTexture2D {}` in bindless.rs beside `Sampler2D`
  (:37). `RwTexture2D`, not `RWTexture2D`: the name also becomes the
  `DescriptorHandleShape` variant and the codegen string, and serde's
  camelCase rename and clippy's `upper_case_acronyms` both want the
  mixed-case form. The Slang spelling lives only in shader source.
- `create_storage_texture` (renderer.rs:768-814) funnels through a new
  `register_storage_texture`, wrapping the single `storage_textures.add` call
  site the way `register_texture` (:577-589) wraps the texture slab: insert
  into the heap, and **destroy the image, view and allocation on failure** —
  the image is fully created before the insert, nothing else can free it, and
  the symptom of skipping this is the misleading VMA teardown abort Phase 3
  documents.
- `storage_texture_as_sampled` (:818-852) is unchanged; the sampled alias
  keeps its combined-image-sampler slot at binding 1.

## 4. Reflection and codegen

- `DescriptorHandleShape` (crates/slang-reflection/src/json/parameters.rs:379)
  gains `RwTexture2D`, serialized `rwTexture2D`. `DescriptorHandleStructField`
  is unchanged — `binding` and `field_name` are shape-agnostic.
- The shape gate (crates/slang-reflection/src/reflection/parameters.rs:660)
  also accepts `inner.starts_with("RWTexture2D<")`. The declared name always
  prints the element type, bare `RWTexture2D.Handle` included — every spike
  variant confirmed it (phase_11.md §9.1). The rejection message widens to
  name both supported shapes. The array, unparseable and varying-input gates
  (:644-675) cover the new shape with no edits.
- `declares_bindless_handle` (json/parameters.rs:108) matches
  `StructField::DescriptorHandle(_)` — shape-agnostic, no change.
- Codegen: adding the variant forces the `match handle.shape` arm at
  build_tasks.rs:1013; it emits `BindlessHandle<RwTexture2D>`. The two
  alignment tables key on the `BindlessHandle<` prefix (:1440, :1481) — no
  change.
- The check_crate stub
  (crates/cli/fixtures/check_crate/src/renderer/bindless.rs) gains the same
  one-line marker. phase_08 §2.2 calls the stub the easiest thing in the
  phase to miss; it recurs here, again.
- Tests and fixtures:
  - `unsupported_handle_shapes_are_rejected` (build_tasks.rs:2963) keeps
    `Texture2D.Handle` and `SamplerState.Handle` as rejected shapes; only its
    message assertion changes.
  - The probe comment at slang-reflection lib.rs:439-440 says reflection
    rejects `RWTexture2D<float>.Handle`; after this phase it does not. Update
    the comment. The test stays — it pins SPIR-V binding decorations, which
    no reflection-JSON test covers.
  - New fixture
    `crates/cli/fixtures/alignment/storage_handle_params.compute.slang`:
    `RWTexture2D<float>.Handle` and `RWTexture2D<float4>.Handle` side by side
    (one marker serves both element types), a `Sampler2D.Handle`, and a
    scalar to force a padding gap. Expected: three `BindlessHandle` fields in
    the generated params, no storage-texture field in `Resources`, and
    `bindlessHeapSet: 1`.

## 5. Migration

One pass per A/B cycle. The phase_11.md §3 scaffolding (scripted stroke, const
`DT`, fixed frame count, three checkpoints, X11 capture) was reverted at Phase
11 teardown; re-instate it first and capture a fresh baseline from the
unconverted build.

The per-pass recipe extends phase_11.md §4: varying `Texture2D<T>` reads
become `Sampler2D<T>.Handle` fields, varying `RWTexture2D<T>` writes become
`RWTexture2D<T>.Handle` fields, constant slots stay descriptors,
`just shaders watercolor`, delete the duplicate `create_compute_pipeline` call
and the parity indexing, write the handles in the draw closure. The
parity-ordering footgun paragraph (phase_11.md §4) applies verbatim: compute
handles from the captured pre-flip `sim`, display handles from the post-flip
fields.

Order — easiest mixed pass first, widest last, per the §5 sketch:

### 5.1 project_velocity, 2 → 1

main.rs:570-594. Reads pressure and wetMask; writes u and v **in place**
through `read_storage`. Read-modify-write through a handle is spike variant
(b): `OpImageRead`/`OpImageWrite` through the heap access chain.

### 5.2 update_velocity, 2 → 1

main.rs:504-533. Five sampled reads, two writes. Phase 11's follow-up folds
in here: `bilinearSampleR` / `interpolateU` / `interpolateV`
(watercolor_common.slang) change signature from `Texture2D<float>` to
`Sampler2D<float>`. Behaviour-neutral: every such read uses `[]`, and Slang's
subscript emits `OpImageFetch` for both declarations (phase_11.md Follow-up).

### 5.3 flow_outward, 2 → 1

main.rs:617-636.

### 5.4 capillary_flow, 2 → 1

main.rs:681-703.

### 5.5 brush, 2 → 1

main.rs:478-501. All six varying slots are `read_storage` — the pass the
reads-only work could never touch. Its `Resources` collapses to the params
buffer alone (`BrushParams` already carries the stroke buffer as a
`ReadAddr<StrokePoint>`).

### 5.6 advect, 4 → 1

main.rs:641-678, the 2×2 creation loop. Last because it is the widest: 15
varying slots across the four variants (phase_11.md §5), and its `SampleLevel`
reads are already `Sampler2D<float4>`, so both filtered-read helper signatures
in watercolor_common.slang are exercised.

### 5.7 The residual read-style sweep

After 5.1-5.6, `wc_pressure_jacobi` is the last `Texture2D<T>` reader. Convert
its reads to `Sampler2D<T>` descriptors — no handles, no collapse — so the
example ends with one read-descriptor style. This completes phase_11.md's
Follow-up. A/B it like any pass.

Not migrated, already recorded in the parent doc and phase_11.md §6:

- **pressure_jacobi stays at 2.** `pressure_parity` flips per dispatch inside
  the iteration loop (main.rs:893-896); a once-per-frame uniform cannot carry
  it, and two per-parity uniform buffers are two descriptor sets, which is
  two pipelines. Phase 12's push constants are the channel.
- **blur_h + blur_v stay at 2.** One shader, two dispatches per frame that
  differ by the `direction` uniform and the write target
  (main.rs:1053-1065) — the same once-per-frame limit. Phase 12 takes
  12 → 10.

## 6. Docs

`docs/bindless.md` describes what §§1-5 change; it updates in the same
commits:

- :17 — storage images leave the "descriptors remain in two places" list; the
  `ParameterBlock` stays.
- :36-37 — "only `Sampler2D` handles" becomes the two supported shapes and
  the two heap bindings (1 and 3).
- :43 — drop the stray `- [ ]` checkbox while there.

## 7. Verification

Evidentiary weight, strongest first, per phase_11.md §7 — a green sweep
proves nearly nothing here:

1. **Per-pass A/B against the three-checkpoint baseline, target
   `compare -metric AE` = 0, plus the poison control.** Identity alone cannot
   distinguish "handles work" from "stale binding still doing the work", and
   a wrong heap slot reaches a valid, different image with no validation
   error — for a *storage* handle that means writing to the wrong texture,
   equally silent. After each collapse, temporarily write a wrong slot into
   one **write** handle and confirm the frame changes.
2. **Forced heap exhaustion.** Temporarily shrink the storage binding's count
   and confirm the intended "heap is full" error names the storage binding,
   and teardown does not abort inside VMA — the positive proof of §3's
   destroy-on-failure path, the Phase 3 precedent.
3. **Forced undersized limits.** Constant to 2,000,000: the gate warn-skips
   llvmpipe naming six limits now, and falls back as in Phase 3.
4. **`spirv-dis` on one converted `.comp.spv`**: heap arrays at set 1,
   bindings 1 and 3; no capability beyond `RuntimeDescriptorArray`; no
   `NonUniform`.
5. **Validation layers** via `just sweep` (16 ok / 0 fail, injected-fault
   self-test firing) and the live run: catches layout and unbound-set errors,
   not wrong slots.
6. **`just test` / `just lint` / `cargo check --workspace --all-targets` /
   `cargo fmt`**, with snapshot review: the new fixture's snapshots reviewed
   via `just insta`, never blind-accepted; the only pre-existing snapshot
   expected to change is the alignment-tests atlas (additive lines). The
   `just shaders` diff must be confined to watercolor — any other example
   moving means the shape gate over-widened.
7. **Hot reload** (lavapipe + `SDL_VIDEODRIVER=offscreen`): touch a converted
   shader mid-run; the interface-change panic still fires when a field is
   added.

**Teardown:** revert the scaffolding and every forced constant, re-run
`just shaders watercolor`, confirm the committed artifacts are byte-identical
to the converted state, final `just sweep`.

## 8. Corrections to `../bindless_textures.md`

In the house strikethrough style, as part of landing this plan:

1. :1802 — ~~"the storage heap array sits at binding 2 in set 1"~~: binding 2
   is where the default `VkMutable` preset aliases every non-sampler type;
   under the pinned `None` preset the storage array is at **binding 3**,
   which the same paragraph's later sentences and the lib.rs:561 test already
   say.
2. :1799-1800 — "No plan doc exists yet" links here instead.
3. On completion, not now: the Non-goals storage-image bullet (:116-119) and
   the status header (:3-6).

## 9. Outcome

**Done, 2026-08-20. 20 pipelines → 12, `compare -metric AE` = 0 at all three
checkpoints for every one of the seven migration steps.**

### 9.1 The collapses

| pass             | before | after |
| ---------------- | ------ | ----- |
| brush            | 2      | **1** |
| update_velocity  | 2      | **1** |
| project_velocity | 2      | **1** |
| flow_outward     | 2      | **1** |
| advect           | 4      | **1** |
| capillary_flow   | 2      | **1** |
| everything else  | 6      | 6     |

`main.rs` now holds twelve pipeline-creation calls and no creation loop.
`paint_brush`'s `Resources` is the params buffer alone, and
`wc_advect_and_transfer_pigment` moved all fifteen varying slots into the
uniform, leaving one `paper_height` descriptor.

### 9.2 Verification

| check | result |
|---|---|
| scaffolding self-test, two unconverted runs | 0 / 0 / 0 differing pixels |
| the three checkpoints differ from each other | 31,635 and 32,307 px |
| A/B per step, all seven steps | **0 / 0 / 0** each |
| poison — project_velocity `u` write handle | 7,442 / 14,737 / 17,401 px |
| poison — update_velocity `u_out` | 6,401 / 12,576 / 14,677 px |
| poison — flow_outward `saturation` | 1,448 / 4,360 / 9,470 px |
| poison — capillary_flow `wet_mask_out` | 7,520 / 14,488 px |
| poison — brush `pigment_0_3` | 19,555 / 31,635 / 32,307 px |
| poison — advect `deposit_out_0_3` | 19,554 / 31,617 / 32,298 px |
| forced heap exhaustion | names the storage binding, exit 1, no VMA abort |
| the same with the destroy-on-failure path removed | SIGABRT inside VMA |
| forced undersized limits (2,000,000) | warn-skips llvmpipe naming six limits |
| `spirv-dis` on the converted `.comp.spv` | heap arrays at set 1 bindings 1 and 3; capabilities `RuntimeDescriptorArray` + the two `WithoutFormat` bits the classic path already carries; no `NonUniform` |
| `just sweep` | 16 ok / 0 skip / 0 fail, self-test detected the injected fault |
| `just test` | green; 1 snapshot changed (the alignment atlas, additive) + 2 new |
| `just lint`, `cargo check --workspace --all-targets` | clean |
| hot reload: touch a converted shader | recompiles, run continues |
| hot reload: add a field | the interface-change panic fires |
| `just shaders` diff | confined to watercolor |

The forced-exhaustion check is the strongest single result here. Removing the
`register_storage_texture` cleanup reproduces the Phase 3 symptom exactly — a
`VmaDeviceMemoryBlock::Destroy` assertion, not a message naming the heap — so
the destroy-on-failure path is load-bearing and proven, not assumed.

### 9.3 Where this plan was measured wrong

1. **§5's "constant slots stay descriptors" understates what each step does.**
   For §5.7's claim to hold — that `wc_pressure_jacobi` is the last
   `Texture2D<T>` reader after 5.1-5.6 — each step must also convert its
   *constant* `Texture2D<T>` slots to `Sampler2D<T>` descriptors. That is
   behaviour-neutral and costs nothing on the Rust side, because both
   descriptor types already codegen `&TextureHandle`.
2. **flow_outward's `pressure` is constant, not varying.** The ladder in the
   header counts it among the storage writes, but Jacobi always lands pressure
   on side 0 (`JACOBI_ITERATIONS` is even), so `pressure` stayed a descriptor
   and only `saturation` became a handle. The collapse is unaffected.
3. **§5.6 says advect's `SampleLevel` reads are already `Sampler2D<float4>`,
   which is true, but it does not mention `advectAndTransferGroup`'s
   `Texture2D<float4> depositIn` parameter.** That helper signature must change
   too, or the pass does not compile.

### 9.4 What the scaffolding needed beyond phase_11.md §9.4

Phase 11's three scaffolding lessons all held. One more appeared:

- **The window takes a stray quit event mid-hold.** The app exits with status
  0, having printed only the checkpoints it reached, and it happens with no
  grabbing at all — so `import` is not the cause. It is intermittent and not a
  function of hold length. The capture script retries a lost attempt up to
  three times; the run is deterministic, so a real crash fails every attempt.

## Out of scope

- **Phase 12** (compute push constants) — jacobi and the blur pair, 12 → 10.
- **The two-handles-one-image ownership refactor** (`StorageTextureHandle` +
  aliased `TextureHandle`) — carved out by the parent doc and phase_11.md
  alike.
- **`NonUniformEXT` / handle indexing.** Every handle this phase writes is
  CPU state in a uniform; the shader reads *the* handle and indexes nothing.
- **Graphics-stage storage handles.** Reflection is stage-agnostic, so the
  gate widens for fragment shaders too, but no example writes a storage image
  from a raster stage, and `fragment_stores_and_atomics` is debug-only today;
  nothing here relies on it.
