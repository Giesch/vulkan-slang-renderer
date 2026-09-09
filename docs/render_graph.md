# Render graph

A `RenderGraph` is built once in `Game::setup` and stored in game state. Each
frame, `graph.execute(renderer, &params)` takes a tuple of per-frame values,
performs every CPU buffer write itself, and submits the frame. The graph is
the terminal call: it consumes the `FrameRenderer`.

The graph fixes two ordering hazards of the manual API:

- Uniform and storage writes happen inside the graph, after the flight-slot
  wait and before command recording. User code cannot order them wrong.
- Ping-pong parity does not exist in user code. The graph versions each
  logical texture and rotates physical images itself.

`examples/particles` is the minimal graph example. `examples/watercolor` is
the full one: a conditional node, a runtime-count loop, per-iteration push
blocks, a storage upload, and 14 logical textures.

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
and the resolved bindings.

## Logical textures

Declare textures on `GraphResources`; reference them in bindings structs
with an access mode:

```rust
let mut res = GraphResources::new();
let wet_mask = res.texture(W, H, GraphFormat::R32Float);

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
boundary, is graph-internal; odd loop trip counts are legal.

A sampled texture the game creates itself (for example a storage texture
filled with `write_storage_texture` at setup and then exposed through
`storage_texture_as_sampled`) binds via `handle.bindless_handle().into()`.
External sampled textures take no part in version tracking. Mutable external
storage textures are rejected when the graph is built; use a logical graph
texture for storage writes.

## Buffers

Buffer handles are affine; the graph captures `Copy` slot keys from them at
build time. The game keeps the handles.

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

`upload(&buffer)` is a node that copies a `Vec<T>` from the params tuple
into the buffer each frame.

## Nodes

Nodes are plain values passed to `RenderGraph::new` as one tuple, in
execution order. Compute nodes precede draw nodes. Pipelines and uniform
buffers are created as usual and referenced by the constructors:

```rust
dispatch(&pipeline, &params_buffer, group_count, bindings)
dispatch_with_push(&pipeline, &params_buffer, group_count, bindings,
    push_bindings, push_data)
upload(&storage_buffer)
optional(node_or_tuple)          // frame element becomes Option<...>
repeat((body,))                  // frame element becomes (LoopCount, (body,))
draw_vertex_count(&pipeline, &params_buffer, n, bindings)
draw_indexed(&pipeline, &params_buffer, bindings)
draw_index_range(&pipeline, &params_buffer, first, count, bindings)
draw_indexed_indirect(&pipeline, &params_buffer, &args, first, count, bindings)
picking::<Cursor>(&picking_pipeline)
```

Every draw form has a `_with_push` variant taking
`push_values::<B>(bindings, data)`. A push block resolves per dispatch, so
inside a `repeat` its texture references rotate per iteration. The push
block's data half is fixed at build time.

## Build-time validation

`RenderGraph::new` lowers and validates the complete graph before creating
its logical textures. It returns all detected problems in one error:

```text
render graph validation failed:
  - first problem
  - second problem
```

Texture-access diagnostics identify the `res.texture(...)` call site. Access
hazards are checked per dispatch or draw, so independent draws in the main
raster pass do not interfere with each other's checks.

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
  its gate. In particular, `optional(picking(...))` by itself is empty.
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
- Fixed texture dimensions must be nonzero and no larger than the device's
  `maxImageDimension2D`.

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
elements per level; `repeat` and `optional` nest their bodies' elements.

Positions that carry a bare number require a newtype:

- a loop count is a `LoopCount(u32)`, exported by the module
- the picking cursor implements the `PickingCursor` marker trait

Neither position accepts a bare primitive, so a count cannot swap with an
adjacent scalar silently.

The graph's type names the node tuple once, in the game struct:

```rust
type WcGraph = RenderGraph<(ComputeNode<BrushParams>, /* ... */)>;
```

## Barriers and synchronization

The graph queues through the same machinery as the manual API and emits the
same conservative barriers: a global compute-to-compute barrier between
consecutive dispatches, the cross-frame barrier, and the compute-to-graphics
barrier. A disabled `optional` node changes no barriers. The declared access
modes are the input for deriving minimal barriers later; nothing in the API
changes when that lands.

Version cursors advance even when the submit aborts on swapchain
recreation, matching the manual API's behavior.

## Limits

- Tuple arity is 12 per nesting level.
- `repeat` and `optional` do not nest. A `repeat` holds compute and upload
  nodes only.
- A shader used by a graph node must have a reflected uniform parameter
  block so codegen can implement `GraphShaderParams` for it.
- Logical textures support `GraphFormat::R32Float` and
  `GraphFormat::Rgba32Float`.
- Texture dimensions are at least 1 and at most the device's
  `maxImageDimension2D`.
- `read_previous` requires that some node `write()` the texture. A mutate
  edits the current version in place, so mutate-only producers are rejected.
- Draw order is declaration order inside one render pass; there are no
  offscreen passes.
- Dispatch group counts, index ranges, indirect command ranges, and push
  data are fixed at build time.
- Egui stays outside the graph.
