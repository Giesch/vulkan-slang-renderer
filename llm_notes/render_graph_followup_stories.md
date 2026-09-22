# Render-graph example adoption — the four deferred follow-up stories

Written: 2026-09-22, against revision `fd449ac` (working tree clean).
Lineage: split out of the example-graph-adoption plan (`plan-001.md` in
session `0b5t7p-rich`) and its execution report; that story migrated the
eleven everyday examples (commit `5ac52e3`) and left these four crates on the
legacy draw path, each behind one missing graph capability.

**Status: none of this work is started.** This document is a plan, written
before the work; per this directory's contract, verify every claim against
the code before acting on it. Scope accounting: 13 of the 17 non-Roc examples
now run prepared render graphs (11 from the adoption story + the particles and
watercolor references); these four stories complete the set.

Grounding note: since the adoption story, the graph gained const-generic node
arrays — `[N; K]` of one homogeneous node type, no length limit
(`crates/render-graph/src/runtime.rs:1369`, `docs/render_graph.md:257`). They
do **not** unblock any story below: `K` is a compile-time constant, and every
blocked workload here is sized by *setup-time data*.

Common shape for all four stories (mirror the adoption story): ground and
plan first with the three-layer review; land the capability with graph-crate
tests (backend-neutral — see `crates/render-graph/AGENTS.md`) plus renderer
compile-harness coverage where the API surface changes; update
`docs/render_graph.md` wherever a "Limits" entry becomes stale; then migrate
the example with a concrete alias/state/setup/execute split, an example-local
regression test against production helpers, and the standard gates
(`cargo check --workspace --all-targets`, `cargo test -p mltrs-render-graph`,
`cargo test -p mltrs-renderer`, `just test`, `cargo fmt`, `just lint`,
`just sweep`, plus before/after parity evidence).

---

## Story 1 — Space Invaders: per-frame vertex count

**Story.** Migrate `examples/space_invaders` to a prepared render graph by
adding graph support for a draw whose vertex count is a per-frame value.

**Current behavior** (`examples/space_invaders/src/main.rs`):
- `draw` computes `vertex_count = visible_sprites as u32 * 6` at line 386 —
  the count depends on which sprites carry `SPRITE_FLAG_VISIBLE` this frame
  (lines 381–385) — then calls `renderer.draw_vertex_count(&self.pipeline,
  vertex_count, …)` (line 388).
- Everything else in the closure is already graph-expressible: one
  `SpaceInvadersParams` uniform (projection, read bindings for the
  `sprites_buffer` and `debug_boxes_buffer` storage buffers, the sprite-sheet
  bindless texture — lines 389–396), one per-frame storage write of the
  debug boxes (line 398), and one per-frame **re-sorted** upload of the
  sprites (`self.sprites.clone()`, `sort_by(sprite_draw_order)`, write at
  lines 400–402; the clone-then-sort per frame is pre-existing behavior —
  preserve it verbatim in the migration).
- Owners: `sprites_buffer`/`debug_boxes_buffer` (storage),
  `params_buffer`, `sprite_sheet_texture`; `sprites`/`debug_boxes` Vecs are
  mutated in `update`/`draw`.

**The gap.** `draw_vertex_count` fixes `vertex_count: u32` at build time
(`crates/render-graph/src/runtime.rs:998–1007`,
`LowerDrawCall::VertexCount(vertex_count)`); `docs/render_graph.md` Limits:
"Dispatch group counts, index ranges, indirect command ranges, and push data
are fixed at build time" (draw vertex counts are covered by the same rule in
spirit — the doc lists them via the node form at `docs/render_graph.md:247`).

**Required capability.** A per-frame vertex count as a first-class frame
value, in the established newtype style so it cannot silently swap with an
adjacent scalar (`LoopCount` precedent, `docs/render_graph.md:380`):
- A `VertexCount(u32)`-style frame element (name to be settled in planning;
  consider `DrawCount` if it should also cover future per-frame instance
  counts) with `PickingCursor`-like marker treatment if appropriate.
- Either a new node constructor (`draw_vertex_count_per_frame` or an
  `Option`-like variant) or an extension of `DrawNode`'s `Frame` to carry the
  count; lowering (`LowerDrawCall`), validation ("fixed at build time" rule),
  the renderer recording path, and the compile harness all need cases.
- Validation questions to settle in planning: interaction with the
  "at most one picking node" rule is irrelevant here, but zero-count frames
  (no visible sprites) must be legal — `vkCmdDraw` with count 0 is a no-op
  and the example relies on frames where everything is invisible (game-over
  screens toggle visibility).

**Migration sketch.** Graph =
`(upload(sprites), upload(debug_boxes), DrawVertexCountNode<SpaceInvadersParams-with-per-frame-count>)`;
frame = sorted sprites `Vec`, debug boxes `Vec`, and the count newtype plus
`SpaceInvadersParamsData`. Preserve the `window_resolution`-based projection
and the sort order exactly.

**Acceptance criteria (draft).**
- AC1: example stores a concrete prepared graph, builds/prepares in setup,
  executes in draw; no legacy path remains.
- AC2: vertex count tracks the live visible-sprite count (a regression test
  calls the production frame-input helper with differing visible counts and
  asserts the count newtype and sprite order).
- AC3: sorted-upload behavior and params values are byte-identical to legacy
  (helper test on the sort); gates + sweep + parity evidence recorded.
- AC4: docs "Limits" entry updated; graph-crate test covers the new frame
  kind end-to-end on the fake backend; compile-harness case added.

## Story 2 — Sprite Batch: immutable-buffer upload

**Story.** Migrate `examples/sprite_batch` by letting graph uploads target
immutable buffers (or, alternatively, an operator-approved buffer-kind
adaptation of the example).

**Current behavior** (`examples/sprite_batch/src/main.rs`):
- `sprites_buffer` is an `ImmutableBufferHandle<Sprite>` created at setup for
  `SPRITE_COUNT = 8192` (line 80), bound read-only in the params via
  `gpu.current_immutable_addr(&self.sprites_buffer)` (line 146).
- Every `update` randomizes all sprites (lines 131–133, SDL3 RNG), and `draw`
  re-uploads the whole payload with `gpu.write_immutable(&mut
  self.sprites_buffer, &self.sprites)` (line 153) — the payload genuinely
  changes every frame; this is not a setup-once buffer wearing the wrong
  type.
- One `SpriteBatchParams` uniform per frame (fixed orthographic projection
  from the fixed initial window size, atlas texture bindless), draw is
  `renderer.draw_vertex_count(&self.pipeline, sprites.len() * 6, …)` — a
  *constant* count, so no Story-1 dependency.

**The gap.** The graph's `upload(slot)` mints only from
`StorageSlot<T>` (`crates/render-graph/src/runtime.rs:1237`,
`pub fn upload<T: GPUWrite>(slot: impl Into<StorageSlot<T>>)`), and only
`&StorageBufferHandle<T>` converts into it
(`crates/renderer/src/renderer/render_graph.rs:21`). An immutable buffer
uploads per frame in the legacy API (`write_immutable`) but has no graph
path. Note the lowered validator already accepts `Storage | Immutable`
upload targets (`crates/render-graph/src/runtime/validate.rs`, the
`UploadTargetKind` check), so the gap is the public constructor surface, not
the execution machinery.

**Required capability.** An immutable upload node: either widen `upload` to
a slot-agnostic form (`impl Into<UploadSlot<T>>` with `From<&
ImmutableBufferHandle<T>>`) or add `upload_immutable`. Decide in planning:
whether immutable uploads stage like storage uploads (same
`MaybeUninit` staging discipline, `crates/render-graph/AGENTS.md` contract)
and what the renderer's batch preflight must check for immutable mapping.
Graph-crate fake-backend test + renderer upload regression coverage + a
compile-harness case; doc line in the Buffers section of
`docs/render_graph.md` ("`upload(slot)` … the slot is a `StorageSlot<T>`" —
`docs/render_graph.md:224` — needs the widened wording).

**Alternative needing operator sign-off:** retype the example's buffer to
`create_storage_buffer` and upload via the existing storage path. GPU-visible
behavior is identical (rewritten in full every frame), but it changes the
buffer kind and the params' address type (`current_immutable_addr` →
`addr`), which is why the adoption plan parked it as "immutable-buffer upload
support **or** separately approved shader/buffer adaptation". Prefer the
capability; fall back only on an explicit decision.

**Acceptance criteria (draft).** Concrete graph with two uploads replaced by
one immutable upload + params; payload capacity and per-frame rewrite
preserved (helper test asserting two successive frames carry different
payloads of exactly `SPRITE_COUNT` sprites); gates + sweep + parity; docs and
tests as above.

## Story 3 — GPU Picking: staging the picking pipeline's uniform

**Story.** Migrate `examples/gpu_picking` by giving the graph's picking node
a params buffer and bindings like every other node.

**Current behavior** (`examples/gpu_picking/src/main.rs`):
- Two pipelines: the visual `DrawVertexCount` pipeline (params:
  camera, `picked_object_id` read back via `renderer.picked_object_id()`
  — a two-frame-old readback — cube count, cubes read binding) and a
  `PickingPipelineHandle` from the `gpu_picking_id` shader with its own
  `GpuPickingIdParams` uniform (camera, count, cubes read binding).
- `draw` (lines 98–130) calls `renderer.draw_vertex_count_with_picking(
  &self.pipeline, 3, &self.picking_pipeline, mouse_position, |gpu| { … })`
  writing **both** uniforms and the cubes storage inside the closure.
- `input` tracks `MouseMotion` into `mouse_x`/`mouse_y`.

**The gap.** The graph picking node is uniform-less:
`pub fn picking<C: PickingCursor>(pipeline: impl Into<PickingPipelineKey>) ->
PickingNode<C>` (`crates/render-graph/src/runtime.rs:1208`), `Frame = C`
(the cursor), and `plan` only registers `PickingDrawConfig`. There is no way
for a graph to write `GpuPickingIdParams` each frame, so the picking pass
would run with a stale uniform. (Contrast: everything else in the example —
cubes storage upload, visual params, read bindings — is expressible today,
and the picking node's cursor-frame contract already matches this example's
`mouse_position` value; `optional(picking(...))` is even supported,
`docs/render_graph.md:347`.)

**Required capability.** A picking node constructor taking
`params_buffer` (+ `Bindings`) for the picking pipeline's parameter block,
staged per frame exactly like a draw node's: probably
`picking_with_params::<C>(&pipeline, &params_buffer, bindings)`, with the
generated `GpuPickingIdParamsData`/`GpuPickingIdParamsBindings` split already
present in `examples/gpu_picking/src/generated/shader_atlas/gpu_picking_id.rs`.
Lowering/validation rules to extend: the "at most one picking node /
picking requires at least one draw" checks unchanged; the new node must
respect the same uniform-slot reuse rules as draws. Renderer side: the
picking pass recording already consumes a uniform buffer in the legacy path,
so the backend work is staging + slot liveness, not new Vulkan. Graph-crate
tests + compile-harness case + doc updates in the Nodes and Limits sections.

**Migration sketch.** Graph = `(upload(cubes), draw_vertex_count(visual),
picking_with_params(picking, id_params, bindings))` with frame = cubes `Vec`,
`GpuPickingParamsData` (carrying the readback id), cursor value,
`GpuPickingIdParamsData`. `picked_object_id()` is read *before* `execute`
consumes the frame renderer, like aspect reads in the adopted examples.

**Acceptance criteria (draft).** No legacy path; helper test asserting both
param sets track a moved mouse and changed cubes (mirroring
`ray_marching_frame_input_tracks_boxes_and_resolution`); the picking
readback's two-frame-old semantics documented in the example; gates + sweep +
parity including a hover-pick interaction check where capturable.

## Story 4 — Toon Link, both modes: setup-sized material draw lists

**Story.** Migrate `examples/toon_link` (GameCube and Modern modes) by adding
a graph construct for a sequence of indirect draws whose length is fixed at
setup by loaded asset data, with per-element push constants.

**Current behavior**:
- GameCube mode (`examples/toon_link/src/main.rs`): setup builds a draw list
  from the converted manifest — 24 batches partitioned into `runs: Vec<Run>`
  by pipeline (5 pipelines, 7 recorded `cmd_draw_indexed_indirect` calls;
  header comment lines 3–4, `Run` at line 852, partitioning at lines
  844–920). Each frame, `draw` queues one
  `queue_draw_indexed_indirect_with_push_constants` per run (lines 975–986,
  1135–1137): per-run `MultiDraw { individual_draws }` push block pointing
  at its span of the `individual_draw_buffer` singleton, args from the
  immutable `args_buffer`, then `submit_draws` writes the single shared
  `ToonLinkParams` uniform (lines 1163–1167). The material buffer is
  write-once at setup.
- Modern mode (`examples/toon_link/src/modern.rs:752–765`): same shape with
  `pipeline_order` (one indirect draw per pipeline) and `ModernMultiDraw`
  push blocks, one shared `ModernParams` uniform.
- The mode switch (`ToonLinkHost`) alternates two independent `Game`s, so
  each mode can carry its own prepared graph.

**The gap.** The run count is manifest data known only at setup, but a
graph's node count is a compile-time tuple/array shape. Node arrays
(`[N; K]`, `crates/render-graph/src/runtime.rs:1369`) need a const `K`;
nested tuples likewise. The adoption story's multi_mesh could spell out 18
nodes literally because its workload is a source constant — Toon Link's is
not. Additionally each run needs its own push constant value fixed at setup
(`MultiDraw { individual_draws }` differs per run), which tuple-of-nodes
would express but a runtime-sized list cannot today; and `repeat` cannot hold
draw nodes (`docs/render_graph.md:409–410`).

**Required capability — the biggest item in this set.** A "setup-sized draw
list" node: homogeneous indirect-draw commands, count fixed when the graph is
built (from `runs.len()` / `pipeline_order.len()`), each element carrying its
own build-time push input and args range, executed in list order inside the
main raster pass. Design space to settle in planning:
- a `DrawListNode`-style runtime-sized container validated once at
  `RenderGraph::new` (order, args ranges, per-element push compatibility),
  lowering to the same `LowerDrawCall::IndexedIndirect` stream the tuple path
  uses; frame element stays the single shared params value (per-run data is
  build-time, matching legacy);
- versus teaching the validator about "one draw node, N commands" where N is
  captured at build;
- interaction with the phase-1 exclusions (`docs/render_graph.md` Limits:
  per-frame dispatch counts etc.) — this must remain a *setup*-sized list,
  not a per-frame-resized one, or it wanders into reserved future phases.
Renderer side: indirect recording already exists
(`draw_indexed_indirect` node + `DrawIndexedIndirectCommand` ABI,
`docs/render_graph.md:34–51`). Tests: graph-crate ordering/validation
coverage at several list sizes (1, 2, many), compile-harness case, docs
(Limits + Nodes).

**Migration sketch.** GameCube: graph = `(upload? none — material buffer is
setup-only, DrawListNode<ToonLinkParams, MultiDraw> sized runs.len(),
shared params write)`; frame = `ToonLinkParamsData`. Modern: same with its
pipeline count. The per-run push inputs and singleton slice offsets are
build-time values from `draw_list.runs` / `pipeline_order`.

**Acceptance criteria (draft).** All runs' pipeline associations, order, args
ranges, and push blocks preserved (a helper test pins the run table exactly,
like `multi_mesh_draw_ranges_and_pipeline_order`); both modes migrate; the
7-vs-24 accounting from the header comment stays observable; hot-reload
limitation documented in `docs/toon_link.md` is unaffected; gates + sweep
(toon_link needs its machine-local converted assets — record skips plainly
where absent) + parity including the mode switch.

---

## Appendix — non-blocking follow-ups recorded by the adoption review

- **Node-construction inspection seam.** The adoption story's Test Audit
  found that constructed draw nodes are opaque to example tests (private
  `DrawNode` fields; no public accessors or recording backend), so a typo'd
  node tuple entry would pass all automated checks. The operator accepted
  this as residual risk; a follow-up could add a minimal public inspection
  capability (read accessors or a recording `FrameBackend`), which would also
  harden this document's Story 4 tests.
- **Parity evidence waived in the adoption story** (machine-limited there):
  multi_mesh/watercolor window captures, wide/tall resize exercise, and egui
  interaction. Any of these can be produced on a capable machine later; the
  waiver is recorded in the adoption execution report, not here.
- The five-layer review pattern and the verification gates used by the
  adoption story are the template for each story above; the execution-report
  habit (baselines, gates, evidence tables, explicit gaps) should be reused.
