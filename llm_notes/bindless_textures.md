# Bindless Textures via Slang `DescriptorHandle`

**Status: Phase 0 spiked, Phases 1+ not started.** Design note for adopting bindless
texture access using Slang's `DescriptorHandle<T>` with its default SPIR-V lowering.

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

That collapses toon_link to one pipeline plus a material buffer, and is a
prerequisite for the batching sketched in
[render-graph/05_multi_draw_rendering.md](render-graph/05_multi_draw_rendering.md).

## Why this option and not the others

- **Raw descriptor arrays** (`Texture2D textures[]` + `NonUniformResourceIndex`)
  would require hand-written `[[vk::binding]]` annotations, which breaks the
  positional-binding assumption stated twice in reflection/pipeline_layout.rs:187,239.
- **Overriding `getDescriptorFromHandle`** is an escape hatch, not a starting
  point. It stays available later *without touching shader source*. Deferring it
  costs nothing **until something needs a material index that varies within a
  single draw** — the spike found Slang emits no `NonUniformEXT` and offers no
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
  within one draw. See Phase 6.
- `VK_DESCRIPTOR_BINDING_VARIABLE_DESCRIPTOR_COUNT_BIT` — see Phase 3.
- Storage images (`RWTexture2D`, watercolor) stay on per-pipeline descriptors.
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

## Phase 1 — reject handle fields loudly (lands alone, before any Vulkan work)

Today a `Sampler2D.Handle` field compiles, generates a `UVec2` binding, and passes
every generated `offset_of!`/`size_of` assertion while being silently wrong. Close
that first, so every later intermediate state of this branch is safe.

- In `crates/renderer/src/shaders/reflection/parameters.rs`, detect a declared
  `full_name()` starting with `DescriptorHandle<` and bail, in the style of the
  `StructuredBuffer` rejection (:298).
- It goes in the **early-continue block alongside the existing enum special case**
  (:177-198), *not* in the `TypeKind::Resource` match — the type reflects as
  `TypeKind::Vector`, so by the time the `kind()` match runs the information is
  gone. The enum case is the model for the *placement* only: it checks
  `field.ty().kind() == TypeKind::Enum`, while the `full_name()`-prefix
  technique this check needs is the one the *pointer* arm already uses
  (:360-364). Don't inherit the enum arm's `Binding::Uniform`-only bail
  wholesale either — a handle in a vertex-input position needs its own message.
- **Arrays of handles bypass this check**: the `TypeKind::Array` arm
  (reflection/parameters.rs:433-471) never recurses into
  `reflect_struct_fields`, so `Sampler2D.Handle handles[4]` won't hit the loud
  rejection. It *is* still rejected — `validate_array_element` only accepts
  16-byte vec4-shaped elements — but with a generic array message, same shape
  as the existing `enum_arrays_are_rejected` case. Acceptable; add a fixture to
  pin it (this also closes the spike's open question about handle arrays on the
  reflection side).
- Phase 5 flips this from rejection to support. The *shape* rejection (anything
  that isn't `Sampler2D`) survives into Phase 5, and is what lets Phase 3 create
  only one heap binding.

**Verify:** `just test`, plus rejection tests next to `structured_buffer_is_rejected`
(crates/cli/src/build_tasks.rs:2335) — scalar handle field, handle array, handle
in a vertex-input position.

## Phase 2 — device features (behaviorally invisible; land alone)

`create_logical_device` (renderer.rs:3474) currently requests zero
descriptor-indexing bits. Add to `vulkan_12_features` (:3510):

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
It's a **core-1.0 `VkPhysicalDeviceFeatures` bit**, so it goes on the base
`features` builder (renderer.rs:3491), *not* on `vulkan_12_features`.
Universally supported in practice, but validation flags its absence.

`shader_sampled_image_array_non_uniform_indexing` is deliberately **omitted**:
nothing the compiler emits needs it, because `NonUniformEXT` never appears. Add it
only alongside whatever resolves that (see Phase 6) — requesting it now would imply
a guarantee we don't have.

Mirror every bit — including the core-1.0 one — in the physical-device gate
(renderer.rs:3209-3237) and extend the bail message at renderer.rs:3290-3295.

**Verify:** `just sweep` stays clean. Lavapipe supports descriptor indexing, so CI
covers this.

## Phase 3 — the heap, created but not bound

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
- Its own pool with `DescriptorPoolCreateFlags::UPDATE_AFTER_BIND_POOL` and the
  matching `DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL`.
- **Stage flags: `ShaderStageFlags::ALL`**, matching how reflected global
  bindings already come out (reflection/pipeline_layout.rs:40, :262). And don't
  assume `MAX_BINDLESS_TEXTURES` fits: validate it at startup against
  `maxPerStageDescriptorUpdateAfterBindSampledImages` and
  `maxDescriptorSetUpdateAfterBindSampledImages` — the suitability gate only
  warn-skips devices, so a too-small limit must fail loudly here instead.
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

**Verify:** `just sweep` clean with validation on. The heap is allocated and
populated but referenced by nothing, so any error here is a pure layout/lifetime
bug.

## Phase 4 — pipeline layouts and per-pipeline binding

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
- **Bind the heap after each `cmd_bind_pipeline`, not once per command buffer.**
  Pipeline-layout compatibility preserves a binding at set N only across layouts
  with identical set layouts for 0..N. Set 0 is the per-shader param block, so it
  differs between every pair of pipelines and each pipeline switch disturbs the heap
  binding. Add a second `cmd_bind_descriptor_sets` at `first_set = <heap index>`
  next to the existing bind-from-0 calls (renderer.rs:1584, :1770, :2048). Cheap;
  the failure mode if skipped is a "descriptor set not bound" validation error at
  the first pipeline switch.
- Append and bind **only for shaders that declare a handle** — the `DescriptorHandle<`
  scan from Phase 1/5. The reported space index is not a usage signal; a handle-free
  shader still reports one.
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

## Phase 5 — reflection and codegen for handle fields

The `Resources` struct is *not* where handles go — that's the whole point. A handle
is a uniform/std430 field the app writes, exactly like an `Addr<T>`.

- `crates/renderer/src/shaders/json/parameters.rs`: add
  `StructField::DescriptorHandle(DescriptorHandleStructField { field_name, resource_shape })`
  to the enum at :84.
- `crates/renderer/src/shaders/reflection/parameters.rs`: turn Phase 1's rejection
  into recognition, in the same early-continue block. Parse the declared
  `full_name()` the way pointer access modes are parsed today (:352-427). Keep a
  field-specific rejection for unsupported shapes.
- New `crates/renderer/src/renderer/bindless.rs`, modeled on `addr.rs`:
  `BindlessHandle<T>` — 8 bytes (`uint2`), `PhantomData<fn() -> T>`,
  `const _: () = assert!(size_of::<BindlessHandle<T>>() == 8)`, `Serialize`,
  `pub(super)` constructor. Minted from a `TextureHandle` by an accessor mirroring
  `Gpu::addr` (renderer.rs:5178) — which works only because Phase 3 stored the
  slot **in the `TextureHandle`**: `Gpu` (renderer.rs:5132-5136) holds just
  `flight_slot` and the buffer storages, so the accessor must read the slot
  straight off the handle, no `TextureStorage` lookup. Only the low 32 bits
  carry the slot index — the default lowering reads component 0 only — but the
  type stays 8 bytes to match the layout.
- `crates/cli/src/build_tasks.rs`:
  - `gather_struct_defs` (:881) emits the field as `BindlessHandle<Marker>`; add
    it to **three** tables, not two. The pair that already special-cases the
    8-byte `Addr` types (:1339-1342, :1378-1381) are both *alignment*-only; the
    size table is the test-only `rust_size_of` (:2037) feeding the
    `field_size_tripwire` test. That table has no `Addr<` arm today, so
    pointer-width fields are silently skipped by the tripwire — contradicting
    the `Pointer` arm's own comment (:2078-2080). Add the `Addr`/`ReadAddr`/
    `ImmutableAddr` arm alongside the `BindlessHandle<` one; the compile-time
    checks matter more here than usual (see Verification), so the handle should
    not join the silently-skipped set.
  - `required_resource` (:1070) must return `None` for handle fields.
- Add alignment fixtures under `crates/cli/fixtures/alignment/` — handle alone,
  handle next to an `Addr`, handle inside a pointee struct. CLAUDE.md requires
  `just test` for any `build_tasks.rs` / template change; accept snapshots with
  `cargo insta test --workspace --accept`.

## Phase 6 — vendored slang and first consumers

- `DescriptorHandle` is core-module, so no new vendored module is strictly needed.
  Consider a `mltrs::TexHandle` typealias in `crates/cli/vendor/mltrs/` for
  symmetry with `addr.slang`; if added, re-seed with `just vendor-shaders`.
- Convert `depth_texture` first — one texture, one param block, minimal proof
  (examples/depth_texture/shaders/source/depth_texture.shader.slang).
- Then `toon_link` for the actual payoff: `build_material_pipelines`
  (examples/toon_link/src/main.rs:780-826) collapses to one pipeline plus a
  `Material` buffer behind `ImmutableAddr`. Per-material pipelines and per-material
  uniform buffers both disappear.

  **This does not need `NonUniformEXT`.** toon_link issues one index-range draw per
  batch (main.rs:1178-1186) and keeps doing so; the material index is uniform within
  each draw. The win here is ~24 fewer pipelines and ~24 fewer uniform buffers, not
  fewer draws.

  Merging those draws is the follow-on
  ([render-graph/05_multi_draw_rendering.md](render-graph/05_multi_draw_rendering.md)),
  and even that likely stays uniform if the material index comes from `gl_DrawID`.
  The decoration is only needed if the index varies *within* one draw — packed into
  vertex or instance data. That's the point at which the `getDescriptorFromHandle`
  non-goal has to be revisited, together with the
  `shader_sampled_image_array_non_uniform_indexing` bit Phase 2 omits — the two
  must land together (the decoration without the feature is a validation error).
- `just shaders <name>` after each; `just test`; `just sweep`;
  `just toon_link link-verify-p1`.

## Phase 7 — docs

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
  needed; see Phase 6). If the `mltrs::TexHandle` alias from Phase 6 is added,
  repeat the rule in a comment there — it's what shader authors actually read.

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
| 1 | `just test` + a new rejection test |
| 2 | `cargo check --workspace --all-targets`, `just lint`, `just sweep` |
| 3 | `just sweep` — validation clean with the heap allocated but unbound |
| 4 | `just sweep` + `just watch <example>`; hot reload still works; `just test` (reflection JSON snapshots) |
| 5 | `just test` (snapshots), new alignment fixtures |
| 6 | `just shaders`, `just test`, `just sweep`, `just toon_link link-verify-p1` |

Per [`docs/testing.md`](../docs/testing.md), read before accepting any snapshot or
adding a validation check. Layout bugs behind device addresses and heap indices
produce no validation errors, which is why the generated `offset_of!`/`size_of`
assertions (build_tasks.rs:1189) matter more here than usual.

## References

- [DescriptorHandle&lt;T&gt; core module reference](https://docs.shader-slang.org/en/stable/external/core-module-reference/types/descriptorhandle-0a/index.html)
- [Slang user guide: convenience features](http://shader-slang.org/slang/user-guide/convenience-features) — default SPIR-V lowering, `-bindless-space-index`, `getDescriptorFromHandle` override
- [slang#8610](https://github.com/shader-slang/slang/discussions/8610) — how the heap indirection works
- [slang#8063](https://github.com/shader-slang/slang/issues/8063) — same-set bindings vs. `VARIABLE_DESCRIPTOR_COUNT`
- [MoltenVK#2278](https://github.com/KhronosGroup/MoltenVK/issues/2278) — descriptor indexing + variable count fault
