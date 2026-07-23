# Bevy Render Graph Conversation

A conversation exploring how the `bevy_render` crate works, with focus on the render graph, GPU synchronization, compute shaders, and related topics.

Dan: This took place in the bevy repo, on latest main, on 2025-01-11.

---

## Table of Contents

1. [Overview of bevy_render Crate](#overview-of-bevy_render-crate)
2. [Render Graph Deep Dive](#render-graph-deep-dive)
3. [GPU Synchronization](#gpu-synchronization)
4. [Running Compute Shaders Per FixedUpdate](#running-compute-shaders-per-fixedupdate)
5. [ShaderType Derive Macro](#shadertype-derive-macro)
6. [SPIR-V Passthrough Feature](#spir-v-passthrough-feature)
7. [EXPERIMENTAL_PASSTHROUGH_SHADERS](#experimental_passthrough_shaders)
8. [Render Graph Node Execution](#render-graph-node-execution)

---

## Overview of bevy_render Crate

**Question:** Can you summarize how the bevy_render crate works, and describe its module structure?

### Architecture Overview

The `bevy_render` crate is the core rendering infrastructure for the Bevy game engine. It uses a **two-world architecture** for performance:
- **Main World**: Runs simulation/game logic
- **Render World**: A separate `SubApp` that handles GPU preparation and rendering

This separation enables **pipelined rendering** - the next frame's extraction can happen while the current frame renders.

### Module Structure

| Module | Purpose |
|--------|---------|
| `renderer` | GPU device, queue, command encoding (wgpu wrapper) |
| `render_graph` | Modular node-based render graph system |
| `render_phase` | Queuing, sorting, and batching of drawable entities |
| `render_resource` | GPU resources (pipelines, buffers, bind groups, textures) |
| `camera` | Camera extraction and management |
| `view` | View configuration, attachments, uniforms |
| `mesh` | Mesh rendering and GPU buffer management |
| `texture` | Texture handling and GPU images |
| `extract_component` | Component extraction from main to render world |
| `extract_resource` | Resource extraction between worlds |
| `batching` | CPU and GPU-based instance batching |
| `globals` | Global uniforms (time, etc.) |
| `storage` | GPU storage buffer utilities |
| `gpu_readback` | GPU-to-CPU readback |
| `sync_world` | Entity synchronization between worlds |

### Key Types

- **RenderDevice / RenderQueue**: Wrappers around wgpu's Device and Queue
- **RenderContext**: Command encoder and buffers for a frame
- **RenderGraph**: Defines the rendering pipeline with nodes and edges
- **BinnedRenderPhase / SortedRenderPhase**: Collections of drawable items (binned for opaque, sorted for transparent)
- **MainWorld**: Resource available during extraction to read simulation data

### Render Schedule Flow

```
ExtractCommands → PrepareMeshes → ManageViews → Queue → PhaseSort → Prepare → Render → Cleanup
```

1. **ExtractSchedule**: Copies necessary data from MainWorld to RenderWorld
2. **PrepareAssets**: Converts extracted data to GPU resources
3. **Queue**: Adds drawable items to render phases
4. **PhaseSort**: Sorts transparent/ordered items
5. **Prepare**: Creates bind groups and uploads uniforms
6. **Render**: Executes the RenderGraph
7. **Cleanup**: Despawns temporary entities

### Key Plugins

- **RenderPlugin**: Main plugin; initializes renderer and RenderApp
- **WindowRenderPlugin**: Surface/window target management
- **CameraPlugin**: Camera extraction
- **ViewPlugin**: View targets and attachments
- **MeshRenderAssetPlugin**: Mesh GPU preparation
- **TexturePlugin**: Texture asset preparation
- **BatchingPlugin**: GPU preprocessing and batching

### Data Flow Example

```
Main World Entity (Transform, Mesh, Material)
    ↓ ExtractSchedule
Render World (copies of components)
    ↓ PrepareAssets
GPU Resources (buffers, bind groups)
    ↓ Queue
Render Phase Items
    ↓ Render Graph Execution
GPU Draw Commands → Screen
```

---

## Render Graph Deep Dive

**Question:** Can you tell me more about render_graph?

### Module Structure

```
render_graph/
├── mod.rs              # Exports and RenderGraphError enum
├── node.rs             # Node trait, ViewNode, EmptyNode, RunGraphOnViewNode
├── graph.rs            # RenderGraph core + GraphInputNode
├── context.rs          # RenderGraphContext for node execution
├── edge.rs             # Edge types (SlotEdge, NodeEdge)
├── node_slot.rs        # SlotValue, SlotType, SlotInfo
├── app.rs              # RenderGraphExt trait for ergonomic API
└── camera_driver_node.rs # Built-in CameraDriverNode
```

### Core Types

**RenderGraph** - The container for nodes and subgraphs:
```rust
pub struct RenderGraph {
    nodes: HashMap<InternedRenderLabel, NodeState>,
    sub_graphs: HashMap<InternedRenderSubGraph, RenderGraph>,
}
```

**Node Trait** - The fundamental interface:
```rust
pub trait Node: Downcast + Send + Sync + 'static {
    fn input(&self) -> Vec<SlotInfo> { Vec::new() }
    fn output(&self) -> Vec<SlotInfo> { Vec::new() }
    fn update(&mut self, _world: &mut World) {}
    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        world: &'w World,
    ) -> Result<(), NodeRunError>;
}
```

**RenderGraphContext** - Passed to nodes during execution:
- `get_input()` / `get_input_texture()` / `get_input_buffer()` - Read input slots
- `set_output()` - Write output slots
- `run_sub_graph()` - Queue a subgraph for execution
- `view_entity()` - Get the current view entity

### Slot System

Slots pass data between nodes with four types:
```rust
pub enum SlotType {
    Buffer,
    TextureView,
    Sampler,
    Entity,
}
```

### Edge Types

Two kinds of edges define dependencies:

1. **NodeEdge** - Simple ordering (A runs before B)
2. **SlotEdge** - Ordering + data passing (A's output feeds B's input)

```rust
graph.add_node_edge(NodeA, NodeB);  // Ordering only
graph.add_slot_edge(NodeA, "out_tex", NodeB, "in_tex");  // Data flow
```

### Graph Execution (RenderGraphRunner)

1. Initialize queue with nodes that have no input dependencies
2. Process nodes in topological order
3. Execute each node's `run()` method
4. Pass slot values between connected nodes
5. Handle subgraph execution when queued
6. Submit all command buffers to GPU

### ViewNode Trait

Simplifies nodes that operate on a specific view entity:
```rust
pub trait ViewNode {
    type ViewQuery: ReadOnlyQueryData;

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        view_query: QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError>;
}
```

The `ViewNodeRunner<N>` wrapper handles query state management automatically.

### Built-in Nodes

| Node | Purpose |
|------|---------|
| `EmptyNode` | No-op node for bundling dependencies |
| `GraphInputNode` | Entry point for graphs with inputs |
| `RunGraphOnViewNode` | Runs a configured subgraph |
| `CameraDriverNode` | Iterates sorted cameras and queues their render subgraphs |

### Creating a Custom Node

```rust
#[derive(RenderLabel)]
pub struct MyPostProcess;

pub struct MyPostProcessNode;

impl Node for MyPostProcessNode {
    fn input(&self) -> Vec<SlotInfo> {
        vec![SlotInfo::new("source", SlotType::TextureView)]
    }

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let source = graph.get_input_texture("source")?;
        let pipeline_cache = world.resource::<PipelineCache>();

        // Issue GPU commands...
        let mut pass = render_context.command_encoder()
            .begin_render_pass(&descriptor);

        Ok(())
    }
}

// Register in app:
app.add_render_graph_node::<MyPostProcessNode>(Core2d, MyPostProcess);
app.add_render_graph_edge(Core2d, PreviousNode, MyPostProcess);
```

### Subgraphs

Graphs can contain nested subgraphs for hierarchical composition. The `CameraDriverNode` uses this to run per-camera render graphs (like `Core2d` or `Core3d`) for each extracted camera.

---

## GPU Synchronization

**Question:** How is GPU synchronization handled?

Bevy uses **wgpu's implicit synchronization model** - there are **no explicit barriers, fences, or semaphores** in Bevy's code. The wgpu abstraction handles all GPU-side synchronization for the underlying backend (Vulkan, Metal, DX12, OpenGL).

### Command Buffer Flow

```
RenderContext (accumulates commands)
    ↓
CommandEncoder (immediate recording)
    ↓
CommandBuffer (finished)
    ↓
RenderQueue.submit() (GPU execution in submission order)
```

**RenderContext** (`renderer/mod.rs:500-681`) manages:
- A lazy `CommandEncoder` created on-demand
- A queue of `QueuedCommandBuffer` entries (either pre-built or parallel tasks)
- All buffers are collected, sorted by submission order, and submitted as one batch

### Render Graph Dependencies

The graph uses **data-flow dependencies**, not GPU sync primitives:

- **SlotEdge**: Passes data (texture views, buffers) between nodes
- **NodeEdge**: Ordering-only dependencies

**Execution** (`graph_runner.rs:104-291`):
1. Nodes with no dependencies execute first
2. Downstream nodes wait until input slots are filled
3. Sequential, topological execution (one node at a time)
4. All command buffers submitted together at the end

### Resource Synchronization

| Pattern | Mechanism |
|---------|-----------|
| Texture uploads | `create_texture_with_data()` via staging buffer |
| Buffer readback | `buffer.map_async()` + `device.poll()` |
| Layout transitions | Handled implicitly by wgpu |
| Render pass loads/stores | `LoadOp`/`StoreOp` (not explicit barriers) |

### Frame Lifecycle

```
1. graph.update(world)           - Prepare nodes
2. RenderGraphRunner::run()      - Execute graph, collect command buffers
3. queue.submit(commands)        - Submit all to GPU
4. present_frames()              - Call surface_texture.present() (non-blocking)
```

**Swapchain sync** (`view/window/mod.rs:279-294`):
- `surface.get_current_texture()` - Acquires next image (may wait)
- `surface_texture.present()` - Schedules presentation (non-blocking)

### Pipelined Rendering

With `PipelinedRenderingPlugin`, CPU work is parallelized:

```
Main Thread:   [Frame N sim]     [Frame N+1 sim]    [Frame N+2 sim]
                      ↓                ↓                  ↓
Extract:           [Extract N]  →  [Extract N+1]  →  [Extract N+2]
                      ↓                ↓
Render Thread:              [Frame N render]  [Frame N+1 render]
```

This uses `async_channel::bounded(1)` for thread coordination - purely CPU-side, no GPU sync changes.

### Why No Explicit Barriers?

wgpu abstracts GPU synchronization:
- Command execution order = submission order
- Resource transitions managed by backend
- Render pass attachments use `LoadOp`/`StoreOp` instead of barriers

---

## Running Compute Shaders Per FixedUpdate

**Question:** In bevy, is there a way to run a compute shader once per FixedUpdate schedule execution? Either directly in a CPU system, or by passing the number of executions needed?

**Short answer:** There's no built-in mechanism, but you can implement it by extracting a flag or counter from the main world to the render world.

### The Challenge

- `FixedUpdate` runs in the **main world** (0, 1, or multiple times per frame)
- Compute shaders dispatch in the **render world** (via render graph nodes)
- These are separate ECS worlds that communicate only through extraction

### Solution: Extract an Execution Count

**Option 1: Boolean Flag (runs once if FixedUpdate ran at all)**

```rust
#[derive(Resource, Default, Clone, ExtractResource)]
struct FixedUpdateRan(bool);

// Main world
app.init_resource::<FixedUpdateRan>()
   .add_plugins(ExtractResourcePlugin::<FixedUpdateRan>::default())
   .add_systems(FixedFirst, |mut flag: ResMut<FixedUpdateRan>| flag.0 = true)
   .add_systems(Update, |mut flag: ResMut<FixedUpdateRan>| flag.0 = false);

// In render graph node
if !world.resource::<FixedUpdateRan>().0 {
    return Ok(());  // Skip compute this frame
}
// ... dispatch compute shader
```

**Option 2: Execution Counter (runs N times for N FixedUpdate executions)**

```rust
#[derive(Resource, Default, Clone, ExtractResource)]
struct FixedUpdateCount(u32);

// Main world - increment each FixedUpdate
app.add_systems(FixedFirst, |mut count: ResMut<FixedUpdateCount>| count.0 += 1);

// Render world - track last processed count
#[derive(Resource, Default)]
struct LastProcessedCount(u32);

// In render graph node or system
let current = world.resource::<FixedUpdateCount>().0;
let last = world.resource::<LastProcessedCount>().0;
let executions_needed = current - last;

for _ in 0..executions_needed {
    // dispatch_workgroups(...)
}
// Update last processed
```

**Option 3: Pass Count via Uniform Buffer**

```rust
// Extract the count to a GPU uniform
#[repr(C)]
#[derive(ShaderType)]
struct SimulationParams {
    steps_to_run: u32,
}

// In your compute shader
for (var i = 0u; i < params.steps_to_run; i++) {
    // Run one simulation step
}
```

### Key Files for Reference

| File | Purpose |
|------|---------|
| `examples/shader/compute_shader_game_of_life.rs` | Compute dispatch pattern |
| `crates/bevy_time/src/fixed.rs` | FixedUpdate accumulator logic |
| `crates/bevy_render/src/extract_component.rs` | Extraction mechanism |

### Direct Dispatch from Main World?

**No** - GPU commands can only be issued from the render world. The main world doesn't have access to `RenderContext` or command encoders. You must use extraction to communicate what the render world should do.

---

## ShaderType Derive Macro

**Question:** What does the ShaderType derive do?

`ShaderType` is a derive macro from the **encase** crate (re-exported by Bevy) that makes Rust structs compatible with GPU buffer layouts.

### What It Does

When you derive `ShaderType`, the macro generates code that:

1. **Computes correct alignment and padding** - GPU shaders require specific memory layouts (std140 for uniform buffers, std430 for storage buffers)
2. **Enables serialization to GPU-compatible bytes** - Implements `WriteInto` for writing to buffers
3. **Provides compile-time size info** - Via the `ShaderSize` trait

### Why It's Needed

GPU buffers have strict alignment requirements. For example:
- `vec3` in WGSL is 16-byte aligned (not 12!)
- Structs may need padding between fields
- Arrays have specific stride requirements

`ShaderType` handles all this automatically.

### Example Usage

```rust
#[derive(ShaderType, Clone)]
pub struct MyUniform {
    pub color: Vec4,        // 16 bytes, 16-byte aligned
    pub intensity: f32,     // 4 bytes
    pub flags: u32,         // 4 bytes
    // encase adds 8 bytes padding here for 16-byte struct alignment
}

// Used with UniformBuffer or StorageBuffer
let mut buffer = UniformBuffer::new(my_uniform);
buffer.write_buffer(device, queue);
```

### Real Examples from Bevy

**GlobalsUniform** (`bevy_render/src/globals.rs:42`):
```rust
#[derive(ShaderType, Clone, Resource, ExtractResource)]
pub struct GlobalsUniform {
    time: f32,
    delta_time: f32,
    frame_count: u32,
    #[cfg(all(feature = "webgl", ...))]
    _wasm_padding: f32,  // Manual padding for WebGL2's 16-byte requirement
}
```

### Where It's Used

| Type | Purpose |
|------|---------|
| `UniformBuffer<T: ShaderType>` | Single uniform value |
| `StorageBuffer<T: ShaderType>` | Single storage value |
| `BufferVec<T: ShaderType>` | Array of values |
| `GpuArrayBuffer<T: GpuArrayBufferable>` | Batched GPU data |

### Import Path

```rust
use bevy::render::render_resource::ShaderType;
// or
use bevy::render::render_resource::encase::ShaderType;
```

---

## SPIR-V Passthrough Feature

**Question:** What is the 'spirv passthrough' feature, and how is it used? Are there risks or tradeoffs to using it?

The `spirv_shader_passthrough` feature allows SPIR-V shader bytecode to be sent directly to the GPU driver, bypassing Bevy's normal shader processing pipeline (naga).

### How It's Defined

```toml
# In Cargo.toml
spirv_shader_passthrough = ["bevy_internal/spirv_shader_passthrough"]

# Which enables wgpu's spirv feature
spirv_shader_passthrough = ["wgpu/spirv"]
```

### How It Works

**Without passthrough** (normal path):
```
SPIR-V bytes → naga parsing/validation → wgpu → GPU driver
```

**With passthrough enabled**:
```
SPIR-V bytes → wgpu (direct) → GPU driver
```

The implementation in `render_device.rs:59-93`:

```rust
#[cfg(feature = "spirv_shader_passthrough")]
match &desc.source {
    wgpu::ShaderSource::SpirV(source)
        if self.features().contains(wgpu::Features::EXPERIMENTAL_PASSTHROUGH_SHADERS) =>
    {
        // SAFETY:
        // This call passes binary data to the backend as-is and can potentially
        // result in a driver crash or bogus behavior.
        // No attempt is made to ensure that data is valid SPIR-V.
        unsafe {
            self.device.create_shader_module_passthrough(
                wgpu::ShaderModuleDescriptorPassthrough {
                    label: desc.label,
                    spirv: Some(source.clone()),
                    ..Default::default()
                },
            )
        }
    }
    _ => // fallback to normal path
}
```

### Primary Use Case

The feature exists mainly for **rust-gpu** and other third-party shader compilers that generate SPIR-V with features naga doesn't support.

### Risks and Tradeoffs

| Aspect | Risk Level | Details |
|--------|-----------|---------|
| **Driver crashes** | High | Invalid SPIR-V can crash GPU drivers |
| **No validation** | High | Bevy/wgpu perform zero safety checks |
| **Platform limited** | Medium | Only works on Vulkan |
| **Feature gated** | Medium | Requires `wgpu::Features::EXPERIMENTAL_PASSTHROUGH_SHADERS` at runtime |
| **Debugging** | Hard | Errors manifest as driver crashes, not error messages |

### Requirements

1. **Cargo feature**: Enable `spirv_shader_passthrough`
2. **Runtime feature**: GPU must support `EXPERIMENTAL_PASSTHROUGH_SHADERS`
3. **Backend**: Only Vulkan (not Metal, DX12, WebGPU, or OpenGL)
4. **Valid SPIR-V**: You are responsible for ensuring correctness

### When to Use It

**Use passthrough when:**
- Using rust-gpu to compile Rust to SPIR-V
- Your SPIR-V uses extensions naga doesn't support
- You need specific Vulkan SPIR-V features
- You've validated your SPIR-V through other means

**Don't use passthrough when:**
- Targeting multiple backends (Metal, DX12, WebGPU)
- You want safety validation
- You're writing shaders in WGSL/GLSL (use the normal path)
- You need cross-platform compatibility

---

## EXPERIMENTAL_PASSTHROUGH_SHADERS

**Question:** In this context, what does "GPU must support EXPERIMENTAL_PASSTHROUGH_SHADERS" mean? Is that a bevy-specific flag, or a vulkan extension, or something else?

It's a **wgpu feature flag** - not a Bevy-specific flag, and not directly a Vulkan extension.

### The Layering

```
Bevy Cargo feature:     spirv_shader_passthrough
         ↓ enables
wgpu feature flag:      Features::EXPERIMENTAL_PASSTHROUGH_SHADERS
         ↓ maps to
Backend capability:     Vulkan (always), Metal (partial), DX12 (partial)
```

### What It Actually Represents

`EXPERIMENTAL_PASSTHROUGH_SHADERS` is wgpu's abstraction for "can I send raw shader bytecode directly to the GPU driver without wgpu parsing it?"

On **Vulkan**, this is always possible because Vulkan natively accepts SPIR-V bytecode. There's no special Vulkan extension required - it's a core Vulkan capability.

The "experimental" part refers to **wgpu's API stability**, not hardware support:
- The API may change between wgpu versions
- It was recently renamed from `SPIRV_SHADER_PASSTHROUGH` (wgpu < 27)
- It's marked unsafe because wgpu provides zero validation

### How It's Checked at Runtime

In `render_device.rs:59-67`:

```rust
wgpu::ShaderSource::SpirV(source)
    if self
        .features()
        .contains(wgpu::Features::EXPERIMENTAL_PASSTHROUGH_SHADERS) =>
{
    // Use passthrough path
}
```

This checks whether the **wgpu device was created with this feature enabled**, not whether the hardware supports it (on Vulkan, the hardware always supports raw SPIR-V).

### Summary

| Layer | Name | What It Means |
|-------|------|---------------|
| Bevy | `spirv_shader_passthrough` | Cargo feature, compiles in the code path |
| wgpu | `EXPERIMENTAL_PASSTHROUGH_SHADERS` | Runtime feature flag, must be enabled on device |
| Vulkan | (none needed) | SPIR-V passthrough is core Vulkan functionality |
| Metal | (limited) | Only works for some shader types |
| DX12 | (limited) | Partial support |

---

## Render Graph Node Execution

**Question:** Going back to the bevy render graph. How are nodes in the render graph executed within bevy?

### Entry Point

Execution starts from `render_system` in `renderer/mod.rs:34-58`:

```rust
pub fn render_system(world: &mut World, ...) {
    // 1. Update all nodes (call node.update())
    world.resource_scope(|world, mut graph: Mut<RenderGraph>| {
        graph.update(world);
    });

    // 2. Run the graph
    let res = RenderGraphRunner::run(
        graph,
        render_device.clone(),
        diagnostics_recorder,
        &render_queue.0,
        world,
        |encoder| {
            // Finalizer: screenshots, readbacks
        },
    );
}
```

### Execution Algorithm

The graph uses a **work queue with lazy dependency checking** (not a pre-computed topological sort):

```rust
// graph_runner.rs:142-180

// 1. Queue nodes with no inputs (they can run immediately)
let mut node_queue: VecDeque<&NodeState> = graph
    .iter_nodes()
    .filter(|node| node.input_slots.is_empty())
    .collect();

// 2. Process nodes from queue
'handle_node: while let Some(node_state) = node_queue.pop_back() {
    // Skip already-processed nodes
    if node_outputs.contains_key(&node_state.label) {
        continue;
    }

    // Check if all dependencies have outputs ready
    for (edge, input_node) in graph.iter_node_inputs(node_state.label) {
        match edge {
            Edge::SlotEdge { output_index, input_index, .. } => {
                if let Some(outputs) = node_outputs.get(&input_node.label) {
                    // Collect input value from dependency's output
                    slot_indices_and_inputs.push((*input_index, outputs[*output_index].clone()));
                } else {
                    // Dependency not ready - re-queue this node
                    node_queue.push_front(node_state);
                    continue 'handle_node;
                }
            }
            Edge::NodeEdge { .. } => {
                // Just check if dependency ran, no data transfer
                if !node_outputs.contains_key(&input_node.label) {
                    node_queue.push_front(node_state);
                    continue 'handle_node;
                }
            }
        }
    }

    // 3. Execute the node
    node_state.node.run(&mut context, render_context, world)?;

    // 4. Store outputs for downstream nodes
    node_outputs.insert(node_state.label, values);

    // 5. Queue downstream nodes
    for (_, node_state) in graph.iter_node_outputs(node_state.label) {
        node_queue.push_front(node_state);
    }
}
```

### Key Characteristics

| Aspect | Detail |
|--------|--------|
| **Order** | Strictly sequential (one node at a time) |
| **Queue behavior** | Pop from back, push to front when re-queuing |
| **Dependency storage** | `HashMap<InternedRenderLabel, SmallVec<[SlotValue; 4]>>` |
| **No parallelism** | Nodes never run concurrently |

### Slot Value Transfer

Values flow between nodes via typed slots:

```rust
pub enum SlotValue {
    Buffer(Buffer),
    TextureView(TextureView),
    Sampler(Sampler),
    Entity(Entity),
}
```

Type-safe accessors in `RenderGraphContext`:
```rust
let texture = graph.get_input_texture("source")?;  // Returns &TextureView
graph.set_output("result", output_texture)?;       // Type-checked
```

### Subgraph Execution

Subgraphs are **queued during node execution**, then **executed immediately after**:

```rust
// In a node's run() method:
graph.run_sub_graph(MySubGraph, inputs, view_entity, debug_group)?;

// After node.run() returns (graph_runner.rs:245-258):
for run_sub_graph in context.finish() {
    Self::run_graph(
        sub_graph,
        &mut render_context,  // Same shared context
        world,
        &run_sub_graph.inputs,
        run_sub_graph.view_entity,
    )?;
}
```

This is how `CameraDriverNode` works - it queues per-camera subgraphs (like `Core2d` or `Core3d`) which execute inline.

### Command Encoder Sharing

A **single `CommandEncoder`** is shared across all nodes:

```rust
pub struct RenderContext<'w> {
    command_encoder: Option<CommandEncoder>,  // Lazily created, shared
    // ...
}

impl RenderContext {
    pub fn command_encoder(&mut self) -> &mut CommandEncoder {
        self.command_encoder.get_or_insert_with(|| {
            self.render_device.create_command_encoder(&default())
        })
    }
}
```

All nodes write to the same encoder. After graph execution completes, the accumulated commands are submitted:

```rust
let (commands, render_device, _) = render_context.finish();
queue.submit(commands);
```

### Where Parallelism Does Exist

While **node execution is sequential**, command buffer generation can be parallelized:

```rust
// renderer/mod.rs:612-631 - in finish()
bevy_tasks::ComputeTaskPool::get().scope(|task_pool| {
    for queued_command_buffer in self.command_buffer_queue {
        match queued_command_buffer {
            QueuedCommandBuffer::Ready(buf) => { /* already done */ }
            QueuedCommandBuffer::Task(task) => {
                task_pool.spawn(async move {
                    task(render_device)  // Runs in parallel
                });
            }
        }
    }
});
```

### Visual Summary

```
render_system()
    ↓
RenderGraphRunner::run()
    ↓
run_graph(main_graph)
    ↓
┌─────────────────────────────────────┐
│ Node Queue: [A, B, C] (no inputs)   │
│                                     │
│ while let Some(node) = queue.pop() │
│   ├─ Check dependencies ready?      │
│   │   ├─ No → re-queue, continue   │
│   │   └─ Yes → proceed             │
│   ├─ node.run(&mut context)        │
│   ├─ Store outputs in HashMap      │
│   ├─ Execute queued subgraphs      │
│   └─ Queue downstream nodes        │
└─────────────────────────────────────┘
    ↓
queue.submit(all_commands)
```

---

## Key File References

| Topic | File Path |
|-------|-----------|
| Render system entry | `crates/bevy_render/src/renderer/mod.rs:34-124` |
| Graph runner | `crates/bevy_render/src/renderer/graph_runner.rs` |
| RenderContext | `crates/bevy_render/src/renderer/mod.rs:500-681` |
| Node trait | `crates/bevy_render/src/render_graph/node.rs:69-106` |
| RenderGraph | `crates/bevy_render/src/render_graph/graph.rs:74-78` |
| Edge types | `crates/bevy_render/src/render_graph/edge.rs` |
| Slot types | `crates/bevy_render/src/render_graph/node_slot.rs` |
| Window presentation | `crates/bevy_render/src/view/window/mod.rs:91-294` |
| GPU readback | `crates/bevy_render/src/gpu_readback.rs` |
| Pipelined rendering | `crates/bevy_render/src/pipelined_rendering.rs` |
| FixedUpdate | `crates/bevy_time/src/fixed.rs` |
| Compute example | `examples/shader/compute_shader_game_of_life.rs` |
| ShaderType docs | `crates/bevy_render/src/render_resource/bind_group.rs:150-152` |
| SPIR-V passthrough | `crates/bevy_render/src/renderer/render_device.rs:46-94` |
