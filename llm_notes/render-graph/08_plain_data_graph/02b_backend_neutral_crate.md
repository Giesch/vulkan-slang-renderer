# Phase 2b — Backend-Neutral Crate and Everyday Example Migration

STATUS: IMPLEMENTED. The crate extraction landed on 2026-09-13 and the
example migration on 2026-09-17. This work has no phase in
[`../08_plain_data_graph.md`](../08_plain_data_graph.md). It is recorded
here because the parent plan's later phases build on the resulting layout.
It closes no ledger row.

## Constraints

Preserve the public tuple API, generated split types, constructor names,
frame types, picking compatibility, and submission-aware cursor commits.
Keep the graph's dependency closure free of the renderer, ash, vk-mem, SDL,
and shader-slang. Keep one nominal identity for each ABI wrapper type, owned
by the graph and re-exported by the renderer. Do not wire the pure compiler
into execution; that remains phase 3.

## What landed

### The `mltrs-render-graph` crate

`crates/render-graph` owns construction, validation, lowering, planning,
staging, and execution orchestration. Its dependencies are `anyhow` and
`serde`. `tests/dependency_boundary.rs` walks `cargo metadata` and asserts
that no renderer, ash, vk-mem, SDL, or shader-slang package appears in the
resolved closure. `AGENTS.md` in the crate records its contracts.

Modules:

| File | Owns |
| --- | --- |
| `src/lib.rs` | `GPUWrite`, `PushConstantBlock`, module declarations |
| `src/addr.rs` | `Addr<T>`, `ReadAddr<T>`, `ImmutableAddr<T>` |
| `src/bindless.rs` | `BindlessHandle<T>`, `Sampler2D`, `RwTexture2D` |
| `src/backend.rs` | `GraphFormat`, `PhysicalImage`, `BufferAddressKind`, `BackendTypes`, `PreparationBackend`, `BindingLookup`, `FrameLookup`, `FrameBackend`, `IndexedIndirectArgs` |
| `src/commands.rs` | `CommandBatch`, `PendingDrawCommand`, `DrawCallConfig`, `IndirectRequest`, `PickingDrawConfig`, `PushConstantBytes` |
| `src/runtime.rs` | Typed node API, `RenderGraph`, `PreparedRenderGraph`, `PlanCtx`, `BindingResolver`, `GraphShaderParams`, slot and pipeline keys, `CompatibleWith` |
| `src/runtime/{desc,validate,lower,compile,expand}.rs` | The phase-1 pure core, moved byte-for-byte where possible |
| `src/runtime/{test_desc,backend_tests}.rs` | Test-only desc builder and a deterministic fake backend |

The renderer's graph traits `renderer::gpu_write::{GPUWrite, PushConstantBlock}`
are private and cover every graph implementor through one-way blanket impls.
The `private_backend_traits` harness case checks that the backend traits are
not nameable through either renderer path.

### Backend contract

The renderer implements five traits in
`crates/renderer/src/renderer/graph_backend.rs`:

- `BackendTypes for Renderer`: `IndirectCommand = DrawIndexedIndirectCommand`,
  `Resource = (StorageTextureHandle, TextureHandle)`.
- `PreparationBackend for Renderer`: `max_image_dimension_2d`, and
  `prepare_image`, which creates, clears, and aliases one storage texture and
  returns its `PhysicalImage` plus the owned handles.
- `BindingLookup for Renderer`: current, previous, and singleton buffer
  addresses by index.
- `FrameLookup for Renderer`: liveness of uniform, storage, and singleton
  slots, and the whole-index count of a pipeline.
- `FrameBackend for FrameRenderer`: `submit(batch, on_submitted)`.

`crates/renderer/src/renderer/render_graph.rs` is the compatibility facade:
`pub use mltrs_render_graph::*`, the `PreparedRenderGraph<N>` alias
specialized to `Renderer`, `From<&Handle>` for the five slot kinds and the
five pipeline keys, and `ToVk for GraphFormat`.

### Lifecycle

- `RenderGraph::new(resources, nodes)` lowers and validates. It needs no
  renderer, so a logical graph can be built and rejected in a unit test.
- `graph.prepare(&mut renderer)` consumes the logical graph. It runs
  `extent_limit_errors` before any allocation, allocates every physical
  image, and returns `PreparedRenderGraph<N, B>`, which retains the backend's
  keepalives. Preparation has no rollback; resources the renderer registered
  before a failure stay registered under its teardown rules.
- `prepare` requires `N: CompatibleWith<B>`. The proof is sealed and
  recursive through tuples, arrays, `RepeatNode`, and `OptionalNode`. An
  `IndirectDrawNode<S, P, I>` satisfies it only when `I` is the backend's
  exact `IndirectCommand`. External code cannot implement the proof.
- `PreparedRenderGraph::execute(frame, &N::Frame)` checks every captured
  buffer slot for liveness, plans through `PlanCtx`, and calls
  `FrameBackend::submit` with the cursor install as `on_submitted`. A
  prepared graph rejects a frame from another backend family.
- `Renderer::draw_frame` calls `on_submitted` after `queue_submit2` and
  before `queue_present_khr`. A presentation error does not undo the commit.

### Upload and indirect preflight

`FrameRenderer::submit` validates every staged destination before it queues
any work: slot liveness, upload kind (uniform, storage; read-only never
accepted), mapped memory, exact payload size for uniforms, and fit for
storage. Allocation padding is not writable capacity. Indirect draws also
pass `validate_indirect_layout`: record size, alignment, and stride identity
with `DrawIndexedIndirectCommand`, 4-byte offset alignment, a nonzero count
within `maxDrawIndirectCount`, and the end of the range within the
allocation. A rejected batch returns `DrawError` before any write,
submission, or cursor commit. Staged bytes are `MaybeUninit<u8>`, so Rust
padding is never read as initialized.

### Indexed indirect draws

`DrawIndexedIndirectCommand` in `crates/renderer/src/renderer/indirect.rs`
has private fields, a `new` constructor, and a 20-byte, 4-aligned `repr(C)`
layout. `IndexedIndirectArgs` exposes field values, not an ABI guarantee.
`IndirectRequest` has private layout fields; only graph planning constructs
one. `Renderer::create_indirect_buffer` creates and initializes an argument
buffer in one call. Out-of-range command counts panic at construction in
every build, because the command processor fetches these records outside
the descriptor model.

### Node arrays and read-only bindings

`[N; K]` implements `GraphNode` with `Frame = [N::Frame; K]`. The array
lowers and plans in index order. `examples/multi_mesh` holds 18 draws as one
array built with `std::array::from_fn`. `ImmutableBufferBinding<T>` converts
into `ReadBufferBinding<T>`, so an immutable or singleton buffer can feed a
`ReadAddr<T>` shader field; `examples/ray_marching` binds its sphere
singleton this way.

### Example migration

Migrated to the graph on 2026-09-17: `basic_triangle`, `depth_texture`,
`dragon`, `koch_curve`, `multi_mesh`, `ray_marching`, `recipes`, `sdf_2d`,
`serenity_crt`, `suzanne`, `viking_room`. Each builds and prepares one graph
in `Game::setup` and executes it with per-frame data in `Game::draw`.
`particles` and `watercolor` were already graph-based.

On the manual `FrameRenderer` API: `toon_link` (both rendering modes; the
phase-4 target), `gpu_picking` (the deferred picking path), `space_invaders`,
`sprite_batch`.

## Validation evidence

- `cargo test -p mltrs-render-graph`: 129 lib tests, 1 dependency-boundary
  test, 4 doctests (3 compile-fail on `PendingPush`). The fake backend covers
  extent rejection before allocation, physical image counts, keepalive drops,
  partial preparation failure, current/previous addresses, dropped buffers,
  upload capacity, command plans, and submission-time cursor commits.
- Renderer unit tests: `indirect_buffer_layout_ranges` in `graph_backend.rs`
  and three upload regressions in `graph_backend/upload_tests.rs` (forged
  storage capacity, unmapped or wrong-kind or missing destinations, forged
  uniform type against logical capacity).
- Compile harness cases added by this work: `direct_graph_import`,
  `extracted_type_identity`, `same_indirect_backend`, `array_nodes`,
  `construction_families`, `prepared_lifecycle`, `trait_bridge`;
  `wrong_indirect_backend`, `wrong_indirect_backend_nested`,
  `wrong_prepared_frame_backend`, `external_compatible_with_impl`,
  `indirect_request_fields`, `wrong_indirect_element`, `logical_no_execute`,
  `consumed_after_prepare`, `private_backend_traits`,
  `push_requires_graph_gpu_write`.
- `docs/render_graph.md` and `docs/testing.md` describe the resulting API
  and test surface.

## Relation to the plan

- No ledger row closes. Production execution is still `plan()`;
  `compile()` and `expand()` remain test-only.
- The crate boundary is the representation boundary the Roc target update
  names: a scripting platform would implement the backend traits or sit
  behind them.
- Decision 4 (three-part key identity) is not implemented. Slot keys are
  bare `Copy` indices and `execute` re-checks liveness each frame
  (`../../tech_debt.md` item 18).
- Decision 3's copy/borrow subchoice is still open; `UploadNode::Frame` is
  `Vec<T>` (`../../tech_debt.md` item 19).
- Setup-length draw lists (phase 4, S4) remain open. `[N; K]` requires a
  compile-time element count.
