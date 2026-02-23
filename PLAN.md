# Compute Shader Support: Same-CB with Pipeline Barriers

## Context

Add compute shader support using the same command buffer as graphics, with pipeline barriers for synchronization (Option A). This provides the primitives — `dispatch()` and `memory_barrier()` — that the future FLAME-inspired render graph (`claude_notes/render-graph/01_flame_render_graph.md`) will call internally.

The render graph is the right layer to own the command buffer split decision. Its base version (`01_flame_render_graph.md`) records all stages into a single CB with auto-barriers — exactly these primitives. When async compute is added later (`02_explicit_parallelism.md`), the graph splits into separate CBs and adds semaphores. Nothing from this implementation gets thrown away.

Includes renderer-managed ping-pong buffers as the foundation for double-buffered simulation state.

---

## Phase 0: Queue Family Validation

Assert that the graphics queue family supports compute. Pure additive, no behavior change.

### File: `src/renderer.rs`

**0A. Validate compute support** (in `QueueFamilyIndices::find()`, line ~1536)

Add a debug assertion that the selected graphics queue family also has `vk::QueueFlags::COMPUTE`. This is nearly universal on desktop GPUs but better to fail early with a clear message.

```rust
assert!(
    queue_families[graphics_family_index]
        .queue_flags
        .contains(vk::QueueFlags::COMPUTE),
    "Graphics queue family must support compute"
);
```

No separate compute queue, command pool, or semaphores needed. Compute dispatches go into the existing graphics command buffer.

---

## Phase 1: Shader Compilation for Compute

Add `*.compute.slang` compilation alongside existing `*.shader.slang`.

### Files: `src/shaders.rs`, `src/shaders/reflection/parameters.rs`, `src/shaders/json.rs`

**1A. New `prepare_reflected_compute_shader()` function**

Parallel to `prepare_reflected_shader()` but expects exactly 1 entry point (compute stage). Extracts `[numthreads(X,Y,Z)]` workgroup size from Slang reflection.

**1B. New `ComputeReflectionJson`**

```rust
pub struct ComputeReflectionJson {
    pub source_file_name: String,
    pub global_parameters: Vec<GlobalParameter>,
    pub compute_entry_point: EntryPoint,
    pub workgroup_size: [u32; 3],
    pub pipeline_layout: ReflectedPipelineLayout,
}
```

**1C. Update reflection** (`src/shaders/reflection/parameters.rs`)

New `reflect_compute_entry_point()` that handles the `Compute` stage case (currently hits `_ => todo!()` at line ~135).

---

## Phase 2: Code Generation for Compute

### Files: `src/shaders/build_tasks.rs`, new template

**2A. Detect shader type from file extension**

- `*.shader.slang` → existing graphics codegen path
- `*.compute.slang` → new compute codegen path

**2B. New template: `templates/shader_compute_entry.rs.askama`**

Generates:
- `Resources<'a>` struct (uniform/storage buffers, textures — no vertices/indices)
- `WORKGROUP_SIZE: [u32; 3]` constant
- `pipeline_config()` returning `ComputePipelineConfig`
- `ComputeShaderAtlasEntry` trait implementation

**2C. New trait: `ComputeShaderAtlasEntry`** (`src/shaders/atlas.rs`)

```rust
pub trait ComputeShaderAtlasEntry {
    fn source_file_name(&self) -> &str;
    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>>;
    fn precompiled_compute_shader(&self) -> PrecompiledShader;
    fn pipeline_layout(&self) -> &ReflectedPipelineLayout;
}
```

**2D. Update atlas module** to include compute shaders in `ShaderAtlas`.

---

## Phase 3: Pipeline Type and Creation

### Files: `src/renderer/pipeline.rs`, `src/renderer.rs`

**3A. Add `Compute` marker**

```rust
pub struct Compute;
impl DrawCall for Compute {}
```

**3B. Add `ComputePipelineConfig`**

Like `PipelineConfig` but without vertex config, draw call type, or index buffers.

**3C. Extend `RendererPipeline` with `PipelineKind`**

`RendererPipeline` currently stores `vertex_pipeline_config: VertexPipelineConfig`, `shader: Box<dyn ShaderAtlasEntry>`, and `disable_depth_test: bool` — all graphics-specific. `ShaderPipelineLayout` stores `vertex_shader` and `fragment_shader` fields — also graphics-specific. These must be refactored for compute.

Replace `RendererPipeline`'s graphics-specific fields with a `PipelineKind` enum that carries everything that differs between graphics and compute:

```rust
pub(super) enum PipelineKind {
    Graphics {
        vertex_pipeline_config: VertexPipelineConfig,
        shader: Box<dyn ShaderAtlasEntry>,
        disable_depth_test: bool,
    },
    Compute {
        shader: Box<dyn ComputeShaderAtlasEntry>,
    },
}

pub(super) struct RendererPipeline {
    pub layout: ShaderPipelineLayout,
    pub pipeline: vk::Pipeline,
    pub kind: PipelineKind,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_sets: Vec<vk::DescriptorSet>,
}
```

Similarly, refactor `ShaderPipelineLayout` to support both graphics (vert+frag) and compute (single shader):

```rust
enum ShaderStages {
    Graphics {
        vertex_shader: PrecompiledShader,
        fragment_shader: PrecompiledShader,
    },
    Compute {
        compute_shader: PrecompiledShader,
    },
}

struct ShaderPipelineLayout {
    stages: ShaderStages,
    // shared between graphics and compute
    pipeline_layout: ash::vk::PipelineLayout,
    descriptor_set_layouts: Vec<(ash::vk::DescriptorSetLayout, DescriptorCounts)>,
}
```

The picking pipeline is a graphics pipeline (fragment shader writing R32_UINT), so it stays in `PipelineKind::Graphics`. Its existing `vertex_pipeline_config: VertexPipelineConfig::VertexCount` becomes part of the `Graphics` variant — a trivial migration.

`ShaderPipelineLayout::create_from_atlas` splits into two methods: the existing one for `&dyn ShaderAtlasEntry` (graphics), and a new `create_from_compute_atlas` for `&dyn ComputeShaderAtlasEntry`.

**3D. Add `create_compute_pipeline()` to Renderer**

```rust
pub fn create_compute_pipeline(
    &mut self,
    config: ComputePipelineConfig,
) -> Result<PipelineHandle<Compute>> {
    // 1. Create shader module from SPIR-V
    // 2. Create pipeline layout (descriptor set layouts)
    // 3. vkCreateComputePipelines
    // 4. Create descriptor pool + sets
    // 5. Store and return typed handle
}
```

Uses `vk::ComputePipelineCreateInfo` with a single `COMPUTE` stage.

---

## Phase 4: PingPongBuffer

Renderer-managed double-buffered storage buffers for compute read/write patterns.

### Files: new `src/renderer/ping_pong_buffer.rs`, `src/renderer.rs`

**4A. PingPongBufferHandle type**

```rust
pub struct PingPongBufferHandle<T> {
    buffers: [StorageBufferHandle<T>; 2],
    current_read: usize,
}

impl<T> PingPongBufferHandle<T> {
    /// Buffer containing last frame's compute output (bind as read-only)
    pub fn read_buffer(&self) -> &StorageBufferHandle<T> {
        &self.buffers[self.current_read]
    }

    /// Buffer to write this frame's compute output into
    pub fn write_buffer(&self) -> &StorageBufferHandle<T> {
        &self.buffers[1 - self.current_read]
    }

    /// Call after dispatch, before draw, to swap read/write roles.
    /// After swap, read_buffer() returns what compute just wrote.
    pub fn swap(&mut self) {
        self.current_read = 1 - self.current_read;
    }
}
```

User calls `swap()` explicitly after `dispatch()` but before `draw()`. This is transparent and matches the tutorial's pattern. The future render graph automates this.

**4B. Descriptor set handling**

Descriptor sets are bound to specific `vk::Buffer` handles at creation time (line ~2609 in renderer.rs). With ping-pong, the compute pipeline needs to alternate which buffer is "in" vs "out" each frame.

Solution: Create **2x descriptor sets** for compute pipelines using ping-pong buffers — one set for each configuration (read=A/write=B and read=B/write=A). The `current_read` index selects which to bind. Combined with the existing `BUFFER_FRAME_COUNT` multiplier, a compute pipeline with ping-pong gets `2 * BUFFER_FRAME_COUNT` descriptor set groups.

**4C. `create_ping_pong_buffer()` on Renderer**

Creates two `StorageBufferHandle<T>` via the existing `create_storage_buffer`. Start with HOST_VISIBLE buffers (same as existing storage buffers) for simplicity.

**4D. Initial data upload**

```rust
impl<T> PingPongBufferHandle<T> {
    pub fn write_initial_data(&self, gpu: &mut Gpu, data: &[T]) {
        // Write to BOTH sides so the first frame's read has valid data
        gpu.write_storage(&self.buffers[0], data);
        gpu.write_storage(&self.buffers[1], data);
    }
}
```

---

## Phase 5: FrameRenderer Dispatch and Barrier API

Dispatch and barrier commands are accumulated on `FrameRenderer` and replayed into the command buffer at the start of `record_command_buffer()`, before the picking pass and main render pass.

**Why accumulate instead of recording directly?** `FrameRenderer` is a thin wrapper (`pub struct FrameRenderer<'f>(&'f mut Renderer)`) and the command buffer isn't begun until `record_command_buffer()` is called internally by `draw_frame()`. There is no active command buffer when `dispatch()` is called by user code. A single ordered vec with an enum preserves interleaving order for multi-stage compute (dispatch A, barrier, dispatch B, barrier).

### File: `src/renderer.rs` — `FrameRenderer` impl

**5A. Accumulation types and `FrameRenderer` struct change**

```rust
enum PendingComputeCommand {
    Dispatch {
        pipeline_index: usize,
        group_count: [u32; 3],
    },
    Barrier {
        src_stage: vk::PipelineStageFlags,
        dst_stage: vk::PipelineStageFlags,
        src_access: vk::AccessFlags,
        dst_access: vk::AccessFlags,
    },
}
```

Change `FrameRenderer` from a tuple struct to a named struct:

```rust
pub struct FrameRenderer<'f> {
    renderer: &'f mut Renderer,
    pending_compute: Vec<PendingComputeCommand>,
}
```

**5B. Add `dispatch()`**

```rust
impl<'f> FrameRenderer<'f> {
    pub fn dispatch(
        &mut self,
        pipeline: &PipelineHandle<Compute>,
        group_count_x: u32,
        group_count_y: u32,
        group_count_z: u32,
    ) {
        self.pending_compute.push(PendingComputeCommand::Dispatch {
            pipeline_index: pipeline.index(),
            group_count: [group_count_x, group_count_y, group_count_z],
        });
    }
}
```

**5C. Add `memory_barrier()`**

```rust
impl<'f> FrameRenderer<'f> {
    pub fn memory_barrier(
        &mut self,
        src_stage: vk::PipelineStageFlags,
        dst_stage: vk::PipelineStageFlags,
        src_access: vk::AccessFlags,
        dst_access: vk::AccessFlags,
    ) {
        self.pending_compute.push(PendingComputeCommand::Barrier {
            src_stage, dst_stage, src_access, dst_access,
        });
    }
}
```

**5D. Replay in `record_command_buffer()`**

`record_command_buffer()` gains a `pending_compute: &[PendingComputeCommand]` parameter. After `begin_command_buffer` and before the picking render pass, it replays each command in order:

- `Dispatch` → `cmd_bind_pipeline(COMPUTE)`, `cmd_bind_descriptor_sets(COMPUTE)`, `cmd_dispatch()`
- `Barrier` → `cmd_pipeline_barrier()`

This naturally puts compute before picking (since the picking shader may read storage buffers that compute just wrote), solving the command buffer ordering requirement.

**5E. Frame lifecycle with picking**

The full recording order with both compute and picking active:

```
1. acquire_next_image
2. CPU buffer writes
3. wait_for_fences(frames_in_flight[current_frame])
4. reset fence + command buffer
5. begin_command_buffer
6.   replay pending_compute:           <- NEW
       dispatch(...)
       memory_barrier(compute->graphics)
7.   [picking render pass, if active]  <- reads storage buffers
8.   [copy picking pixel to readback]
9.   begin_render_pass (main)
10.    draw calls...
11.  end_render_pass
12.  blit + egui render pass
13. end_command_buffer
14. queue_submit (unchanged)
15. present
```

No new fences, semaphores, or submission changes.

---

## Phase 6: Particle System Example

Tutorial-matching particle system that validates the full pipeline.

### New files:
- `shaders/source/particles.compute.slang` — update positions from velocities
- `shaders/source/particle_render.shader.slang` — render particles from SSBO
- `examples/particles.rs` — game implementation

### Target API:

```rust
fn setup(renderer: &mut Renderer) -> Result<Self> {
    let particles = renderer.create_ping_pong_buffer::<Particle>(PARTICLE_COUNT)?;
    // write initial data to both sides...

    let compute_pipeline = renderer.create_compute_pipeline(
        particles_compute::Shader::init().pipeline_config(
            particles_compute::Resources {
                params: &sim_params,
                particles_in: particles.read_buffer(),
                particles_out: particles.write_buffer(),
            }
        )
    )?;
    // render pipeline setup...
}

fn draw(&mut self, mut frame: FrameRenderer) -> Result<(), DrawError> {
    // Compute pass (before render pass, same command buffer)
    frame.dispatch(&self.compute_pipeline, PARTICLE_COUNT / 256, 1, 1);

    self.particles.swap(); // now read_buffer() returns what compute just wrote

    frame.memory_barrier(
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::PipelineStageFlags::VERTEX_SHADER,
        vk::AccessFlags::SHADER_WRITE,
        vk::AccessFlags::SHADER_READ,
    );

    // Graphics pass
    frame.draw_vertex_count(&self.render_pipeline, PARTICLE_COUNT * 6, |gpu| {
        gpu.write_uniform(&mut self.sim_params, SimParams { delta_time: 0.016 });
    })
}
```

---

## Phase 7: Hot Reload

### Files: `src/shader_watcher.rs`, `src/renderer.rs`

**7A. Watch compute shaders**

Extend `shader_watcher` to also watch `*.compute.slang` files alongside `*.shader.slang`.

**7B. Branch on `PipelineKind` in hot reload**

`check_for_shader_recompile()` and `try_shader_recompile()` currently call `create_graphics_pipeline()` unconditionally. With `PipelineKind` on `RendererPipeline`, hot reload must inspect the kind and branch:

- `PipelineKind::Graphics { .. }` → existing path: `ShaderPipelineLayout::create_from_atlas()` + `create_graphics_pipeline()`
- `PipelineKind::Compute { .. }` → new path: `ShaderPipelineLayout::create_from_compute_atlas()` + `vkCreateComputePipelines`

The deferred cleanup of old pipelines (`old_pipelines` vec) works the same for both — it stores `vk::Pipeline` + `vk::PipelineLayout` + descriptor set layouts regardless of kind.

---

## Optional: Consolidate `descriptor_sets_for_frame`

`descriptor_sets_for_frame()` and `picking_descriptor_sets_for_frame()` are nearly identical. With compute adding a third caller, consider extracting this into a method on `RendererPipeline`:

```rust
impl RendererPipeline {
    fn descriptor_sets_for_frame(&self, frame: usize) -> &[vk::DescriptorSet] {
        let sets_per_frame = self.layout.descriptor_set_layouts.len();
        self.descriptor_sets.chunks(sets_per_frame).nth(frame).unwrap()
    }
}
```

This is optional cleanup, not a blocker for compute support.

---

## Mapping to Future Render Graph

| This Implementation | Render Graph v1 (01_flame) | Render Graph v2 (02_async) |
|---|---|---|
| `dispatch()` | `graph.execute()` calls `cmd_dispatch()` | Same, but into compute CB |
| `memory_barrier()` | Auto-inserted from resource dependencies | Replaced by semaphore at sim/render boundary |
| `PingPongBufferHandle<T>` | Foundation for simulation state | Automatic swap in graph frame management |
| `ComputeShaderAtlasEntry` | Config passed to `.compute()` in graph | Same |
| `create_compute_pipeline()` | Graph creates pipelines internally | Same |
| Same CB, same queue | Graph records all stages into same CB | Graph splits into 2 CBs + semaphores |

The progression:
1. **Now**: `dispatch()` + `memory_barrier()` — user manages ordering and barriers manually
2. **Render graph v1**: `graph.execute()` records dispatches and barriers into the same CB — wraps these primitives, automates barriers
3. **Render graph v2**: `graph.execute()` splits simulation into a separate CB with semaphores — the graph manages this, primitives unchanged

---

## Pre-implementation: Update 02_explicit_parallelism.md

Add a section noting that the async compute implementation should use **timeline semaphores** instead of binary semaphores + fences. The renderer already targets Vulkan 1.3 (where timeline semaphores are core) and enables `PhysicalDeviceTimelineSemaphoreFeatures` in debug builds (`src/renderer.rs:1855-1856`). This should be enabled unconditionally.

Update the `Command Buffer Management` section (line ~230) and `ParallelExecution` struct (line ~236) to replace:

```rust
struct ParallelExecution {
    graphics_cb: vk::CommandBuffer,
    compute_cb: vk::CommandBuffer,
    graphics_done: vk::Semaphore,   // binary
    compute_done: vk::Semaphore,    // binary
}
```

with:

```rust
struct ParallelExecution {
    graphics_cb: vk::CommandBuffer,
    compute_cb: vk::CommandBuffer,
    timeline: vk::Semaphore,        // single timeline semaphore
    timeline_value: u64,            // monotonically increasing
}
```

Key advantages over the current binary semaphore + fence design in that doc:
- One timeline semaphore replaces 2 binary semaphores + 2 fences per frame-in-flight
- CPU wait (`vkWaitSemaphores`) replaces fence wait — no separate fence objects
- No reset needed — counter increments monotonically
- Matches the Vulkan compute shader tutorial's recommended approach

Also update the `Simulation-Focused Parallelism` section (line ~1326) to note that the cross-frame simulation/rendering pattern benefits from timeline semaphores: compute signals value N, graphics waits on value N, next frame's compute waits on value N before reusing the CB and signals N+1.

---

## Implementation Order

Each step includes automated checks (`cargo check --all`, `just test`) and a **manual verification pause** where the user visually confirms existing examples still work before proceeding.

1. Phase 0 — Queue family validation (one assertion)
   - `cargo check --all`

2. Phase 3A — Compute marker type (trivial, additive)
   - `cargo check --all`

3. Phase 1 — Compute shader compilation
   - `cargo check --all`, `just test`
   - **PAUSE**: run an existing example (`just dev basic_triangle` or similar), confirm it renders correctly. Shader compilation changes could affect the build system.

4. Phase 2 — Compute code generation
   - `just shaders`, `just test`, `cargo check --all`
   - **PAUSE**: run an existing example, confirm no regressions. Code generation and template changes touch the build pipeline that all shaders depend on.

5. Phase 3B-D — Compute pipeline creation
   - `cargo check --all`
   - **PAUSE**: run an existing example. `RendererPipeline` and `PipelineKind` changes touch the pipeline storage that graphics pipelines use.

6. Phase 4 — PingPongBuffer type
   - `cargo check --all`
   - **PAUSE**: run an existing example. Storage buffer and descriptor set changes could affect existing buffer handling.

7. Phase 5 — FrameRenderer dispatch + barrier API
   - `cargo check --all`
   - **PAUSE**: run an existing example. This touches `FrameRenderer` and the command buffer recording path — the most likely place for regressions in existing draw calls.

8. Phase 6 — Particle example (end-to-end validation)
   - `just shaders`, `cargo check --all`
   - **PAUSE**: run `just dev particles` and confirm particles render and move. Then run an existing example to confirm coexistence.

9. Phase 7 — Hot reload
   - `cargo check --all`
   - **PAUSE**: run `just dev particles`, edit `particles.compute.slang` while running, confirm hot reload works. Then run an existing example with shader hot reload to confirm no regression.
