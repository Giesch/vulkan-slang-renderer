# Bindless Textures via Slang `DescriptorHandle`

**Status: Phases 0-5 done, Phases 6-10 not started. Phase 11 is an optional
follow-up, added later and a prerequisite for nothing.** Design note for adopting bindless
texture access using Slang's `DescriptorHandle<T>` with its default SPIR-V lowering.

**Phases 6-9 were one phase until Phase 6 planning found a prerequisite this
doc never anticipated**: the toon_link payoff needs a per-draw material index,
and this renderer has no per-draw data channel at all. Real push-constant
support is that channel, and it is a reflection + codegen + renderer job in its
own right — hence Phases 7 and 8, which are not bindless work and would be
worth doing regardless. See Phase 7's opening for the measurement that ruled out
the cheaper `firstInstance` alternative.

The phases below were **revised after the Phase 0 spike** — the measured answers are
in [bindless_textures/phase_0_spike.md](bindless_textures/phase_0_spike.md), and
several of them contradicted what this doc originally assumed. Two phase boundaries
moved, one instruction ("bind the heap once per command buffer") turned out to be
wrong, and the `BINDLESS_SPACE_INDEX` constant the spike recommended was dropped
again on review. Read the spike doc for the evidence; this doc is the plan.

Supersedes the "not planned" note in
[render-graph/03_bindless.md](render-graph/03_bindless.md), which stays useful as
background on descriptor indexing and Metal argument buffers. For how this relates
to the BDA work, see
[vulkan_1_3_migration/bindless_vs_bda_terminology.md](vulkan_1_3_migration/bindless_vs_bda_terminology.md).

## Why

A texture is currently welded to a pipeline. `create_descriptor_sets`
(renderer.rs:4141) runs exactly once, at pipeline creation, from a positionally
ordered `&[&Texture]`; changing a texture means a new pipeline. `toon_link` pays
this in full — `build_material_pipelines`
(examples/toon_link/src/main.rs:780-826) creates one pipeline *and* one uniform
buffer per material, sharing only the mesh.

`DescriptorHandle<T>` lowers to a `uint2` of **ordinary data** — measured, not
assumed — which is the same shape the renderer already committed to for buffers:
BDA pointers in a param block, with `StructuredBuffer` descriptors actively rejected
(reflection/parameters.rs:298; reflection/pipeline_layout.rs:328 is a
documented-unreachable `panic!` backstop behind it). A texture handle can
therefore live inside a std430 struct behind an `ImmutableAddr<T>`:

```slang
struct Material {
    Sampler2D.Handle tex0;   // == DescriptorHandle<Sampler2D>
    Sampler2D.Handle tex1;
    float4 tint;
}
struct ToonLinkParams {
    mltrs::ImmutableAddr<Material> materials;
    mltrs::MVPMatrices mvp;
}
```

That collapses toon_link to ~~one pipeline~~ **five pipelines** plus a material
buffer, and is a prerequisite for the batching sketched in
[render-graph/05_multi_draw_rendering.md](render-graph/05_multi_draw_rendering.md).
The floor is per-material *raster* state (blend, depth write, cull, color mask),
which is not descriptor state and so does not go away — counted in Phase 9.

## Why this option and not the others

- **Raw descriptor arrays** (`Texture2D textures[]` + `NonUniformResourceIndex`)
  would require hand-written `[[vk::binding]]` annotations, which breaks the
  positional-binding assumption stated twice in reflection/pipeline_layout.rs:187,239.
- **Overriding `getDescriptorFromHandle`** is an escape hatch, not a starting
  point. It stays available later *without touching shader source*. Deferring it
  costs nothing **until something needs a material index that varies within a
  single draw** (see Phase 9) — the spike found Slang emits no `NonUniformEXT` and offers no
  source-level way to request it, and this override is the only seam where the
  decoration could be added. It is also the way to a single mutable-type heap
  (`BindlessDescriptorOptions.VkMutable`, needs `VK_EXT_mutable_descriptor_type`)
  or a layout that doesn't match Slang's default.
- **`VK_EXT_descriptor_heap`** (Slang's `spvDescriptorHeapEXT` capability) shipped
  with Vulkan 1.4.340 in Jan 2026; NVIDIA and AMD have drivers, Intel ANV is
  experimental, MoltenVK has nothing. Off the table for a 1.3 baseline. It is the
  *same source-level feature*, so today's code becomes descriptor-heap code via a
  compile flag whenever we want it.

## What's already in place

- Vulkan 1.3 floor with `bufferDeviceAddress` — gated at renderer.rs:3209-3237,
  enabled at :3510-3512. Descriptor indexing is core 1.2, so it costs no new
  extension.
- Every texture already carries its own view + sampler (`Texture`,
  renderer/texture.rs:55-64) and lives in an append-only slab
  (`TextureStorage`, texture.rs:11-43) — the natural backing for heap slots.
- `addr.rs` is the exact template for the new handle type: `#[repr]` newtype,
  `PhantomData<fn() -> T>`, `const _: () = assert!(size_of == 8)`,
  `pub(super)` constructor so only the renderer can mint one.
- `bindless_space_index` is exposed by the `shader-slang` crate (our fork of the
  repo named slang-rs), already pinned in the
  root `Cargo.toml` (`v0.1.1+slang-2026.13.1`), as is
  `ShaderReflection::bindless_space_index()`. **No further slang-rs work is
  needed.** `slang_IBindlessResourceMetadata` (`slang-sys/src/bindings.rs:1041`) is
  *not* usable — bindgen emitted it as an opaque struct with no vtable, so
  `usesBindlessResourceHeap()` can't be called without hand-writing the vtable and
  a `castAs` path. We don't need it: "does this shader use the heap" is answered by
  the reflection walk finding any field whose declared `full_name()` starts with
  `DescriptorHandle<`.
- Retire-after-N-frames precedent for resources still referenced by in-flight
  command buffers: `old_pipelines` (renderer.rs:116-121, freed at :2619-2645).
  No phase uses it anymore — textures are immortal for now — but it's the
  pattern for the future heap remove API (see Phase 3's future-work note).

## Non-goals

- `VK_EXT_descriptor_heap` / `spvDescriptorHeapEXT`.
- Overriding `getDescriptorFromHandle` — *conditional*: this is the only lever for
  `NonUniformEXT`, so it stops being a non-goal the moment a material index varies
  within one draw. See Phase 9.
- `VK_DESCRIPTOR_BINDING_VARIABLE_DESCRIPTOR_COUNT_BIT` — see Phase 3.
- Storage images (`RWTexture2D`, watercolor) stay on per-pipeline descriptors.
  Still true; Phase 11 scopes what lifting it would take, and gates it on a spike.
- egui keeps its own descriptors (`renderer/egui.rs`, third-party renderer).
- Uniform buffers stay descriptor-bound; something has to carry the handles.

---

## Phase 0 — spike ✅ done

**Answers are in [bindless_textures/phase_0_spike.md](bindless_textures/phase_0_spike.md).**
The short version:

- A handle reflects as **`TypeKind::Vector`** (a `uint2`) in the `Uniform` category,
  size/align 8 — including inside a `Std430DataLayout` pointee. Its identity
  survives only on the **declared** type's `full_name()`
  (`DescriptorHandle<Sampler2D<...>>`), so today's reflection silently degrades it
  to a `uint2` with **no error at all**. That footgun is what Phase 1 closes.
- Slang emits one unbounded `UniformConstant` `OpTypeRuntimeArray` per descriptor
  type, in the bindless space. Binding map confirmed: **0 sampler, 1 combined image
  sampler, 2 sampled image**. A handle-free shader emits nothing.
  `RuntimeDescriptorArray` capability is required.
- **`NonUniformEXT` is never emitted**, and there is no source-level way to request
  it — not on the index, not on the handle.
- The reported bindless space is `max(requested, first free space)`; Slang resolves
  collisions itself and reflection reports the truth. It is *not* a usage signal.

`bindless_space_index` is a **target** option — it goes on `TargetDesc`, not the
session `CompilerOptions`. But Phase 4 drops passing it entirely: the spike's
unset-option row shows reflection reports the index fine without it, so no
`TargetDesc` changes are needed. If the option is ever passed after all, there
are **three** target-desc constructions to cover, not two:
`prepare_reflected_shader_with_optimization` (shaders.rs:80-82), the compute
equivalent (:176-178), and `reflect_shared_module_types` (:256-258).

## Phase 1 — reject handle fields loudly ✅ done

Today a `Sampler2D.Handle` field compiles, generates a `UVec2` binding, and passes
every generated `offset_of!`/`size_of` assertion while being silently wrong. Closed
first, so every later intermediate state of this branch is safe.

- In `crates/renderer/src/shaders/reflection/parameters.rs`, a declared
  `full_name()` starting with `DescriptorHandle<` bails, in the style of the
  `StructuredBuffer` rejection (:298).
- It went in the **early-continue block alongside the existing enum special case**
  (:177-198), *not* in the `TypeKind::Resource` match — the type reflects as
  `TypeKind::Vector`, so by the time the `kind()` match runs the information is
  gone. The enum case is the model for the *placement* only: it checks
  `field.ty().kind() == TypeKind::Enum`, while the `full_name()`-prefix
  technique this check needs is the one the *pointer* arm already uses. Both
  decode sites now share a `declared_full_name` helper. The guard is
  deliberately **not** gated on `Binding::Uniform` the way the enum arm is: a
  handle in a vertex-input position has a `VaryingInput` binding, and gating
  would let exactly that case fall through to the `Vector` arm.
- ~~**Arrays of handles bypass this check**~~ — **measured wrong.** Slang prints
  an array's declared `full_name()` as the element's with `[N]` appended
  (`DescriptorHandle<Sampler2D<vector<float,4>>>[4]`), so the prefix guard fires
  on arrays too and they get the same specific message. The `TypeKind::Array`
  arm still never recurses into `reflect_struct_fields`, so if that suffix form
  ever changes the fallback is `validate_array_element`'s generic vec4-only
  message — vaguer, but still loud. `handle_arrays_are_rejected` pins the
  current behaviour. (This also closes the spike's open question about handle
  arrays on the reflection side.)
- Phase 5 flips this from rejection to support. The *shape* rejection (anything
  that isn't `Sampler2D`) survives into Phase 5, and is what lets Phase 3 create
  only one heap binding.

**Verified:** `just test` green with no snapshot changes, plus three rejection
tests in `crates/cli/src/build_tasks.rs` next to `enum_vertex_inputs_are_rejected`
(they reuse the `reflect_rejected_shader` helper declared above them) — scalar
handle field, handle array, handle in a vertex-input position.

## Phase 2 — device features (behaviorally invisible; land alone) ✅ done

`create_logical_device` (renderer.rs:3474) requested zero descriptor-indexing
bits. Added to `vulkan_12_features` (:3510):

```rust
.descriptor_indexing(true)
.runtime_descriptor_array(true)
.descriptor_binding_partially_bound(true)
.descriptor_binding_sampled_image_update_after_bind(true)
.descriptor_binding_update_unused_while_pending(true)
```

`runtime_descriptor_array` is **required** — the spike confirmed Slang emits
`OpTypeRuntimeArray` and `OpCapability RuntimeDescriptorArray`.

`shader_sampled_image_array_dynamic_indexing` is **also required** — the spike's
disassembly loads the heap index from a buffer (`OpLoad` → `OpAccessChain` into
the runtime array), which is non-constant indexing of a sampled-image array.
It's a **core-1.0 `VkPhysicalDeviceFeatures` bit**, so it went on the base
`features` builder (renderer.rs:3491), *not* on `vulkan_12_features`.
Universally supported in practice, but validation flags its absence.

`shader_sampled_image_array_non_uniform_indexing` is deliberately **omitted**:
nothing the compiler emits needs it, because `NonUniformEXT` never appears. Add it
only alongside whatever resolves that (see Phase 9) — requesting it now would imply
a guarantee we don't have.

All six bits — including the core-1.0 one — are mirrored in the physical-device
gate (renderer.rs:3219-3237), which is what turns an unsupported device into a
`log::warn!` naming the exact missing feature rather than a device-creation
failure. The bail message at renderer.rs:3290-3295 summarizes the group instead
of enumerating all six; the per-device warn already prints exact names.

Both feature requests went in the **unconditional** part of their builders, not
the `cfg!(debug_assertions)` blocks the shader-println features use — the heap
exists in release builds too.

**Verified:** `cargo check --workspace --all-targets`, `just lint`, and
`just test` clean; `just sweep` 16 ok / 0 fail with the injected-fault self-test
still firing. Lavapipe supports descriptor indexing, so CI covers this.

## Phase 3 — the heap, created but not bound ✅ done

New module `crates/renderer/src/renderer/descriptor_heap.rs`. **Nothing is bound in
this phase** — binding requires pipeline layouts that declare the heap set, which is
Phase 4. Keeping them apart is what makes this phase independently verifiable.

- **One binding: 1, combined image sampler.** The binding *numbers* are fixed by
  Slang (0 sampler, 1 combined, 2 sampled image), so this is a real constraint, not
  a convention. Only binding 1 is created, which is sound because Phase 1 rejects
  every handle shape other than `Sampler2D`. Adding separate texture/sampler
  handles later means adding bindings 0 and 2 *and* relaxing that rejection
  together.
- **Fixed-size array, not variable count.** Only the *last* binding in a set may
  carry `VARIABLE_DESCRIPTOR_COUNT`, and Slang puts several arrays in the one
  bindless set — that collision is [slang#8063](https://github.com/shader-slang/slang/issues/8063).
  Use a fixed `MAX_BINDLESS_TEXTURES` (start ~4096) plus
  `DescriptorBindingFlags::PARTIALLY_BOUND`. It also dodges
  [MoltenVK#2278](https://github.com/KhronosGroup/MoltenVK/issues/2278). The shader
  side declares an unbounded array; that's compatible with a fixed-count layout as
  long as nothing indexes past the count.
- Its own pool with `DescriptorPoolCreateFlags::UPDATE_AFTER_BIND` and the
  matching `DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL`. ~~pool flag
  `UPDATE_AFTER_BIND_POOL`~~ — the two flags are spelled differently in ash: the
  *pool* constant is `UPDATE_AFTER_BIND`, only the *layout* one carries the
  `_POOL` suffix.
- **The binding flags matter as much as the create flags**, and this plan
  originally listed only `PARTIALLY_BOUND`. The set uses
  `PARTIALLY_BOUND | UPDATE_AFTER_BIND | UPDATE_UNUSED_WHILE_PENDING`:
  `UPDATE_AFTER_BIND` is the bit that actually grants update-after-bind
  semantics (and hence the "one set, not `MAX_FRAMES_IN_FLIGHT` copies" claim
  below), and all three features were requested in Phase 2.
- **Stage flags: `ShaderStageFlags::ALL`**, matching how reflected global
  bindings already come out (reflection/pipeline_layout.rs:40, :262). And don't
  assume `MAX_BINDLESS_TEXTURES` fits: validate it at startup, **in the
  device-suitability gate** (`descriptor_heap::undersized_limits`, called from
  `choose_physical_device`), *not* in `DescriptorHeap::new`.
  ~~a too-small limit must fail loudly in the heap because the suitability gate
  only warn-skips~~ — wrong twice over. Warn-skipping isn't a soft failure: the
  gate's closing `bail!` is just as loud. And checking after a device is already
  chosen throws away the fallback — a machine with one undersized GPU and one
  capable GPU hard-fails instead of picking the capable one. Measured: with the
  constant temporarily raised to 2,000,000, the gate skips llvmpipe with a warn
  naming all four limits and their values, and the example runs on Intel Iris Xe
  instead. This also matches Phase 2, which mirrored all six feature bits into
  the gate for the same reason. The model to copy is `unsupported_formats`
  (renderer.rs:3298) — a `Vec<String>` of formatted reasons — not
  `missing_features`, whose bare `&str` names can't carry the offending value.
  **Four limits, not the two this plan originally named**: a combined
  image sampler consumes a sampler descriptor *and* a sampled-image descriptor,
  so `maxPerStageDescriptorUpdateAfterBind{Samplers,SampledImages}` and
  `maxDescriptorSetUpdateAfterBind{Samplers,SampledImages}` all apply. They're
  read off `VkPhysicalDeviceVulkan12Properties`, matching the
  `PhysicalDeviceVulkan12Features` style at renderer.rs:3210. Measured headroom
  at 4096 slots is enormous everywhere checked: Intel Iris Xe 2.0e8, RTX 3070 Ti
  1048576, llvmpipe (what CI runs) 1000000.
- **One set, not `MAX_FRAMES_IN_FLIGHT` copies.** Update-after-bind is exactly
  what removes the need to duplicate.
- `insert_texture(&Texture) -> BindlessIndex` writes one descriptor. Take the
  image layout from `texture.image_layout` rather than hardcoding
  `SHADER_READ_ONLY_OPTIMAL` — the sampled aliases that
  `storage_texture_as_sampled` (renderer.rs:794) creates live in `GENERAL` —
  mirroring what `create_descriptor_sets` already does at renderer.rs:4219.
- **Textures are immortal for now; there is no slot release.** The slot
  allocator is a monotonic counter checked against `MAX_BINDLESS_TEXTURES` — no
  free-list. As part of this phase, delete the dead `drop_texture`
  (renderer.rs:729, zero callers in the workspace) and the then-dead
  `TextureStorage::take` (texture.rs:33, whose only caller is `drop_texture`).
  `destroy_texture` stays: the shutdown `take_all` path (renderer.rs:2880) uses
  it after `device_wait_idle`, where immediate destruction is safe.
- **Future work, out of scope here:** when texture removal is needed, it comes
  as a bindless-specific add/remove API on the heap — deferred slot reuse *and*
  deferred Vulkan object destruction, both `MAX_FRAMES_IN_FLIGHT` frames, via
  the retirement pattern at renderer.rs:2619-2645 (writing a never-referenced
  slot is safe under `PARTIALLY_BOUND` + `UPDATE_UNUSED_WHILE_PENDING`, but
  overwriting or destroying one an in-flight command buffer still references is
  not). Note that pattern currently lives in the
  `#[cfg(debug_assertions)]`-only hot-reload path and would need an
  unconditional home in the frame loop. Bound (classic-path) textures stay
  owned by their pipelines / immortal either way.

Then:

- Store the slot **in the returned `TextureHandle`** (texture.rs:4-9), not on
  `Texture`: `Gpu` (renderer.rs:5132-5136) has no access to `TextureStorage`,
  so Phase 5's accessor can only work if the handle itself carries the slot —
  and with no release path, nothing else ever needs to look it up. Note **slab
  index ≠ heap slot** stays true as a matter of principle even though both are
  monotonic today; keep them as separate fields. Allocate in a single private
  renderer method (e.g. `register_texture`) wrapping both `textures.add` call
  sites — `create_texture_with_mips` (renderer.rs:557, which `create_texture`
  :495 and `create_texture_with_options` :517 both funnel into) and
  `storage_texture_as_sampled` (renderer.rs:825), which bypasses the
  `create_texture_*` chain entirely — so *every* texture gets one, and any
  future creation path must visibly opt in.
- **`register_texture` must destroy the texture when the heap is full.** Not
  anticipated by this plan, found by testing it (see below): the `Texture` is
  fully created — image, allocation, view, sampler — before it reaches the slab,
  so an `insert_texture` error that just propagates leaks all four. Nothing else
  can free it, because a texture is only ever freed via `TextureStorage`. The
  symptom is not a leak message but a hard `abort()` inside VMA's
  "Some allocations were not freed before destruction of this memory block!"
  assertion at renderer teardown, which is a thoroughly misleading place to
  start debugging from.

**Verify:** `just sweep` clean with validation on. The heap is allocated and
populated but referenced by nothing, so any error here is a pure layout/lifetime
bug.

**Verified:** `cargo check --workspace --all-targets`, `just lint` and
`just test` clean, the latter with **no snapshot changes** (as expected — this
phase touches no reflection, codegen or template code). `just sweep` 16 ok /
0 fail with the injected-fault self-test still firing.

A green sweep alone doesn't distinguish "the heap works" from "the heap code
never ran", so both failure paths were confirmed positively by temporarily
moving `MAX_BINDLESS_TEXTURES`:

- **= 1**: `toon_link` fails on its second texture with the intended
  `bindless texture heap is full` error. This is also what surfaced the
  leak-on-full bug above — before the fix, the same run aborted inside VMA
  instead of reporting the error.
- **= 2,000,000**: the suitability gate warn-skips llvmpipe naming all four
  limits and their values. Under the lavapipe-pinned sweep (no other device) that
  ends at the clean "no suitable graphics device available" bail; with the pin
  dropped it falls back to Intel Iris Xe and runs normally.

## Phase 4 — pipeline layouts and per-pipeline binding ✅ done

Much smaller than it looks. The spike measured `descriptor_set_count() == 0` and a
single reflected binding range (the `ParameterBlock`): **the heap never enters
`ReflectedPipelineLayout` at all.** So `reflect_pipeline_layout`
(reflection/pipeline_layout.rs:11-27), the `descriptor_sets_for_frame` chunk width
(renderer.rs:2301, picking at :2315), and `descriptor_pool_sizes` (renderer.rs:4043)
need **no changes** — there is nothing to filter out and nothing that can leak in.
This work is purely additive.

- **No `BINDLESS_SPACE_INDEX` constant.** Don't pass a floor; let Slang place the
  heap immediately after the reflected sets, and record
  `reflection.bindless_space_index()` in the reflection JSON as per-shader data. A
  `VkDescriptorSetLayout` doesn't encode its own set number, so one heap layout and
  one heap set work at whatever index each shader reports. This also means no gap
  indices, so `vk_create` (renderer.rs:4965/4999) just **appends** the heap layout —
  no empty placeholder layouts.
  **The index and the usage flag landed as one field**, `bindless_heap_set:
  Option<u32>` on `ReflectedPipelineLayout` — they are only ever read together,
  and `None` is exactly "don't append, don't bind". Measured across all 27
  shaders in the workspace: every one reports space **1**, which is also every
  one's reflected set count, so the append is a no-gap append everywhere today.
  That correspondence is *asserted*, not assumed —
  `PipelineLayoutBuilder::build()` flattens away unused reserved slots, so a
  future nested or empty parameter block is the shape that could break it. The
  check runs **before any set layout is created**: bailing partway through that
  loop leaks every layout made so far, and the resulting
  "has not been destroyed" validation error prints *ahead* of the real message.
  (Found by running the forced-mismatch case below, not by reading the code.)
- **Bind the heap after each `cmd_bind_pipeline`, not once per command buffer.**
  Pipeline-layout compatibility preserves a binding at set N only across layouts
  with identical set layouts for 0..N. Set 0 is the per-shader param block, so it
  differs between every pair of pipelines and each pipeline switch disturbs the heap
  binding. Add a second `cmd_bind_descriptor_sets` at `first_set = <heap index>`
  next to the existing bind-from-0 calls (~~renderer.rs:1584, :1770, :2048~~ —
  stale; the three sites are the compute dispatch, the picking pass and the main
  draw loop, wrapped in `Renderer::cmd_bind_bindless_heap`). Cheap;
  the failure mode if skipped is a "descriptor set not bound" validation error at
  the first pipeline switch.
- Append and bind **only for shaders that declare a handle** — the `DescriptorHandle<`
  scan from Phase 1/5. The reported space index is not a usage signal; a handle-free
  shader still reports one.

  **Confirmed, and it needed confirming.** slang-rs documents
  `bindless_space_index()` as returning `-1` when no heap space was reserved,
  and the spike only ever measured a non-negative value with an explicit floor
  requested — so with the floor now dropped it was genuinely open whether the
  return value had become self-gating. It has not: every handle-free shader in
  the workspace reports **1**, not `-1`. Hence a separate flag,
  `DECLARES_BINDLESS_HANDLE` in `shaders/reflection.rs`.

  That flag is a hardcoded `false` for this phase, and unavoidably so: Phase 1
  rejects every handle field, so no shader that would set it can reach
  reflection. **The Phase 4 code path therefore does not execute under any
  normal run** — which is why the forced-true run below is the actual
  verification, not the sweep. Phase 5 is what makes the flag real.
- **The bind-after-each-pipeline rule has one implicit exception: egui.**
  `egui_ash_renderer` records its own pipeline and descriptor binds into our
  command buffer (renderer.rs:~2256), and the rule holds only because egui is
  recorded **last** — nothing of ours binds after it. Leave a comment at the
  egui call site stating that ordering invariant, so a pass added after egui
  doesn't hit a mysteriously-unbound heap.
- **Hot reload:** `assert_shader_interface_unchanged` (renderer.rs:4814) is a
  whole-JSON equality compare that **panics** ("run `just shaders` and
  rebuild"). The heap index and the per-shader uses-heap flag become part of
  that interface, so adding or removing a handle during `just dev` is a hard
  stop like any other interface change — expected, but it means handle-bearing
  shaders can't be iterated live across interface changes. The check is
  debug-only; release builds trust the embedded JSON, which is why the
  uses-heap flag genuinely must live in the reflection JSON rather than be
  recomputed.

**Verify:** `just sweep` + `just watch <example>`; hot reload still works. Also
`just test` — putting the heap index in the reflection JSON regenerates the `.json`
snapshots.

**Verified:** `cargo check --workspace --all-targets`, `just lint` and
`just test` clean. `just shaders` regenerated all 27 compiled JSONs and the
snapshot diffs were confirmed to be *only* the new `"bindlessHeapSet": null`
key before accepting. `crates/renderer/src/shaders/fixtures/basic_triangle.json`
is a hand-copy of the example's compiled JSON (it backs
`reflection_value_roundtrip_is_stable`) and had to be re-copied by hand —
`just shaders` does not touch it. `just sweep` 16 ok / 0 fail with the
injected-fault self-test still firing.

The field is deliberately **not** `#[serde(default)]`: a stale committed
`.json` should fail loudly at atlas init rather than read as `None`.

As in Phase 3, a green sweep proves nothing here on its own — with the flag
false everywhere, none of the new code runs. Both directions were forced:

- **`DECLARES_BINDLESS_HANDLE = true`**: all 27 shaders report
  `bindlessHeapSet: 1`, every pipeline layout declares the heap set and every
  draw, dispatch and picking pass binds it. `just sweep` 16 ok / 0 fail with
  validation on — so the layout, the set index and the three bind sites are all
  correct against a real driver, covering the graphics (all examples), compute
  (particles, watercolor) and picking (gpu_picking) paths.
- **forced index of 3** against one reflected set: `basic_triangle` exits with
  the intended `bindless heap set index 3 does not follow the 1 reflected
  descriptor set(s)` error. This is the run that surfaced the leak described
  above — before moving the check ahead of the creation loop, the same run also
  printed a `VkDescriptorSetLayout has not been destroyed` validation error
  first.

Hot reload was checked live (lavapipe + `SDL_VIDEODRIVER=offscreen`), both
paths: touching a shader logs `recompiling shaders...` →
`finished recompiling shaders` with no validation output, and adding a field to
the param block still panics with `shader interface changed`. The heap index
now being part of that compared JSON does not change either outcome.

## Phase 5 — reflection and codegen for handle fields ✅ done

The `Resources` struct is *not* where handles go — that's the whole point. A handle
is a uniform/std430 field the app writes, exactly like an `Addr<T>`.

**Paths below are stale**: reflection moved to `crates/slang-reflection/src/` in
the workspace split, so `crates/renderer/src/shaders/{json,reflection}/` is now
`crates/slang-reflection/src/{json,reflection}/`.

- ~~`crates/renderer/src/shaders/json/parameters.rs`~~
  `crates/slang-reflection/src/json/parameters.rs`: add
  `StructField::DescriptorHandle(DescriptorHandleStructField { field_name, resource_shape })`
  to the enum at :84. **The field also needs a `binding`** — the plan omitted it,
  but `field_offset_size` has to answer for a handle like any other uniform
  field. Landed as `{ field_name, binding, shape }` with a new
  `DescriptorHandleShape` enum (one `Sampler2D` case), kept separate from
  `ResourceShape` — that one describes descriptor-*bound* resources and drives
  the `Resources` struct a handle never appears in.
- ~~`crates/renderer/src/shaders/reflection/parameters.rs`~~
  `crates/slang-reflection/src/reflection/parameters.rs`: turn Phase 1's rejection
  into recognition, in the same early-continue block. Parse the declared
  `full_name()` the way pointer access modes are parsed today (:352-427). Keep a
  field-specific rejection for unsupported shapes.
- **Replace `DECLARES_BINDLESS_HANDLE` in `shaders/reflection.rs`**, which
  Phase 4 left as a hardcoded `false` because nothing could set it. It has to
  become "did the parameter walk see a handle field", threaded up to
  `reflect_pipeline_layout`'s `declares_bindless_handle` argument — most likely
  by scanning the produced `global_parameters` for the new
  `StructField::DescriptorHandle` variant, which needs no plumbing through the
  recursive `reflect_struct_fields`. Until this lands, no shader can reach the
  heap regardless of what Phases 2-4 built.

  The scan landed as `GlobalParameter::declares_bindless_handle` in
  `json/parameters.rs` rather than in the reflection walk, so it is unit-testable
  without compiling a shader. **It must recurse**, and the pointee arm is the one
  that matters: a handle inside a `Std430DataLayout` pointee is the whole
  per-material use case, and a top-level-only scan would leave the heap unbound
  for exactly it — with no downstream complaint, since the shader would simply
  sample a set that was never bound.
- **The prefix alone is not enough to accept a field — split off the `[N]`
  suffix first.** Phase 1 measured that an array's declared `full_name()` is the
  element's with `[N]` appended, so `starts_with("DescriptorHandle<")` matches
  `DescriptorHandle<Sampler2D<vector<float,4>>>[4]` too. Flipping the guard
  naively would emit one 8-byte `BindlessHandle` for a 32-byte field. That does
  not pass silently — the generated `offset_of!` on the following field, or
  `size_of` on the struct if the array is last, fails to compile — but a compile
  error inside generated code is a far worse diagnostic than a reflection bail
  naming the field, which is the whole reason Phase 1 exists.
- **Arrays of handles should stay rejected**, and now for a better reason than
  the spike's "not tested". A handle is an 8-byte element, and only 16-byte
  vector elements have stride == size in *both* std140 and std430 (see
  `validate_array_element`'s docstring and the `std140_arrays` fixture comment).
  An 8-byte element rounds up to a 16-byte stride under std140 but stays 8 under
  std430, so `Sampler2D.Handle h[4]` would lay out differently in a
  `ParameterBlock` than in a `Std430DataLayout` pointee. Supporting handle
  arrays is therefore a stride-aware-codegen problem, not a bindless one — the
  same blocker every other non-vec4 array element has. Reasoned from the repo's
  documented layout rules; a handle array's actual stride was never measured.
- New `crates/renderer/src/renderer/bindless.rs`, modeled on `addr.rs`:
  `BindlessHandle<T>` — 8 bytes (`uint2`), `PhantomData<fn() -> T>`,
  `const _: () = assert!(size_of::<BindlessHandle<T>>() == 8)`, `Serialize`,
  `pub(super)` constructor. Minted from a `TextureHandle` by an accessor
  ~~mirroring `Gpu::addr` (renderer.rs:5178)~~ — which works only because Phase 3
  stored the slot **in the `TextureHandle`**: `Gpu` (renderer.rs:5132-5136) holds
  just `flight_slot` and the buffer storages, so the accessor must read the slot
  straight off the handle, no `TextureStorage` lookup. Only the low 32 bits
  carry the slot index — the default lowering reads component 0 only — but the
  type stays 8 bytes to match the layout.

  **It landed on `Gpu` and was then moved off it**, to
  `TextureHandle::bindless_handle`. Mirroring `Gpu::addr` was the wrong instinct:
  `addr` needs `Gpu` because a device address is per-frame, while a heap slot is
  fixed for the life of the texture, so the `Gpu` version took a `&self` it never
  read and implied a frame-dependence that doesn't exist. The gate it appeared to
  provide was illusory too — a handle written into a param struct outlives the
  draw closure regardless, so `Gpu` was never restricting anything. When deferred
  slot reuse lands (Phase 3's future-work note), neither placement helps.
- `crates/cli/src/build_tasks.rs`:
  - `gather_struct_defs` (:881) emits the field as `BindlessHandle<Marker>`; add
    it to ~~**three** tables, not two~~ **five sites**. The pair that already
    special-cases the 8-byte `Addr` types (:1339-1342, :1378-1381) are both
    *alignment*-only; the size table is the test-only `rust_size_of` (:2037)
    feeding the `field_size_tripwire` test. That table has no `Addr<` arm today,
    so pointer-width fields are silently skipped by the tripwire — contradicting
    the `Pointer` arm's own comment (:2078-2080). Add the `Addr`/`ReadAddr`/
    `ImmutableAddr` arm alongside the `BindlessHandle<` one; the compile-time
    checks matter more here than usual (see Verification), so the handle should
    not join the silently-skipped set.

    The two the plan missed are both *exhaustive matches on `StructField`*, so
    they don't need finding — rustc names all three (`gather_struct_defs`,
    `field_offset_size`, `check_field_sizes`) the moment the variant is added.
    `check_field_sizes` is the one with a judgement call: the handle belongs in
    the leaf group that falls through to the size check, **not** in the inert
    group next to `Enum`, because `rust_size_of` now answers 8 for it.
  - `required_resource` (:1070) must return `None` for handle fields — **already
    true** via its `_ => None` catch-all; no change needed.
- **Not anticipated: `crates/cli/fixtures/check_crate` needs a `BindlessHandle`
  stub.** `alignment_tests` runs `cargo check` on the generated fixtures against
  that stub crate (build_tasks.rs:1925), so a new emitted type has to exist there
  too or the new fixtures fail to compile. `src/shaders/json.rs` needs nothing —
  it ignores unknown JSON fields.
- Add alignment fixtures under `crates/cli/fixtures/alignment/` — handle alone,
  handle next to an `Addr`, handle inside a pointee struct. CLAUDE.md requires
  `just test` for any `build_tasks.rs` / template change; accept snapshots with
  `cargo insta test --workspace --accept`.

  **Their entry points must take no parameters.** The first drafts wrote
  `fragMain(float2 uv)` and hit `type kind reflection not implemented: Vector` —
  a pre-existing, unrelated limit in `reflect_entry_points`, which handles only
  `Struct` and `Scalar` entry-point parameters. Real shaders pass interstage data
  through a struct; the fixtures sample at a constant uv instead, matching the
  other alignment fixtures' parameter-free style.

**Verified:** `cargo check --workspace --all-targets`, `just lint` and
`just test` clean, plus `cargo fmt`. Test changes: `handle_fields_are_rejected`
deleted (that shape is now the accepted path), `handle_arrays_are_rejected` and
`handle_vertex_inputs_are_rejected` re-pointed at the new messages, new
`unsupported_handle_shapes_are_rejected` (`Texture2D.Handle` / `SamplerState.Handle`),
and four `declares_bindless_handle` unit tests covering the nested-struct and
pointer-pointee paths.

Unlike Phases 3 and 4, the green run *is* the verification here — the new
fixtures exercise the path directly, so nothing had to be forced. What proves the
flag actually discriminates is the **contrast**, not either half alone:

- The three `handle_*` fixtures report `bindlessHeapSet: 1`; **every pre-existing
  alignment fixture snapshot was byte-identical** (only new `.snap` files were
  written, no modified ones).
- `just shaders` regenerated all 27 example shaders with a **zero-byte diff** —
  no example declares a handle yet, so the flag stays off everywhere. A diff here
  would have meant the flag was over-triggering, which is the failure mode a
  hardcoded `true` would also have passed the fixture half with.

Measured layouts, confirming a handle is 8 bytes at 8-byte alignment in both
layout rule sets (`handle_mixed`, std140: `scale` 0, `tex` **8** — a 4-byte gap —
`items` 16, `mask` **24**, `tint` 32, `offset` 48, size 64; `handle_pointee`'s
`Material`, std430: `albedo` 0, `normal` 8, `tint` 16, `roughness` 32, size 48).
The generated `Resources` struct is empty for all three — a handle consumes no
descriptor.

`spirv-dis` on `handle_pointee.frag.spv` closes the loop between the reported
index and the actual binary: `OpCapability RuntimeDescriptorArray`, the param
block at `DescriptorSet 0`, and `%__slang_resource_heap` at **`DescriptorSet 1`,
`Binding 1`** — the set the JSON reports and the one combined-image-sampler
binding `DescriptorHeap::new` creates. No `NonUniform` decoration appears, as
expected, which is the uniformity constraint Phase 10 has to write down.

`just sweep` 16 ok / 0 fail with the injected-fault self-test still firing, and
hot reload was re-checked live (lavapipe + `SDL_VIDEODRIVER=offscreen`): touching
a shader mid-run logs `recompiling shaders...` → `finished recompiling shaders`
with no validation output. Neither proves anything new on its own — no example
reaches the heap until Phase 6 — they are regression checks that the reflection
changes left the existing paths alone.

**Still unproven, by design:** no handle *value* has reached a GPU. The layout is
pinned by the generated `offset_of!`/`size_of` asserts and the SPIR-V above, and
Phase 4 already swept every example with the heap forcibly bound, but
`TextureHandle::bindless_handle` has no caller until Phase 6 converts
`depth_texture`.

## Phase 6 — `depth_texture`, the first handle on a GPU

One texture, one param block, no new machinery — the smallest thing that proves
the whole Phase 0-5 stack works end to end. Ordered first for exactly that
reason: Phases 7 and 8 are a large detour, and it would be foolish to take them
before knowing a handle value survives the trip.

- `Sampler2D texture` → `Sampler2D.Handle texture`
  (examples/depth_texture/shaders/source/depth_texture.shader.slang:14). The
  `params.texture.Sample(...)` call site at :44 is unchanged — pinned by the
  `handle_params` fixture.
- After `just shaders depth_texture`: the generated `Resources` loses its
  `texture` field (a handle consumes no descriptor), `DepthTextureParams` gains
  a `BindlessHandle<Sampler2D>` — expect offset 192, after the 192-byte
  `mvp` — and `main.rs` writes it in `draw` as `self.texture.bindless_handle()`.
  **Keep the `TextureHandle` field**: it is what owns the heap slot, and since
  Phase 3 stored the slot on the handle it is also the only way to reach it.
- **No `mltrs::TexHandle` typealias.** This plan originally said "consider one
  for symmetry with `addr.slang`" — decided against. `Addr<T>` needs an alias
  because `Ptr<T, Access, AddressSpace, Layout>` is unreadable; `Sampler2D.Handle`
  is already terse, and an alias would hide that it is a *combined* image
  sampler, which is the one distinction the heap actually constrains (binding 1,
  not 0 or 2 — Phase 3). So there is no new vendored module and no
  `just vendor-shaders` re-seed, and the uniformity rule that would have lived
  in its comment goes in `docs/` instead (Phase 10).

**Verify:** `just test`, then read the regenerated
`shaders/compiled/depth_texture.json`: `bindlessHeapSet` flips `null` → `1`, the
`combinedTextureSampler` range at binding 1 disappears, and `texture` moves from
a `descriptorTableSlot` binding to a uniform field. Then **run it** — this is the
first handle value to cross to a GPU, and a wrong slot renders the wrong texture
with no validation error, so the visual check is the verification, not a
formality. `just sweep` covers it headlessly with validation on.

## Phase 7 — push constants: reflection and codegen

**Why this phase exists.** Phase 9 needs a per-draw material index, and *this
renderer has no per-draw data channel*. Measured, not assumed: the draw loop
records `cmd_draw_indexed(index_count, 1, first_index, 0, 0)` (renderer.rs:2121),
descriptor sets are bound per-pipeline with no dynamic offsets (`&[]`,
renderer.rs:2096), and `cmd_push_constants` is called **nowhere** in the
workspace. With 5 pipelines each drawing several batches, the index has to vary
between draws that share a pipeline *and* its one descriptor set. Nothing today
can carry it.

**Why not the cheap trick.** `vkCmdDrawIndexed`'s `firstInstance` is a ~15-line
change and would be dynamically uniform by construction (instance count is
always 1). It is a trap. Slang lowers HLSL semantics with **D3D** meaning:
disassembling a committed artifact,
`spirv-dis examples/sprite_batch/shaders/compiled/sprite_batch.vert.spv` shows
`SV_VertexID` becoming `VertexIndex - BaseVertex` —

```
OpCapability DrawParameters
OpDecorate %8 BuiltIn BaseVertex
OpDecorate %gl_VertexIndex BuiltIn VertexIndex
%70 = OpISub %int %69 %68
```

— so by construction `SV_InstanceID` is `InstanceIndex - BaseInstance`, which
under that scheme is **identically 0**. It compiles, validates and runs clean
while painting every batch with material 0. The correct semantic exists
(`SV_VulkanInstanceID`; `strings` on the pinned `libslang-static.a` confirms it
alongside `SV_StartInstanceLocation` and `SV_BaseInstanceID`), but the approach
also needs an extra `nointerpolation uint` interstage varying to reach the
fragment stage, and it consumes the base-instance channel that
[render-graph/05_multi_draw_rendering.md](render-graph/05_multi_draw_rendering.md):423-425
explicitly reserves. Push constants are the honest channel: genuinely per-draw,
readable in both stages with no varying, 128 guaranteed bytes.

**This is not new design.**
[render-graph/05_multi_draw_rendering.md](render-graph/05_multi_draw_rendering.md)
§4 (:176-240) already specifies the feature as its "Phase B", down to the ≤128 B
compile-time assert (:348). Phases 7-8 are that work, scoped to what Phase 9
needs.

**The layout half is already complete** — worth stating, because the `todo!()`
at reflection/pipeline_layout.rs:342 reads like the opposite.
`add_push_constatant_range_for_constant_buffer` (pipeline_layout.rs:53; the typo
is in the source) builds the range, it reaches the JSON, and `ToVk` turns it
into a `vk::PushConstantRange` at renderer.rs:5254-5267. That `todo!()` is
unreachable for push constants because :241 early-returns first. The
`.offset()` bug is fixed too (vulkan_1_3_migration/bda_renderer_plumbing.md:63).
What is missing is the block's *contents* and anything that writes it.

Declaration form: `[[vk::push_constant]] ConstantBuffer<MyDraw> draw;`

- **Open the gate at reflection/parameters.rs:34-38**, which today hard-bails on
  *any* non-`ParameterBlock` global — and a push-constant global reflects as
  `TypeKind::ConstantBuffer`, so it is rejected outright. Add a
  `GlobalParameter::PushConstant` variant carrying the same
  `{ parameter_name, element_type }` shape, so it lands in the existing
  `globalParameters` array rather than a new JSON root key.
- **Reuse `reflect_struct_fields` unchanged.** Offsets come from slang via
  `param_binding` (parameters.rs:809-841) and are never computed, so no new
  layout logic is needed. `param_binding`'s `todo!()` catch-all does need a
  `ParameterCategory::PushConstantBuffer` arm.
- **Reject a push block in a compute shader** (the compute twin of the gate, at
  parameters.rs:736-740) until Phase 8's dispatch-side counterpart exists.
  Otherwise reflection and codegen would accept one and the dispatch path would
  silently never push it.
- **Extend `declares_bindless_handle`** to scan a push block: a
  `Sampler2D.Handle` there must still set `bindlessHeapSet`.
- **Reflection and codegen cannot be split into separate phases.** Adding a
  `GlobalParameter` variant breaks three irrefutable patterns —
  build_tasks.rs:357, :611, and the `let Self::ParameterBlock(block) = self;` in
  json/parameters.rs:100. All three are compile errors, which is the point, but
  it means the two land together.
- **Codegen emits the struct with `generate_std430_struct_fields`, and asserts
  the computed size equals the reflected size** — the precedent is the pointer
  arm at build_tasks.rs:920-926. Vulkan push blocks are std430-shaped, but that
  is a *guess* about what Slang reports and the assert is what turns a wrong
  guess into a loud failure instead of a silently misaligned struct. If it fires,
  the lever is `program_layout.type_layout(ty, LayoutRules::DefaultConstantBuffer)`,
  the same explicit re-query the std430 pointer path already does at
  parameters.rs:417-419.
- Emit the **≤128 B compile-time assert** (05_multi_draw_rendering.md:348).
- **The push struct does not go through `Resources`.** `Resources` is
  descriptor-set bindings in set-layout order; a push block is per-draw, not
  per-pipeline. `required_resource`'s `_ => None` (build_tasks.rs:1092) already
  handles it with no edit — as it did for handles in Phase 5. Surface the type
  on the generated `Shader` instead, modelled on
  `pub const WORKGROUP_SIZE` (shader_compute_entry.rs.askama:94), so Phase 8's
  API can be typed rather than raw bytes.
- **The four size/alignment tables need no edits**: they key on emitted Rust
  type names that already have arms, and both layout generators are
  field-kind-agnostic. (Phase 5's note about *five* sites is now stale in a
  second way — `rust_size_of` and `check_field_sizes` were deleted in `2f07b2a`
  and replaced by `assert_no_uniform_bytes_dropped`, build_tasks.rs:1332.)
- **Known gap, not worth closing here:** a push struct declared in a *shared*
  `.slang` module will not hoist. `shared_imports_for_shader`
  (build_tasks.rs:1692-1731) finds shared types only via field type names of
  local structs, and a top-level push block is not one. Phase 9 declares its
  block locally.

**Verify:** a new `crates/cli/fixtures/alignment/push_constants.shader.slang`
(a block with a scalar, a `float4x4` and a handle, exercising both the ≤128 B
assert and the handle-in-push-block heap flag) — `alignment_tests` discovers
fixtures automatically and `cargo check`s the generated layout asserts against
`fixtures/check_crate`. The pass condition is the Phase 5 **contrast**: new
snapshots written, and **zero** pre-existing snapshots modified. Plus a
`spirv-dis` check that emitted member offsets match reflected ones —
`pointer_pointee_spirv_layout` (build_tasks.rs:2096) is the model. A rejection
test for the compute case, via the existing `reflect_rejected_shader` helper.

## Phase 8 — push constants: renderer and per-draw API

- **Retain the range.** It is currently dropped after `create_pipeline_layout`;
  `ShaderPipelineLayout` (renderer.rs:5001-5013) doesn't keep it. Add a field
  next to `bindless_heap_set` and populate it at the same four
  `create_from_atlas` sites, where `reflected_layout.push_constant_ranges` is
  already in scope — so `vk_create`'s return tuple does not change. Assert at
  most one range: a *global* push block reflects with stage flags `All`, because
  `current_stage_flags` is `All` when `add_global_scope_parameters` reaches it
  (pipeline_layout.rs:272), so vertex + fragment produce **one** range, not two.
- **Payload on `PendingDrawCommand::Draw`, not inside `DrawCallConfig`** — the
  latter is `Copy`. Store it inline as `[u8; 128]` plus a length rather than a
  `Vec<u8>`: 128 is the spec floor so it is exactly right-sized, it keeps the
  type `Copy`, and it costs no allocation per draw per frame.
- `cmd_push_constants` in the record loop between `cmd_bind_bindless_heap`
  (renderer.rs:2105) and the `match draw_call` (:2107), mirroring
  `cmd_bind_bindless_heap`'s shape. Push constants survive descriptor-set binds
  and are invalidated only by an incompatible pipeline bind, so once per loop
  iteration is both necessary and sufficient.
- **New `_with_push_constants` queue methods rather than a fourth parameter on
  the three existing `queue_draw_*`.** The ~14 existing callers declare no push
  block, so threading a parameter through all of them buys nothing that the
  asserts below don't already give. This is a judgement call and the opposite of
  what the rejected `firstInstance` design needed, where every variant had to
  carry the value or silently ignore it.
- **Two record-loop debug asserts, both directions**, which is what makes the
  above safe: a pipeline whose layout declares a range receives bytes of exactly
  that size, and a draw carrying bytes targets a pipeline that declares a range.
  A length mismatch is `VUID-vkCmdPushConstants-offset-01795` and validation
  would catch it; a *missing* push is undefined data with no diagnostic at all.
  The picking path records its own hardcoded single draw
  (renderer.rs:1813-1821) and needs the same assert.
- **No device-suitability check.** `maxPushConstantsSize`'s 128 B guarantee is
  the spec floor, so a gate in `undersized_limits` would be dead code — unlike
  the bindless heap limits in Phase 3, which genuinely vary. Phase 7's
  compile-time assert is the real check.

Two constraints recorded deliberately, because both are invisible until they
bite:

- **Compute push blocks stay rejected** (Phase 7's gate). The dispatch path
  (renderer.rs:1568-1652) has no push call; adding one is symmetric — between
  :1622 and :1624, plus the same retained field on
  `ComputeShaderPipelineLayout` (:5082).
- **A push block cannot carry a BDA address.** `Gpu` — which mints
  `Addr`/`ImmutableAddr` — is constructed at renderer.rs:2474, *after* every
  `queue_draw_*` call and after `submit_draws`. So an address minted in the
  submit closure does not exist at queue time. This directly contradicts
  05_multi_draw_rendering.md §4's design, which puts an
  `ImmutableAddr<MaterialData>` in the push block. The fix is small — address
  minting takes `&self` and `flight_slot` is already correct at queue time, so
  an `&self` minting API on `FrameRenderer` would return the same values — but
  it belongs to that doc. Phase 9 sidesteps it: its push block is one `uint`,
  and the `ImmutableAddr<Material>` stays in the param block where the closure
  writes it.

**Verify:** `just test` with **no** snapshot churn (this phase touches no
reflection, codegen or template code), `just lint`, `just sweep`. As in Phases 3
and 4, a green sweep proves nothing on its own while no example declares a push
block — force the path with a temporary block on one example and confirm both
that the value arrives and that deleting the push call trips the new debug
assert rather than rendering garbage.

## Phase 9 — `toon_link`, the actual payoff

`build_material_pipelines` (examples/toon_link/src/main.rs:780-826)
~~collapses to one pipeline plus a `Material` buffer behind `ImmutableAddr`.
Per-material pipelines and per-material uniform buffers both disappear.~~
**Half right — see below.**

The shape:

```slang
struct ToonLinkDraw { uint materialIndex; }
[[vk::push_constant]] ConstantBuffer<ToonLinkDraw> draw;

struct Material {
    Sampler2D.Handle tex0;          // std430    0
    Sampler2D.Handle tex1;          //           8
    TevParams tev;                  //          16
    GXAlphaCompare alphaCompare;    //        1344  (last, keeps 16-alignment)
}

struct ToonLinkParams {
    mltrs::ImmutableAddr<Material> materials;
    mltrs::MVPMatrices mvp;
    DebugMode debugMode;
}
```

Both entry points read `params.materials[draw.materialIndex]` directly. **That
is the concrete win over the `firstInstance` alternative Phase 7 rejected: no
interstage varying, no `FragVertex` change, no vertex-input change.**

Two layout facts worth pinning before writing it:

- `TevParams` is **layout-identical in std140 and std430** — every member is a
  `uint4`/`float4` or an array of one, so array stride is 16 either way. Moving
  it into a std430 pointee costs nothing.
- `GXAlphaCompare` is **not** (std140 pads it to 32; std430 leaves 20/align-4).
  It must move *entirely* into `Material`. A type appearing in both layouts
  trips the "shared type has an incompatible layout" panic at
  build_tasks.rs:1447 — loud, but better avoided by construction.

`tev.slang` needs no signature change: convert at the boundary
(`Sampler2D tex0 = material.tex0;`) and leave `tevSampleTexmap`/`evalStages`
taking `Sampler2D`, which keeps `tev.slang` free of bindless concepts. Its
comment at tev.slang:207-214 ("Dynamic sampler indexing does not exist: tex0/tex1
are two distinct ParameterBlock fields…") has both premises invalidated and must
be rewritten — still a branch, still uniform, but now because the index is a
per-draw push constant.

**This does not need `NonUniformEXT`.** toon_link issues one index-range draw per
batch (main.rs:1178-1186) and keeps doing so; the material index is uniform within
each draw. The win here is ~~~24 fewer pipelines and~~ ~24 fewer uniform buffers,
not fewer draws.

**The pipeline half of that claim is wrong: bindless does not collapse
toon_link to one pipeline.** A pipeline here is not differentiated only by its
textures — `raster_state` (main.rs:693) varies cull mode, depth compare, depth
write, blend mode *and* color write mask per material, the last via
`decal_role`. None of that is descriptor state, so removing the texture
descriptors leaves it untouched. Bindless removes the *texture*-driven pipeline
explosion; the *state*-driven one survives.

**Counted, not estimated: 24 materials → 5 distinct raster states.** Derived by
grouping `link.manifest.json` and applying `raster_state`/`blend_mode`/
`decal_role` by hand (`CULL_OVERRIDE` is `None`, so cull comes from the
manifest):

| derived `RasterState` (cull, depth test, depth write, blend, color write) | materials |
|---|---|
| Back, LessEqual, write, Opaque, RGB | 11 |
| Back, Always, no-write, Blend(DstA, InvDstA), RGB — `Composite` | 4 |
| Back, LessEqual, no-write, Blend(SrcA, InvSrcA), A — `Mask` | 4 |
| Back, Always, no-write, Opaque, A — `Erase` | 4 |
| None, LessEqual, write, Opaque, RGB | 1 |

The one assumption is that the 12 `BlendMode::None_` materials are the opaque
ones (`decal_role` returns `Ok(None)` for those), which the 4/4/4 eye/brow
assertion at main.rs:156-158 corroborates: 12 translucent + 12 opaque = 24.
Worth re-deriving if the manifest changes — the honest headline for this phase is
**24 pipelines → 5, and 24 uniform buffers → 1**. Dedup with a linear scan:
`RasterState` is `Eq + Copy` with 24 candidates, and adding `Hash` to a renderer
type for this would be backwards. Assert the result (`ensure!(pipelines.len() == 5)`)
so a manifest change that re-explodes the count is loud rather than a silent
regression.

`alpha_compare` is *not* a sixth dimension: it rides in the uniform data as a
shader-side discard, not in the pipeline.

**Draw-per-material is load-bearing, not a limitation to design away.** It is
what makes the material index dynamically uniform for free, which is the
property the whole uniformity constraint above is about — and a push constant is
the cleanest possible expression of it, being per-draw constant by definition.
Visibility-buffer architectures (Nanite-style material binning) exist to *recover* this property
after GPU-driven culling makes CPU-side material sorting impossible; toon_link
has not given it up and has nothing to recover. It also could not adopt that
shape if it wanted to — blending and per-material raster state are fixed-function
raster, which a per-material compute pass cannot vary, and the comment at
main.rs:721-723 notes that *not* writing depth is what lets the eye/brow decals
composite at all. The trigger for revisiting is losing CPU-side material sorting,
which nothing here is near.

Merging those draws is the follow-on
([render-graph/05_multi_draw_rendering.md](render-graph/05_multi_draw_rendering.md)),
and even that likely stays uniform if the material index comes from `gl_DrawID`.
The decoration is only needed if the index varies *within* one draw — packed into
vertex or instance data. That's the point at which the `getDescriptorFromHandle`
non-goal has to be revisited, together with the
`shader_sampled_image_array_non_uniform_indexing` bit Phase 2 omits — the two
must land together (the decoration without the feature is a validation error).

One invariant this phase falsifies: `MaterialSlot`'s doc comment (main.rs:67-74)
promises "one pipeline is baked per material slot, in slot order". Its
replacement earns its keep, because slot order becomes the `Material` buffer's
order and `slot.raw()` becomes the pushed index — so a slot/batch mixup now
selects the wrong *texture* as well as the wrong TEV state.

**Verify:** `just shaders toon_link`, `just test`, `just sweep`. But a green
sweep is weak evidence here — a wrong material index produces no validation
output at all. The real check is a visual A/B against a pre-change build, on
four things that each isolate a different failure:

- tunic and hat are their own colors, not all `ear`'s → the index really varies
  per draw (a dead push constant paints everything slot 0);
- eyes and brows composite *through* the bangs → the 5-pipeline grouping
  preserved the mask/face-hair/composite/erase draw order;
- `sleeve` is still double-sided → the 1-material `CullMode::None` state didn't
  get merged into the 11-material group;
- debug modes `RawTex0` (6) and `RawTex1` (7) show the right albedo and ramp per
  batch, and toggling `eflight` / dragging `env_actor_c0` still reaches only the
  lit materials.

Also `just toon_link link-verify-p1` — but recorded for what it is: it diffs
`convert-link --info` against the gclib oracle and runs the converter's ignored
tests, and never builds `toon_link` at all. A free unchanged-converter guard,
not evidence about this change.

## Phase 10 — docs

- Rewrite the status header on [render-graph/03_bindless.md](render-graph/03_bindless.md);
  it currently says texture binding remains per-pipeline descriptors.
- Update `docs/` for the shader-authoring workflow (handles are data, not
  `Resources` entries).
- **State the uniformity invariant as a hard rule in `docs/`**, not just here:
  *a texture handle — and any index used to select the struct that carries it —
  must be dynamically uniform within a draw.* Don't source handles or their
  selecting indices from vertex or instance data, and don't select between two
  handles per-invocation (e.g. a per-pixel ternary on two material textures) —
  both produce a divergent heap access that Slang compiles without
  `NonUniformEXT` and without complaint. Nothing enforces this: not the
  compiler, not validation (it's data-dependent), not reflection (which sees
  declarations, not indexing expressions), and not `just sweep` — the failure
  is wrong-texture rendering on wave-scalarizing hardware (AMD) while staying
  green on hardware that tolerates it. Divergent indexing becomes legal only
  when the `getDescriptorFromHandle` override lands together with the
  `shader_sampled_image_array_non_uniform_indexing` feature bit (core 1.2, and
  part of the `descriptorIndexing` bundle Vulkan 1.3 mandates — no extension
  needed; see Phase 9). Phase 6 decided against a `mltrs::TexHandle` alias, so
  `docs/` is the only place this rule lives — there is no vendored comment for
  shader authors to read instead, which makes writing it down properly matter
  more, not less.
- **Say how the invariant is actually satisfied today**, not just what it
  forbids: the material index rides in a push constant, and a push constant is
  per-draw constant by definition. That is the pattern to copy.
- Update [render-graph/05_multi_draw_rendering.md](render-graph/05_multi_draw_rendering.md)
  §4 (:225-237), which states the push-constant path is "completely dead — no
  `.slang` declares one, there is no `cmd_push_constants` call, and no `Gpu`
  API". False after Phase 8. Its line references into `renderer.rs` are stale
  too and can be refreshed in the same pass. Mark §9's "Phase B" partly done,
  with the address-in-push-block half (blocked on the `Gpu` ordering constraint
  in Phase 8) explicitly still open.

## Phase 11 — watercolor (follow-up; investigate first)

Not planned as part of the original work, and **not a prerequisite for anything**.
Added because measuring toon_link's real payoff (24 → 5, above) prompted checking
what watercolor's would be, and it turns out to be the better demo.

**The prize: 22 pipelines → 10, and every duplicate is pure descriptor
duplication.** Counted: 18 `create_compute_pipeline` calls against 9 distinct
`*.compute.slang` shaders — exactly 2× — plus `display_pipelines:
[PipelineHandle<DrawVertexCount>; 4]` from one graphics shader. Every duplicate
exists only to bind the other side of a ping-pong pair, which the source says
outright ("Brush pipeline: 2 variants for wet_mask/pigment parity",
"4 pipelines for (sim_parity × deposit_parity)", examples/watercolor/src/main.rs:473,652).

Two reasons this beats toon_link as a showcase:

- **Zero raster-state component.** toon_link's residual 5 pipelines are blend /
  depth-write / cull / color-mask variants that bindless cannot touch. Watercolor's
  duplicates are identical shader code with identical state and a different image
  bound — exactly what the heap erases, with no floor underneath.
- **Uniform by construction, with no index at all.** Parity is CPU state written
  into the param block per frame, so the shader reads *the* handle rather than
  selecting one. None of the uniformity hazards in Phase 10 apply, and it needs no
  per-draw channel (contrast toon_link, where the index has to come from
  somewhere — Phases 7-8 exist to build that channel). It is therefore the
  *safest* first non-trivial consumer, not just the most rewarding.

Synchronization is not the usual objection here: the renderer has no automatic
barrier tracking for bindless to break (`PendingComputeCommand::Barrier` is
already app-driven), and the `storage_texture_as_sampled` aliases already live in
`GENERAL`, so no layout invariant changes.

**Only the read half fits inside today's heap, and even that needs a source
change.** Watercolor's compute passes read `Texture2D<float>`, which is a separate
*sampled image* — Slang heap binding 2 — not `Sampler2D` (combined, binding 1,
the only binding `DescriptorHeap` creates). Those declarations have to become
`Sampler2D<float>` first. Sampling method is unaffected; a combined descriptor
serves `Load` as well as `Sample`.

**The write half (`RWTexture2D`) is out of scope until a spike says otherwise**,
and stays a non-goal above. Storage images are `VK_DESCRIPTOR_TYPE_STORAGE_IMAGE`,
a different type from the heap's one binding. Entering the heap would need:

- **Slang's heap binding number for storage images — unmeasured.** The Phase 0
  spike confirmed 0 sampler / 1 combined / 2 sampled and never probed storage.
  This is the spike to run before estimating anything else here.
- `descriptorBindingStorageImageUpdateAfterBind` and
  `shaderStorageImageArrayDynamicIndexing`, neither requested in Phase 2.
- `maxPerStageDescriptorUpdateAfterBindStorageImages` and
  `maxDescriptorSetUpdateAfterBindStorageImages` in `undersized_limits`.
- A second heap binding, plus relaxing the Phase 5 shape rejection — which Phase 3
  notes must happen together.

So the plausible shapes are: **(a)** convert reads only, keeping storage writes on
per-pipeline descriptors — collapses the pipelines that vary by *read* target,
which is most of them; or **(b)** spike storage-image handles first and convert
both. Start with the spike, since it decides whether (a) is a stepping stone or
the destination.

**This does not delete `StorageTexture`.** That type owns `vk::Image`,
`vk_mem::Allocation`, `vk::ImageView`, format and extent — it is an *ownership*
type, and bindless changes how shaders reach a resource, not who owns it. The
actual wart is that watercolor holds **two handles for one image**, a
`StorageTextureHandle` plus a `TextureHandle` from `storage_texture_as_sampled`
aliasing the same `VkImage`. Collapsing those into one texture type carrying both
usages and both views is a separate refactor that bindless neither requires nor
provides, and which could be done today, independently.

**Verify:** `just shaders watercolor`; `just test`; `just sweep`. Watercolor is a
simulation, so a green sweep is weak evidence — a wrong ping-pong handle renders a
plausible-looking but wrong image with no validation output. Compare frames
against the pre-migration build, and convert one pass at a time rather than all
nine at once.

---

## macOS notes

The feature choice doesn't change for macOS — if anything `DescriptorHandle` is the
*more* portable option, being the one abstraction with a native Metal
argument-buffer lowering if a Metal target is ever added. Three parameters change:

- **Keep the heap small and fixed.** MoltenVK implements descriptor indexing on top
  of Metal argument buffers (`MVK_CONFIG_USE_METAL_ARGUMENT_BUFFERS`, on by default
  since ~SDK 1.3.290). Metal's argument-buffer limits are far below desktop
  Vulkan's. A few thousand slots is safe; unbounded is not.
- **No variable descriptor count** (already the plan), and treat `UPDATE_AFTER_BIND`
  as needing real testing on Metal rather than assumed.
- **Prefer combined image samplers** over separate texture + sampler, so one heap
  binding lights up instead of two. This is already what Phase 3 does, and the spike
  confirmed a `Sampler2D.Handle` lights up only binding 1 while the separate form
  lights up 0 and 2.

The macOS floor doesn't move: requiring Vulkan 1.3 + `bufferDeviceAddress` already
pins us to recent MoltenVK on Apple Silicon. The honest caveat is maturity — the
argument-buffer path is the least-exercised part of MoltenVK, so budget for
macOS-specific debugging Linux and Windows won't surface.

Separately: `platform.rs` never adds `VK_KHR_portability_subset` to
`REQUIRED_DEVICE_EXTENSIONS` despite the spec requiring it when advertised. That's
an existing latent bug worth fixing before trusting any macOS result.

## Verification

| Phase | Check |
|---|---|
| 0 | ✅ scratch compile + `spirv-dis`; answers in [bindless_textures/phase_0_spike.md](bindless_textures/phase_0_spike.md) |
| 1 | ✅ `just test` + three rejection tests |
| 2 | ✅ `cargo check --workspace --all-targets`, `just lint`, `just sweep` |
| 3 | ✅ `just sweep` 16 ok / 0 fail with the heap allocated but unbound, plus a temporary `MAX_BINDLESS_TEXTURES = 1` run to prove the write path executes |
| 4 | ✅ `just sweep` 16 ok / 0 fail, hot reload live-checked both ways, `just test` with only the new JSON key; plus forced `DECLARES_BINDLESS_HANDLE = true` and forced-mismatch runs to prove the path executes |
| 5 | ✅ `just test` with three new handle fixtures at `bindlessHeapSet: 1` while every pre-existing fixture stayed byte-identical, `just shaders` zero-diff across all 27 example shaders, `spirv-dis` confirming the heap at set 1 / binding 1, `just sweep` 16 ok / 0 fail, hot reload live-checked |
| 6 | `just shaders depth_texture`, `just test`, `just sweep` — plus a **visual** run, since a wrong heap slot renders the wrong texture silently |
| 7 | `just test` with a new `push_constants` fixture and **zero** pre-existing snapshot changes; `cargo check` of the generated layout asserts; `spirv-dis` confirming emitted member offsets match reflected ones |
| 8 | `cargo check --workspace --all-targets`, `just lint`, `just test` with no snapshot churn, `just sweep` — plus a forced push-block run, since nothing declares one yet |
| 9 | `just shaders toon_link`, `just test`, `just sweep`, `just toon_link link-verify-p1` — plus the four-point visual A/B, since a wrong material index is silent |
| 10 | docs only |
| 11 | `just shaders watercolor`, `just test`, `just sweep` — plus a frame comparison against the pre-migration build, since a wrong ping-pong handle is silent |

Per [`docs/testing.md`](../docs/testing.md), read before accepting any snapshot or
adding a validation check. Layout bugs behind device addresses, heap indices and
push constants produce no validation errors, which is why the generated
`offset_of!`/`size_of` assertions (build_tasks.rs:1189) matter more here than
usual — and why Phases 6, 8 and 9 all lean on a run-and-look step that a green
`just sweep` cannot substitute for.

## References

- [DescriptorHandle&lt;T&gt; core module reference](https://docs.shader-slang.org/en/stable/external/core-module-reference/types/descriptorhandle-0a/index.html)
- [Slang user guide: convenience features](http://shader-slang.org/slang/user-guide/convenience-features) — default SPIR-V lowering, `-bindless-space-index`, `getDescriptorFromHandle` override
- [slang#8610](https://github.com/shader-slang/slang/discussions/8610) — how the heap indirection works
- [slang#8063](https://github.com/shader-slang/slang/issues/8063) — same-set bindings vs. `VARIABLE_DESCRIPTOR_COUNT`
- [MoltenVK#2278](https://github.com/KhronosGroup/MoltenVK/issues/2278) — descriptor indexing + variable count fault
