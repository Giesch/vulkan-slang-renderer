# FLAME-Inspired Render Graph

## Overview

A high-level API for organizing compute and graphics shaders into a unified execution graph with explicit resource dependencies. Inspired by [FLAME GPU](https://flamegpu.com/), a framework for GPU-accelerated agent-based simulations that organizes CUDA kernels into layers with automatic synchronization.

This render graph sits on top of the low-level compute shader support (see `PLAN.md`) and provides:

1. **Unified pipeline declaration** - Compute dispatches and render passes in one structure
2. **Explicit resource dependencies** - Declare what each stage reads and writes
3. **Automatic synchronization** - Barriers and layout transitions inserted automatically
4. **GPU-driven execution** - Indirect dispatch and draw from GPU buffers

---

## The Problem

Vulkan requires explicit synchronization between operations:

- **Pipeline barriers** between compute stages
- **Memory barriers** when compute writes and graphics reads
- **Image layout transitions** when usage changes (storage image → sampled texture)
- **Render pass dependencies** for attachment access

Getting this wrong causes:
- Validation errors
- Rendering artifacts (reading stale data)
- GPU hangs
- Subtle timing-dependent bugs

The low-level API requires users to understand these concepts:

```rust
frame.dispatch(blur_h_pipeline, 8, 8, 1);

// User must know to insert this barrier
frame.pipeline_barrier(
    vk::PipelineStageFlags::COMPUTE_SHADER,
    vk::PipelineStageFlags::COMPUTE_SHADER,
    &[vk::MemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)],
    &[],
    &[],
);

frame.dispatch(blur_v_pipeline, 8, 8, 1);

// Another barrier for compute → graphics
frame.pipeline_barrier(
    vk::PipelineStageFlags::COMPUTE_SHADER,
    vk::PipelineStageFlags::FRAGMENT_SHADER,
    // ... plus image layout transition
);

frame.begin_render_pass(...);
```

---

## The Solution: Shader Graph

Declare the execution graph once, with resource dependencies explicit in the structure:

```rust
let post_process = ShaderGraph::builder()
    .compute(blur_h::config(...), [width/16, height/16, 1])
    .compute(blur_v::config(...), [width/16, height/16, 1])
    .render_pass(RenderPassConfig { ... })
        .draw(composite::config(...))
    .end_render_pass()
    .build(renderer)?;
```

The graph builder:
1. Analyzes which resources each stage reads/writes (from generated `Resources` struct)
2. Determines dependencies between stages
3. Inserts appropriate barriers and layout transitions
4. Validates that all reads have corresponding prior writes

Execution is a single call:

```rust
fn draw(&mut self, mut frame: FrameRenderer) -> Result<(), DrawError> {
    frame.execute(&self.post_process)?;
    Ok(())
}
```

---

## Benefits

### 1. Correctness by Construction

Dependencies are derived from resource usage. If stage B reads a resource that stage A writes, the barrier is automatic. You cannot forget it.

### 2. Self-Documenting Data Flow

The graph structure shows how data flows through your frame:

```rust
ShaderGraph::builder()
    .compute(particle_sim::config(...))      // particles buffer: write
    .compute(particle_cull::config(...))     // particles: read, visible: write
    .render_pass(...)
        .draw(particle_render::config(...))  // visible: read
    .end_render_pass()
```

### 3. Validation at Build Time

The graph can detect errors before rendering:
- Reading a resource that was never written
- Writing to a resource that's still being read
- Missing render pass attachments

### 4. GPU-Driven Pipelines

Indirect dispatch and draw integrate naturally:

```rust
.compute(count_work::config(...), [1, 1, 1])
.compute_indirect(process_work::config(...), &dispatch_args_buffer)
```

### 5. Optimization Opportunities

The graph has global knowledge to:
- Merge compatible barriers
- Batch layout transitions
- Potentially reorder independent stages
- Merge render passes with compatible attachments

### 6. Reduced Boilerplate

No manual barrier calls, no layout transition tracking, no stage flag lookups.

---

## FLAME GPU Inspiration

FLAME GPU organizes agent functions (CUDA kernels) into **layers**:

```
Layer 0: [movement_kernel, feeding_kernel]     <- run in parallel
         ─────────── barrier ───────────
Layer 1: [reproduction_kernel]                 <- depends on layer 0
         ─────────── barrier ───────────
Layer 2: [death_kernel, visualization_kernel]  <- depends on layer 1
```

- Functions within a layer can run in parallel
- Layers execute sequentially with implicit barriers between them
- Dependencies are declared via agent state read/write declarations

Our render graph follows this model:
- Shader stages are organized with explicit resource dependencies
- The graph determines which stages can potentially overlap
- Barriers are inserted at dependency boundaries

---

## Comparison with Frostbite Frame Graph

Frostbite's Frame Graph (presented at [GDC 2017](https://www.gdcvault.com/play/1024612/FrameGraph-Extensible-Rendering-Architecture-in) by Yuriy O'Donnell) is the industry reference for render graph implementations. This section compares our design to identify gaps and inform future development.

### Feature Comparison

| Feature | Frostbite | Our Design | Gap |
|---------|-----------|------------|-----|
| **Transient resources** | Automatic - declare in graph, system allocates | Manual - create resources externally | **Major gap** |
| **Memory aliasing** | Automatic - non-overlapping resources share memory (50%+ savings) | None | **Major gap** |
| **Async compute** | First-class - `ExecuteNextAsync()` routes to parallel queue | Not addressed | **Moderate gap** |
| **Pass culling** | Automatic - unused passes removed | Not addressed | **Minor gap** |
| **Per-frame graph rebuild** | Yes - built from scratch each frame | Built once at setup | **Design decision** |
| **Barrier insertion** | Automatic | Automatic | Equivalent |
| **Resource state tracking** | Automatic | Automatic | Equivalent |

### Major Gaps

#### 1. Transient Resource System

Frostbite's most impactful feature. Instead of manually creating resources:

```rust
// Our design: create resources manually
let blur_temp = renderer.create_storage_image(width, height, format)?;

let graph = ShaderGraph::builder()
    .compute(blur_h::config(..., output: &blur_temp))
    .compute(blur_v::config(..., input: &blur_temp))
    .build()?;
```

Frostbite declares resource requirements and the system allocates:

```cpp
// Frostbite: declare resource needs, system allocates
builder.createTexture("blur_temp", width, height, format);

// Memory automatically aliased with other non-overlapping resources
// blur_temp might share memory with gbuffer (used earlier, no longer needed)
```

**Why this matters:**
- G-buffer textures (albedo, normal, position) are only needed until lighting pass
- Shadow maps only needed until shadow sampling
- Post-process intermediates only needed briefly
- Frostbite reports **50%+ memory savings** from aliasing

**Impact:** Without this, users must manually manage all resources and cannot benefit from memory reuse.

#### 2. Memory Aliasing

Related to transient resources. Frostbite tracks resource lifetimes:

```
Frame timeline:
  [====gbuffer====]
                    [====lighting====]
                                       [==blur_h==][==blur_v==]

Memory layout (with aliasing):
  |-- gbuffer memory --|-- reused for blur_temp --|
```

Resources with non-overlapping lifetimes share the same GPU memory. This requires:
- Knowing exactly when each resource is first/last used
- Placing aliasing barriers (different from regular barriers)
- Memory heap management with sub-allocation

#### 3. Async Compute

Frostbite runs compute work on a separate queue *in parallel* with graphics:

```
Graphics queue: [--shadow pass--][--gbuffer--][--lighting--]
Compute queue:  [--SSAO compute--------------|
                                              ^-- sync point
```

Our design assumes single-queue execution. Adding async compute requires:
- Multiple command buffers
- Semaphore-based synchronization (not just barriers)
- Careful dependency tracking across queues
- Handling "hidden edge cases" (Frostbite notes this is difficult to fully automate)

### Minor Gaps

#### 4. Pass Culling

If a debug visualization pass exists but nothing reads its output, Frostbite skips it:

```cpp
builder.addPass("debug_wireframe", ...);  // defined but unused
// Automatically culled during compile if no other pass reads from it
```

Useful for:
- Debug visualizations toggled off
- Conditional features (ray tracing fallback paths)
- Platform-specific passes

#### 5. Per-Frame vs Once-at-Setup

Frostbite rebuilds the graph every frame, enabling:

```cpp
if (settings.bloom_enabled) {
    builder.addPass("bloom", ...);
}
if (debug_mode) {
    builder.addPass("wireframe_overlay", ...);
}
```

Our design implies building once at setup. This affects:
- Dynamic feature toggles
- Resolution changes mid-session
- Debug visualization toggling

### Use Cases That Become Difficult

#### AAA Open World Games

These stress memory budgets with:
- Large shadow cascades (4+ cascades at high resolution)
- G-buffer at native 4K resolution
- Multiple post-process stages (bloom, DOF, motion blur, TAA)
- Reflection probes and volumetrics

Without memory aliasing, you need enough VRAM for all resources simultaneously, even when they don't overlap temporally.

#### Console Development

Consoles have fixed memory budgets and special fast memory regions (Xbox ESRAM/Series X, PS4 garlic/onion memory). Frostbite's transient system automatically:
- Places hot resources in fast memory when beneficial
- Aliases aggressively to fit in limited budgets
- Handles platform-specific memory hierarchies

#### VR Rendering

VR renders two views (sometimes four with fixed foveated rendering). Intermediate resources per-view add up quickly. Memory aliasing is nearly required to hit memory budgets at 90fps.

#### Dynamic Quality Scaling

Games that adjust resolution or quality settings dynamically benefit from per-frame graph rebuilding. Our "build once" approach would require:
- Multiple pre-built graphs for different quality levels
- Graph recreation on settings change
- Or a different pattern entirely

### What Our Design Handles Well

Despite the gaps, our design excels at:

1. **Learning/indie projects** - Simpler mental model, explicit resource management, easier to debug
2. **Fixed pipelines** - If your render pipeline doesn't change at runtime, build-once is fine
3. **Compute-heavy workloads** - The FLAME GPU inspiration shines for GPU simulations
4. **Correctness** - Automatic barriers prevent synchronization bugs
5. **Incremental adoption** - Can mix graph execution with manual commands

### Potential Additions to Close Gaps

#### Transient Resources (High Value)

```rust
let graph = ShaderGraph::builder()
    .transient_image("blur_temp", width, height, vk::Format::R16G16B16A16_SFLOAT)

    .compute(blur_h::config(blur_h::Resources {
        output: Transient("blur_temp"),  // reference by name
    }))
    .compute(blur_v::config(blur_v::Resources {
        input: Transient("blur_temp"),
    }))

    .build()?;  // allocates blur_temp, potentially aliased with other transients
```

#### Async Compute (Medium Value)

```rust
.async_compute_begin()  // switch to compute queue
    .compute(ssao::config(...))
.async_compute_end()    // sync back to graphics queue

// Or mark individual passes
.compute(ssao::config(...))
    .on_async_queue()
```

#### Per-Frame Building (Design Change)

```rust
fn draw(&mut self, mut frame: FrameRenderer) -> Result<()> {
    let graph = ShaderGraph::builder()
        .compute_if(self.bloom_enabled, bloom::config(...))
        .render_pass(...)
        .build_transient()?;  // lightweight build, uses arena allocator

    frame.execute(&graph)?;
    Ok(())
}
```

### References

- [GDC Vault - FrameGraph: Extensible Rendering Architecture in Frostbite](https://www.gdcvault.com/play/1024612/FrameGraph-Extensible-Rendering-Architecture-in)
- [Frostbite Frame Graph PDF Slides](https://ubm-twvideo01.s3.amazonaws.com/o1/vault/gdc2017/Presentations/ODonnell_Yuriy_FrameGraph.pdf)
- [Render Graphs Overview - Riccardo Loggini](https://logins.github.io/graphics/2021/05/31/RenderGraphs.html)

---

## API Design

### Basic Structure

```rust
let graph = ShaderGraph::builder()
    // Compute stages
    .compute(config, dispatch_size)
    .compute_indirect(config, &indirect_buffer, offset)

    // Render pass with draws
    .render_pass(RenderPassConfig { ... })
        .draw(config)
        .draw_indexed(config)
        .draw_indirect(config, &indirect_buffer, offset)
    .end_render_pass()

    .build(renderer)?;
```

### Resource Tracking

Each shader's generated `Resources` struct exposes its dependencies:

```rust
// Generated code
impl<'a> blur_h::Resources<'a> {
    pub fn read_resources(&self) -> impl Iterator<Item = ResourceRef> {
        [ResourceRef::texture(self.input)].into_iter()
    }

    pub fn written_resources(&self) -> impl Iterator<Item = ResourceRef> {
        [ResourceRef::storage_image(self.output)].into_iter()
    }
}
```

The graph builder collects these automatically when you add a stage.

### Render Pass Configuration

```rust
RenderPassConfig {
    color_attachments: &[
        Attachment {
            image: &color_buffer,
            load_op: LoadOp::Clear([0.0, 0.0, 0.0, 1.0]),
            store_op: StoreOp::Store,
        },
    ],
    depth_attachment: Some(Attachment {
        image: &depth_buffer,
        load_op: LoadOp::Clear(1.0),
        store_op: StoreOp::DontCare,
    }),
}
```

### Indirect Commands

For GPU-driven workloads:

```rust
#[repr(C)]
#[derive(Clone, Copy, Std430)]
pub struct DispatchIndirectCommand {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Std430)]
pub struct DrawIndirectCommand {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub first_vertex: u32,
    pub first_instance: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Std430)]
pub struct DrawIndexedIndirectCommand {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
    pub first_instance: u32,
}
```

---

## Code Examples

### Example 1: Post-Processing Chain

```rust
let post_process = ShaderGraph::builder()
    // Horizontal blur (compute)
    .compute(
        blur_h::config(blur_h::Resources {
            params: &blur_params,
            input: &scene_texture,    // reads
            output: &blur_temp,       // writes
        }),
        [width / 16, height / 16, 1],
    )

    // Vertical blur (compute)
    .compute(
        blur_v::config(blur_v::Resources {
            params: &blur_params,
            input: &blur_temp,        // reads (written above)
            output: &blur_output,     // writes
        }),
        [width / 16, height / 16, 1],
    )

    // Composite (graphics)
    .render_pass(RenderPassConfig {
        color_attachments: &[Attachment::new(&swapchain).clear([0.0, 0.0, 0.0, 1.0])],
        depth_attachment: None,
    })
        .draw(composite::config(composite::Resources {
            scene: &scene_texture,
            blur: &blur_output,       // reads (written above)
            vertices: fullscreen_quad(),
            indices: quad_indices(),
        }))
    .end_render_pass()

    .build(renderer)?;
```

**Barriers inserted automatically:**
1. After blur_h compute → before blur_v compute (storage image barrier)
2. After blur_v compute → before composite render pass (layout transition + barrier)

### Example 2: GPU-Driven Particle System

```rust
let particles = ShaderGraph::builder()
    // Simulate all particles
    .compute(
        simulate::config(simulate::Resources {
            particles: &particle_buffer,    // read + write
            params: &sim_params,
        }),
        [MAX_PARTICLES / 64, 1, 1],
    )

    // Compact live particles, write indirect draw args
    .compute(
        compact::config(compact::Resources {
            particles: &particle_buffer,    // reads
            live_indices: &live_buffer,     // writes
            draw_args: &draw_indirect,      // writes
        }),
        [MAX_PARTICLES / 64, 1, 1],
    )

    // Render only live particles (GPU determines count)
    .render_pass(RenderPassConfig {
        color_attachments: &[Attachment::new(&swapchain).clear([0.0, 0.0, 0.1, 1.0])],
        depth_attachment: Some(Attachment::new(&depth).clear(1.0)),
    })
        .draw_indirect(
            render::config(render::Resources {
                particles: &particle_buffer,
                live_indices: &live_buffer,
                camera: &camera_ubo,
            }),
            &draw_indirect,
            0,
        )
    .end_render_pass()

    .build(renderer)?;
```

### Example 3: Cascaded Operations with Indirect Dispatch

```rust
let adaptive_processing = ShaderGraph::builder()
    // Count items needing processing, write dispatch size
    .compute(
        count_work::config(count_work::Resources {
            items: &item_buffer,
            dispatch_args: &dispatch_indirect,  // writes
        }),
        [1, 1, 1],  // single workgroup
    )

    // Process items (dispatch count from GPU)
    .compute_indirect(
        process::config(process::Resources {
            items: &item_buffer,
            results: &result_buffer,
        }),
        &dispatch_indirect,
        0,
    )

    .build(renderer)?;
```

### Example 4: Multiple Render Passes

```rust
let deferred = ShaderGraph::builder()
    // G-buffer pass
    .render_pass(RenderPassConfig {
        color_attachments: &[
            Attachment::new(&albedo_buffer).clear([0.0; 4]),
            Attachment::new(&normal_buffer).clear([0.0; 4]),
            Attachment::new(&position_buffer).clear([0.0; 4]),
        ],
        depth_attachment: Some(Attachment::new(&depth).clear(1.0)),
    })
        .draw(gbuffer::config(...))
    .end_render_pass()

    // Lighting pass (compute, reads G-buffer)
    .compute(
        lighting::config(lighting::Resources {
            albedo: &albedo_buffer,      // reads
            normal: &normal_buffer,      // reads
            position: &position_buffer,  // reads
            output: &lit_buffer,         // writes
            lights: &light_buffer,
        }),
        [width / 16, height / 16, 1],
    )

    // Final composite
    .render_pass(RenderPassConfig {
        color_attachments: &[Attachment::new(&swapchain).clear([0.0; 4])],
        depth_attachment: None,
    })
        .draw(tonemap::config(tonemap::Resources {
            hdr_input: &lit_buffer,
            ...
        }))
    .end_render_pass()

    .build(renderer)?;
```

---

## Resource State Transitions

The graph tracks resource states and inserts transitions:

| Previous State | Next State | Barrier/Transition |
|----------------|------------|-------------------|
| ComputeWrite | ComputeRead | Compute → Compute memory barrier |
| ComputeWrite | FragmentRead | Compute → Fragment memory barrier |
| ComputeWrite | VertexRead | Compute → Vertex memory barrier |
| ColorAttachment | ComputeRead | End render pass + layout transition (COLOR_ATTACHMENT → GENERAL) |
| ColorAttachment | FragmentRead | End render pass + layout transition (COLOR_ATTACHMENT → SHADER_READ_ONLY) |
| ComputeWrite (image) | ColorAttachment | Layout transition (GENERAL → COLOR_ATTACHMENT) |
| ComputeWrite (image) | FragmentRead | Layout transition (GENERAL → SHADER_READ_ONLY) |
| DepthAttachment | FragmentRead | End render pass + layout transition |
| Undefined | ColorAttachment | Layout transition (UNDEFINED → COLOR_ATTACHMENT) |

---

## Execution Model

### Graph Building (once at setup)

```rust
impl ShaderGraph {
    fn build(builder: ShaderGraphBuilder) -> Result<Self> {
        // 1. Collect all stages and their resource usage
        let stages = builder.stages;

        // 2. Build dependency graph
        let mut dependencies: Vec<Vec<usize>> = vec![vec![]; stages.len()];
        for (idx, stage) in stages.iter().enumerate() {
            for read_resource in stage.read_resources() {
                if let Some(writer_idx) = find_last_writer(read_resource, &stages[..idx]) {
                    dependencies[idx].push(writer_idx);
                }
            }
        }

        // 3. Compute barriers needed at each stage boundary
        let barriers = compute_barriers(&stages, &dependencies);

        // 4. Validate: all reads have writers, no cycles, etc.
        validate(&stages, &dependencies)?;

        Ok(ShaderGraph { stages, barriers })
    }
}
```

### Graph Execution (each frame)

```rust
impl FrameRenderer<'_> {
    pub fn execute(&mut self, graph: &ShaderGraph) -> Result<()> {
        for (idx, stage) in graph.stages.iter().enumerate() {
            // Insert barriers before this stage
            for barrier in &graph.barriers_before[idx] {
                self.insert_barrier(barrier);
            }

            // Execute the stage
            match stage {
                Stage::Compute { pipeline, dispatch } => {
                    self.bind_compute_pipeline(pipeline);
                    self.bind_descriptors(pipeline);
                    match dispatch {
                        Dispatch::Direct(x, y, z) => self.cmd_dispatch(*x, *y, *z),
                        Dispatch::Indirect(buffer, offset) => {
                            self.cmd_dispatch_indirect(buffer, *offset)
                        }
                    }
                }
                Stage::RenderPass { config, draws } => {
                    self.cmd_begin_render_pass(config);
                    for draw in draws {
                        self.bind_graphics_pipeline(draw.pipeline);
                        self.bind_descriptors(draw.pipeline);
                        match &draw.dispatch {
                            DrawDispatch::Direct { vertices, indices } => { ... }
                            DrawDispatch::Indirect { buffer, offset } => { ... }
                        }
                    }
                    self.cmd_end_render_pass();
                }
            }
        }
        Ok(())
    }
}
```

---

## Open Questions

### API Design

1. **Graph mutability**: Should graphs be immutable after building, or support dynamic updates?
   - Immutable: simpler, can pre-bake all barriers
   - Mutable: more flexible, but harder to optimize

2. **Resource rebinding**: Can the same graph structure be executed with different resources?
   ```rust
   // Same blur graph, different input/output each frame?
   graph.rebind("input", &frame_texture)?;
   frame.execute(&graph)?;
   ```

3. **Conditional stages**: How to skip stages at runtime?
   ```rust
   .compute_if(|| settings.bloom_enabled, bloom::config(...))
   ```
   Or should this be separate graphs selected at runtime?

4. **Dynamic dispatch sizes**: For non-indirect cases, how to specify runtime sizes?
   ```rust
   .compute(config, |frame| [frame.width / 16, frame.height / 16, 1])
   ```

5. **Multiple queues**: Should the graph support async compute on separate queue?
   - Significantly more complex
   - Requires semaphore-based synchronization
   - Probably a future extension

### Resource Handling

6. **Transient resources**: Resources that exist only within the graph?
   ```rust
   .with_transient_image("blur_temp", width, height, format)
   .compute(blur_h::config(...))  // writes to "blur_temp"
   .compute(blur_v::config(...))  // reads from "blur_temp"
   // blur_temp memory can be reused after graph completes
   ```

7. **External resources**: Resources written outside the graph?
   ```rust
   .external_write(&buffer, Stage::Compute)  // "assume this was written by compute"
   .compute(reader::config(...))              // can now read it
   ```

8. **Read-modify-write**: Same resource read and written by one stage?
   - Common for simulation (particles read + write same buffer)
   - Need to handle ping-pong buffering or in-place updates

### Render Passes

9. **Subpasses**: Support Vulkan subpasses for tile-based GPUs?
   ```rust
   .render_pass(...)
       .subpass()
           .draw(gbuffer::config(...))
       .next_subpass()
           .draw(lighting::config(...))  // input attachment from previous
   .end_render_pass()
   ```

10. **Multisampling resolve**: How to express MSAA resolve operations?

11. **Render pass merging**: Should adjacent compatible render passes auto-merge?

### Validation & Debugging

12. **Debug visualization**: Generate a graph diagram (DOT/graphviz)?
    ```rust
    println!("{}", graph.to_dot());
    ```

13. **Error messages**: How to report "resource X read in stage Y but never written"?
    - Need good stage names for debugging

14. **Performance warnings**: Detect potentially slow patterns?
    - Unnecessary layout transitions
    - Barriers that could be merged

### Integration

15. **Escape hatch**: How to mix graph execution with manual commands?
    ```rust
    frame.execute(&graph_part_1)?;
    frame.do_something_manual();
    frame.execute(&graph_part_2)?;
    ```

16. **Composition**: Nested graphs or graph fragments?
    ```rust
    let blur = blur_subgraph(...);
    let main = ShaderGraph::builder()
        .include(&blur)
        .render_pass(...)
    ```

17. **Hot reload**: When a shader reloads, can the graph update in place?

---

## Implementation Phases

### Phase 1: Core Graph Structure
- `ShaderGraph` and `ShaderGraphBuilder` types
- Compute stage support with direct dispatch
- Basic barrier insertion between compute stages
- `frame.execute(&graph)` API

### Phase 2: Render Pass Integration
- `render_pass()` / `end_render_pass()` builder methods
- Draw commands within render passes
- Compute ↔ graphics barriers and layout transitions

### Phase 3: Indirect Execution
- `compute_indirect()` with dispatch from buffer
- `draw_indirect()` and `draw_indexed_indirect()`
- Indirect buffer dependency tracking

### Phase 4: Validation & Debugging
- Build-time validation (missing writes, cycles)
- Debug graph visualization
- Runtime validation in debug builds

### Phase 5: Advanced Features (Future)
- Transient resources
- Conditional execution
- Subpass support
- Multi-queue async compute

---

## Relationship to PLAN.md

This render graph builds on the compute shader support in `PLAN.md`:

- **PLAN.md Phase 1-2**: Resource access reflection (needed for tracking reads/writes)
- **PLAN.md Phase 3-7**: Basic compute support (graph wraps these primitives)
- **PLAN.md Phase 8-9**: Barrier helpers (graph generates these automatically)

The render graph is a higher-level abstraction. Users can choose:
- **Low-level API**: Manual dispatch, barriers, render passes (full control)
- **Shader Graph**: Declarative graph with automatic synchronization (easier, less error-prone)

Both APIs will coexist, and the graph implementation uses the low-level primitives internally.
