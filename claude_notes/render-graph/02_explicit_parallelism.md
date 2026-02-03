# Explicit Parallelism and Async Compute

## Overview

This document explores API designs for expressing GPU parallelism in the render graph, specifically async compute - running compute work on a separate queue in parallel with graphics work.

This is a **future consideration** for the render graph design (see `flame_render_graph.md`). The base compute shader support (`PLAN.md`) assumes single-queue execution.

---

## Background: How Async Compute Works

Modern GPUs expose multiple hardware queues:

| Queue Type | Vulkan | Can Execute |
|------------|--------|-------------|
| Graphics | `VK_QUEUE_GRAPHICS_BIT` | Graphics, compute, transfer |
| Compute | `VK_QUEUE_COMPUTE_BIT` | Compute, transfer |
| Transfer | `VK_QUEUE_TRANSFER_BIT` | Transfer only (DMA) |

When work is submitted to different queues, the GPU *may* execute them in parallel if:
- They use different hardware units (shader cores vs fixed-function rasterization)
- They have complementary bottlenecks (ALU-bound vs bandwidth-bound)
- Resources don't conflict

### Intra-Frame Parallelism

The primary use case is overlapping work **within a single frame**:

```
Frame N:
Graphics queue: [--shadow maps--][--gbuffer--][--lighting--][--post--]
Compute queue:       [--SSAO-----------------]
                                              ^-- sync point
```

Shadow map rendering is rasterization-bound (fixed-function hardware busy, shader cores underutilized). SSAO is compute-bound (shader cores busy). They can overlap because they use different hardware.

### Cross-Frame Parallelism

Work can also overlap **between frames**:

```
Frame N:   [--shadow--][--gbuffer--][--lighting--][--post--]
Frame N+1:                                   [--shadow--][--gbuffer--]
                                              ^-- start next frame early
```

Or overlapping current frame's post-process with next frame's simulation:

```
Frame N:   [--render--][--post-process--]
Frame N+1:        [--particle sim compute--][--render--]
```

### When Async Compute Helps

| Graphics Work | Good Compute Overlap | Reason |
|---------------|---------------------|--------|
| Shadow maps (rasterization-bound) | Heavy ALU compute | Different hardware units |
| Fill-rate limited passes | Low-bandwidth compute | Shader cores idle during fill |
| Geometry-heavy (vertex-bound) | Bandwidth-heavy compute | Vertex processing ≠ memory |

### When Async Compute Hurts

- Both tasks ALU-bound → fight for shader cores
- Both tasks bandwidth-bound → fight for memory
- Tasks thrash shared caches
- Sync overhead exceeds parallel gains
- GPU doesn't have true async (serializes anyway)

AMD notes async compute "can reduce performance when not used optimally."

---

## API Options

### Option A: No Async (Simplest)

Single queue execution. Users who need async drop to low-level Vulkan.

```rust
let graph = ShaderGraph::builder()
    .compute(ssao::config(...))      // runs first
    .render_pass(shadow_config)       // runs second
    .build()?;
```

**Pros:** Simple mental model, no sync complexity, predictable performance
**Cons:** Leaves performance on the table for advanced users

### Option B: Manual Async Hints

Stages can hint they're good async candidates. System decides whether to actually parallelize.

```rust
let graph = ShaderGraph::builder()
    .compute(ssao::config(...))
        .prefer_async()  // hint, not guarantee
    .render_pass(shadow_config)
    .build()?;
```

**Pros:** Simple API, system can apply heuristics
**Cons:** Non-deterministic behavior, hard to reason about performance

### Option C: Automatic with Heuristics

Graph analyzes bottleneck characteristics and decides automatically.

```rust
let graph = ShaderGraph::builder()
    .compute(ssao::config(...))
    .render_pass(shadow_config)
    .build()?;
// System profiles and decides
```

**Pros:** Zero user burden
**Cons:** Complex to implement well, magic behavior, hard to debug

### Option D: Explicit Parallel Sections

Users explicitly declare what should run in parallel.

```rust
let graph = ShaderGraph::builder()
    .parallel()
        .graphics_section()
            .render_pass(shadow_cascade_0)
            .render_pass(shadow_cascade_1)
        .compute_section()
            .compute(ssao::config(...))
            .compute(light_cull::config(...))
    .end_parallel()  // sync point

    .render_pass(lighting_config)  // after sync, can use SSAO result
    .build()?;
```

**Pros:** Explicit, predictable, user controls optimization
**Cons:** See detailed analysis below

---

## Option D: Detailed Analysis

### API Structure

```rust
impl ShaderGraphBuilder {
    /// Begin a parallel section where graphics and compute run simultaneously
    pub fn parallel(self) -> ParallelSectionBuilder { ... }
}

impl ParallelSectionBuilder {
    /// Define work for the graphics queue
    pub fn graphics_section(self) -> GraphicsSectionBuilder { ... }

    /// Define work for the async compute queue
    pub fn compute_section(self) -> ComputeSectionBuilder { ... }

    /// End parallel section, insert sync point
    pub fn end_parallel(self) -> ShaderGraphBuilder { ... }
}
```

### Execution Model

```rust
// User writes:
.parallel()
    .graphics_section()
        .render_pass(shadow_config)
    .compute_section()
        .compute(ssao::config(...))
.end_parallel()

// System generates:
// 1. Allocate command buffer for graphics queue
// 2. Allocate command buffer for compute queue
// 3. Record shadow pass to graphics CB
// 4. Record SSAO to compute CB
// 5. Submit graphics CB, signal semaphore A
// 6. Submit compute CB, signal semaphore B
// 7. Next stage waits on both semaphores
```

### Downsides

#### 1. User Must Understand Hardware Bottlenecks

The API makes it easy to declare parallelism but doesn't help users know when it's beneficial:

```rust
.parallel()
    .graphics_section()
        .render_pass(heavy_fragment_shader)  // ALU-bound
    .compute_section()
        .compute(heavy_math_compute)          // Also ALU-bound
.end_parallel()
// Result: SLOWER than sequential due to shader core contention
```

The API provides a footgun - it looks like an optimization but can hurt performance. Users must profile to know if it helps.

#### 2. Synchronization Complexity

What happens when resources cross the parallel boundary?

```rust
.parallel()
    .graphics_section()
        .render_pass(writes: &depth_buffer)
    .compute_section()
        .compute(reads: &depth_buffer)  // Problem!
.end_parallel()
```

Options for handling this:

| Approach | Behavior | Tradeoff |
|----------|----------|----------|
| Build-time error | Reject graph, require restructure | Safe but restrictive |
| Implicit sync | Insert wait, sections not truly parallel | Hidden perf cliff |
| Undefined behavior | Race condition | Unacceptable |

Recommendation: **Build-time error** with clear message explaining the conflict.

#### 3. Command Buffer Management

Each parallel section needs separate command buffers:

```rust
struct ParallelExecution {
    graphics_cb: vk::CommandBuffer,
    compute_cb: vk::CommandBuffer,
    graphics_done: vk::Semaphore,
    compute_done: vk::Semaphore,
}
```

Implementation complexity:
- Command buffer pools per queue family
- Semaphore allocation and recycling
- Multi-queue submission ordering
- Fence management for frame-in-flight

This is hidden from users but adds significant implementation burden.

#### 4. Nested Parallelism

Should this be allowed?

```rust
.parallel()
    .graphics_section()
        .parallel()  // Nested parallel?
            .graphics_section()
                ...
```

Recommendation: **Disallow** at the type level. `GraphicsSectionBuilder` doesn't have a `.parallel()` method.

#### 5. Unbalanced Sections

```rust
.parallel()
    .graphics_section()
        .render_pass(shadow_cascade_0)
        .render_pass(shadow_cascade_1)
        .render_pass(shadow_cascade_2)
        .render_pass(shadow_cascade_3)  // Total: ~8ms
    .compute_section()
        .compute(quick_cull)             // Total: ~0.5ms
.end_parallel()
```

The compute queue finishes in 0.5ms, then sits idle for 7.5ms waiting for graphics. The sync overhead may exceed the parallel benefit.

The API doesn't prevent or warn about unbalanced sections. Mitigation options:
- Documentation/best practices
- Runtime profiling with warnings
- Build-time heuristic estimates (difficult)

#### 6. Queue Availability

Not all GPUs have true async compute:

| Vendor | Async Compute Support |
|--------|----------------------|
| AMD GCN+ | Strong, dedicated compute units |
| NVIDIA | Varies, often same hardware with scheduling |
| Intel | Limited, may serialize |
| Mobile | Varies widely |

```rust
// On GPU without true async compute
.parallel()
    .graphics_section(...)
    .compute_section(...)
.end_parallel()
// May run sequentially with extra sync overhead = net negative
```

The API needs runtime capability detection:

```rust
impl ShaderGraph {
    pub fn build(builder: ShaderGraphBuilder, renderer: &Renderer) -> Result<Self> {
        let has_async = renderer.supports_async_compute();

        if !has_async {
            // Flatten parallel sections to sequential
            // Or warn user
        }
    }
}
```

#### 7. Debugging Difficulty

Parallel execution complicates debugging:

- **GPU captures** (RenderDoc, NSight) show interleaved work from both queues
- **Timing analysis** must account for overlap
- **Validation errors** may point to wrong queue/command buffer
- **Race conditions** are timing-dependent, may not reproduce

#### 8. Platform Performance Variance

The same code performs differently across GPUs:

```rust
.parallel()
    .graphics_section()
        .render_pass(shadows)
    .compute_section()
        .compute(ssao)
.end_parallel()
```

| Platform | Result |
|----------|--------|
| AMD RX 6800 | 15% faster (good async compute) |
| NVIDIA RTX 3080 | 5% faster (moderate benefit) |
| Intel Arc | 2% slower (sync overhead > parallel gain) |
| Apple M1 | Unknown (unified memory changes equation) |

Users may need per-platform tuning, which the API doesn't facilitate.

---

## Downsides Summary

| Downside | Severity | Mitigation |
|----------|----------|------------|
| Users must understand bottlenecks | High | Documentation, profiling tools, examples |
| Cross-section resource conflicts | High | Build-time validation with clear errors |
| Command buffer complexity | Medium | Implementation detail, hidden from users |
| Nested parallelism | Low | Disallow at type level |
| Unbalanced sections | Medium | Runtime profiling, warnings |
| Queue availability variance | Medium | Runtime detection, automatic fallback |
| Debugging difficulty | Medium | Better tooling, debug modes |
| Platform performance variance | High | Per-platform profiles, optional hints |

---

## Alternative: Hints with Automatic Scheduling

Instead of explicit sections, provide hints and let the system decide:

```rust
let graph = ShaderGraph::builder()
    .compute(ssao::config(...))
        .async_hint(AsyncHint::Prefer)    // "This is a good async candidate"
        .bottleneck(Bottleneck::ALU)       // "This is compute-bound"

    .render_pass(shadow_config)
        .bottleneck(Bottleneck::Rasterization)  // "This is raster-bound"

    .build(renderer)?;
```

The graph builder:
1. Analyzes resource dependencies
2. Identifies stages that *could* overlap (no conflicts)
3. Checks GPU capabilities
4. Uses bottleneck hints to predict benefit
5. Decides whether to parallelize

```rust
impl ShaderGraph {
    fn build(builder: ShaderGraphBuilder, renderer: &Renderer) -> Result<Self> {
        let async_candidates = find_parallelizable_stages(&builder.stages);

        for (stage_a, stage_b) in async_candidates {
            let benefit = estimate_parallel_benefit(
                stage_a.bottleneck,
                stage_b.bottleneck,
                renderer.gpu_profile(),
            );

            if benefit > THRESHOLD {
                schedule_parallel(stage_a, stage_b);
            }
        }

        Ok(...)
    }
}
```

**Pros:**
- System handles complexity
- Can incorporate profiling data over time
- Graceful fallback on unsupported hardware
- Users provide semantic hints, not scheduling decisions

**Cons:**
- Non-deterministic (harder to reason about)
- Complex implementation
- "Magic" behavior users can't predict
- Hints may be wrong

---

## Hybrid Approach

Combine explicit sections (for users who know what they're doing) with automatic fallback:

```rust
let graph = ShaderGraph::builder()
    // Explicit parallel section
    .parallel()
        .graphics_section()
            .render_pass(shadow_config)
        .compute_section()
            .compute(ssao::config(...))
    .end_parallel()

    // Automatic mode for the rest
    .compute(post_process::config(...))
    .render_pass(composite_config)

    .build_with_options(BuildOptions {
        async_compute: AsyncComputeMode::ExplicitOnly,  // or Automatic, or Disabled
        fallback_on_unsupported: true,
    })?;
```

This gives advanced users control while allowing simpler usage for others.

---

## Recommendation

For the initial render graph implementation:

1. **Start with Option A** (no async) - simpler, correct, predictable
2. **Design data structures** to support future async (don't paint ourselves into a corner)
3. **Add Option D** (explicit parallel) as an advanced feature once basics work
4. **Gather profiling data** from real usage before attempting automatic scheduling

The explicit API is honest about what's happening but requires user expertise. This matches how Frostbite approaches it - manual hints with careful heuristics, acknowledging that full automation is impractical.

---

## Open Questions

1. **Fallback behavior**: When async isn't supported, should explicit parallel sections error, warn, or silently flatten?

2. **Profiling integration**: Should the graph collect timing data to inform async decisions?

3. **Multi-frame parallelism**: Should the API support overlapping frame N's post-process with frame N+1's shadows?

4. **Transfer queue**: Should there be a third section type for DMA/upload work?

5. **Subgroup operations**: Some compute work benefits from running on the graphics queue (subgroup operations in fragment). How to express this?

---

## Scoped API with Profiling Guardrails

An alternative API design uses closures to define parallel sections. This provides natural boundaries for instrumentation and enables compile-time constraints.

### Scoped Closure API

Instead of builder method chaining with explicit `end_parallel()`:

```rust
// Scoped API - closures define boundaries
.parallel(|p| {
    p.compute_section(|c| {
        c.compute(ssao::config(...), [width/16, height/16, 1])
         .compute(light_cull::config(...), [1, 1, 1])
    })
    .graphics_section(|g| {
        g.render_pass(shadow_config)
            .draw(shadow_shader::config(...))
         .end_render_pass()
    })
})
```

### Automatic Timestamp Injection

The scope boundaries allow automatic GPU timestamp query injection:

```rust
impl<'a> ParallelScope<'a> {
    pub fn compute_section<F>(self, f: F) -> Self
    where
        F: FnOnce(ComputeSectionBuilder) -> ComputeSectionBuilder
    {
        let builder = ComputeSectionBuilder::new();

        // Inject timestamp before section
        let builder = builder.write_timestamp(TimestampId::ComputeStart);

        // User's work
        let builder = f(builder);

        // Inject timestamp after section
        let builder = builder.write_timestamp(TimestampId::ComputeEnd);

        self.add_compute_section(builder)
    }
}
```

### Imbalance Detection

At runtime, read back timestamps and compute section durations:

```rust
struct ParallelSectionMetrics {
    graphics_duration_ns: u64,
    compute_duration_ns: u64,
    sync_wait_ns: u64,  // time faster section waited for slower
}

impl ParallelSectionMetrics {
    fn imbalance_ratio(&self) -> f32 {
        let max = self.graphics_duration_ns.max(self.compute_duration_ns);
        let min = self.graphics_duration_ns.min(self.compute_duration_ns);
        if min == 0 { return f32::INFINITY; }
        max as f32 / min as f32
    }

    fn wasted_time_ns(&self) -> u64 {
        self.graphics_duration_ns.abs_diff(self.compute_duration_ns)
    }
}
```

### Dev-Time Warnings

In debug builds, after collecting metrics over N frames:

```rust
impl ShaderGraph {
    fn check_parallel_balance(&self, metrics: &[ParallelSectionMetrics]) {
        for (idx, m) in metrics.iter().enumerate() {
            let avg_imbalance = m.average_imbalance_ratio();
            let avg_wasted = m.average_wasted_time_ns();

            if avg_imbalance > 4.0 {
                tracing::warn!(
                    "Parallel section {} is unbalanced: \
                     graphics={:.2}ms, compute={:.2}ms (ratio {:.1}x). \
                     Consider moving work between sections or removing parallelism.",
                    idx,
                    m.avg_graphics_ms(),
                    m.avg_compute_ms(),
                    avg_imbalance,
                );
            }

            if avg_wasted > 1_000_000 {  // > 1ms wasted per frame
                tracing::warn!(
                    "Parallel section {} wastes {:.2}ms per frame waiting. \
                     Net benefit may be negative.",
                    idx,
                    avg_wasted as f64 / 1_000_000.0,
                );
            }
        }
    }
}
```

### Net-Negative Detection

Measure whether parallelism actually helped:

```rust
struct ParallelEfficiencyMetrics {
    parallel_total_ns: u64,      // wall time with parallelism
    sequential_estimate_ns: u64, // graphics + compute durations summed
    sync_overhead_ns: u64,       // semaphore wait cost
}

impl ParallelEfficiencyMetrics {
    fn speedup(&self) -> f32 {
        self.sequential_estimate_ns as f32 / self.parallel_total_ns as f32
    }

    fn is_beneficial(&self) -> bool {
        self.parallel_total_ns < self.sequential_estimate_ns
    }
}
```

Warning when parallelism hurts performance:

```rust
if !metrics.is_beneficial() {
    tracing::warn!(
        "Parallel section {} is SLOWER than sequential! \
         Parallel={:.2}ms, Sequential estimate={:.2}ms, \
         Sync overhead={:.2}ms. Consider removing .parallel().",
        idx,
        metrics.parallel_total_ms(),
        metrics.sequential_estimate_ms(),
        metrics.sync_overhead_ms(),
    );
}
```

### A/B Testing Mode

The scoped API enables automatic comparison between parallel and sequential execution:

```rust
let graph = ShaderGraph::builder()
    .parallel(|p| { ... })
    .build_with_options(BuildOptions {
        parallel_mode: ParallelMode::ABTest {
            frames_per_mode: 100,  // alternate every 100 frames
        },
    })?;
```

After running both modes, automatic report:

```
Parallel section 0 A/B test results (200 frames):
  Parallel mode:   avg 4.2ms, p99 5.1ms
  Sequential mode: avg 4.8ms, p99 5.5ms
  Speedup: 1.14x (parallel is 14% faster)
  Recommendation: Keep parallel
```

Or when parallelism hurts:

```
Parallel section 0 A/B test results (200 frames):
  Parallel mode:   avg 3.8ms, p99 4.9ms
  Sequential mode: avg 3.5ms, p99 4.2ms
  Speedup: 0.92x (parallel is 8% SLOWER)
  Recommendation: Remove .parallel(), sync overhead exceeds benefit
```

### Type-Level Constraints

The scoped closures prevent misuse at compile time:

```rust
// ComputeSectionBuilder doesn't have render_pass() - compile error
.parallel(|p| {
    p.compute_section(|c| {
        c.render_pass(...)  // Error: method not found
    })
})

// Can't nest parallel - GraphicsSectionBuilder has no .parallel() method
.parallel(|p| {
    p.graphics_section(|g| {
        g.parallel(...)  // Error: method not found
    })
})

// Must provide both sections - enforced by ParallelScope consuming self
.parallel(|p| {
    p.compute_section(|c| { ... })
    // Missing graphics_section - build() can require both were called
})
```

### Full Example

```rust
let graph = ShaderGraph::builder()
    // Non-parallel work
    .compute(early_cull::config(...), [64, 1, 1])

    // Parallel section with automatic profiling
    .parallel(|p| {
        p.graphics_section(|g| {
            g.render_pass(shadow_config)
                .draw(shadow_caster::config(...))
             .end_render_pass()
             .render_pass(shadow_config_cascade_2)
                .draw(shadow_caster::config(...))
             .end_render_pass()
        })
        .compute_section(|c| {
            c.compute(ssao::config(...), [width/16, height/16, 1])
             .compute(ssr::config(...), [width/16, height/16, 1])
        })
    })

    // After sync point, results from both sections available
    .render_pass(lighting_config)
        .draw(deferred_lighting::config(...))
    .end_render_pass()

    .build_with_options(BuildOptions {
        profiling: ProfilingMode::Enabled,
    })?;

// Query metrics after running
if let Some(metrics) = graph.parallel_section_metrics(0) {
    println!(
        "Section 0: {:.2}ms graphics, {:.2}ms compute, {:.1}x speedup",
        metrics.graphics_ms(),
        metrics.compute_ms(),
        metrics.speedup(),
    );
}
```

### Benefits of Scoped API

| Benefit | How Scopes Enable It |
|---------|---------------------|
| Automatic timestamp injection | Clear start/end boundaries from closure |
| Resource conflict detection | Closure captures show which resources used |
| Nested parallel prevention | Type system - inner builders lack `.parallel()` |
| A/B testing | Can swap implementation without changing user code |
| Compile-time validation | Scope types constrain allowed operations |
| Debug visualization | Scope structure maps to profiler UI hierarchy |
| Imbalance warnings | Duration comparison between paired sections |

### Implementation Considerations

The scoped API requires careful design of the builder types:

```rust
// Main builder has .parallel()
struct ShaderGraphBuilder { ... }

// ParallelScope only allows adding sections
struct ParallelScope<'a> { ... }

// Section builders are specialized
struct ComputeSectionBuilder { ... }   // only compute operations
struct GraphicsSectionBuilder { ... }  // only graphics operations

// Neither section builder has .parallel() - prevents nesting
```

Profiling data flows back through the graph:

```rust
struct ShaderGraph {
    stages: Vec<Stage>,
    parallel_sections: Vec<ParallelSectionInfo>,

    // Ring buffer of recent metrics per section
    metrics_history: Vec<VecDeque<ParallelSectionMetrics>>,
}

impl ShaderGraph {
    // Called after each frame's timestamps are available
    fn record_metrics(&mut self, frame_metrics: FrameMetrics) {
        for (idx, section_metrics) in frame_metrics.parallel_sections.iter().enumerate() {
            self.metrics_history[idx].push_back(*section_metrics);
            if self.metrics_history[idx].len() > HISTORY_SIZE {
                self.metrics_history[idx].pop_front();
            }
        }

        // Check for warnings periodically
        if self.frames_recorded % WARNING_CHECK_INTERVAL == 0 {
            self.check_parallel_balance();
        }
    }
}
```

---

## Ownership-Based Resource Safety

Rust's borrow checker can catch parallel resource conflicts at compile time. By using `&` for read access and `&mut` for write access in the generated `Resources` structs, invalid parallel access becomes a compile error.

### Borrow Checker Semantics Match Parallel Safety

| Situation | Borrows | Borrow Checker | Parallel Safety |
|-----------|---------|----------------|-----------------|
| Multiple readers | `&` + `&` | Allowed | Safe - no data race |
| Single writer | `&mut` alone | Allowed | Safe - exclusive access |
| Writer + reader | `&mut` + `&` | **Compile error** | Would be unsafe |
| Multiple writers | `&mut` + `&mut` | **Compile error** | Would be unsafe |

The borrow checker rules exactly match what's safe for parallel GPU execution.

### Current Design (No Ownership Safety)

```rust
// Generated Resources - everything is &, read/write not distinguished
pub struct Resources<'a> {
    pub input: &'a TextureHandle,        // read
    pub output: &'a StorageImageHandle,  // write (but same & type!)
}
```

Resource conflicts must be detected at graph build time (runtime error).

### New Design (Ownership-Based Safety)

```rust
// Generated Resources - & for read, &mut for write
pub struct Resources<'a> {
    pub input: &'a TextureHandle,            // read - immutable borrow
    pub output: &'a mut StorageImageHandle,  // write - mutable borrow
}
```

Now the borrow checker catches conflicts at compile time:

```rust
.parallel(|p| {
    p.graphics_section(|g| {
        g.render_pass(RenderPassConfig {
            depth_attachment: &mut depth_buffer,  // mutable borrow
            ..
        })
    })
    .compute_section(|c| {
        c.compute(ssao::Resources {
            depth: &depth_buffer,  // immutable borrow
            ..
        })
    })
})
```

```
error[E0502]: cannot borrow `depth_buffer` as immutable because it is
              also borrowed as mutable
  --> src/main.rs:45:20
   |
40 |     depth_attachment: &mut depth_buffer,
   |                       ----------------- mutable borrow occurs here
...
45 |     depth: &depth_buffer,
   |            ^^^^^^^^^^^^^ immutable borrow occurs here
```

### Valid Cases Still Compile

**Multiple readers (allowed):**

```rust
.parallel(|p| {
    p.graphics_section(|g| {
        g.draw(shader::Resources {
            shared_texture: &texture,  // & borrow
        })
    })
    .compute_section(|c| {
        c.compute(other::Resources {
            shared_texture: &texture,  // another & borrow - OK!
        })
    })
})
```

**Independent writes to different resources (allowed):**

```rust
.parallel(|p| {
    p.graphics_section(|g| {
        g.render_pass(RenderPassConfig {
            color: &mut color_buffer_a,  // &mut to resource A
        })
    })
    .compute_section(|c| {
        c.compute(blur::Resources {
            output: &mut color_buffer_b,  // &mut to resource B - OK!
        })
    })
})
```

**Sequential write-then-read (allowed):**

```rust
// Outside parallel section - sequential execution
.compute(pass1::Resources {
    output: &mut buffer,  // mutable borrow
})
// Borrow ends when closure/method call returns

.compute(pass2::Resources {
    input: &buffer,  // immutable borrow - OK, previous borrow ended
})
```

### Mapping Resource Access to Borrow Types

This aligns with the resource access reflection from `PLAN.md` Phase 1:

| Slang Type | ResourceAccess | Generated Rust |
|------------|---------------|----------------|
| `Texture2D<T>` | Read | `&'a TextureHandle` |
| `RWTexture2D<T>` | ReadWrite | `&'a mut StorageImageHandle` |
| `StructuredBuffer<T>` | Read | `&'a StorageBufferHandle<T>` |
| `RWStructuredBuffer<T>` | ReadWrite | `&'a mut StorageBufferHandle<T>` |
| `ConstantBuffer<T>` | Read | `&'a UniformBufferHandle<T>` |
| Color attachment | Write | `&'a mut TextureHandle` |
| Depth attachment | Write | `&'a mut DepthBufferHandle` |

The code generator already knows read vs write from Slang reflection - it just emits `&` vs `&mut` accordingly.

### Implementation Considerations

#### 1. Handles Must Not Be Copy

For `&mut` to provide safety, handles cannot be `Copy`:

```rust
// Without Copy, borrow checker tracks the handle
pub struct StorageImageHandle {
    index: usize,
    // Deliberately no: #[derive(Copy, Clone)]
}

// Users must pass references, can't accidentally duplicate
fn use_image(handle: &mut StorageImageHandle) { ... }
```

If handles were `Copy`, users could bypass safety:

```rust
// BAD: If Copy, this would compile but be unsafe
let handle_copy = *storage_image;  // Copy the handle
.parallel(|p| {
    p.graphics_section(|g| { /* use &mut storage_image */ })
    p.compute_section(|c| { /* use &handle_copy */ })  // Oops, same resource!
})
```

#### 2. Render Pass Attachments

Render pass configuration takes mutable borrows for written attachments:

```rust
pub struct RenderPassConfig<'a> {
    pub color_attachments: &'a [&'a mut TextureHandle],
    pub depth_attachment: Option<&'a mut DepthBufferHandle>,
    pub input_attachments: &'a [&'a TextureHandle],  // read-only
}
```

#### 3. Read-Modify-Write Resources

Some shaders read and write the same resource (e.g., particle simulation updating positions in place):

```rust
// Slang: RWStructuredBuffer<Particle> particles;
pub struct Resources<'a> {
    pub particles: &'a mut StorageBufferHandle<Particle>,  // read AND write
}
```

The `&mut` borrow covers both operations - the shader can internally read and write, and the borrow checker ensures no other shader accesses it in parallel.

#### 4. Temporary Mutable Borrows

For sequential operations, mutable borrows are released after each stage:

```rust
let graph = ShaderGraph::builder()
    .compute(write_pass::Resources {
        output: &mut buffer,  // &mut borrowed here
    }, [64, 1, 1])
    // Borrow released - write_pass config consumed

    .compute(read_pass::Resources {
        input: &buffer,  // & borrow OK now
    }, [64, 1, 1])

    .build()?;
```

This works because the `Resources` struct is consumed when passed to `.compute()`, releasing the borrow.

### Error Message Clarity

Rust's borrow errors are precise but not domain-specific:

```
error[E0502]: cannot borrow `depth_buffer` as immutable because it is
              also borrowed as mutable
```

Users familiar with Rust will understand. For clarity, documentation should explain:

> "This error indicates a parallel resource conflict. One section writes to the
> resource (`&mut`) while another section reads from it (`&`). Either:
> - Move the dependent operation outside the parallel section
> - Use separate resources for each section
> - Remove the parallel section if the operations must be sequential"

### Combining with Scoped API

The ownership-based safety combines naturally with the scoped parallel API:

```rust
.parallel(|p| {
    p.graphics_section(|g| {
        g.render_pass(RenderPassConfig {
            depth_attachment: &mut depth_buffer,  // &mut captured by closure
            color: &mut color_buffer,
        })
    })
    .compute_section(|c| {
        c.compute(ssao::Resources {
            // depth: &depth_buffer,  // Would be compile error!
            output: &mut ssao_buffer,  // Different resource - OK
        }, [width/16, height/16, 1])
    })
})
// Both borrows released after parallel() returns
```

The closure captures enforce that borrows are held for the duration of the parallel section, exactly matching the GPU execution semantics.

### Benefits Summary

| Benefit | Description |
|---------|-------------|
| Compile-time safety | Invalid parallel access is caught before running |
| Zero runtime cost | No dynamic resource conflict checking needed |
| Self-documenting | `&mut` in signature shows resource is written |
| Familiar semantics | Standard Rust borrow checker rules |
| Aligns with reflection | ResourceAccess::Read/ReadWrite maps to `&`/`&mut` |
| Composable with scopes | Closure captures match parallel execution lifetime |

### Limitations

| Limitation | Description |
|------------|-------------|
| Learning curve | Users must understand Rust borrowing for GPU resources |
| Handle ergonomics | Non-Copy handles require explicit references everywhere |
| Complex lifetimes | Multiple parallel sections may need lifetime annotations |
| False positives | Some safe patterns may be rejected by borrow checker |

For the "false positives" case, consider:

```rust
// This might be safe if compute only reads region A and graphics only writes region B
// But borrow checker doesn't know about regions - rejects it
.parallel(|p| {
    p.compute_section(|c| { /* reads &buffer */ })
    p.graphics_section(|g| { /* writes &mut buffer */ })  // Error!
})
```

Users would need to split into separate buffers, even if the access patterns don't actually conflict. This is conservative but safe.

---

## Hardware Landscape

Async compute support varies dramatically across GPU vendors and generations. The fundamental problem is that Vulkan exposes queue families, but there's no way to query whether those queues map to truly parallel hardware.

### The Core Problem

> "There is no current way in Vulkan to expose the exact details how each VkQueue is mapped."

A GPU might report multiple queue families with compute support, but:
- They could map to the same underlying hardware (time-sliced)
- They could map to dedicated compute units (true parallelism)
- They could be separate command streams on shared hardware (scheduling only)

You cannot determine this from Vulkan APIs alone.

### NVIDIA

#### Maxwell and Older (2014, <1.5% of Steam)

**Released:** 2014 (GTX 900 series: GTX 960, 970, 980)

> "In some drivers (e.g. NVIDIA on Maxwell), all VkQueues are just time sliced on the same execution unit, and there is zero performance benefit from using more than one."

> "Even Maxwell featured Asynchronous Compute on paper. Unfortunately, due to the fact that expensive software based context switching had to be employed before it could be used, resulted in lowered performance."

Maxwell GPUs may report **16+ queues** in a single queue family, but they all execute on the same hardware. Using multiple queues adds synchronization overhead with **zero parallelism benefit**.

**Steam Hardware Survey (December 2025):**
- GTX 960: 0.21%
- GTX 970: 0.24%
- GT 730 and older: ~0.5%
- **Total Maxwell and older: <1.5%**

**Driver Support:** NVIDIA ended Game Ready driver updates for Maxwell in October 2025. Only security patches until 2028.

**Result:** Async compute is **net negative** on Maxwell. These cards are 10+ years old and represent a negligible portion of the market.

#### Pascal (2016, ~5% of Steam)

**Released:** May 2016 (GTX 10 series: GTX 1050, 1060, 1070, 1080)

Pascal introduced hardware-scheduled async compute with dynamic load balancing:

> "Pascal features a dynamic load balancing system. This allows the scheduler to dynamically adjust the amount of the GPU assigned to multiple tasks. Nvidia therefore has safely enabled asynchronous compute in Pascal's driver."

However, Pascal still differs from AMD's approach:

> "Pascal still can't execute async code concurrently without pre-emption. This is quite different from AMD's GCN architecture which has Asynchronous Compute engines that enable the execution of multiple kernels concurrently without pre-emption."

Pascal also introduced pixel-level and instruction-level preemption (vs Maxwell's draw-level only), enabling faster context switches.

**Steam Hardware Survey (December 2025):**
- GTX 1050: 0.67%
- GTX 1050 Ti: 1.40%
- GTX 1060: 1.81%
- GTX 1070: 0.76%
- GTX 1080: 0.49%
- **Total Pascal: ~5%**

**Driver Support:** NVIDIA ended Game Ready driver updates for Pascal in October 2025.

**Result:** Async compute **works but with limited benefit** (5-10%). Hardware scheduling helps, but preemption-based approach is less efficient than AMD's dedicated compute engines.

#### Turing and Newer (2018+, ~90% of NVIDIA users)

Turing (RTX 20 series), Ampere (RTX 30 series), and Ada (RTX 40 series) have progressively improved async compute. These architectures represent the vast majority of current NVIDIA users and benefit from async compute when workloads have complementary bottlenecks.

### AMD GCN/RDNA

Real hardware separation with dedicated Asynchronous Compute Engines (ACEs).

#### GCN 1.0 (2012, negligible usage)

**Released:** January 2012 (HD 7000 series)

First GCN generation with 2 ACEs. Async compute support was later disabled in drivers for some DX12 games due to limitations:

> "AMD allegedly disabled asynchronous-compute technology support on older generations of Graphics CoreNext (GCN) architecture since Radeon Software 16.9.2."

**Driver Support:** AMD ended support for GCN 1.0 in 2021 after 10 years.

**Result:** Effectively **no async compute** on GCN 1.0 in modern software. Negligible market share.

#### GCN 1.1+ (2013+, all modern AMD)

**Released:** 2013 (R9 290/290X and later)

GCN 1.1 introduced 8 ACEs, enabling true hardware async compute:

> "AMD R9 Fury has 1 queue in family 0 (full support), 3 queues in family 1 (compute), and 2 queues in family 2 (transfer)."

AMD exposes separate queue families that map to different hardware. Compute queues can truly run in parallel with graphics work.

However, there are limitations:

> "GCN hardware contains a single geometry frontend, so no additional performance will be gained by creating multiple direct queues. Any command lists scheduled to a direct queue will get serialized onto the same hardware queue."

- Multiple **graphics** queues don't help (single geometry frontend)
- Multiple **compute** queues beyond one show diminishing returns
- Best results: 1 graphics queue + 1 compute queue

**AMD Architecture Timeline:**

| Architecture | Release | Example Cards | ACEs |
|--------------|---------|---------------|------|
| GCN 1.0 | 2012 | HD 7000 | 2 (limited) |
| GCN 1.1 | 2013 | R9 290/290X | 8 |
| GCN 1.2 | 2015 | R9 380/Fury | 8 |
| Polaris | 2016 | RX 400/500 | 8 |
| Vega | 2017 | Vega 56/64 | 8 |
| RDNA | 2019 | RX 5000 | Improved |
| RDNA 2 | 2020 | RX 6000 | Improved |
| RDNA 3 | 2022 | RX 7000 | Improved |

**Result:** Async compute provides **10-20% improvement** when workloads have complementary bottlenecks. All AMD GPUs from 2013 onward have good async compute support.

### Intel

Limited async compute support:

> "Best practice is to use 1 general + 1 transfer on NVIDIA/Intel (and on Intel you should think twice about utilising the transfer queue)"

Intel integrated and discrete GPUs have limited or no benefit from multiple queues. The transfer queue may not provide real DMA parallelism.

**Result:** Async compute is typically **net negative** on Intel.

### ARM Mali (Mobile)

Single queue family with command stream separation:

> "A VkQueue on Arm Mali does not map to just one hardware queue, it maps to both vertex/tiling and fragment queues. Multiple queues on Arm Mali do not map to different kinds of hardware, rather they just represent separate streams of commands."

All Vulkan queues go through the same tile-based rendering pipeline. "Async compute" is really just command scheduling, not hardware parallelism.

> "In this particular sample, we got a ~5% FPS gain on a Mali-G77 GPU, but these results are extremely content specific."

**Result:** Small benefits (~5%) possible but very content-dependent.

### Apple Silicon (M1/M2/M3)

Uses Metal API, not Vulkan natively. Limited cross-queue concurrency:

> "For commands on different Metal command queues, there's only 2x concurrency across the entire GPU. This makes it similar to early dual-core CPUs."

Apple GPUs achieve parallelism primarily within a single command encoder, not across multiple queues.

> "Sub-core concurrency only happens among commands within the same MTLComputeCommandEncoder."

**Result:** Limited benefit from multi-queue approaches. Focus on single-queue occupancy instead.

### Qualcomm Adreno (Mobile)

Similar to ARM Mali - tile-based architecture with limited queue-level parallelism. Async compute benefits depend on whether compute work can overlap with binning/tiling phases.

### Summary Table

| Hardware | Release | Steam % | True Parallelism | Async Benefit |
|----------|---------|---------|------------------|---------------|
| NVIDIA Maxwell | 2014 | <1.5% | **None** - time sliced | Negative |
| NVIDIA Pascal | 2016 | ~5% | Limited - preemption | Small (5-10%) |
| NVIDIA Turing+ | 2018+ | ~35% | Yes | Moderate |
| AMD GCN 1.0 | 2012 | ~0% | Limited (2 ACEs) | Limited |
| AMD GCN 1.1+ | 2013+ | ~15% | **Yes** - 8 ACEs | Good (10-20%) |
| Intel | Various | ~5% | Minimal | Negative to none |
| ARM Mali | Various | Mobile | **None** - command streams | Small (~5%) |
| Apple M1/M2 | 2020+ | ~3% | 2x max | Limited |

### Practical Targeting

For desktop PC games (ignoring mobile):

| Target | Coverage | Async Support |
|--------|----------|---------------|
| NVIDIA Turing+ and AMD GCN 1.1+ | ~93% | Full benefit |
| Include Pascal | ~98% | Works, limited benefit |
| Include Maxwell | ~99.5% | Must fall back to sequential |

**Recommendation:** Target Pascal+ (NVIDIA) and GCN 1.1+ (AMD). This covers **~98% of Steam users** with hardware that has at least some async compute capability. The <1.5% on Maxwell or older can safely fall back to sequential execution.

### Implications for API Design

Given this hardware variance:

1. **Cannot assume async helps** - It may hurt on 50%+ of hardware
2. **Need runtime detection** - Profile actual performance, not queue counts
3. **Graceful fallback required** - Parallel sections must flatten to sequential
4. **A/B testing valuable** - Let users discover what works on their GPU
5. **Conservative defaults** - Don't enable async by default
6. **Per-vendor tuning** - May need GPU-specific code paths

### Detection Strategies

Since Vulkan doesn't expose hardware topology, options include:

**1. Vendor/Device ID Database**

```rust
fn supports_true_async_compute(device: &PhysicalDevice) -> bool {
    match (device.vendor_id(), device.device_id()) {
        (VENDOR_AMD, _) => true,  // GCN/RDNA have real async
        (VENDOR_NVIDIA, id) if id >= PASCAL_FIRST => true,  // Pascal+
        _ => false,  // Conservative default
    }
}
```

**2. Runtime Profiling**

```rust
fn probe_async_benefit(renderer: &Renderer) -> AsyncSupport {
    // Run identical workload with/without async
    let sequential_time = benchmark_sequential();
    let parallel_time = benchmark_parallel();

    if parallel_time < sequential_time * 0.95 {
        AsyncSupport::Beneficial
    } else if parallel_time > sequential_time * 1.05 {
        AsyncSupport::Harmful
    } else {
        AsyncSupport::Neutral
    }
}
```

**3. User Override**

```rust
BuildOptions {
    async_compute: AsyncComputeMode::Auto,  // or ForceOn, ForceOff
}
```

### Recommendation

Combine all three strategies:

1. Start with vendor database for initial guess
2. Run A/B profiling to validate
3. Allow user override for edge cases
4. Cache results per GPU for future runs

The scoped API with profiling guardrails handles this naturally - users declare intent with `.parallel()`, the system measures actual benefit, and warnings guide optimization.

---

## Simulation-Focused Parallelism

This section describes a constrained design that significantly reduces the downsides of Option D by focusing on a specific use case: games with GPU-driven simulation.

### Design Constraints

**1. Rebuild the graph every frame**

Instead of building once at setup, the graph is constructed each frame:

```rust
fn draw(&mut self, mut frame: FrameRenderer) -> Result<(), DrawError> {
    let graph = RenderGraph::builder()
        .simulation(|sim| { ... })
        .rendering(|r| { ... })
        .build()?;

    frame.execute(&graph)?;
    Ok(())
}
```

This directly solves:
- Resolution and settings changes (graph adapts each frame)
- Hot reload (rebuilt graph picks up new shaders)
- Conditional stages (just don't add them when disabled)

Implementation considerations:
- Use arena allocators to avoid per-frame heap allocation
- Barrier analysis must be O(n) in stage count
- Cache barrier schedules when graph shape matches previous frame

**2. Limit parallelism to two well-defined patterns**

Rather than arbitrary parallel sections, support only:

**Pattern A: Cross-frame simulation/rendering**
```
Frame N:   [──Render results of Sim(N-1)──]
           [──Simulate(N) for next frame──]
```

Simulation for frame N+1 runs on the compute queue while rendering frame N runs on the graphics queue. They naturally use different hardware (shader cores vs rasterizer) and different memory (simulation buffers vs textures/framebuffer).

**Pattern B: Intra-layer compute parallelism (FLAME-style)**
```
Layer 0: [movement] [feeding] [reproduction]  ← logically parallel
         ─────────────── barrier ─────────────
Layer 1: [death] [spawning]                   ← logically parallel
```

Within a simulation layer, compute shaders are independent and can execute concurrently. Barriers between layers enforce dependencies. This matches FLAME GPU's execution model.

### Revised API

The API reflects these constraints explicitly:

```rust
let graph = RenderGraph::builder()
    // Simulation for NEXT frame (compute queue)
    .simulation(|sim| {
        sim.layer(|l| {
            l.compute(movement::config(...))
             .compute(feeding::config(...))
        })
        .layer(|l| {
            l.compute(death::config(...))
             .compute(spawning::config(...))
        })
    })

    // Rendering for CURRENT frame (graphics queue)
    .rendering(|r| {
        r.render_pass(shadow_config)
            .draw(shadow::config(...))
         .end_render_pass()
         .render_pass(main_config)
            .draw(world::config(...))
            .draw(agents::config(...))
         .end_render_pass()
    })

    .build()?;
```

Key differences from generic Option D:
- No arbitrary `.parallel()` sections
- `.simulation()` and `.rendering()` have clear semantics
- Layers within simulation are explicit
- Cross-frame relationship is baked into the API

### Why This Reduces Downsides

| Original Downside | Status | Reason |
|-------------------|--------|--------|
| Users must understand hardware bottlenecks | **Solved** | Pattern is inherently good: compute-bound simulation vs rasterization-bound rendering |
| Unbalanced sections | **Solved** | Simulation and rendering are naturally large, comparable workloads |
| Semaphore overhead | **Solved** | Amortized over frame-length work, one sync per frame |
| Stall propagation | **Solved** | Cross-frame has natural slack; slow simulation delays frame N+2, not current render |
| Priority/preemption | **Solved** | Rendering has vsync deadline, simulation is deferrable |
| Platform performance variance | **Reduced** | Compute vs graphics is the ideal async pattern across all vendors |
| Memory bandwidth contention | **Reduced** | Simulation buffers and render targets use different memory |
| Testing non-determinism | **Reduced** | Structured pattern is predictable; layer ordering is defined |
| Queue availability variance | **Remains** | Unsolvable at Vulkan API level; fallback to sequential is clean |
| Barrier optimization loss | **Remains** | Sync point prevents some merging; less critical at frame scale |

### New Considerations

**1. Double-buffering is mandatory**

Simulation writes frame N+1 while rendering reads frame N. This requires two copies of simulation state:

```rust
struct SimulationState {
    agents: [StorageBuffer<Agent>; 2],  // ping-pong buffers
    frame_index: usize,
}

impl SimulationState {
    fn current(&self) -> &StorageBuffer<Agent> {
        &self.agents[self.frame_index % 2]
    }

    fn next(&mut self) -> &mut StorageBuffer<Agent> {
        &mut self.agents[(self.frame_index + 1) % 2]
    }
}
```

The borrow checker enforces correct usage—`current()` returns `&` (read for rendering), `next()` returns `&mut` (write for simulation). Different buffers means no borrow conflicts.

Memory cost: 2x simulation state. For a 1M agent simulation at 64 bytes/agent, this is ~128MB total—acceptable for desktop.

**2. One frame of input latency**

Cross-frame parallelism delays simulation results by one frame:

```
Without parallelism:
  Input → Simulate → Render → Display     (same frame)

With parallelism:
  Input → Simulate ──────────────────────→ Render → Display
                   (one frame later)
```

For simulation-heavy games (city builders, factory games, RTS, agent-based games), this is typically acceptable. The tradeoff is well-understood and should be documented, not hidden.

**3. Simulation determinism**

Within a FLAME-style layer, shaders are *logically* parallel but execute in a defined order. Between layers, barriers enforce sequencing. The simulation produces identical results regardless of how it overlaps with rendering.

This matters for:
- Replays (must reproduce exactly)
- Networking (clients must agree on state)
- Testing (results must be reproducible)

The layer-based model preserves determinism by design.

### What Remains

Two downsides are not addressed by this design:

**1. Queue availability variance**

Vulkan provides no API to query whether queue families map to truly parallel hardware. Detection strategies:

| Strategy | Tradeoff |
|----------|----------|
| Vendor/device ID database | Requires maintenance, breaks on new hardware |
| Runtime A/B profiling | Adds startup time, results vary by workload |
| User override | Burden on users |

Recommendation: Use vendor database as initial guess, with optional A/B testing and user override. When async isn't beneficial, parallel sections flatten to sequential with minimal overhead.

**2. Barrier optimization loss**

The sync point between simulation and rendering prevents barrier merging that would otherwise be possible. This is an inherent cost of the pattern, but at frame-scale workloads the impact is small relative to total frame time.

### Target Hardware

This design targets desktop GPUs where async compute provides real benefit:

| Hardware | Async Support | Coverage |
|----------|---------------|----------|
| AMD GCN 1.1+ (2013+) | Excellent (8 ACEs) | ~15% of Steam |
| NVIDIA Turing+ (2018+) | Good | ~35% of Steam |
| NVIDIA Pascal (2016) | Limited but functional | ~5% of Steam |

Combined coverage: ~55% benefit significantly, ~98% work correctly (Pascal+ and all AMD).

Excluded: Mobile (different thermal/power constraints), Intel (minimal async benefit), NVIDIA Maxwell (time-sliced only, <1.5% of Steam).

---

## References

### Async Compute & Render Graphs

- [AMD GPUOpen - Concurrent Execution with Asynchronous Queues](https://gpuopen.com/learn/concurrent-execution-asynchronous-queues/)
- [Vulkan Documentation - Async Compute Sample](https://docs.vulkan.org/samples/latest/samples/performance/async_compute/README.html)
- [ARM Developer - Using Asynchronous Compute on Mali GPUs](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/using-asynchronous-compute-on-arm-mali-gpus)
- [GDC 2017 - Frostbite Frame Graph](https://www.gdcvault.com/play/1024612/FrameGraph-Extensible-Rendering-Architecture-in)

### Hardware Architecture & Queue Behavior

- [GameDev.net - Vulkan Queues and How to Handle Them](https://www.gamedev.net/forums/topic/683966-vulkan-queues-and-how-to-handle-them/)
- [Vulkan Documentation - Queues Guide](https://docs.vulkan.org/guide/latest/queues.html)
- [Khronos Forums - Mechanism for Querying Physical Queue Topology](https://github.com/KhronosGroup/Vulkan-Docs/issues/569)
- [GitHub - Apple GPU Microarchitecture Benchmarks](https://github.com/philipturner/metal-benchmarks)
- [Alyssa Rosenzweig - Dissecting the Apple M1 GPU](https://rosenzweig.io/blog/asahi-gpu-part-3.html)

### NVIDIA Architecture Deep Dives

- [AnandTech - Pascal Asynchronous Concurrent Compute](https://www.anandtech.com/show/10325/the-nvidia-geforce-gtx-1080-and-1070-founders-edition-review/9)
- [Wikipedia - Pascal Microarchitecture](https://en.wikipedia.org/wiki/Pascal_(microarchitecture))
- [eTeknix - Pascal GTX 1080 Async Compute Explored](https://www.eteknix.com/pascal-gtx-1080-async-compute-explored/)
- [Guru3D - NVIDIA Ends GTX 10 Series Driver Support](https://www.guru3d.com/story/nvidia-ends-game-ready-driver-updates-for-gtx-10-series-in-october-2025/)

### AMD Architecture

- [Wikipedia - Graphics Core Next](https://en.wikipedia.org/wiki/Graphics_Core_Next)
- [TechPowerUp - AMD GCN Async Compute Support](https://www.techpowerup.com/228447/amd-cripples-older-gcn-gpus-of-async-compute-support)
- [Medium - AMD TeraScale, GCN & RDNA Architecture Deep-Dive](https://medium.com/high-tech-accessible/an-architectural-deep-dive-into-amds-terascale-gcn-rdna-gpu-architectures-c4a212d0eb9)

### Market Data

- [Steam Hardware Survey](https://store.steampowered.com/hwsurvey/videocard/)
- [Jon Peddie Research - Steam Hardware Survey Analysis](https://www.jonpeddie.com/news/a-closer-look-at-the-recent-steam-hardware-survey/)
