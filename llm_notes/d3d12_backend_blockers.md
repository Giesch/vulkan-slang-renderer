# D3D12 backend: blockers and differences

Written 2026-08-29, after the viewport Y-flip change. A survey of what a
D3D12 backend behind the same `Game` / `FrameRenderer` API must solve. It
is a snapshot; verify each item against the code before acting on it.
`vulkan_types_correspondence.md` covers the object-by-object mapping of
`vk::*` types and is not repeated here.

Ordered by cost, highest first.

## 1. Buffer device addresses (blocker)

`mltrs::Addr<T>`, `ReadAddr<T>` and `ImmutableAddr<T>`
(`crates/cli/vendor/mltrs.slang:21-39`) are Slang `Ptr<T, ...,
AddressSpace.Device, Std430DataLayout>`. Slang lowers them to SPIR-V
`PhysicalStorageBuffer` pointers. The DXIL target has no pointer type, so a
shader that reads through one does not compile for D3D12.

12 of 16 examples read their buffer data this way, and `docs/bindless.md`
names it the preferred path.

No D3D12 feature is planned to close this gap. `hlsl-specs` issue #17
("Loading from Buffer Addresses") is in the Shader Model Backlog with no
owner; DXIL uses a 32-bit pointer model. Shader Model 6.9 (2026) shipped
without it. Slang documents pointer support for SPIR-V, C++ and CUDA only.

### How the renderer uses `Addr<T>` today

- Every shader use is a flat array handle in a parameter block:
  `mltrs::ReadAddr<Sphere> spheres;` then `spheres[i]`. There is no pointer
  arithmetic and no pointer to a single field.
- One level of nesting: `DrawSlot { mltrs::ImmutableAddr<Material> material; }`
  (`examples/toon_link/shaders/source/toon_link.shader.slang:22-24`), where a
  buffer element points at another buffer.
- CPU-side, an address is a whole buffer
  (`crates/renderer/src/renderer/storage_buffer.rs`, `addr()`) or a whole
  buffer plus an element offset (`get_element_device_address_for_frame_immutable`)
  for per-frame ring slices.
- Reflection requires a pointee to be a struct (`PointerStructField`,
  `crates/slang-reflection/src/json/parameters.rs`).

### Option A: `DescriptorHandle<StructuredBuffer<T>>` (recommended)

Slang's `DescriptorHandle<T>` accepts `StructuredBuffer`, `RWStructuredBuffer`,
`ByteAddressBuffer` and `ConstantBuffer<T>`, lowers to a `uint2` of plain
data, and is supported on SPIR-V and HLSL SM 6.6 (`ResourceDescriptorHeap`).
The renderer already runs this machinery for textures
(`bindless_textures.md`, `crates/renderer/src/renderer/descriptor_heap.rs`).

`Addr<T>` becomes a struct: `{ StructuredBuffer<T>.Handle buffer; uint base; }`
with `operator[]` reading `buffer[base + i]`. `ReadAddr` / `ImmutableAddr`
map to `StructuredBuffer`; `Addr` maps to `RWStructuredBuffer`. Shader call
sites (`spheres[i]`) do not change. The nested `DrawSlot -> Material` case
works because the handle is plain data inside a buffer element.

Changes:

- The Vulkan heap gains a `STORAGE_BUFFER` array binding beside the image
  bindings; every storage buffer joins the heap at creation, as textures do.
- Per-frame ring slices become `(handle, base_index)` instead of
  `address + byte_offset`; `element_byte_offset` becomes an element index.
- Rust `Addr<T>` (`crates/renderer/src/renderer/addr.rs`) grows from 8 bytes
  (8-aligned) to 12 bytes (4-aligned); codegen and `alignment_tests` move.
- Reflection validates the handle field the way it validates texture handles,
  and stops reading SPIR-V pointer layouts.

Costs:

- Handles are assumed dynamically uniform. Any per-fragment or per-particle
  choice of buffer needs `nonuniform()`; a missed one is a silent wrong read
  on some hardware. `toon_link`'s `drawSlots[SV_DrawIndex].material` is
  uniform per draw.
- A heap slot must stay valid while a frame is in flight, so buffers get the
  deferred-destroy path textures use.
- `ImmutableAddr`'s SPIR-V `Restrict` hint has no descriptor equivalent.
- GPU-computed raw addresses (`bda_footguns.md`) become impossible. No example
  does this.
- The `uint2` layout and heap-index semantics are measured on SPIR-V only;
  measure them on DXIL before trusting generated Rust structs.

Two ways to adopt it:

- Handles on both targets, one `mltrs.slang`, one Rust `Addr` layout. This
  drops BDA on Vulkan. Recommended if D3D12 is a real goal.
- BDA on Vulkan, handles on DXIL, selected by a Slang capability or a
  target-specific `#ifdef`. Two `Addr` layouts on the Rust side, two
  reflection paths. Keeps `Restrict` and raw addresses on Vulkan.

### Option B: `StructuredBuffer<T>` bound through descriptor sets

Every buffer becomes a bound `StructuredBuffer<T>` in the parameter block, and
reflection stops rejecting them. Portable, but each distinct buffer costs a
descriptor set write per draw, and nested buffer references (`DrawSlot ->
Material`) are not expressible. Not recommended.

Decide this before writing more examples. Each new `Addr<T>` shader is
one more rewrite later.

## 2. Compile target and artifact layout

`crates/slang-reflection/src/lib.rs:148,249,332` compile
`CompileTarget::Spirv` only. `mltrs shaders compile` emits `*.spv` +
`*.json` into `shaders/compiled/`, and `src/generated/` bindings that
assume SPIR-V. Needed:

- A second target (`CompileTarget::Dxil`, or `Hlsl` for offline `dxc`).
- Per-target artifact names, and a `--target` flag or a both-by-default
  rule.
- Reflection is target-independent in Slang, so `*.json` can stay shared
  if the binding model (item 4) is shared.
- `SpvBytes` in `crates/renderer/src/shaders.rs` gets a `DxilBytes`
  sibling. `ToVk` / `VkCreate` get `ToDx` siblings. The split between
  `mltrs-slang-reflection` and the renderer is already the right seam.

## 3. Coordinate conventions (resolved)

After the viewport flip change:

- CPU projections: `glam::camera::{rh,lh}::proj::directx`.
- Clip space Y-up, depth `[0, 1]`, texture space Y-down.
- Front faces counter-clockwise in Y-up space.
- The only flip is `Renderer::flipped_viewport` (negative height).

D3D12 uses exactly these conventions. The D3D12 backend uses a
positive-height viewport and sets `FrontCounterClockwise = TRUE`.
`mltrs.slang`, `docs/coordinates.md`, and every example shader are shared
unchanged.

Do not reintroduce a flip into shared shader code.

## 4. Binding model

Reflection (`shaders/compiled/*.json`, `pipelineLayout`) records:

- `descriptorSetLayouts`: one set per `ParameterBlock<T>`, holding a
  `constantBuffer` (every example) and any bound textures / storage
  buffers.
- `pushConstantRanges`: 3 examples use them, sizes 8 to 24 bytes.
- `bindlessHeapSet`: the global texture heap.

D3D12 mapping:

- Descriptor set → root signature descriptor table, or a root CBV for a
  set that is only one constant buffer (most examples).
- Push constants → root constants. The root signature budget is 64
  DWORDs total (256 bytes) shared with root descriptors. Current usage
  is far below this; keep the reflection-side check that a push block
  fits.
- Bindless heap → `ResourceDescriptorHeap` (SM6.6). Shaders already use
  Slang's portable `Sampler2D.Handle` / `RWTexture2D.Handle`
  (`docs/bindless.md`), which Slang lowers to either target. No shader
  change.

`ReflectionLayoutBindings` (`crates/renderer/src/shaders.rs`) is the
Vulkan-specific reader of this data; the D3D12 backend needs a root
signature builder that reads the same JSON.

## 5. Front face is hardcoded

`crates/renderer/src/renderer.rs` sets `vk::FrontFace::COUNTER_CLOCKWISE`
in the pipeline builder; `RasterState` (`renderer/pipeline.rs`) exposes
only `CullMode`. That is correct for the convention in item 3, but the
convention lives in one backend. Move "front faces are CCW" into the
backend-neutral `RasterState` contract so each backend derives its
enum from the same definition.

## 6. Matrix layout

`extern static const bool columnMajor` (`mltrs.slang:14`) is injected at
compile time from `MATRIX_LAYOUT` in `crates/slang-reflection/src/lib.rs`.
It is target-independent, and DXIL compilation must inject the same value.
No design change; a test that both targets receive the same specialization
constant is enough.

## 7. Std430 layout

`Addr<T>` pins `Std430DataLayout`. The Rust side (`crates/renderer/src/renderer/addr.rs`, the generated
bindings, `alignment_tests`) assumes std430 for buffers and std140 for
constant buffers. D3D12 structured buffers are tightly packed with 4-byte
alignment, and HLSL constant buffers use 16-byte packing rules that
differ from std140 in a few cases (arrays of scalars, `float3` followed
by `float`). Slang applies the target's layout, so the generated Rust
`#[repr(C)]` structs may need per-target offsets. The `alignment_tests`
fixtures (`crates/cli/fixtures/`) are the place to catch this; run them
against DXIL reflection before trusting any generated struct.

## 8. Renderer internals (plumbing, not design)

All of these are backend-private already:

- `ash`, `vk-mem`, SDL3 Vulkan surface, swapchain recreation
  (`render_finished_recreate.md`).
- Dynamic rendering, image layout transitions, pipeline barriers.
- Validation-layer counting in `renderer/debug.rs` and the exit-code
  contract `scripts/headless-sweep.sh` depends on. D3D12 needs the
  debug layer + `ID3D12InfoQueue` wired to the same counter, or the
  sweep has no D3D12 verdict.
- `just sweep` runs on lavapipe. D3D12 has WARP, but only on Windows.
  A Linux-hosted CI cannot sweep the D3D12 backend.

## 9. Things that do not change

- `Game`, `FrameRenderer`, `PipelineHandle`, the asset helpers in
  `crates/mltrs`.
- `mltrs.slang` and every example `.shader.slang`, given item 1 is solved
  at the Slang level.
- `docs/coordinates.md`, `docs/textures.md` (ktx2/bc7 are API-neutral).
- The convert-link pipeline and the `gx` crate.

## Summary

Item 1 is the only true blocker and the only one that grows with each new
example. Items 2 and 4 are large but mechanical. Item 3 is done. The rest
are one-file changes or CI concerns.
