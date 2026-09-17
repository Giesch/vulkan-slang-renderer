# Render graph

## Crate ownership and backend contract

`mltrs-render-graph` owns construction, validation, scheduling, staging, and
execution orchestration. It has no renderer, Vulkan, SDL, or Slang dependency.
`mltrs-renderer` implements its backend interfaces and retains Vulkan allocation,
synchronization, and command recording. Existing `renderer::render_graph` paths
(and the corresponding `mltrs` reexports) remain a compatibility facade.
`Addr`, `ReadAddr`, `ImmutableAddr`, `BindlessHandle`, and texture markers are
single graph-owned types, not matching copies. The bindless sampled-handle to
`SampledTexBinding` conversion is available through either import path.

`PreparationBackend` allocates physical images and returns shader metadata plus
backend-owned keepalives. `PreparedRenderGraph<N, B>` retains those keepalives;
the renderer facade specializes `B` to `Renderer`. `BindingLookup` resolves
current, previous, and singleton addresses without exposing backend storage.
`FrameBackend` consumes a frame of the same backend family as preparation.
Backend-facing raw key/slot constructors require live resources of the declared
type and capacity; an integer alone does not establish resource validity.

Graph uploads retain `MaybeUninit` staging so padding is not read as initialized
bytes. Before queueing graph work, the renderer validates every staged destination
against backend-owned liveness, mapping, upload access, and logical byte capacity.
Uniform payload sizes must match exactly; storage payloads must fit. Allocation
padding is not writable payload capacity. Forged slot metadata cannot authorize
uploads to immutable/GPU-only storage or bypass these checks; rejection returns
`DrawError` without applying any writes, submitting, or committing graph state.
The renderer applies writes after the flight-slot wait and preserves
previously queued work. The submission callback commits texture cursors exactly
once after successful queue submission, before presentation. A pre-submit error
does not commit; a presentation error after submission does not roll back cursors.

### Indexed-indirect records

The renderer's `DrawIndexedIndirectCommand` has private fields. Construct it with
`DrawIndexedIndirectCommand::new(index_count, instance_count, first_index,
vertex_offset, first_instance)` and use its named accessors or setters. Its Vulkan
ABI is 20 bytes, alignment 4, with field offsets 0, 4, 8, 12, and 16.

`IndexedIndirectArgs` describes field values, not memory-layout compatibility.
Each `BackendTypes` implementation names its exact `IndirectCommand` type.
`IndirectDrawNode<S, P, I>` retains the argument element type, and the sealed
`CompatibleWith<B>` proof checks that exact type at preparation, recursively
through tuples, arrays, repeats, and optional nodes. External code cannot
implement the proof to bypass this check. A prepared graph also rejects another
backend's frame.

Only graph planning can construct an erased `IndirectRequest` or command batch.
Backends receive read-only buffer, offset, count, element-size, and alignment
metadata derived from the concrete record. The renderer checks buffer liveness,
byte ranges, layout, and device indirect-draw restrictions before recording.

The game builds and logically validates a render graph once in `Game::setup`.
This step does not use the renderer. Calling `graph.prepare(&mut renderer)`
consumes the logical graph against a live renderer, checking device limits.
It allocates, clears, and aliases every physical texture, and returns the
prepared graph. Only the prepared graph has an `execute` method. Each frame,
`prepared.execute(renderer, &params)` takes a tuple of per-frame values. It
performs every CPU buffer write, and submits the frame.
`execute` consumes the `FrameRenderer`.

Store the prepared graph in game state:

```rust
type Graph = PreparedRenderGraph<(ComputeNode<SpecificShaderParams>, /* ... */)>;

struct GameState {
    graph: Graph
}
```

Construction inputs are graph-owned values. Typed adapters mint them from
live renderer handles. Node constructors take `impl Into<Key>`, so a
pipeline handle reference (`&pipeline`) converts at the call site. Mint
buffer slots with `UniformSlot::from(&buffer)`, `StorageSlot::from(&buffer)`,
or `(&buffer).into()` inline. The keys are `Copy`. Each key carries the
pipeline's family and push interface in its type. A vertex-count pipeline
cannot drive an indexed draw. A push-constant pipeline cannot build a
no-push command.

The graph fixes two ordering hazards of the manual API:

- Uniform and storage writes happen inside the graph, after the flight-slot
  wait and before command recording. User code cannot order them wrong.
- Ping-pong parity does not exist in user code. The graph versions each
  logical texture and rotates physical images itself.

`examples/particles` is the minimal graph example. `examples/watercolor` is
the full one: a conditional node, a runtime-count loop, per-iteration push
blocks, a storage upload, and 14 logical textures. The everyday examples
(`basic_triangle`, `depth_texture`, `dragon`, `koch_curve`, `ray_marching`,
`recipes`, `sdf_2d`, `serenity_crt`, `suzanne`, `viking_room`) are graph-based
too: each builds and prepares one graph in `Game::setup` and executes it with
per-frame data in `Game::draw`. `examples/multi_mesh` shows 18 draws of
one node type as a node array, `[DrawNode<_>; 18]`, with a frame array of the
same length.

## Error staging

Errors arrive in three stages, each with its own scope:

- `RenderGraph::new` — logical validation, GPU-free. Every logical problem
  is reported in one aggregated error (see below). A logical graph can be
  built and validated in tests with no renderer at all.
- `graph.prepare(&mut renderer)` — device limits and resource creation:
  texture extents against `maxImageDimension2D` (checked before any
  allocation), then texture allocation, clear, and sampled-alias creation.
  A failure produces no executable graph. Resources the renderer already
  registered before the failure stay registered — the renderer's existing
  ownership rules destroy them at teardown; preparation has no rollback.
- `prepared.execute` — runtime conditions on live resources: dropped buffer
  slots and oversized uploads. These are re-checked on every execution, so
  resources dropped after preparation are rejected rather than
  dereferenced.

The stages report independently; a prepare-time error does not re-aggregate
logical errors, which were already ruled out by construction.

## Graph-local GPU-data traits

Generated shader types implement the graph's own marker traits,
`render_graph::GPUWrite` and `render_graph::PushConstantBlock` (the push
marker requires the data marker). The renderer's
`renderer::gpu_write::{GPUWrite, PushConstantBlock}` are distinct traits
that are private to the renderer crate and cover every graph implementor
through one-way blanket impls. The public traits are defined at the `mltrs_render_graph` crate root and
reexported under `renderer::render_graph`, not directly under `renderer`. There is
no blanket in the other direction: a type implementing only the renderer
trait directly cannot enter graph construction. Renderer-owned types that
never enter the graph (like `NoVertex`) keep direct renderer-trait impls.

Game-owned types implement the graph traits. Only code inside the renderer
crate can implement the backend traits directly; public renderer APIs accept
graph implementors through the blanket impls.

## Generated types

Codegen splits every params and push-constant struct by field type:

- `ParamsData` — the non-resource fields. The game supplies one value per
  frame, in the params tuple. A struct with no resource fields is its own
  data type: pass the full `Params`.
- `ParamsBindings` — the `BindlessHandle` and `Addr` fields, as graph
  reference types. The game fills this struct once at build time. The struct
  literal requires every field, so a missing binding is a compile error. A
  push block with only resource fields has no data half: its per-frame
  element is `()`.

The graph reassembles the full struct at execute time from the data value
and the resolved bindings. Type analysis is scoped to each shader. Nested
resource fields (including resources inside arrays) must be flattened into the
parameter block. Unknown handle marker types and generated `Data`/`Bindings`
name collisions produce errors naming the source shader. User fields with an
`_padding_` prefix remain data; only generated padding is zero-filled.

## Logical textures

Declare textures on `ResourcePlanner`; reference them in bindings structs
with an access mode:

```rust
let mut res = ResourcePlanner::new();
let wet_mask = res.texture("wet_mask", W, H, GraphFormat::R32Float);

// in a *ParamsBindings literal:
wet_mask.read()           // sampled; sees the most recent write
wet_mask.write()          // storage; produces the next version
wet_mask.mutate()         // storage; read-modify-write in place
wet_mask.read_previous()  // sampled; the version before the last write
```

Graph textures currently support `GraphFormat::R32Float` and
`GraphFormat::Rgba32Float`.

A read sees the most recent write in schedule order. At the first node of a
frame, that is the previous frame's final version. `read_previous` sees one
write-version back; mutations edit the current version in place and do not
move it.

The graph derives the physical image count per texture: 2 when a node reads
a texture it also writes (or any node uses `read_previous`), otherwise 1.
Physical images are created cleared. Rotation, including across the frame
boundary, is graph-internal; odd loop trip counts are legal. Version cursors
commit after successful GPU submission. A frame skipped during swapchain image
acquisition preserves the previous cursors; a presentation failure after
submission keeps the committed versions.

A sampled texture the game creates itself (for example a storage texture
filled with `write_storage_texture` at setup and then exposed through
`storage_texture_as_sampled`) binds via `handle.bindless_handle().into()`.
External sampled textures take no part in version tracking. Mutable external
storage textures cannot convert to graph storage bindings; use a logical graph
texture for storage writes.

## Buffers

Buffer handles are affine; the graph captures `Copy` slot keys from them at
build time. The game keeps the handles. `execute` checks every captured buffer
slot before planning and returns an error identifying any dropped buffer.
Keep these handles alive for the graph's lifetime, including buffers in skipped
optional nodes.

```rust
let points = StorageSlot::from(&stroke_points_buffer);
points.addr()        // Addr<T> for the current flight slot
points.read_addr()   // ReadAddr<T>

let sim = GpuOnlySlot::from(&particle_buffer);
sim.current()        // Addr<T>, current flight slot
sim.previous()       // ReadAddr<T>, previous flight slot

let table = SingletonSlot::from(&singleton_buffer);
table.addr_at(i)     // ImmutableAddr<T> of element i; bounds-checked here
```

A shader field declared `ReadAddr<T>` takes a `ReadBufferBinding<T>`. Besides
`read_addr()` (storage) and `previous()` (GPU-only), an immutable or singleton
buffer's `ImmutableBufferBinding<T>` converts with `.into()`: the binding keeps
its immutable/singleton identity, so lowering still records the read-only
access and the resolver still routes it to the immutable/singleton storage.
`examples/ray_marching` binds its sphere singleton this way.

`upload(slot)` is a node that copies a `Vec<T>` from the params tuple
into the buffer each frame; the slot is a `StorageSlot<T>`, or the storage
buffer handle it is minted from. An oversized vector returns an error identifying
the storage slot and capacity, before any staged writes reach GPU memory.

## Nodes

Nodes are plain values passed to `RenderGraph::new` as one tuple, in
execution order. Compute nodes precede draw nodes. Pipelines and uniform
buffers are created as usual; each constructor takes `impl Into<..>` for its
pipeline and its buffers, so a handle reference passes directly. An explicit
key or slot (`ComputePipelineKey::from(&pipeline)`,
`StorageSlot::from(&buffer)`, …) is `Copy` and names one resource across many
nodes:

```rust
dispatch(&compute_pipeline, &params_buffer, group_count).with_param_bindings(bindings)
dispatch(&compute_push_pipeline, &params_buffer, group_count)
    .with_param_bindings(bindings)
    .with_push_constant(push_input)
upload(&storage_buffer)
optional(node_or_tuple)          // frame element becomes Option<...>
repeat((body,))                  // frame element becomes (LoopCount, (body,))
draw_vertex_count(&vertex_count_pipeline, &params_buffer, n, bindings)
draw_indexed(&indexed_pipeline, &params_buffer, bindings)
draw_index_range(&indexed_pipeline, &params_buffer, first, count, bindings)
draw_indexed_indirect(&indirect_pipeline, &params_buffer,
    &args_buffer, first, count, bindings)
draw_indexed(&indexed_push_pipeline, &params_buffer, bindings)
    .with_push_constant(push_input)
picking::<Cursor>(&picking_pipeline)
```

An array `[N; K]` of one node type is a node. It runs its elements in index
order, and its frame element is `[N::Frame; K]`. `std::array::from_fn` builds
both arrays from one table of per-node data.

Compute parameter blocks with resource bindings require
`.with_param_bindings(bindings)` before the command can enter a graph.
Blocks whose `Bindings` type is `()` need no attachment or unit argument.
Parameter bindings and push constants can be attached in either order; each
attachment completes its own pending state and can only be supplied once.
Handwritten binding sets used with `dispatch` implement `GraphParamBindingSet`,
using `PendingParamBindings<Self>` as their `Pending` type, as generated sets do.

Every command form attaches its push block with `.with_push_constant(input)`.
The input is the generated `<Block>Input` struct. Write it as a literal, or
build it from the split halves with
`GraphShaderParams::input(&data, &bindings)`. A push block resolves per
dispatch, so inside a `repeat` its texture references rotate per iteration.
The push block's data half is fixed at build time. A command built from a
push-constant pipeline is not a node until its complete push input is
attached; a missing attachment is a compile error.

`render_graph::DrawIndexedIndirectCommand` is the indirect argument record.
Its `repr(C)` layout has five 32-bit fields, 20 bytes, and alignment 4,
matching Vulkan. It is GPU-writable, so it is both the construction input and
the buffer element type. `Renderer::create_indirect_buffer` creates an
argument buffer and initializes every flight slot from a command slice.
`Renderer::write_immutable_all_frames` and `Gpu::write_immutable` update one.
The buffer handle converts to an `ImmutableSlot<DrawIndexedIndirectCommand>`;
the slot retains the uploaded record size for byte offsets. Out-of-range
command counts and ranges panic at
construction, release builds included, because the command processor fetches
these records outside the descriptor model, where `robustBufferAccess` does
not apply. The same construction-time bounds discipline applies to `addr_at`
on `ImmutableSlot` and `SingletonSlot`.

Repeat counts and expansion budgets are the application's responsibility. The
render graph executes the supplied `LoopCount` without an aggregate iteration limit.

## Build-time validation

`RenderGraph::new` lowers and logically validates the complete graph; no
renderer, device limit, or allocation is involved. It returns all detected
logical problems in one error:

```text
render graph validation failed:
  - first problem
  - second problem
```

Texture-access diagnostics name the declared texture. Access hazards are
checked per dispatch or draw, so independent draws in the main raster pass do
not interfere with each other's checks.

The validator enforces these rules:

- A command cannot write one logical texture more than once, combine
  `mutate` with a read or write of that texture, or combine `write` with
  `read_previous`. Draw nodes can only read graph textures.
- Every texture read with `read_previous` must have a `write` somewhere in
  the graph. A `mutate` does not advance the version and does not satisfy
  this requirement.
- A repeat-body uniform block cannot reference a texture written by that
  repeat. Put that binding in the node's push block so it resolves again for
  each iteration.
- Compute and upload nodes must precede the main raster pass. A `repeat` can
  contain compute and upload nodes, but no draw. `repeat` and `optional`
  scopes cannot nest in either combination.
- An `optional` scope must contain a frame value and use an optional value as
  its gate. `optional(picking(...))` uses its cursor value and is supported.
- A graph can contain at most one picking node, and picking requires at least
  one draw.
- A reused uniform-buffer slot must resolve to the same data size and resource
  bindings at every node. Different sources for one slot are rejected.
- Pipeline kinds, per-frame value kinds, parameter schemas, and binding kinds
  must match their nodes. Resource IDs in uniform sources, push blocks, and
  raster targets are checked at their declarations, including uniform
  sources that no node consumes.
- An upload must target a storage buffer, and its declared maximum element
  count must fit the buffer. Indirect draw arguments must use an immutable
  buffer. Buffer byte offsets must fit in `u32`.
- Fixed texture dimensions must be nonzero. Extents beyond the device's
  `maxImageDimension2D` are not logical errors: preparation checks them
  against the live device before allocating.

The phase-1 description also rejects features reserved for later phases:
window-relative texture sizes, color/depth attachment texture usages,
offscreen raster targets, a second raster pass, a raster pass inside
`optional`, and per-frame dispatch group counts. These errors name the owning
future phase and ledger item.

## The params tuple

`execute` takes one element per node, in node order. Element types are the
per-shader `ParamsData` types, so the tuple literal is checked field-for-field
and arity-for-arity: a forgotten parameter, a forgotten field, and a
cross-shader transposition are all compile errors. The tuple is capped at 12
elements per level; `repeat` and `optional` nest their bodies' elements. A
node array takes a frame array of the same length, which has no cap.

Positions that carry a bare number require a newtype:

- a loop count is a `LoopCount(u32)`, exported by the module
- the picking cursor implements the `PickingCursor` marker trait

Neither position accepts a bare primitive, so a count cannot swap with an
adjacent scalar silently.

The graph's type names the node tuple once, in the game struct; after
`prepare` the stored value is the prepared graph:

```rust
type WcGraph = PreparedRenderGraph<(ComputeNode<BrushParams>, /* ... */)>;
```

## Barriers and synchronization

The graph queues through the same machinery as the manual API and emits the
same conservative barriers: a global compute-to-compute barrier between
consecutive dispatches, the cross-frame barrier, and the compute-to-graphics
barrier. A disabled `optional` node changes no barriers. The declared access
modes are the input for deriving minimal barriers later; nothing in the API
changes when that lands.

Version cursors commit immediately after successful GPU queue submission.
A frame skipped during swapchain image acquisition preserves the previous
cursors. A presentation failure after submission keeps the committed versions.

## Limits

- Tuple arity is 12 per nesting level. A node array `[N; K]` has no length limit.
- `repeat` and `optional` do not nest. A `repeat` holds compute and upload
  nodes only.
- A shader used by a graph node must have a reflected uniform parameter
  block so codegen can implement `GraphShaderParams` for it.
- Logical textures support `GraphFormat::R32Float` and
  `GraphFormat::Rgba32Float`.
- Texture dimensions are at least 1 (a logical check) and at most the
  device's `maxImageDimension2D` (a preparation-time check).
- `read_previous` requires that some node `write()` the texture. A mutate
  edits the current version in place, so mutate-only producers are rejected.
- Draw order is declaration order inside one render pass; there are no
  offscreen passes.
- Dispatch group counts, index ranges, indirect command ranges, and push
  data are fixed at build time.
- Egui stays outside the graph.
