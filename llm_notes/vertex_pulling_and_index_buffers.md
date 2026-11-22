# Vertex pulling, unified graphics pipelines, and full-parity Roc render graph

Date: 2026-11-18. Status: approved architectural plan for a fresh PR stack
replacing PRs `#37`–`#44`.

## Overview and core decisions

This plan replaces both Phase 2 of
[`vertex_input_and_mesh_binding.md`](vertex_input_and_mesh_binding.md) and the
exploratory Roc PR stack (`#37`–`#44`) with a single unified architecture across
Rust and Roc:

1. **Vertex pulling with hardware index buffers is the default and only vertex
   input model.**
   - Fixed-function Vulkan vertex input (`VkPipelineVertexInputStateCreateInfo`,
     `VertexDescription`, `MeshHandle<V>`, `TypedVertex<V>`, `RuntimeVertex`,
     `NoVertex`, and `mltrs_slang_reflection::json::vertex_layout`) is removed.
   - Shaders read vertex data through buffer device addresses (`mltrs::ImmutableAddr<V>`,
     `mltrs::ReadAddr<V>`, or `mltrs::Addr<V>`) indexed by `uint vertexId : SV_VertexID`.
   - Indexed draws bind an untyped hardware `IndexBufferHandle` /
     `IndexBufferSlot` (`[u32]`) via `vkCmdBindIndexBuffer`, preserving the GPU
     post-transform vertex reuse cache and `vkCmdDrawIndexed` /
     `vkCmdDrawIndexedIndirect`.
2. **Pipeline kinds are determined directly by shader semantics (`Graphics`,
   `GraphicsIndirect`, `Compute`).**
   - Because `VkPipelineVertexInputStateCreateInfo` is always empty, direct
     vertex-count (`vkCmdDraw`) and direct indexed (`vkCmdDrawIndexed`) draws
     use the exact same `VkPipeline` and vertex entry-point signature
     (`uint vertexId : SV_VertexID`). `DrawVertexCount` and `DrawIndexed` unify
     into a single `Graphics` pipeline marker (`PipelineHandle<Graphics, P>`).
   - Shaders whose vertex entry point declares `SV_DrawIndex` (reflected in
     Slang JSON as `"semanticName": "SV_DRAWINDEX"`) automatically generate
     `PipelineConfig<'a, GraphicsIndirect, P>` ->
     `PipelineHandle<GraphicsIndirect, P>`, eliminating the unverified manual
     `.indirect()` toggle.
   - Compute shaders continue to generate `ComputePipelineConfig<'a, P>` ->
     `PipelineHandle<Compute, P>`.
3. **Fresh PR stack leading to a full-parity Roc Render Graph API.**
   - Rather than preserving the temporary `IndexedPipeline` + inline `Mesh` API
     from PRs `#37`–`#44`, we will build a clean stack from `main` (reusing
     valuable pieces from `#37`–`#44` such as `RuntimeShader`, `NoAtlas`, Roc
     module/type codegen, `GraphSet` lifecycle, and `roc_platform_abi` glue).
   - The Roc API mirrors the Rust `mltrs-render-graph` API:
     - Generated Roc shader modules split parameter blocks and push blocks into
       `*Data` and `*Bindings` types, matching Rust's `GraphShaderParams` split.
     - Resources (`uniform_buffer`, `singleton_buffer`, `immutable_buffer`,
       `cpu_updated_buffer`, `gpu_updated_buffer`, `index_buffer`, logical
       `texture`s, and pipelines) are explicitly declared and passed as typed
       handles/slots, matching Rust.
     - Logical render-graph lowering and hazard validation are implemented in
       Roc (`RenderGraphDesc.roc`, `RenderGraphLower.roc`,
       `RenderGraphValidate.roc`) as well as Rust, supporting the long-term goal
       of moving render graph construction completely to Roc.
     - `Data` and `Bindings` remain cleanly separated in byte assembly
       (`AssemblyProgram` in `crates/render-graph/src/runtime/compile.rs`),
       rather than coupling `Data` packers to zeroed binding holes (tracked for
       future profiling in [`tech_debt.md` §23](tech_debt.md#23-profile-zeroed-holes-packing-vs-step-by-step-assemblyprogram-splicing-for-split-uniformpush-blocks)).
     - Initial milestone at the stack tip: a working Roc `basic-triangle`
       example using vertex pulling + hardware index buffers, on an API surface
       designed to port all 17 Rust examples to Roc afterward.
4. **Bounds validation and `Std430` layout edge cases.**
   - In the Rust API, index-buffer creation validates in dev builds
     (`cfg!(debug_assertions)`) that all indices are `< vertex_count` (when a
     vertex bound is supplied, e.g., `create_index_buffer(&indices, vertex_count)`),
     skipping the $O(N)$ scan in release builds.
   - Runtime dev-validation control for prebuilt hosts (`roc-platform`) is
     tracked in [`tech_debt.md` §24](tech_debt.md#24-runtime-flag-for-dev-time-bounds-and-resource-validation-in-prebuilt-hosts),
     complemented by setup-time checks in Roc validation.
   - Dedicated tests in both Rust and Roc cover `Std430DataLayout` pointer
     pointees (`ImmutableAddr<Vertex>`) with tricky alignment/padding shapes,
     specifically structs containing `float3` fields (e.g., `float3 position; float3 color;`,
     `float3 + float`, `float3 + float4`).

---

## Detailed technical design

### 1. Slang shader conventions and reflection

#### Vertex pulling shader shape
Every graphics shader has zero varying struct inputs on its vertex entry point.
Vertex entry points only declare system-value scalar parameters:
- Direct graphics shaders (`Graphics`): `uint vertexId : SV_VertexID` (or no
  parameters if constant, though `SV_VertexID` is standard).
- Indirect graphics shaders (`GraphicsIndirect`): `uint vertexId : SV_VertexID`
  and `uint drawIndex : SV_DrawIndex`.

Example (`basic_triangle.shader.slang`):
```slang
#language slang 2026
module basic_triangle;

import mltrs;

struct Vertex {
    float3 position;
    float3 color;
}

struct TriangleParams {
    mltrs::MVPMatrices mvp;
    mltrs::ImmutableAddr<Vertex> vertices;
}

ParameterBlock<TriangleParams> params;

struct FragVertex {
    float4 position : SV_Position;
    float3 color;
}

[shader("vertex")]
FragVertex vertexMain(uint vertexId : SV_VertexID) {
    let v = params.vertices[vertexId];
    let pos = params.mvp.project(v.position);

    return FragVertex(pos, v.color);
}

[shader("fragment")]
float4 fragmentMain(FragVertex input) : SV_Target {
    return float4(input.color, 1.0);
}
```

#### Reflection rules (`crates/slang-reflection` and `crates/cli`)
- **Reject fixed-function vertex structs**: If `vertex_entry_point.parameters`
  contains an `EntryPointParameter::Struct`, reflection / codegen fails with a
  clear diagnostic instructing the author to accept `uint vertexId : SV_VertexID`
  and pull vertex structs through `mltrs::ImmutableAddr<T>`, `ReadAddr<T>`, or
  `Addr<T>`. (Note: this also eliminates `tech_debt.md` §21 and §22 for vertex
  input structs, and allows deleting `crates/slang-reflection/src/json/vertex_layout.rs`.)
- **Infer `Graphics` vs `GraphicsIndirect` from `SV_DRAWINDEX`**:
  Inspect `vertex_entry_point.parameters` for a `ScalarEntryPointParameter::Semantic`
  whose `semantic_name.eq_ignore_ascii_case("SV_DRAWINDEX")`:
  - Absent -> `Graphics`
  - Present -> `GraphicsIndirect`
- **`Std430` layout edge cases (`float3` pointees)**:
  `mltrs::ImmutableAddr<Vertex>` expands to
  `Ptr<Vertex, Access.Immutable, AddressSpace.Device, Std430DataLayout>`. Slang
  reflects `StructField::Pointer(PointerStructField { pointee_type, pointee_size, access })`
  with exact per-field byte offsets and `pointee_size`.
  - Verify with dedicated unit/snapshot/execution tests in both `crates/cli`
    (Rust codegen) and `crates/cli/src/roc_codegen.rs` (Roc codegen) that:
    1. `struct { float3 a; float3 b; }` (`a` at 0, `b` at 16, `pointee_size` 32
       under standard std430, or whatever exact offsets Slang emits in SPIR-V)
       generates matching Rust `#[repr(C)]` padding + `offset_of!` / `size_of`
       assertions and matching Roc `gpu_size` + `to_bytes` offsets.
    2. `struct { float3 a; float s; }` (scalar packing into the tail of `float3`
       at offset 12, total size 16) matches across SPIR-V, Rust struct layout,
       and Roc `to_bytes` packing!

---

### 2. Rust renderer & render-graph API

#### Pipeline markers & handles (`crates/renderer/src/renderer/pipeline.rs`)
- Replace `DrawVertexCount`, `DrawIndexed`, `DrawIndexedIndirect` (and
  `GraphicsPipelineHandle` + `TypedVertex` + `RuntimeVertex` + `NoVertex`) with:
  - `pub struct Graphics;` (`impl DrawCall for Graphics { type Index = GraphicsPipelineIndex; }`)
  - `pub struct GraphicsIndirect;` (`impl DrawCall for GraphicsIndirect { type Index = GraphicsPipelineIndex; }`)
  - `pub struct Compute;` (`impl DrawCall for Compute { type Index = ComputePipelineIndex; }`)
- Single handle type: `pub struct PipelineHandle<K, P = NoPush>`.
- `PipelineConfig<'t, K: DrawCall, P = NoPush>`:
  - `PipelineConfigBuilder::build_graphics<P>(self) -> PipelineConfig<'t, Graphics, P>`
  - `PipelineConfigBuilder::build_graphics_indirect<P>(self) -> PipelineConfig<'t, GraphicsIndirect, P>`
  - Both validate in `init_pipeline` (before GPU allocation) that `config.shader`
    matches the expected `SV_DRAWINDEX` usage (so `RuntimeShader` +
    `PipelineConfigBuilder` cannot mismatch `Graphics` and `GraphicsIndirect`).
- `create_graphics_pipeline` passes `&[]` for vertex binding descriptions and
  `&[]` for vertex attribute descriptions unconditionally.

#### Index buffers (`crates/renderer` + `crates/render-graph`)
- In `crates/renderer`:
  - `pub struct IndexBufferHandle { index: IndexBufferIndex, index_count: u32 }`
  - `pub fn create_index_buffer(&mut self, indices: &[u32], vertex_count: u32) -> anyhow::Result<IndexBufferHandle>`
    - Always rejects `indices.is_empty()` or `vertex_count == 0` before GPU
      allocation.
    - Under `#[cfg(debug_assertions)]`, scans `indices` and bails if any
      `idx >= vertex_count` with an actionable message naming the out-of-bounds
      index and `vertex_count`.
    - Uploads `indices` into a device-local `VK_BUFFER_USAGE_INDEX_BUFFER_BIT | VK_BUFFER_USAGE_TRANSFER_DST_BIT`
      buffer stored in `Renderer::index_buffers`, destroyed on teardown (or
      `drop_index_buffer`).
- In `crates/render-graph`:
  - `pub struct IndexBufferSlot { index: usize, index_count: u32 }` (`Copy`,
    minted via `From<&IndexBufferHandle>` or `IndexBufferSlot::from_backend(index, index_count)`).
  - Graph keys:
    - `GraphicsPipelineKey<P = NoPush>` (replaces both `DrawVertexCountKey` and
      `DrawIndexedKey`!)
    - `GraphicsIndirectPipelineKey<P = NoPush>` (replaces `DrawIndexedIndirectKey`)
    - `ComputePipelineKey<P = NoPush>`
  - Typed draw node constructors:
    - `draw_vertex_count(pipeline: impl Into<GraphicsPipelineKey<P>>, params: impl Into<UniformSlot<S>>, vertex_count: u32, bindings: S::Bindings)`
    - `draw_indexed(pipeline: impl Into<GraphicsPipelineKey<P>>, indices: impl Into<IndexBufferSlot>, params: impl Into<UniformSlot<S>>, bindings: S::Bindings)`
    - `draw_index_range(pipeline: impl Into<GraphicsPipelineKey<P>>, indices: impl Into<IndexBufferSlot>, params: impl Into<UniformSlot<S>>, first_index: u32, index_count: u32, bindings: S::Bindings)`
    - `draw_indexed_indirect(pipeline: impl Into<GraphicsIndirectPipelineKey<P>>, indices: impl Into<IndexBufferSlot>, params: impl Into<UniformSlot<S>>, args: impl Into<ImmutableSlot<I>>, first_command: u32, draw_count: u32, bindings: S::Bindings)`
  - `FrameLookup` checks `index_buffer_live(slot: usize) -> bool` and
    `index_count(slot: usize) -> u32` (replacing `whole_index_count(pipeline: usize)`).
  - `PendingDrawCommand` / `DrawCallConfig`:
    - `VertexCount(u32)`
    - `Indexed { index_buffer: usize, first_index: u32, index_count: u32 }`
      (`draw_indexed` lowers to `WholeIndexed { index_buffer }` and resolves
      `0..lookup.index_count(index_buffer)` during planning, while
      `draw_index_range` validates `range_in_bounds(first_index, index_count, lookup.index_count(index_buffer))`!)
    - `IndexedIndirect { index_buffer: usize, request: IndirectRequest }`

#### Runtime byte-buffer helpers on `Renderer` (for the Roc host)
- `create_uniform_buffer_bytes(&mut self, byte_size: u64) -> anyhow::Result<UniformBufferHandle<UniformBytes>>`
- `create_singleton_buffer_bytes(&mut self, bytes: &[u8], element_stride: u32) -> anyhow::Result<SingletonBufferHandle<BufferBytes>>`
- `create_immutable_buffer_bytes(&mut self, bytes: &[u8], element_stride: u32) -> anyhow::Result<ImmutableBufferHandle<BufferBytes>>`
- `create_cpu_updated_buffer_bytes(&mut self, len: u32, element_stride: u32) -> anyhow::Result<CpuUpdatedBufferHandle<BufferBytes>>`
- `create_gpu_updated_buffer_bytes(&mut self, len: u32, element_stride: u32) -> anyhow::Result<GpuUpdatedBufferHandle<BufferBytes>>`

---

### 3. Plain-data execution (`AssemblyProgram`) in `mltrs-render-graph`

To support full-parity Roc graphs (with split `Data` and `Bindings`, compute
dispatches, textures, storage buffers, push constants, `optional`, `repeat`, and
uploads):

- Wire up the existing `compile.rs` / `AssemblyProgram` architecture in
  `crates/render-graph/src/runtime/` (Phase 3a of `08_plain_data_graph.md`) for
  dynamic/plain-data graphs!
- Specifically, `SchemaLayout` describes a parameter/push block's fields:
  - `Data { src_offset, dst_offset, len }`: copies `len` bytes from the frame's
    packed `*Data` byte vector at `src_offset` into the GPU block at `dst_offset`.
  - `Binding { dst_offset, key }`: resolves the `ResourceRef` (`SampledTex`,
    `StorageTex`, or `Buffer` address) via `BindingResolver` and writes its
    8-byte `u64` value at `dst_offset`.
- This keeps `Data` packers cleanly separated from `Bindings` (a `Data` type has
  its own compact layout and unit-testable `to_bytes`, independent of binding
  holes).

---

### 4. Full-parity Roc Render Graph & Codegen Design

#### A. Roc Shader Codegen (`crates/cli/src/roc_codegen.rs`)
For every shader module `Generated/<Shader>.roc`:
1. **Reachable structs & enums**:
   - Emitted in their owning module (`Generated/Mltrs.roc`, shared modules, or
     `Generated/<Shader>.roc`), with `gpu_size : U32` and
     `to_bytes : T -> List(U8)` for every GPU-packable data/pointee struct
     (including `Vertex` pointees of `ImmutableAddr<Vertex>`, `Particle`, etc.).
2. **`Data` and `Bindings` split for `ParameterBlock` and `PushConstant` types**:
   - Mirror `classify_graph_field` / `graph_split_defs` from `build_tasks.rs`:
     - If a parameter/push struct `FooParams` has **no** resource/pointer fields:
       - `FooParams` is its own data type; `Bindings` is `{}`.
     - If `FooParams` has **both** data and resource/pointer fields:
       - Generate `FooParamsData` (containing only non-resource fields, with
         `gpu_size` and `to_bytes : FooParamsData -> List(U8)`), and
       - Generate `FooParamsBindings` (containing typed binding fields:
         `ImmutableBufferBinding(Vertex)`, `ReadBufferBinding(T)`,
         `BufferBinding(T)`, `SampledTexBinding`, `StorageTexBinding`).
     - If `FooParams` has **only** resource/pointer fields:
       - `Data` is `{}` (zero frame bytes); generate `FooParamsBindings`.
   - Generate the `SchemaDesc` metadata (`gpu_size`, `data_size`, `FieldLayout`s
     mapping `src_offset -> dst_offset`, and binding extraction function
     `FooParamsBindings -> List((FieldKey, GraphBinding))`) inside the shader
     module so `RenderGraph.roc` never relies on untyped string names or manual
     byte offsets!
3. **Typed shader descriptors**:
   - Graphics shader without `SV_DRAWINDEX`:
     `shader : GraphicsShader(ParamsData, ParamsBindings, PushSpec)`
   - Graphics shader with `SV_DRAWINDEX`:
     `shader : GraphicsIndirectShader(ParamsData, ParamsBindings, PushSpec)`
   - Compute shader:
     `shader : ComputeShader(ParamsData, ParamsBindings, PushSpec)`

#### B. Roc Resource & Graph API (`roc-platform/platform/RenderGraph.roc`)
Match the Rust `ResourcePlanner` + `Renderer` + `RenderGraph` API with explicit
resource allocation declarations in `ResourcePlanner` (since Roc setup is
pure declarative data):

```roc
planner0 = ResourcePlanner.empty

# Explicit buffer & resource declarations (matching Rust's explicit handles!)
(planner1, vertices) = ResourcePlanner.singleton_buffer(planner0, "vertices", BasicTriangle.Vertex.packer, [
    { position: left,  color: red },
    { position: top,   color: blue },
    { position: right, color: green },
])
(planner2, indices) = ResourcePlanner.index_buffer(planner1, "indices", [0, 1, 2], 3)
(planner3, params)  = ResourcePlanner.uniform_buffer(planner2, "params", BasicTriangle.params)
(planner4, pipe)    = ResourcePlanner.graphics_pipeline(planner3, {
    name: "basic_triangle",
    shader: BasicTriangle.shader,
    uniforms: { params },
    raster: RasterState.default,
})

node = RenderGraph.draw_indexed(
    pipe,
    indices,
    params,
    { vertices: SingletonSlot.addr(vertices) },
)

Ok(graph) =
    RenderGraph.new(planner4, node)
    |> Result.try(GraphSet.single)
```

And in `draw : Game.Frame -> GraphSet.Submission(_)`:
```roc
draw = |frame| {
    mvp = mvp_with_aspect_ratio(frame.aspect_ratio)
    graph.draw({ mvp })
}
```

Full-parity vocabulary in `ResourcePlanner` & `RenderGraph.roc`:
- **Buffers**:
  - `singleton_buffer(planner, name, elem_packer, initial_items)` -> `SingletonSlot(elem)` (`.addr()`, `.addr_at(i)`)
  - `immutable_buffer(planner, name, elem_packer, initial_items)` -> `ImmutableSlot(elem)` (`.addr()`, `.addr_at(i)`)
  - `indirect_buffer(planner, name, initial_commands)` -> `ImmutableSlot(DrawIndexedIndirectCommand)`
  - `cpu_updated_buffer(planner, name, elem_packer, capacity, initial_items)` -> `CpuUpdatedSlot(elem)` (`.addr()`, `.read_addr()`)
  - `gpu_updated_buffer(planner, name, elem_packer, capacity, initial_items)` -> `GpuUpdatedSlot(elem)` (`.current()`, `.previous()`)
  - `index_buffer(planner, name, indices, vertex_count)` -> `IndexBufferSlot`
  - `uniform_buffer(planner, name, schema)` -> `UniformSlot(params)`
  - External sampled textures (`imported_texture` once asset loading is wired)
    and logical graph textures (`texture(planner, name, width, height, format)` ->
    `GraphTex` with `.read()`, `.read_previous()`, `.write()`, `.mutate()`).
- **Pipelines**:
  - `graphics_pipeline` -> `GraphicsPipelineKey(push)`
  - `graphics_indirect_pipeline` -> `GraphicsIndirectPipelineKey(push)`
  - `compute_pipeline` -> `ComputePipelineKey(push)`
- **Nodes & combinators**:
  - `dispatch` (+ `.with_param_bindings`, `.with_push_constant`)
  - `upload` (per-frame `List(elem)` packed via `elem_packer`)
  - `draw_vertex_count`, `draw_indexed`, `draw_index_range`, `draw_indexed_indirect`
    (+ `.with_push_constant`)
  - `optional`, `repeat`, fixed/tuple combinators `from_tuple_2`..`from_tuple_12`,
    and `list` (`List(RenderGraph(f))` -> `RenderGraph(List(f))`).
- **Roc-side Lowering & Validation (`RenderGraphDesc.roc`, `RenderGraphLower.roc`, `RenderGraphValidate.roc`)**:
  - Lowers the typed Roc nodes and `ResourcePlanner` declarations into
    `GraphDesc` + `SchemaTable` and runs full logical validation in Roc:
    - Texture read/write/mutate/read_previous version & hazard rules
    - Compute-before-raster ordering and `repeat`/`optional` nesting rules
    - Uniform slot reuse consistency (same schema & bindings)
    - Upload target & capacity checks
    - Index-buffer non-emptiness, `index < vertex_count` bounds check, and
      `draw_index_range` `[first_index, first_index + index_count)` bounds check!

---

## Proposed PR Stack Breakdown

### Retiring the existing `#37`–`#44` stack and salvage map

Before starting implementation:

1. **Preserve reference refs locally before resetting or closing anything**:
   Keep the existing local/origin branch tips (`roc-graphs/runtime-shaders`
   through `roc-graphs/graph-set-example` at `8ac67e8`) readable via `git show`
   while building PRs 2–6 so reusable code and tests can be ported directly.
   Stash or commit/branch off the uncommitted Phase 1 working-tree changes on
   `roc-graphs/runtime-shaders` (keeping `llm_notes/tech_debt.md` and
   `llm_notes/vertex_pulling_and_index_buffers.md` for the new stack), then
   create the new `gh stack` directly from `main` (`7df42d0`).
2. **Open new draft PRs per `AGENTS.md` conventions, then close `#37`–`#44`**:
   - Open each new layer in the `vp/*` stack as a **draft PR** with its
     description wrapped in `<details><summary>Model Name</summary>...</details>`.
   - Close PRs `#37`, `#38`, `#39`, `#40`, `#42`, `#43`, and `#44` on GitHub
     with a short note linking to the superseding `vp/*` stack (only delete the
     old `roc-graphs/*` branches once PR 6 is verified so no salvageable code is
     lost).
3. **Complete salvage map from `#37`–`#44` (nothing dropped by accident)**:

| Old PR / Branch | What to salvage into the new stack | What is intentionally discarded |
| --- | --- | --- |
| `#37` `roc-graphs/runtime-shaders` (`f9b4de5`) | `RuntimeShader` SPIR-V/reflection loading, `NoAtlas`, `create_uniform_buffer_bytes`, `FrameRenderer::aspect_ratio` -> **PR 3** | `VertexBytes`, `build_indexed_bytes`, fixed-function vertex binding/attribute builders, `ShaderAtlasEntry::hot_reload(&self) -> bool` (replaced by `Config.shader_hot_reload` in **PR 2**) |
| `#38` `roc-graphs/roc-codegen` (`a81b7c8`) | Module-owned Roc type placement (`Mltrs.roc` vs per-shader modules), `to_bytes` / `gpu_size` logical packers, `ShaderReflection.pack` -> **PR 4** | `VertexInput` wrapper and `vertex_layout`-based vertex struct offsets |
| `#39` `roc-graphs/roc-blueprints` (`09a2860`) | `RenderGraphDesc.roc`, `RenderGraphLower.roc`, `RenderGraphValidate.roc` hazard/schema checks and error formatting, tuple combinators `from_tuple_2`..`from_tuple_12` -> **PR 5** | `IndexedPipeline` owning inline `Mesh` (`vertices` + `indices`), unsplit single-uniform-only node restriction |
| `#40` `roc-graphs/platform-host` (`74a1d26`) | `roc_platform_abi.rs` owning wrappers (`RocOwned`), `graph_lifecycle.rs` + standalone `roc-graph-lifecycle-tests`, `ci/all_tests.sh`, negative Roc validation fixtures -> **PR 6** | `MeshDecl::Indexed { vertex_bytes, indices }` per-pipeline mesh attachment |
| `#42` `roc-graphs/frame-elapsed` (`0f03526`) | `frame.elapsed : F32` in `Game.Frame` and host frame timing -> **PR 6** | — |
| `#43` `roc-graphs/ok-destructure` (`e0a1aab`) | Idiomatic top-level `Ok(graph) = ...` construction in Roc examples and tests -> **PR 5 & PR 6** | — |
| `#44` `roc-graphs/graph-set-example` (`8ac67e8`) | `GraphSet` naming (`GraphSet.single`, `GraphSet.Submission`), lifecycle build-output cleanup in `justfile`, and porting `examples/creative-coding-sep-23` alongside `basic-triangle` -> **PR 5 & PR 6** | — |

---

Start the new stack from `main` (`7df42d0`), keeping each PR focused,
self-contained, and reviewable with `gh stack`:

### PR 1: `vp/index-buffers-and-unified-graphics` (Rust renderer + render-graph + CLI + examples)
- Add `IndexBufferHandle` (`create_index_buffer(&indices, vertex_count)` with
  debug-build `idx < vertex_count` validation) to `mltrs-renderer` and
  `IndexBufferSlot` to `mltrs-render-graph`.
- Migrate the 6 indexed Rust examples (`basic_triangle`, `depth_texture`,
  `viking_room`, `suzanne`, `multi_mesh`, `toon_link` GameCube + Modern) to
  vertex pulling (`mltrs::ImmutableAddr<Vertex>` + `uint vertexId : SV_VertexID`
  + `IndexBufferHandle`).
  - Share a single `Vertex` definition, `SingletonBufferHandle<Vertex>`, and
    `IndexBufferHandle` across `toon_link` GameCube and Modern modes if clean,
    or keep Modern's subset fields if layout differs.
- Unify `DrawVertexCount` and `DrawIndexed` into `Graphics`, rename
  `DrawIndexedIndirect` to `GraphicsIndirect`, and infer `Graphics` vs
  `GraphicsIndirect` from `SV_DRAWINDEX` in Slang reflection.
- Reject varying struct parameters on vertex entry points in Slang reflection /
  Rust codegen; delete `VertexDescription`, `MeshHandle<V>`,
  `IndexedPipelineConfig`, `NoVertex`, and
  `crates/slang-reflection/src/json/vertex_layout.rs`.
- Add `Std430DataLayout` `float3` pointer-pointee layout tests in `crates/cli`
  and `gpu_readback` / compile-harness tests.
- Update `docs/render_graph.md`, `docs/bindless.md`, and `docs/toon_link.md`.
- **Gate**: `just shaders`, `cargo check --workspace --all-targets`, `just test`,
  `just lint`, `just sweep`.

### PR 2: `vp/shader-hot-reload-config` (Runtime Slang hot reload in debug and release builds)
- Remove the `#[cfg(debug_assertions)]` compile-time gates around
  `shader_watcher` (`crates/renderer/src/lib.rs` — which also resolves the
  release-build `unused dependency 'notify'` warning!), `SpvBytes`
  (`crates/renderer/src/shaders.rs`), `shaders_source_dir`,
  `assert_shader_interface_unchanged`, `try_shader_recompile`, and
  `try_compute_shader_recompile` (`crates/renderer/src/renderer.rs`).
- Introduce a runtime `shader_hot_reload: bool` field on renderer configuration
  (e.g., `RendererConfig` passed to `Renderer::init`, plus an override in
  `EnvConfig`, e.g., `VKR_SHADER_HOT_RELOAD` defaulting to
  `cfg!(debug_assertions)` when unset, and exposed on `Game` / Roc `Game.Config`
  so an application or `just roc-platform run` can opt in or out explicitly).
- Do **not** put a `hot_reload(&self) -> bool` method on `ShaderAtlasEntry` or
  `RuntimeShader`: hot reload is strictly a renderer-level `Config` setting!
  - When `config.shader_hot_reload` is `false` or `shaders_source_dir` is empty:
    no `notify` watcher is started (`shader_watcher: None`), and
    `ShaderPipelineLayout::create_from_atlas` /
    `ComputeShaderPipelineLayout::create_from_atlas` load the precompiled SPIR-V
    directly (`Self::from_precompiled`) without invoking Slang.
  - When `config.shader_hot_reload` is `true` and `shaders_source_dir` is
    non-empty: both generated Rust shaders and `RuntimeShader`s watch
    `shaders_source_dir`, recompile `.slang` sources on save, run
    `assert_shader_interface_unchanged` (allowing live edits whenever binding
    / layout types don't change), and swap the rebuilt `vk::Pipeline`!
- While touching `try_shader_recompile` / `try_compute_shader_recompile`, build
  the new `vk::Pipeline` *before* pushing the old pipeline onto
  `self.old_pipelines` (fixing `tech_debt.md` §2 in the same hunk).
- **Gate**: `cargo check --workspace --all-targets`, `just test`, `just lint`
  (verifying zero `unused dependency 'notify'` warnings in `--release`), and
  `just sweep`.

### PR 3: `vp/plain-data-graph-assembly` (`mltrs-render-graph` + `mltrs-renderer` runtime resources)
- Wire up `compile.rs` (`AssemblyProgram` with separated `FrameBytes` +
  `BindingResolver` steps) into a public plain-data / dynamic graph execution
  path in `mltrs-render-graph` supporting compute, uploads, textures, buffer
  bindings, push constants, `optional`, `repeat`, and `Graphics` /
  `GraphicsIndirect` draws with `IndexBufferSlot`.
- Add `RuntimeShader` (graphics, graphics-indirect via `SV_DRAWINDEX`, and
  compute `RuntimeComputeShader`), `NoAtlas`, `aspect_ratio`, and byte-sized
  buffer constructors (`create_uniform_buffer_bytes`,
  `create_singleton_buffer_bytes`, `create_immutable_buffer_bytes`,
  `create_cpu_updated_buffer_bytes`, `create_gpu_updated_buffer_bytes`) in
  `mltrs-renderer` (porting and adapting the reusable pieces from `#37`, with no
  `hot_reload` property on `RuntimeShader` since `Config.shader_hot_reload` in
  PR 2 governs all shaders uniformly).
- **Gate**: `cargo test -p mltrs-render-graph` (including backend execution tests
  for `AssemblyProgram` with `float3` and mixed data/binding schemas),
  `just test`, `just lint`, `just sweep`.

### PR 4: `vp/roc-codegen-split` (`crates/cli/src/roc_codegen.rs` + `ShaderReflection.roc`)
- Port module-owned Roc type generation and `to_bytes` packers from `#38`,
  adapted to vertex pulling (no `VertexInput` / `vertex_layout`).
- Emit `shaders_source_dir` in `Generated/ShaderAtlas.roc` so the Roc platform
  knows where the project's `.slang` sources live when `Config.shader_hot_reload`
  is enabled.
- Add `*Data` and `*Bindings` splitting + `SchemaDesc` generation in
  `roc_codegen.rs` for `ParameterBlock` and `PushConstant` types across graphics,
  graphics-indirect, and compute shaders.
- Add `Std430` `float3` pointee and split-struct byte-layout tests in
  `crates/cli/src/roc_codegen.rs` and `crates/cli/tests/roc_codegen.rs`.
- Update `docs/roc_shader_codegen.md`.
- **Gate**: `just test` and `just roc-codegen-test`.

### PR 5: `vp/roc-render-graph` (`roc-platform/platform/` pure Roc API & validation)
- Implement `ResourcePlanner.roc`, `RenderGraph.roc`, `RenderGraphDesc.roc`,
  `RenderGraphLower.roc`, `RenderGraphValidate.roc`, `ValidatedRenderGraph.roc`,
  and `GraphSet.roc` with full parity for explicit buffers, index buffers,
  logical textures, compute/upload/draw/indirect nodes, push constants,
  `optional`, `repeat`, and tuple/list composition.
- Include Roc-side index-buffer bounds checking (`index < vertex_count` and
  `draw_index_range` bounds) alongside full hazard and schema validation.
- **Gate**: Pure Roc unit and negative compile/validation tests (`roc test` /
  `roc check`).

### PR 6: `vp/roc-platform-host-and-triangle` (`roc-platform` host ABI + `basic-triangle` example)
- Expose `shader_hot_reload : Bool` (and `shaders_source_dir : Str` from
  `ShaderAtlas` or `Game.Config`) in the Roc `Game` / `Config` API so Roc games
  can enable live Slang hot reload against the release `libhost.a` platform
  archive without recompiling the host!
- Define the `HostGraph` wire schema for `ResourcePlanner` + validated
  `GraphDesc` / `SchemaTable`, regenerate `roc_platform_abi.rs`, and wire up
  `roc-platform/src/game.rs` + `graph_lifecycle.rs` (reusing the lifecycle
  design from `#40`–`#44`).
- Update `roc-platform/examples/basic-triangle` to use vertex pulling
  (`mltrs::ImmutableAddr<Vertex>` + `ResourcePlanner.singleton_buffer` +
  `ResourcePlanner.index_buffer` + `RenderGraph.draw_indexed`).
- **Gate**: `just roc-platform test`, `just test`, `just sweep`, and headless
  verification of the Roc `basic-triangle` binary.

---

## Implementation context and repository gotchas

A model picking up this plan cold should keep these repository-specific details
in mind:

1. **All four non-example copies of `basic_triangle` must be updated in PR 1**
   when rejecting varying struct parameters on vertex entry points:
   - `crates/cli/fixtures/shaders/basic_triangle.shader.slang`: used by both the
     Rust `generated_files` insta snapshot test in `crates/cli/src/build_tasks.rs`
     (accept updated snapshots via `cargo insta test --workspace --accept` after
     inspecting diffs) and `real_roc_codegen_gate` in
     `crates/cli/tests/roc_codegen.rs` (`just roc-codegen-test`). Note that
     moving `Vertex` to `mltrs::ImmutableAddr<Vertex>` inside `TriangleParams`
     still emits `ShaderTypes.Vertex` on `main`'s `roc_codegen.rs` (as a pointer
     pointee), keeping `real_roc_codegen_gate` working in PR 1 before PR 4
     rewrites `roc_codegen.rs`!
   - `crates/slang-reflection/src/fixtures/basic_triangle.json`: checked-in
     reflection JSON fixture used by `mltrs-slang-reflection` unit tests.
   - `roc-platform/shaders/source/basic_triangle.shader.slang` +
     `roc-platform/src/generated/` + `roc-platform/src/game.rs`: on `main`,
     `roc-platform` (an excluded Cargo workspace) still compiles a placeholder
     Rust `BasicTriangle` game until PR 6 replaces it with `NoAtlas` +
     `RuntimeShader`. Regenerate its Rust bindings in PR 1 (`cargo run -p mltrs-cli -- shaders compile --crate-dir roc-platform --language rust`)
     and update `roc-platform/src/game.rs` so `cargo check --manifest-path roc-platform/Cargo.toml --all-targets`
     stays green across PRs 1–5.
   - `roc-platform/examples/basic-triangle/shaders/source/basic_triangle.shader.slang`
     (and its `Generated/` + `shaders/compiled/` via `just roc-platform shaders basic-triangle`).
2. **Crate dependency boundaries (enforced by tests and `AGENTS.md`)**:
   - `crates/slang-reflection`: the **only** crate allowed to depend on
     `shader-slang`; must stay free of `ash`, `vk-mem`, and `sdl3`.
   - `crates/render-graph`: must stay free of `ash`, `vk-mem`, `sdl3`,
     `shader-slang`, and `mltrs-renderer` (enforced by
     `crates/render-graph/tests/dependency_boundary.rs`).
   - `crates/cli`: depends on `mltrs-slang-reflection`, **not** `mltrs-renderer`.
     Whenever generated Rust imports or pipeline builder signatures change in
     PR 1, update the stub renderer types in
     `crates/cli/fixtures/check_crate/src/renderer/` to match.
   - Never call `std::env::var` outside `crates/renderer/src/env_config.rs`.
   - Never edit `todo.org`, and never hand-edit `examples/*/src/generated/`
     (always run `just shaders`).
3. **`examples/toon_link` shared `Vertex` and local Wind Waker gates**:
   - `examples/toon_link/shaders/source/skinning.slang` is already imported by
     both `toon_link.shader.slang` (GameCube) and `toon_link_modern.shader.slang`
     (Modern). Declaring the shared `public struct LinkVertex` (or `Vertex`) in
     `skinning.slang` lets `main.rs` and `modern.rs` share a single generated
     Rust struct, one `SingletonBufferHandle<Vertex>`, and one `IndexBufferHandle`
     instead of converting and uploading the mesh twice.
   - Staging files in `examples/toon_link` causes `scripts/pre-commit-checks.rs`
     to run `just toon_link::verify-assets`. Also run `just toon_link test-skinning-gpu`
     and (when `examples/toon_link/assets/link/` is present)
     `just toon_link link-verify-p1`.
4. **Hot reload (`try_shader_recompile` and `assert_shader_interface_unchanged`)**:
   - `Renderer::try_shader_recompile` (`crates/renderer/src/renderer.rs`) passes
     vertex binding/attribute descriptions into `create_graphics_pipeline`; with
     vertex pulling those arguments drop to `&[]` (or are removed from
     `create_graphics_pipeline`'s signature).
   - `assert_shader_interface_unchanged` serializes `ReflectionJson` to detect
     hot-reload layout changes, so `SV_DRAWINDEX` or `Pointer` pointee changes
     during hot reload are caught automatically without extra code.
5. **Roc ABI glue workflow in PR 6**:
   - `roc-platform` is excluded from the root Cargo workspace; always check/test
     it explicitly via `cargo check --manifest-path roc-platform/Cargo.toml --all-targets`
     and `just roc-platform test`.
   - When `roc-platform/platform/main.roc` changes the host-exposed types in
     PR 6, regenerate `roc-platform/src/roc_platform_abi.rs` with
     `just roc-platform glue` and update any `RocOwned` wrapper assertions at the
     bottom of `roc_platform_abi.rs` if entrypoints/types changed.
6. **Check `main` tip before branching**:
   - An independent error-handling PR (`#45`, migrating `anyhow` to `eros`) is
     open on GitHub. Check `git log -n 5 origin/main` before creating PR 1's
     branch: if `#45` has merged, use `eros` error conventions documented in
     `docs/`; otherwise use `anyhow` as on `7df42d0`.
   - Newer clippy versions may flag `clippy::needless_range_loop` in untouched
     files (`crates/convert-link/src/bmd/mat3_dump.rs` lines 322 & 503,
     `examples/toon_link/src/tev_pack.rs` line 580) during `just lint`.
   - Follow the `rust-style`, `slang-style`, and `roc-style` skills when writing
     or editing source files in each language.
