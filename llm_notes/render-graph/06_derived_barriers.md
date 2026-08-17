# Derived Barriers Without a Graph

> **STATUS: DESIGN.** Barrier elision on the existing `FrameRenderer` command
> list. It needs no graph, no builder API, and no change to app code. It is
> [04_design.md](04_design.md) §6 extracted so it can land alone.

## The problem

`FrameRenderer::dispatch` inserts a global barrier whenever the previous queued
command is a dispatch (`crates/renderer/src/renderer.rs:5751-5773`). The masks
are fixed:

```
COMPUTE_SHADER / SHADER_WRITE  ->  COMPUTE_SHADER / SHADER_READ | SHADER_WRITE
```

The rule is positional. It reads no resource. Two dispatches on disjoint
resources get the same barrier as two dispatches in a dependency chain.

`record_command_buffer` also emits an unconditional compute->graphics barrier
whenever the compute list is non-empty (`renderer.rs:1763-1775`).

Watercolor pays for this twice per frame:

- Pass 5 (project velocity) writes `u` and `v`. Pass 6 (blur H) reads
  `wet_mask` and writes `blur_temp`. The only shared image is `wet_mask`, and
  both passes read it. `examples/watercolor/src/main.rs:930-934` records the
  case.
- Pass 8 (flow outward) writes `pressure` and `saturation`. Pass 9 (advect)
  touches neither. Pass 10 (capillary flow) reads `saturation`. A global
  barrier's first scope covers every earlier command, so the barrier before
  pass 10 discharges the 8->10 dependency and the 8->9 barrier is redundant.

## What the renderer must know

A hazard test needs two facts per dispatch: which resources it touches, and in
which direction.

### Descriptor-bound images

Generated `pipeline_config` splits `Resources` into three ordered vectors
(`crates/renderer/src/renderer/pipeline.rs:488-493`):

| Vector | Slang declaration | Access |
|---|---|---|
| `texture_handles` | `Texture2D`, `Sampler2D` | read |
| `storage_texture_handles` | `RWTexture2D` | read and write |
| `uniform_buffer_handles` | the parameter block | CPU write only |

Reflection already separates the two texture kinds. `ResourceShape` has
`Texture2D` and `RWTexture2D`
(`crates/slang-reflection/src/json/parameters.rs:274-277`), set from
`resource_access()` (`reflection/parameters.rs:426-434`).
`ReflectedBindingType` carries the same split as `Texture` and `StorageImage`
(`json/pipeline_builders.rs:58-64`). No reflection change is needed.

`create_compute_pipeline` resolves the handles to `vk::Image`
(`renderer.rs:1427-1447`) and then drops them. `ComputeRendererPipeline` keeps
only the layout, the pipeline, the descriptor pool, the descriptor sets, and
the shader (`pipeline.rs:438-444`).

**Change 1.** Retain the resolved list on the pipeline as
`Vec<(vk::Image, Access)>`. `storage_texture_as_sampled` aliases the same
`vk::Image` (`renderer.rs:818-852`), so a key on image identity tracks the
alias with no extra work.

### BDA buffers

A buffer reaches a shader as an 8-byte address inside the parameter block.
`Addr<T>` is `#[repr(transparent)]` over `u64` and is asserted to be 8 bytes
(`crates/renderer/src/renderer/addr.rs:9-14`, `:55-56`). The type cannot carry
extra data, because the parameter block is copied to the GPU as bytes.

The access direction is already reflected. `PointerAccess` has `ReadWrite`,
`Read`, and `Immutable` (`json/parameters.rs:355-365`).

**Change 2.** Capture the address at write time. Codegen adds one method body
to the `GPUWrite` impl of each parameter struct that holds pointer fields:

```rust
pub trait GPUWrite {
    fn write_pointers(&self, _out: &mut Vec<(u64, PointerAccess)>) {}
}

impl GPUWrite for SimParams {
    fn write_pointers(&self, out: &mut Vec<(u64, PointerAccess)>) {
        out.push((self.particles_in.to_raw(), PointerAccess::Read));
        out.push((self.particles_out.to_raw(), PointerAccess::ReadWrite));
    }
}
```

The access values come from the reflection JSON.
`examples/particles/shaders/compiled/particles.comp.json` records `particlesIn`
with `access: "read"` and `particlesOut` with `access: "readWrite"`. `to_raw()`
exists on all three pointer types (`addr.rs:24`, `:79`, `:146`).

The default body is empty, so shaders without pointer fields and the `u8`,
`u32`, `f32`, and `NoVertex` impls (`gpu_write.rs:16-19`) need no change.

`Gpu::write_uniform` (`renderer.rs:5508-5514`) calls the method on the caller's
value before the copy, and stores the result under
`(uniform buffer index, flight slot)`. It reads CPU memory only. It must not
read the mapping back: uniform buffers use `BufferMemory::PersistentlyMapped`
(`renderer.rs:998`), and readback from that memory can be slow.

Add a `T: GPUWrite` bound to `write_uniform`. Every uniform buffer comes from
`create_uniform_buffer<T: GPUWrite>` (`renderer.rs:987`), so the bound costs
nothing.

### The address is the identity key

Two writable handle kinds mint addresses: `StorageBufferHandle` and
`GpuOnlyBufferHandle` (`storage_buffer.rs:10`, `:84`). Neither has an
element-offset accessor, so both mint slot-base addresses only.

`ImmutableBufferHandle` and `SingletonBufferHandle` do have
`element_byte_offset` (`:41`, `:64`), but both mint `ImmutableAddr` only. The
GPU never writes them, so they take part in no hazard.

Key the hazard table on the raw address. That is `(buffer, slot)` identity.
`current_addr` and `previous_addr` on one handle return different slots and
must stay distinct: the leading cross-frame barrier orders that edge
(`renderer.rs:1742-1759`).

## Change 3: synthesize barriers at record time

`dispatch` decides the barrier when it pushes the command. That is too early.
`draw_frame` runs `gpu_update(&mut gpu)` at `renderer.rs:2580` and
`record_command_buffer` at `:2599`. Move the decision into the second step,
where every address the frame uses is known.

The move is necessary for a second reason. The emitted barrier is a global
`VkMemoryBarrier2`, and its first synchronization scope covers every earlier
command in submission order. A pairwise rule cannot model that.

## The analysis

1. Walk `pending_compute` in order.
2. Collect each dispatch's accesses: the retained image list, plus the pointer
   entries recorded for each of its `uniform_buffer_handles`.
3. Keep a table from resource key to last-writer position. A key is a
   `vk::Image` or a `u64` address.
4. A dispatch at position *k* has a hazard when it reads or writes a resource
   whose last writer is below *k* and above the last emitted barrier.
5. Emit one barrier before position *k* only when a hazard exists. Record *k*
   as the new barrier position.
6. Apply the same test to the compute->graphics barrier, against the graphics
   pipelines' retained image lists.

Read-after-read is not a hazard. Immutable pointers take part in no hazard.

## What this leaves out

Four guards. Each one forces a barrier instead of an elision.

1. **Bindless textures.** A `BindlessHandle<T>` is a `u64` in the parameter
   block (`crates/renderer/src/renderer/bindless.rs:15`). Reflection records
   the field but not the texture. Guard: a non-null `bindlessHeapSet` on the
   pipeline layout forces the barrier. No compute shader uses the heap.
   `depth_texture` and `toon_link` are the only users, and both are graphics.
2. **Addresses in buffer element data.** Codegen emits pointer fields in any
   struct position, including pointee structs
   (`llm_notes/bda_footguns.md:95-102`). A parameter-block capture does not see
   those. Guard: a pointee struct that holds pointer fields forces the barrier.
3. **Unresolved addresses.** Build a registry from `(device_address, byte_len)`
   to `(buffer, slot)` at buffer creation. `RawStorageBuffer.device_address` is
   cached at creation and is stable (`storage_buffer.rs:100-103`). Resolve each
   captured address by range containment. An address that resolves to nothing
   is a fault. Guard: force the barrier.
4. **Sub-resource ranges.** Two dispatches that write disjoint regions of one
   image, or disjoint element ranges of one buffer, read as a write-after-write.
   Reflection cannot narrow this, and nothing in the tree needs it.

Two further limits, which are not guards:

- **Declared access, not real access.** `resource_access()` reports the
  declared type. A shader that declares `RWTexture2D` and only writes it is
  recorded as read and write. Watercolor routes every read through `Texture2D`
  or `Sampler2D`, so the loss is small. A true write-only flag needs a SPIR-V
  scan for `OpImageWrite` and `OpImageRead`.
- **No oracle for BDA.** Vulkan synchronization validation works from
  descriptor bindings. It cannot observe accesses made through a buffer device
  address. `just sweep` therefore checks the image half of this analysis and
  not the buffer half.

## Differences from 04_design.md

| Item | 04_design.md | This design |
|---|---|---|
| Scope | Full render graph: builder, sections, nodes, parity groups, graph-owned ping-pongs, loops, optional nodes | Barrier elision only |
| App API | New builder plus `execute(frame, \|run\| ...)`; watercolor loses about 350 lines | Unchanged. `dispatch` and `write_uniform` keep their signatures |
| Buffer tracking | Declared handles: `ParamsBuffers` and `ParamsPtrs`, two extra generated structs per pointer-bearing shader (§5) | Captured values: one method body per pointer-bearing shader |
| When buffers are known | Build time, so the graph reports domain errors early | Frame time, during `gpu_update` |
| Barrier schedule | Precomputed per reachable parity state at build, replayed at execute (§6) | Recomputed each frame from the pending list |
| Pipeline variants | The graph creates every parity variant, and needs "N pipelines from one atlas entry" first (Phase 1) | None. The app keeps its own variant arrays |
| Ping-pong and parity | Graph-owned: `ParityGroup`, `GraphPingPong`, variant enumeration (§2-§4) | Untouched. The app keeps `PingPong` and its parity fields |
| Compute->graphics edge | Derived from the graph's hazard model | Derived from the same table |

The two share one hazard model. `04_design.md` §6 reads the same
`pipeline_config` handle vectors and reaches the same conclusion about
`storage_texture_as_sampled` aliasing. This design keeps that half and drops the
rest.

The two are not alternatives. This is §6 without §2, §3, §4, §5, and §7. If the
graph is built later, it replaces this analysis with the precomputed form and
this code is deleted. If the graph is not built, the elision still lands.

## Verification

1. `cargo check --workspace --all-targets`, `just lint`, `just test`.
2. `just shaders`. The codegen change touches every generated atlas entry, so
   snapshot churn is expected.
3. `just sweep`. Zero validation messages. This is the oracle for the image
   half of the analysis.
4. `timeout 3 just dev watercolor`. Paint, and confirm the strokes spread as
   before.
5. Count the barriers in a frame. Watercolor must lose exactly two
   inter-dispatch barriers: 5->6 and 8->9. Read the sequence from a RenderDoc
   capture, or from the debug labels in `record_compute_commands`
   (`renderer.rs:1633-1643`).
6. Add a unit test for the analysis. It is CPU logic over a list of accesses
   and needs no device. The existing CPU-only tests are at
   `renderer.rs:6005-6141`.
