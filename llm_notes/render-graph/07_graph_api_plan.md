# Render Graph v1 — Tuple-Params API

STATUS: IMPLEMENTED (2026-09-02). The reference documentation is
`docs/render_graph.md`; the code is
`crates/renderer/src/renderer/render_graph.rs` and
`render_graph/schedule.rs`. `examples/particles` and `examples/watercolor`
use the graph. This design replaces the execute-time API of `04_design.md`
(§3, §5, §7) and its parity concepts (§2, §4). It keeps 04's build-once
structure. Barrier *derivation* stays deferred per `06_derived_barriers.md`,
but the access declarations it needs are part of this design's bindings
structs.

Deviations found during implementation:

- **`read_previous()` was added to the access vocabulary.** Watercolor has
  three deliberate one-version-stale reads (advect samples the pre-update
  velocity; blur H and the display sample the pre-capillary wet mask).
  "Read sees the latest write" cannot express them; "the version before the
  most recent write" reproduces all three exactly. A `read_previous` forces
  two physical images; combining it with a write of the same texture in one
  node is a build error.
- The draw forms consolidated into one `DrawNode<S, P: GraphPush>` behind
  per-form constructors, instead of one struct per form.
- A repeat-body node's uniform value appears once in the params tuple but is
  staged per iteration with identical bytes; validation guarantees the bytes
  cannot differ (no rotating references in loop-body uniform blocks).
- The `mltrs shaders` fixture crate gained a `render_graph` stub module so
  generated split types compile in `check_crate`.

## Problem

The frame API misleads the reader. `FrameRenderer::dispatch` only queues
commands. The CPU buffer writes in the terminal draw's `gpu` closure run
later: after the flight-slot timeline wait, before recording
(`Renderer::draw_frame`, `crates/renderer/src/renderer.rs:2547` — wait
`:2599`, `gpu_update` `:2628`, record `:2653`, submit `:2684`). Watercolor's
draw is lexically inverted: 11 dispatches first
(`examples/watercolor/src/main.rs:698-790`), every write in the closure at
the bottom (`:839-1013`), plus three hand-flipped parity bools whose
comments must explain post-flip state.

## Goals and constraints

- A `RenderGraph` is built once in `setup` and stored in game state.
- Forgetting a per-frame parameter is a compile error.
- The graph performs every CPU buffer write itself. Write ordering is
  unrepresentable in user code.
- No parity flags, no front/back selection, anywhere in user code.
- No user-facing closures, projections, proc macros, or `macro_rules!`
  in the API. The API must port to a scripting language with full type
  inference (Roc, deferred). Branching over graph *structure* happens in
  `setup` around graph definition; the graph type is fixed per code path.
- Barriers stay conservative in v1 (identical stream to today). Hazard
  identity keys on handles and logical resources, never raw device
  addresses (`d3d12_backend_blockers.md`).
- Scope: migrate watercolor and particles. The existing `FrameRenderer`
  API stays for the other examples. The draw section is an ordered draw
  list with full parity to the current forms (vertex-count, indexed,
  index-range, indexed-indirect, each with or without push blocks, plus a
  picking node). toon_link migrates in a future step
  (`examples/toon_link/src/main.rs:961-964` is the indirect+push target).
- Future-compatible: pre-recorded command buffers, D3D12, hot reload.

## Design

### 1. Codegen: split types per shader

For each shader, codegen splits the generated uniform struct by field
type:

- `ParamsData` — non-resource fields (scalars, vectors, enums). One
  nominal type per shader. This is what the game supplies each frame.
- `ParamsBindings` — resource fields (`BindlessHandle<Sampler2D>`,
  `BindlessHandle<RwTexture2D>`, `Addr`/`ReadAddr`/`ImmutableAddr`).
  Field types become graph reference types (see §2). The game fills this
  struct once at build time. The struct literal requires every field, so
  a forgotten texture or pointer binding is a compile error.
- An assembly function combines a `ParamsData` value and resolved
  bindings into the full `#[repr(C)] Params`, which keeps its layout
  asserts and `GPUWrite` impl.

Push blocks get the same split. A push block whose fields are all
resources (`JacobiDispatch`) generates bindings only and contributes no
per-frame data. A push-block data field (`BlurDispatch::direction`) takes
a fixed value at build time in v1.

The full `Params` structs remain; the existing pipeline-creation path is
unchanged. Snapshot tests and `just shaders` cover the new types.

### 2. Resources: versions instead of parity

The builder creates logical resources. Bindings reference them with an
access mode:

- `read(res)` — sees the most recent write in schedule order. At the
  first step of a frame, that is the previous frame's final version.
- `write(res)` — produces the next version.
- `mutate(res)` — in-place read-modify-write of the current version.

The rule "a read sees the latest write" replaces parity, front/back, and
ping-pong pairs. The graph derives the physical buffering per logical
resource: 2 images when any step reads version k while writing version
k+1, 1 image when only mutated. It creates the storage textures and
sampled aliases itself and rotates them internally, including at the
frame boundary. Watercolor keeps the same 16 images with zero
bookkeeping; the `PingPong` struct, the three parity bools, and the
even-iterations assert are deleted, and odd Jacobi trip counts are legal.

The access modes are also the hazard model that
`06_derived_barriers.md` needs. v1 emits the same conservative barriers
as today; a later phase derives minimal barriers from these declarations
without an API change.

Buffer uploads and BDA fields bind the same way: a `ReadAddr<T>` binding
field takes a reference minted from a storage slot; particles'
flight-slot ping-pong binds as previous/current references on a gpu-only
slot. Slot keys are `Copy` and mint from `&Handle` (the handles derive
only `Debug`; the affine handle supports the `drop_*` discipline).

### 3. Nodes: a descriptor tuple, defined once

Nodes are plain descriptor values. The graph takes them as one tuple, in
execution order. There is no type-accumulating builder, so no type-level
append machinery and no builder-flow branching problem.

```rust
let wet_mask = res.texture(W, H, R32_SFLOAT)?;
let pressure = res.texture(W, H, R32_SFLOAT)?;

let graph = RenderGraph::new(renderer, res, (
    node(&brush_pipeline, &brush_params_buffer, GROUPS,
        paint_brush_compute::ParamsBindings {
            wet_mask: r.mutate(wet_mask),
            pressure: r.mutate(pressure),
            stroke_points: r.read_addr(stroke_slot),
        })
        .optional(),                       // per-frame element: Option<ParamsData>
    node(&update_velocity_pipeline, &update_velocity_buffer, GROUPS, ...),
    repeat::<JacobiIterations>((          // element: (JacobiIterations, (ParamsData,))
        node(&jacobi_pipeline, &jacobi_buffer, GROUPS, ...)
            .push(JacobiPushBindings {
                pressure_in:  r.read(pressure),   // resolved per iteration
                pressure_out: r.write(pressure),
            }),
    )),
    // ... remaining compute nodes ...
    draw_vertex_count(&display_pipeline, &display_buffer, 3, ...),
))?;
```

### 4. Execute: a tuple of per-frame values

Each data-bearing node contributes one element to the params tuple, in
node order. Element types are the per-shader nominal `ParamsData` types,
so transposing two elements is a compile error for every pair of nodes
that use different shaders. The tuple literal has fixed arity, so a
forgotten parameter is a compile error. v1 caps the tuple at 12 elements;
`repeat` nests its body's elements, which keeps each level under the cap.

```rust
self.graph.execute(frame, (
    painting.then(|| (brush_data, stroke_points_vec)),  // Option: disabled node skips its dispatch
    update_velocity_data,
    (JacobiIterations(2), (jacobi_data,)),
    // ...
    display_data,
))?;
```

Special element positions require newtypes through marker traits the
graph defines and does not implement for bare primitives:

- loop counts: `struct JacobiIterations(pub u32);` + `impl LoopCount`
  *(superseded: `LoopCount` is now a concrete `LoopCount(u32)` exported by
  the module; the per-game trait impl bought transposition safety only
  between two same-shader loops, which no graph has)*
- picking mouse position: a newtype + `impl PickingCursor`

A bare `u32` in a count position does not compile, so two adjacent
scalar knobs cannot swap silently.

`execute` runs in four steps: expand the schedule (evaluate `Option`
presence and loop counts, resolve versions per step — pure and
unit-testable), queue dispatches and draws through the existing
`FrameRenderer` machinery (push payloads assembled from bindings at queue
time; the auto-barrier logic in `push_dispatch`, `renderer.rs:5863`,
keeps the barrier stream identical to today, including for skipped
optional nodes), then, inside the terminal submit's internal update step
(after the flight-slot wait), assemble each `Params` from its data
element plus resolved bindings and write it. A frame that recreates the
swapchain advances resource versions exactly as it does today.

The graph type is `RenderGraph<(A, B, ...)>`. The game writes this tuple
type once, in its struct field (a type alias keeps it in one place).
Inserting a node shifts later positions; the compiler guides the
mechanical fixups. The Roc port removes the written-out type via
inference; its design is deferred.

### 5. Draw section

Draw nodes follow compute nodes in the same tuple. Declaration order is
draw order. Full parity with the current forms: vertex-count, indexed,
index-range, indexed-indirect (args buffer resolved per execute from an
immutable slot plus flight slot, never baked at build), each with or
without a push block, plus at most one picking node. Picking's
single-draw `debug_assert` (`renderer.rs:6136`) is an artifact; the
record path never reads `pending_draws`, so the graph submits through a
new non-asserting variant. The picking result stays on
`frame.picked_object_id()`. Egui stays outside the graph.

### 6. Validation at build

`RenderGraph::new` returns `Result`. v1 checks: tuple arity within cap,
a resource read before any write exists and the resource is not
persistent-initialized, more than one picking node, picking with zero
draws, nested `repeat`. Execute-time debug asserts: upload length within
slot length, indirect ranges. Hazard-derived barrier validation is a
later phase on the same access declarations.

## What this deletes from watercolor

- The `PingPong` struct and its parity methods.
- `sim_parity`, `pressure_parity`, `deposit_parity`.
- The `JACOBI_ITERATIONS` even-assert.
- Every "post-flip parity" ordering comment.
- The 11-write terminal closure; `draw` shrinks to building the params
  tuple and one `execute` call.

## Where the code lives

- `crates/renderer/src/renderer/render_graph.rs` — public API: resources,
  node constructors, `RenderGraph`, marker traits, errors.
- `crates/renderer/src/renderer/render_graph/schedule.rs` — pure core:
  step model, version resolution, physical assignment, schedule
  expansion, validation. No `ash`; unit-testable.
- `crates/renderer/src/renderer.rs` — module declaration; `pub(crate)`
  `queue_dispatch_raw`, `queue_draw_raw`, non-asserting picking-capable
  submit variant.
- `crates/renderer/src/renderer/uniform_buffer.rs`, `storage_buffer.rs` —
  `pub(super)` index accessors and by-index write/addr methods.
- `crates/cli` templates — the split-type generation.
- Tuple trait impls to arity 12 via an internal (non-user-facing) macro.

## Phases

1. **Codegen split types** (`ParamsData`, `ParamsBindings`, push splits,
   assembly fns). Verify: `just test`, snapshot review,
   `just shaders`, `cargo check --workspace --all-targets`.
2. **Slot keys and renderer accessors** (no behavior change). Verify:
   check, `just lint`.
3. **Graph core**: schedule module with unit tests (version resolution,
   physical assignment, loops with 0/odd/even trips, optional nodes,
   ordering, validation errors), descriptor tuple, arity impls, execute
   over the raw-queue helpers, vertex-count draws. Verify: check,
   `cargo test -p mltrs-renderer`, lint, `just sweep` (unchanged).
4. **Migrate particles** (smallest end-to-end proof: dispatch, draw, BDA
   previous/current, upload-free). Verify: run it, `just sweep`.
5. **Migrate watercolor** — the validation gate. Verify: interactive
   painting (all pigments, all debug views), `just sweep`, lint,
   `cargo fmt`.
6. **Draw-list parity and picking node** (indexed, index-range,
   indexed-indirect, push forms; designed against the toon_link
   pattern). Verify: check --all-targets, tests, lint, sweep.
7. **Docs**: `docs/render_graph.md`; annotate the superseded sections of
   `04_design.md`.

## Risks

- **Arity pressure.** Watercolor lands at ~11 top-level elements (10
  data-bearing uniforms with blur shared, the brush option carrying its
  upload, the Jacobi repeat). That is inside the cap of 12 with no
  headroom. Mitigations: `repeat` nesting; raising the internal arity
  impls is a one-line macro change if a graph outgrows 12.
- **Version-resolution correctness.** The schedule module is pure and
  unit-tested against watercolor's exact shape (mutate-then-read chains,
  loop rotation, frame-boundary carry). `just sweep` and an interactive
  session gate the migration.
- **Codegen churn.** Phase 1 touches every generated module and its
  snapshots. It lands alone, before any graph code.
- **Debuggability.** Physical image identity is graph-internal; debug
  labels must name the logical resource and version so RenderDoc
  sessions stay navigable.
- **BDA hazards are invisible to lavapipe validation** (image half
  only). Unchanged from today; conservative barriers guard until
  derivation lands.
- **Hot reload** is unaffected: the graph stores pipeline indices only;
  the in-place `vk::Pipeline` swap (`renderer.rs:2843`) keys off pending
  command indices, which the graph still populates.
