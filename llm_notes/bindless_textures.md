# Bindless Textures via Slang `DescriptorHandle`

**Status: planned, not started.** Design note for adopting bindless texture access
using Slang's `DescriptorHandle<T>` with its default SPIR-V lowering. Supersedes the
"not planned" note in [render-graph/03_bindless.md](render-graph/03_bindless.md),
which stays useful as background on descriptor indexing and Metal argument buffers.
For how this relates to the BDA work, see
[vulkan_1_3_migration/bindless_vs_bda_terminology.md](vulkan_1_3_migration/bindless_vs_bda_terminology.md).

## Why

A texture is currently welded to a pipeline. `create_descriptor_sets`
(renderer.rs:3904) runs exactly once, at pipeline creation, from a positionally
ordered `&[&Texture]`; changing a texture means a new pipeline. `toon_link` pays
this in full — `build_material_pipelines`
(examples/toon_link/src/main.rs:778-825) creates one pipeline *and* one uniform
buffer per material, sharing only the mesh.

`DescriptorHandle<T>` lowers to a `uint2` of **ordinary data**, which is the same
shape the renderer already committed to for buffers: BDA pointers in a param block,
with `StructuredBuffer` descriptors actively rejected
(reflection/parameters.rs:299, pipeline_layout.rs:329). A texture handle can
Obviouslytherefore live inside a std430 struct behind an `ImmutableAddr<T>`:

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

That collapses toon_link to one pipeline plus a material buffer, and is the
prerequisite for the batching sketched in
[render-graph/05_multi_draw_rendering.md](render-graph/05_multi_draw_rendering.md).

## Why this option and not the others

- **Raw descriptor arrays** (`Texture2D textures[]` + `NonUniformResourceIndex`)
  would require hand-written `[[vk::binding]]` annotations, which breaks the
  positional-binding assumption stated twice in pipeline_layout.rs:192,244.
- **Overriding `getDescriptorFromHandle`** is an escape hatch, not a starting
  point. It stays available later *without touching shader source*, so deferring
  it costs nothing. Reach for it only if we want a single mutable-type heap
  (`BindlessDescriptorOptions.VkMutable`, needs `VK_EXT_mutable_descriptor_type`)
  or a layout that doesn't match Slang's default.
- **`VK_EXT_descriptor_heap`** (Slang's `spvDescriptorHeapEXT` capability) shipped
  with Vulkan 1.4.340 in Jan 2026; NVIDIA and AMD have drivers, Intel ANV is
  experimental, MoltenVK has nothing. Off the table for a 1.3 baseline. It is the
  *same source-level feature*, so today's code becomes descriptor-heap code via a
  compile flag whenever we want it.

## What's already in place

- Vulkan 1.3 floor with `bufferDeviceAddress` (renderer.rs:3273-3286); descriptor
  indexing is core 1.2, so it costs no new extension.
- Every texture already carries its own view + sampler (`Texture`,
  renderer/texture.rs:55-65) and lives in an append-only slab
  (`TextureStorage`, texture.rs:11-43) — the natural backing for heap slots.
- `addr.rs` is the exact template for the new handle type: `#[repr]` newtype,
  `PhantomData<fn() -> T>`, `const _: () = assert!(size_of == 8)`,
  `pub(super)` constructor so only the renderer can mint one.
- `CompilerOptionName::BindlessSpaceIndex = 93` already exists in the vendored
  bindings (`slang-sys/src/bindings.rs:586`), as do
  `spReflection_getBindlessSpaceIndex` (:2816) and
  `slang_IBindlessResourceMetadata` (:1041).
- Retire-after-N-frames precedent for resources still referenced by in-flight
  command buffers: `old_pipelines` (renderer.rs:115-120, freed at :2456-2486).

## Non-goals

- `VK_EXT_descriptor_heap` / `spvDescriptorHeapEXT`.
- Overriding `getDescriptorFromHandle`.
- `VK_DESCRIPTOR_BINDING_VARIABLE_DESCRIPTOR_COUNT_BIT` — see Phase 2.
- Storage images (`RWTexture2D`, watercolor) stay on per-pipeline descriptors.
- egui keeps its own descriptors (`renderer/egui.rs`, third-party renderer).
- Uniform buffers stay descriptor-bound; something has to carry the handles.

---

## Phase 0 — spike, then fill in this section

Throwaway work. **Everything downstream depends on the answers, so record them
here before writing Phase 1 code.**

1. Add `option!(BindlessSpaceIndex, bindless_space_index(index: i32));` to the
   slang-rs fork's `CompilerOptions` impl (`src/lib.rs:~800`, alongside
   `emit_spirv_directly`). The option name enum is already bound; only the
   high-level builder is missing it. Tag a new version, bump the git dep in the
   root `Cargo.toml`.
2. Write a scratch shader putting `Sampler2D.Handle` in a `ParameterBlock`,
   compile it through `prepare_reflected_shader_with_optimization`
   (shaders.rs:62), and `spirv-dis` the result.
3. Answer, in writing:
   - **What does `slang::reflection` report for the handle field?** TypeKind,
     `full_name()`, and whether it lands in the `Uniform` parameter category with
     a `Uniform{offset,size}` binding. This drives every codegen decision in
     Phase 4. Also check what it reports for a handle nested inside a
     `Ptr<..., Std430DataLayout>` pointee.
   - **What global arrays does Slang emit?** Set index, binding indices,
     descriptor types, and whether they're `OpTypeRuntimeArray` (unbounded) or
     sized. Docs say the default binding layout is `0` sampler,
     `1` combined texture sampler, `2` texture/texel/buffer, `3` unknown — confirm.
   - **Does it emit `NonUniformEXT` automatically**, or do we need
     `NonUniformResourceIndex` at the use site?
   - **Does `spReflection_getBindlessSpaceIndex` return the space we asked for**,
     and does `IBindlessResourceMetadata::usesBindlessResourceHeap()` correctly
     report false when no handle is used? (It matters: a shader with no handles
     must not get a heap set bound.)
4. Pick `BINDLESS_SPACE_INDEX`. It must not collide with sets Slang assigns to
   nested `ParameterBlock`s, and `reserve_slot` (pipeline_layout.rs:155) shows
   those are dense from 0. Something like `4` with a runtime assert against
   reflection.

## Phase 1 — device features (behaviorally invisible; land alone)

`create_logical_device` (renderer.rs:3273) currently requests zero
descriptor-indexing bits. Add to `vulkan_12_features`:

```rust
.descriptor_indexing(true)
.runtime_descriptor_array(true)
.descriptor_binding_partially_bound(true)
.shader_sampled_image_array_non_uniform_indexing(true)
.descriptor_binding_sampled_image_update_after_bind(true)
.descriptor_binding_update_unused_while_pending(true)
```

Mirror every bit in the physical-device gate (renderer.rs:3001-3037) and extend
the bail message at renderer.rs:3055-3060. `runtime_descriptor_array` is only
needed if Phase 0 shows Slang emits `OpTypeRuntimeArray`.

**Verify:** `just sweep` stays clean. Lavapipe supports descriptor indexing, so CI
covers this.

## Phase 2 — the heap (renderer-owned; still no shader uses it)

New module `crates/renderer/src/renderer/descriptor_heap.rs`.

- **Fixed-size arrays, not variable count.** Only the *last* binding in a set may
  carry `VARIABLE_DESCRIPTOR_COUNT`, and Slang puts several arrays in the one
  bindless set — that collision is [slang#8063](https://github.com/shader-slang/slang/issues/8063).
  Use a fixed `MAX_BINDLESS_TEXTURES` (start ~4096) plus
  `DescriptorBindingFlags::PARTIALLY_BOUND`. It also dodges
  [MoltenVK#2278](https://github.com/KhronosGroup/MoltenVK/issues/2278).
- Bindings must match whatever Phase 0 observed. Create only the ones we use —
  combined image sampler to start, since every example already uses `Sampler2D`.
- Its own pool with `DescriptorPoolCreateFlags::UPDATE_AFTER_BIND_POOL` and the
  matching `DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL`.
- **One set, not `MAX_FRAMES_IN_FLIGHT` copies.** Update-after-bind is exactly
  what removes the need to duplicate.
- `insert_texture(&Texture) -> BindlessIndex` writes one descriptor. Slot release
  on `destroy_texture` must be **deferred `MAX_FRAMES_IN_FLIGHT` frames** before
  reuse — writing a never-referenced slot is safe under
  `PARTIALLY_BOUND` + `UPDATE_UNUSED_WHILE_PENDING`, but overwriting one an
  in-flight command buffer still references is not. Reuse the retirement pattern
  at renderer.rs:2456-2486.

Then:

- Store the slot on `Texture` (texture.rs:55-65); allocate it in
  `create_texture_with_options` (renderer.rs:514) and `create_texture_with_mips`
  (:544) so *every* texture gets one. Release in the destroy path and `take_all`
  (texture.rs:37).
- Bind the heap set once per command buffer, before the per-draw loop, at
  `first_set = BINDLESS_SPACE_INDEX`.

**Verify:** `just sweep` clean with validation on. Nothing samples through the heap
yet, so any error here is a pure layout/lifetime bug.

## Phase 3 — pipeline layouts must include the heap set

This is the fiddly phase; three places assume "the pipeline owns all its sets."

- `reflect_pipeline_layout` (pipeline_layout.rs:11-27) must **skip** the bindless
  space so it isn't allocated per-pipeline. Drop that index from
  `descriptor_set_layouts` based on `spReflection_getBindlessSpaceIndex`.
- `ReflectedPipelineLayout::vk_create` (renderer.rs:5187-5222) must append the heap
  set layout at `BINDLESS_SPACE_INDEX`, with empty placeholder layouts for any gap
  indices — `vkCreatePipelineLayout` takes a dense array.
- `descriptor_sets_for_frame` (renderer.rs:2143-2155) chunks
  `pipeline.descriptor_sets` by `layout.descriptor_set_layouts.len()`. That length
  now includes the heap layout, but the heap set is neither per-frame nor
  per-pipeline. Keep the chunk width tied to the *reflected* sets only, and bind
  the heap separately. Same for `picking_descriptor_sets_for_frame` (:2157).
- `create_descriptor_pool` / `descriptor_pool_sizes` (renderer.rs:3806-3871) size
  the per-pipeline pool from `descriptor_set_layouts`. If the heap layout leaks in,
  every pipeline tries to allocate thousands of descriptors. Add an assertion, not
  just a filter.
- `assert_shader_interface_unchanged` (renderer.rs:5003) gates hot reload on the
  reflected interface; confirm the skipped bindless set doesn't make two
  equivalent shaders compare unequal.

## Phase 4 — reflection and codegen for handle fields

The `Resources` struct is *not* where handles go — that's the whole point. A handle
is a uniform/std430 field the app writes, exactly like an `Addr<T>`.

- `crates/renderer/src/shaders/json/parameters.rs`: add
  `StructField::DescriptorHandle(DescriptorHandleStructField { field_name, resource_shape })`
  to the enum at :82.
- `crates/renderer/src/shaders/reflection/parameters.rs`: recognize the handle
  type next to the pointer case (:353-427), parsing `full_name()` the same way
  pointer access modes are parsed today. Reject unsupported shapes with a
  field-specific message in the style of the StructuredBuffer error at :299.
- New `crates/renderer/src/renderer/bindless.rs`, modeled on `addr.rs`:
  `BindlessHandle<T>` — 8 bytes (`uint2`), `PhantomData<fn() -> T>`,
  `const _: () = assert!(size_of::<BindlessHandle<T>>() == 8)`, `Serialize`,
  `pub(super)` constructor. Minted from a `TextureHandle` by an accessor mirroring
  `Gpu::addr` (renderer.rs:5367).
- `crates/cli/src/build_tasks.rs`:
  - `gather_struct_defs` (:881) emits the field as
    `BindlessHandle<Marker>`; add it to the size/alignment tables that already
    special-case the 8-byte `Addr` types (:1339-1342, :1378-1381).
  - `required_resource` (:1070) must return `None` for handle fields.
- Add alignment fixtures under `crates/cli/fixtures/alignment/` — handle alone,
  handle next to an `Addr`, handle inside a pointee struct. CLAUDE.md requires
  `just test` for any `build_tasks.rs` / template change; accept snapshots with
  `cargo insta test --workspace --accept`.

## Phase 5 — vendored slang and first consumers

- `DescriptorHandle` is core-module, so no new vendored module is strictly needed.
  Consider a `mltrs::TexHandle` typealias in `crates/cli/vendor/mltrs/` for
  symmetry with `addr.slang`; if added, re-seed with `just vendor-shaders`.
- Convert `depth_texture` first — one texture, one param block, minimal proof
  (examples/depth_texture/shaders/source/depth_texture.shader.slang).
- Then `toon_link` for the actual payoff: `build_material_pipelines`
  (examples/toon_link/src/main.rs:778-825) collapses to one pipeline plus a
  `Material` buffer behind `ImmutableAddr`. This is where per-material pipelines
  and per-material uniform buffers both disappear.
- `just shaders <name>` after each; `just test`; `just sweep`;
  `just toon_link link-verify-p1`.

## Phase 6 — docs

- Rewrite the status header on [render-graph/03_bindless.md](render-graph/03_bindless.md);
  it currently says texture binding remains per-pipeline descriptors.
- Update `docs/` for the shader-authoring workflow (handles are data, not
  `Resources` entries).

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
  binding lights up instead of two.

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
| 0 | scratch compile + `spirv-dis`; answers written into this doc |
| 1 | `cargo check --workspace --all-targets`, `just lint`, `just sweep` |
| 2 | `just sweep` — validation clean with the heap allocated but unused |
| 3 | `just sweep` + `just watch <example>`; hot reload still works |
| 4 | `just test` (snapshots), new alignment fixtures |
| 5 | `just shaders`, `just test`, `just sweep`, `just toon_link link-verify-p1` |

Per [`docs/testing.md`](../docs/testing.md), read before accepting any snapshot or
adding a validation check. Layout bugs behind device addresses and heap indices
produce no validation errors, which is why the generated `offset_of!`/`size_of`
assertions (build_tasks.rs:1180) matter more here than usual.

## References

- [DescriptorHandle&lt;T&gt; core module reference](https://docs.shader-slang.org/en/stable/external/core-module-reference/types/descriptorhandle-0a/index.html)
- [Slang user guide: convenience features](http://shader-slang.org/slang/user-guide/convenience-features) — default SPIR-V lowering, `-bindless-space-index`, `getDescriptorFromHandle` override
- [slang#8610](https://github.com/shader-slang/slang/discussions/8610) — how the heap indirection works
- [slang#8063](https://github.com/shader-slang/slang/issues/8063) — same-set bindings vs. `VARIABLE_DESCRIPTOR_COUNT`
- [MoltenVK#2278](https://github.com/KhronosGroup/MoltenVK/issues/2278) — descriptor indexing + variable count fault
