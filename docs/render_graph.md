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
let wet_mask = res.texture(W, H, vk::Format::R32_SFLOAT);

// in a *ParamsBindings literal:
wet_mask.read()           // sampled; sees the most recent write
wet_mask.write()          // storage; produces the next version
wet_mask.mutate()         // storage; read-modify-write in place
wet_mask.read_previous()  // sampled; the version before the last write
```

A read sees the most recent write in schedule order. At the first node of a
frame, that is the previous frame's final version. `read_previous` sees one
write-version back; mutations edit the current version in place and do not
move it.

The graph derives the physical image count per texture: 2 when a node reads
a texture it also writes (or any node uses `read_previous`), otherwise 1.
Physical images are created cleared. Rotation, including across the frame
boundary, is graph-internal; odd loop trip counts are legal.

A texture the game creates itself (for example one filled with
`write_storage_texture` at setup) binds externally via
`handle.bindless_handle().into()`. External textures take no part in version
tracking.

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

`RenderGraph::new` validates the structure and returns `Err` for: an
undeclared texture, a node that writes a texture twice, mutate combined with
read or write of the same texture in one node, a write combined with
`read_previous` of the same texture in one node, nested `repeat`, a draw
inside `repeat`, a compute node after a draw node, a repeat-body node whose
uniform block references a texture that rotates in the same repeat (move the
reference into the push block), more than one picking node, and a picking
node without draws.

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
- `repeat` does not nest, and holds compute nodes only.
- Draw order is declaration order inside one render pass; there are no
  offscreen passes.
- Dispatch group counts, index ranges, indirect command ranges, and push
  data are fixed at build time.
- Egui stays outside the graph.
