# Multi-draw indirect: `vkCmdDrawIndexedIndirect` and `SV_DrawIndex`

> **STATUS: DESIGN.** Adds GPU multi-draw-indirect to the renderer, and moves
> `examples/toon_link` onto it. Line numbers are as of `1eee614`.
>
> This is distinct from the repo's existing "multi draw", which is a CPU-side
> queue of separate draw commands. See §1.

Companion documents:
[`render-graph/05_multi_draw_rendering.md`](render-graph/05_multi_draw_rendering.md)
(the ordered CPU draw list, and its rejection of base-instance indexing),
[`bindless_textures/phase_09.md`](bindless_textures/phase_09.md) (why
`toon_link` carries a material pointer rather than a material index, and the
conditions under which that reverses),
[`link_rendering/follow_up.md`](link_rendering/follow_up.md) (the deferred
`instanceCount`/`vertexOffset` work).

## Goal

Add `vkCmdDrawIndexedIndirect` to the renderer API. Use it in `toon_link` to
record 7 indirect commands in place of 24 draw commands.

The model has 2874 triangles in 24 batches. The work delivers an API and the
`SV_DrawIndex` pattern. It does not target a frame-time gain.

Two phases:

- **Phase 1** — the renderer API, a `DrawIndexedIndirect` marker, and the
  `toon_link` conversion.
- **Phase 2** — reflection enforces that the marker and the shader's
  `SV_DrawIndex` declaration agree.

## 1. What exists

`toon_link` records 24 `vkCmdDrawIndexed` calls per frame, one per manifest
batch (`examples/toon_link/src/main.rs:1022-1036`). Each call binds a pipeline,
binds a descriptor set, binds the bindless heap, and pushes an 8-byte
`ToonLinkDraw` block holding an `ImmutableAddr<Material>`.

The repo's "multi draw" is `FrameRenderer::pending_draws`. The queue replays as
N separate draw commands inside one `cmd_begin_rendering`
(`crates/renderer/src/renderer.rs:2121-2232`). The record loop elides a
vertex/index rebind when consecutive draws share buffers. It rebinds the
pipeline, the descriptor sets and the heap for every draw.

`todo.org:460` lists draw indirect as unstarted.

## 2. What is missing

1. **The device feature.** `multiDrawIndirect` is not enabled
   (`renderer.rs:3812-3819`), and is absent from the suitability check
   (`renderer.rs:3437`).
2. **Buffer usage.** No buffer carries `INDIRECT_BUFFER`. Every storage buffer
   is created with `STORAGE_BUFFER | SHADER_DEVICE_ADDRESS`
   (`renderer.rs:1125`).
3. **A command struct.** No `DrawIndexedIndirectCommand` type exists.
4. **A `DrawCallConfig` variant** and a record-loop arm
   (`renderer.rs:6091-6095`, `renderer.rs:2203-2226`).
5. **A `DrawIndexedIndirect` draw-call marker**, and the config transition that
   reaches it (`crates/renderer/src/renderer/pipeline.rs:64-91`, `:350-384`).
6. **A `FrameRenderer` queue method** (`renderer.rs:5891-6007`).

Everything else in the record loop is reused unchanged: the pipeline bind, the
`last_bound_buffers` elision, the descriptor sets, `cmd_bind_texture_heap` and
`cmd_push_constants`.

## 3. Verified facts

- `shaderDrawParameters` is enabled (`renderer.rs:3831`). `SV_DrawIndex`
  therefore needs no new device feature.
- Slang maps `SV_DrawIndex` to the SPIR-V `DrawIndex` builtin
  (`../slang/source/slang/slang-ir-legalize-varying-params.h:64`,
  `slang-emit-spirv.cpp:7621`).
- Reflection accepts a system-value entry-point parameter.
  `reject_non_varying_entry_point_parameter` allows `VaryingInput`
  (`crates/slang-reflection/src/reflection/parameters.rs:228`).
  `examples/depth_texture` declares
  `vertMain(uint vertexIndex : SV_VertexID, Vertex vertex)`, and its generated
  `attribute_descriptions` hold 3 entries at locations 0, 1 and 2.
- A `VaryingInput` binding occupies no bytes, so codegen emits no Rust field
  for it (`crates/slang-reflection/src/json/parameters.rs:188`).
- Reflection records the semantic of a scalar entry-point parameter. From
  `examples/depth_texture/shaders/compiled/depth_texture.json`:
  `{ "kind": "scalar", "parameterName": "vertexIndex", "semanticName":
  "SV_VERTEXID", "scalarType": "uint32" }`. The Rust type is
  `SemanticScalarEntryPointParameter`
  (`crates/slang-reflection/src/json/parameters.rs:84-89`). Slang upper-cases
  the semantic.
- `multiDrawIndirect` is available on both the development GPU and lavapipe.
  `vulkaninfo` reports `multiDrawIndirect = true`,
  `drawIndirectFirstInstance = true`, `drawIndirectCount = true` and
  `maxDrawIndirectCount = 4294967295` under
  `/usr/share/vulkan/icd.d/lvp_icd.json`. `just sweep` can therefore exercise
  the feature.
- `_draw_call` on `PipelineConfig` is `PhantomData` (`pipeline.rs:311`).
  Nothing reads the marker at run time.
- `toon_link` creates 5 pipelines, one per distinct `RasterState`
  (`main.rs:777-831`, asserted by `EXPECTED_RASTER_STATES`).

**Unverified.** No shader in the repo declares a `nointerpolation uint`
inter-stage varying. Add the field and run `just shaders toon_link` before
writing the rest of §5.

## 4. Phase 1 — renderer changes

### 4.1 Enable the feature

`crates/renderer/src/renderer.rs`:

- `create_logical_device` (`:3812`): add `.multi_draw_indirect(true)` to the
  core `PhysicalDeviceFeatures` builder.
- `choose_physical_device` (`:3437`): add
  `(features.multi_draw_indirect, "multiDrawIndirect")` to `missing_features`.

Leave `drawIndirectCount` and `drawIndirectFirstInstance` disabled. No work in
this document reads a GPU-written draw count, and `firstInstance` stays free.

### 4.2 The command struct

New in `crates/renderer/src/renderer/`, exported through the renderer prelude:

```rust
/// Matches `VkDrawIndexedIndirectCommand`.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DrawIndexedIndirectCommand {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
    pub first_instance: u32,
}

impl GPUWrite for DrawIndexedIndirectCommand {}
```

`GPUWrite` is the bound every `create_*_buffer` method requires
(`crates/renderer/src/renderer/gpu_write.rs:9`).

### 4.3 Indirect buffer usage

Add `vk::BufferUsageFlags::INDIRECT_BUFFER` to `create_raw_storage_buffer`
(`renderer.rs:1122-1127`). One flag covers all four storage families: storage,
immutable, gpu-only and singleton. Those buffers are host-visible,
host-coherent and BDA-addressable, so a compute shader can also write indirect
arguments through an `Addr<DrawIndexedIndirectCommand>`.

### 4.4 `DrawCallConfig` and the record loop

```rust
enum DrawCallConfig {
    VertexCount(u32),
    IndexCount(u32),
    IndexRange { first_index: u32, index_count: u32 },
    IndexedIndirect {
        buffer: vk::Buffer,
        offset: vk::DeviceSize,
        draw_count: u32,
    },
}
```

The record arm (`renderer.rs:2203`) calls `cmd_draw_indexed_indirect` with a
stride of `size_of::<DrawIndexedIndirectCommand>()`. The commands are always
tightly packed, so the stride is not part of the API.

### 4.5 The `DrawIndexedIndirect` marker

`crates/renderer/src/renderer/pipeline.rs`, beside `DrawIndexed` (`:78-84`).
Each marker names its Vulkan call, so the name follows `DrawVertexCount` and
`DrawIndexed`.

```rust
/// A marker that the pipeline uses cmd_draw_indexed_indirect draw calls,
/// reading draw arguments from a buffer.
#[derive(Debug)]
pub struct DrawIndexedIndirect;
impl DrawCall for DrawIndexedIndirect {
    type Index = GraphicsPipelineIndex;
}
```

The transition goes on the finished config, not on `IndexedPipelineConfig`. The
vertex source and the draw-call kind are orthogonal, so a parallel
pre-vertex-source type carries no information:

```rust
impl<'t, V: VertexDescription, P> PipelineConfig<'t, V, DrawIndexed, P> {
    /// Draw with cmd_draw_indexed_indirect. The vertex source is unchanged.
    pub fn indirect(self) -> PipelineConfig<'t, V, DrawIndexedIndirect, P> {
        // field-by-field rebuild; only `_draw_call` differs
    }
}
```

`with_raster_state` is generic over `D` (`pipeline.rs:386-394`), so it is
callable either side of `.indirect()`.

This transition is temporary. The app asserts it, so it cannot check that the
shader reads `SV_DrawIndex`. §7 replaces it.

### 4.6 Queue methods

`FrameRenderer`, beside `queue_draw_index_range` (`renderer.rs:5918`):

```rust
pub fn queue_draw_indexed_indirect(
    &mut self,
    pipeline: &PipelineHandle<DrawIndexedIndirect>,
    args: &SingletonBufferHandle<DrawIndexedIndirectCommand>,
    first_command: u32,
    draw_count: u32,
);

pub fn queue_draw_indexed_indirect_with_push_constants<P: PushConstantBlock>(
    &mut self,
    pipeline: &PipelineHandle<DrawIndexedIndirect, PushBlock<P>>,
    args: &SingletonBufferHandle<DrawIndexedIndirectCommand>,
    first_command: u32,
    draw_count: u32,
    push: &P,
);
```

- **`SingletonBufferHandle` only.** `toon_link`'s arguments are static. A
  singleton buffer holds one `vk::Buffer` and no flight slot, so the queue
  method needs no ring arithmetic.
  [Superseded: the queue methods take `ImmutableBufferHandle` instead. A
  singleton has no write path after upload, so per-frame argument updates
  were unexpressible; the immutable family's flight ring makes them safe.
  `toon_link` fills its args buffer with `write_immutable_all_frames` at
  setup.]
- **`first_command` counts elements.** Convert it to a byte offset in the queue
  method, through the existing `element_byte_offset` bounds assert
  (`crates/renderer/src/renderer/storage_buffer.rs:344`).
- **Bounds check at queue time.** Assert
  `first_command + draw_count <= args.len()`, mirroring `index_range_in_bounds`
  (`renderer.rs:6082`). Add a unit test beside `cull_mode_mapping`
  (`renderer.rs:6112`). The index ranges inside the commands cannot be checked.
  That limit is inherent to indirect drawing.

`whole_index_count` (`renderer.rs:5873`) and `index_range_in_bounds` stay on
the `DrawIndexed` path. The indirect path calls neither.

Push constants stay per command. `cmd_push_constants` already runs once per
`PendingDrawCommand`, which is once per indirect command.

### 4.7 Compute-to-indirect barrier

`renderer.rs:1798-1810` makes compute writes visible to
`VERTEX_SHADER | FRAGMENT_SHADER` with `SHADER_READ`. §4.3 makes a
compute-written arguments buffer expressible, so widen the destination scope:

- add `vk::PipelineStageFlags2::DRAW_INDIRECT` to the destination stage mask
- add `vk::AccessFlags2::INDIRECT_COMMAND_READ` to the destination access mask

CPU-written arguments need no barrier. The buffers are persistently mapped and
host-coherent, and `vkQueueSubmit` carries the host-write dependency.

## 5. Phase 1 — shader changes

`examples/toon_link/shaders/source/toon_link.shader.slang`.

One indirect command sets push constants once. Per-draw data comes from
`SV_DrawIndex`. That is the base-pointer-plus-index shape that
[`bindless_textures/phase_09.md`](bindless_textures/phase_09.md) §1 names as
the condition for reversing the pointer decision.

`SV_DrawIndex` counts within the indirect command, from 0 to `draw_count - 1`.
It is not the batch index. Each run therefore carries its own slot table:

```slang
struct ToonLinkDraw {
    mltrs::ImmutableAddr<Material> materials;   // element 0 of the table
    mltrs::ImmutableAddr<uint>     drawSlots;   // this run's first draw
}
```

`SV_DrawIndex` is a vertex-stage builtin. The vertex stage resolves the slot
and forwards it flat:

```slang
FragVertex vertMain(Vertex vertex, uint drawIndex : SV_DrawIndex) {
    let slot = draw.drawSlots[drawIndex];
    let tev = draw.materials[slot].tev;
    // …
}

struct FragVertex {
    // …
    nointerpolation uint slot;
}

float4 fragMain(FragVertex fragVertex) : SV_TARGET {
    let material = draw.materials[fragVertex.slot];
    // …
}
```

The push block grows from 8 bytes to 16 bytes.

### 5.1 Uniformity

Two claims at `examples/toon_link/shaders/source/tev.slang:224-247` rest on the
material arriving as a per-draw push constant. Restate them:

- **The implicit-LOD `Sample` stays correct.** Derivatives are quad-scoped. A
  quad belongs to one primitive, and a primitive belongs to one sub-draw. So
  `texmap` and the stage-loop bound are quad-uniform. Change the comment to say
  quad-uniform. Do not change the code.
- **Bindless handles are non-uniform at subgroup scope.** A driver may pack
  fragments of two sub-draws into one wave. The compiler decorates every heap
  access `NonUniform`, so the access is correct. The cost is a waterfall loop
  on a driver that reports
  `shaderSampledImageArrayNonUniformIndexingNative = false`
  ([`../docs/bindless.md`](../docs/bindless.md) §Uniformity).

### 5.2 The CPU bounds check moves

`singleton_addr_at` asserts the element index.
[`bindless_textures/phase_09.md`](bindless_textures/phase_09.md) §1 records
that this assert is load-bearing, because BDA loads are not covered by
`robustBufferAccess`. Under `SV_DrawIndex` the slot comes from a GPU-side
table. Replace the assert by validating the slot table on the CPU when it is
built. Every entry must be less than `materials.len()`.

## 6. Phase 1 — `toon_link` changes

`examples/toon_link/src/main.rs`.

1. **Group `draw_order` into runs.** A run is a maximal span of consecutive
   entries that share one pipeline index (`materials.pipeline_of_slot`). The
   current manifest gives 7 runs over 24 batches: mask ×4, face_hair ×2,
   composite ×4, erase ×4, then `rest` splits 4 / 1 / 5 around `sleeve`, the
   only `CullMode::None` material. Compute the split. Do not hardcode it.
2. **Build two singleton buffers at setup**, both in flattened run order:
   - `SingletonBufferHandle<DrawIndexedIndirectCommand>` — one entry per batch,
     `{ index_count: batch.index_count, instance_count: 1,
     first_index: batch.first_index, vertex_offset: 0, first_instance: 0 }`.
   - `SingletonBufferHandle<u32>` — the material slot for the same entry.

   Run `r` starts at element `base_r` in both buffers. Keep the existing
   `MaterialSlot` and `BatchIndex` newtype discipline, and add a third newtype
   for this index space.
3. **Record one call per run:**

   ```rust
   for run in &self.runs {
       let push = ToonLinkDraw {
           materials: renderer.singleton_addr(&self.materials_buffer),
           draw_slots: renderer.singleton_addr_at(&self.slot_buffer, run.first),
       };
       renderer.queue_draw_indexed_indirect_with_push_constants(
           &self.materials.pipelines[run.pipeline],
           &self.args_buffer,
           run.first,
           run.count,
           &push,
       );
   }
   ```
4. **The isolate slider becomes a one-element indirect call.**
   _[Superseded: the isolate slider is removed, along with `CommandIndex` and
   `command_of_batch`.]_ The 5 pipelines
   are `PipelineHandle<DrawIndexedIndirect, PushBlock<ToonLinkDraw>>`, so the
   direct queue methods do not accept them. Isolating batch `b` calls
   `queue_draw_indexed_indirect_with_push_constants(pipeline, args,
   element_of(b), 1, &push)`, with `draw_slots` at the same element.
   `SV_DrawIndex` is 0 for that call, which is correct.

   Update the types on `MaterialTable.pipelines` (`main.rs:766`) and
   `ToonLink::pipeline` (`main.rs:887`).
5. **Extend `validate_manifest`** (`main.rs:530`) with the slot-table bound
   check from §5.2.

## 7. Phase 2 — reflection-enforced marker

The app asserts `PipelineConfig::indirect()`. Nothing checks that the shader
reads `SV_DrawIndex`. Both mismatches render a wrong picture with no error:

- an indirect pipeline with no `SV_DrawIndex` — every sub-draw reads slot 0
- a direct pipeline that declares `SV_DrawIndex` — the value is always 0

Phase 2 makes both a hard error at pipeline creation.

### 7.1 No schema change

Reflection already records the semantic (§3). The committed
`examples/*/shaders/compiled/*.json` do not move, so
`assert_shader_interface_unchanged` (`renderer.rs:5151`) stays quiet and no
example needs regenerating.

### 7.2 The check

1. **A reflection helper**, beside `GlobalParameter::declares_bindless_handle`
   (`crates/slang-reflection/src/json/parameters.rs:107`), which is the model
   to copy:

   ```rust
   impl EntryPoint {
       /// Whether this entry point declares an `SV_DrawIndex` parameter.
       pub fn declares_draw_index(&self) -> bool
   }
   ```

   Match `ScalarEntryPointParameter::Semantic` with
   `semantic_name.eq_ignore_ascii_case("SV_DrawIndex")`.

2. **An associated const on `DrawCall`** (`pipeline.rs:65-68`), so
   `create_pipeline` can branch on the marker:

   ```rust
   pub trait DrawCall {
       type Index: PipelineIndex;
       /// Whether this draw-call kind supplies a meaningful `SV_DrawIndex`.
       const NEEDS_DRAW_INDEX: bool = false;
   }
   ```

   `DrawIndexedIndirect` overrides it to `true`.

3. **The gate in `create_pipeline`** (`renderer.rs:1236`), which returns
   `anyhow::Result`. Compare `D::NEEDS_DRAW_INDEX` against
   `config.shader.reflection_json().vertex_entry_point.declares_draw_index()`.
   `ensure!` they agree. Name the shader and both sides in the message.

   `reflection_json()` carries a "dev only" comment
   (`crates/renderer/src/shaders/atlas.rs:32`), but it is not `cfg`-gated and
   the generated `Shader` holds it unconditionally. The gate works in release.

4. **Reject `SV_DrawIndex` on the fragment entry point.** It is a vertex-stage
   SPIR-V builtin. Confirm that Slang rejects it. If Slang accepts it, gate it
   in `reject_non_varying_entry_point_parameter`
   (`crates/slang-reflection/src/reflection/parameters.rs:219`), which is
   already where entry-point parameters are validated.

### 7.3 Why a check, and not derivation

The stronger form derives the marker from reflection, which makes a mismatch
unrepresentable. It does not fit cheaply. `config_return_type`
(`crates/cli/src/build_tasks.rs:582`) returns
`IndexedPipelineConfig<'a, V, PushSlot>`, which does not name the draw-call
marker. The marker appears only where `with_vertices` and `with_shared_mesh`
hardcode `DrawIndexed` (`pipeline.rs:352-363`). Derivation needs one of:

- a parallel `IndirectPipelineConfig` type, about 50 lines of near-duplicate
- a fourth generic parameter on `IndexedPipelineConfig`, which needs an awkward
  default position to stay non-breaking

Either option also needs codegen changes and snapshot churn.

The `create_pipeline` gate catches every mismatch at startup, because every
pipeline is created during setup. Revisit derivation if the gate proves
insufficient.

## 8. Out of scope

- `vkCmdDrawIndexedIndirectCount` and the `drawIndirectCount` feature. Add both
  when a compute pass decides the draw count.
- Ringed and GPU-written argument sources: `ImmutableBufferHandle`,
  `StorageBufferHandle`, `GpuOnlyBufferHandle`. §4.7 widens the barrier for
  them. The queue overloads are not written.
  [Partly superseded: `ImmutableBufferHandle` is the argument source now,
  replacing the singleton rather than joining it as an overload. GPU-written
  sources (`StorageBufferHandle`, `GpuOnlyBufferHandle`) remain out of
  scope.]
- `instanceCount > 1` and `vertexOffset != 0`. Both stay at 1 and 0, as
  [`link_rendering/follow_up.md`](link_rendering/follow_up.md) §2 records.
- Picking. It `debug_assert`s an empty draw queue (`renderer.rs:6041`).
- Render-graph integration.
  [`render-graph/05_multi_draw_rendering.md`](render-graph/05_multi_draw_rendering.md)
  describes an ordered CPU draw list, which is a different mechanism.

## 9. Verification

### 9.1 Phase 1

1. `just shaders toon_link`. This regenerates the SPIR-V, the reflection JSON
   and `src/generated/`. It confirms that `SV_DrawIndex`, the
   `nointerpolation uint` varying and the 16-byte push block all survive
   reflection. The generated `ToonLinkDraw` size assert must read 16.
2. `cargo check --workspace --all-targets`, then `cargo fmt`.
3. `just lint`.
4. `just test`. No snapshot should move. Phase 1 changes no template and no
   part of `build_tasks.rs`. A moved snapshot means something unintended
   changed.
5. `just toon_link`. Walk every `DebugMode` variant. The eye and brow decals
   are the sensitive part, because the 5 groups must still draw in order, so a
   run-grouping fault shows there first. Toggle the isolate slider across all
   24 batches. Each isolated batch must match the corresponding region of the
   full frame. _[Superseded: the isolate slider is removed; the per-batch
   toggle check no longer applies.]_

   The direct path and the indirect path cannot coexist in one binary, so the
   comparison runs against the pre-change commit. Capture the same camera angle
   from each build and diff the screenshots.
6. `just sweep`. The change affects command recording, so run the whole sweep.
   `toon_link` is skipped where its assets are absent. Run the sweep on a
   machine that holds `examples/toon_link/assets/link/converted`, so the new
   record path is validated. The gate is exit 0 with 16 ok
   ([`../docs/testing.md`](../docs/testing.md)).
7. Confirm the command count. A RenderDoc capture or
   `VK_LAYER_LUNARG_api_dump` must show 7 draw commands in place of 24.

### 9.2 Phase 2

- Add two failing cases: a shader that declares `SV_DrawIndex` built as
  `DrawIndexed`, and a shader that does not declare it built as
  `DrawIndexedIndirect`.
- `just test`. No snapshot should move. Phase 2 changes no template.
- `just sweep`. Every example must still start. A false positive in the gate
  reports as a setup failure, not as a wrong picture.
- `crates/cli/fixtures/check_crate/src/renderer/mod.rs:29-33` stubs `DrawCall`,
  `DrawIndexed` and `DrawVertexCount`. Add `DrawIndexedIndirect` there when
  generated code names the marker. The gate itself does not need it.

## 10. Accepted limitation

`toon_link` is the only planned consumer, and it is not CI-verifiable. Its
assets are machine-local
([`link_rendering/follow_up.md`](link_rendering/follow_up.md) §5). The sweep
compiles the new renderer path on every machine, and exercises it only where
the assets exist. Closing that gap needs a second example, on tracked assets,
on the indirect path.
