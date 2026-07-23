# Watercolor Race Fixes: Per-Dispatch Compute Streams

> **STATUS: PLANNED.** Implementation plan for replacing the renderer's
> pipelined-compute *mode* with two coexisting per-frame command streams
> selected per dispatch. This is Phase 0.5 of the render-graph plan
> (`render-graph/04_design.md` §11), independently motivated: it deletes the
> mode-toggle footgun and the frame-0 barrier gap. Line anchors are current as
> of commit `d574950`.

## Motivation (current state)

The watercolor display pass races this frame's compute across queues: the
display samples the slot the concurrently-running compute is writing. The
finding is documented in `render-graph/04_design.md` §8 and is **currently
unfixed** in the example — `examples/watercolor.rs` calls
`enable_pipelined_compute()` (`:414`), so graphics N waits only on compute N−1
while frame N's compute and graphics run concurrently. The eventual home for
the fix is the render graph's per-resource `CrossFrameMode` (§8/§10 there).

This refactor does **not** itself fix the watercolor race. It removes the two
renderer-level footguns that stand in the way of any clean fix (below), and
gives the graph's `.simulation()` / `.rendering()` sections a 1:1 runtime
target.

## Renderer footguns this closes

1. **Frame-0 barrier gap.** The first compute frame is rerouted through the
   combined path (`use_pipelined` requires `compute_frames > 0`,
   `src/renderer.rs:2096`), where compute is recorded at the top of the
   graphics CB (`:1399-1416`) with **no** compute→graphics barrier — same-queue
   submission order provides no execution/memory dependency, and the
   example's own barriers are COMPUTE→COMPUTE only. Benign today (blank
   canvas) but wrong, and every non-pipelined app carries the same footgun
   (particles protects itself with a manual COMPUTE→VERTEX barrier,
   `examples/particles.rs:97`).
2. **Mode-dependent semantics.** The same `dispatch()` call means different
   queues, different legal barrier stage masks, and different data-visibility
   guarantees depending on a global flag — the app cannot even emit the right
   barrier itself, because in pipelined mode its commands land on a possibly
   compute-only queue where FRAGMENT-stage barriers violate queue-capability
   VUIDs.

## Design (decisions settled in discussion)

Two per-frame command streams that always coexist; the domain is a property
of each dispatch, not of the renderer:

| Stream | API | Recorded into | Sync guarantee (renderer-owned) |
|---|---|---|---|
| **Frame** | `dispatch()`, `memory_barrier()` (names unchanged — the unmarked path is the always-correct one) | top of the graphics CB | trailing global barrier COMPUTE→VERTEX\|FRAGMENT, SHADER_WRITE→SHADER_READ, emitted whenever the stream is non-empty |
| **Pipelined** | `dispatch_pipelined()`, `memory_barrier_pipelined()` | separate compute CB / compute queue | timeline semaphore; graphics waits compute N−1 (default) or N (synced policy) |

- **No mode flags.** `ComputePlacement` (`:5149`), `pipelined_compute` /
  `enable_pipelined_compute()` (`:853`) are deleted; "pipelined-ness" is
  which list has commands this frame. The one surviving knob is the sync
  policy: the current `enable_pipelined_compute()` (implicitly always
  previous-frame) becomes an explicit
  `set_pipelined_compute_sync(PipelinedComputeSync::{PreviousFrame, SameFrame})`
  (default `PreviousFrame`, matching today's behavior). The render graph's
  per-resource `CrossFrameMode` subsumes this later.
- **Frame-0 reroute dropped** (accepted behavior change): a
  `PreviousFrame`-sync app's graphics reads initial texture/buffer state on
  frame 0 — semantically consistent ("previous output" of frame 0 *is* the
  initial state). `SameFrame` apps get the same guarantee on frame 0 as every
  other frame.
- **Cross-stream rules:** frame-stream compute may read pipelined output
  (the graphics submit's compute-timeline wait already includes the COMPUTE
  stage, `:2166-2173` region); pipelined work must NOT read this frame's
  frame-stream output (nothing orders it) — a domain rule the graph will
  enforce; document on `dispatch_pipelined` until then.

## Implementation steps

### Step 1 — split the pending stream (`src/renderer.rs`)

- `FrameRenderer` (`:5171`): `pending_compute: Vec<PendingComputeCommand>`
  (`:5173`) becomes `pending_frame_compute` + `pending_pipelined_compute`.
- `dispatch`/`memory_barrier` (push sites `:5212`, `:5225`) push to the frame
  list; add `dispatch_pipelined`/`memory_barrier_pipelined` pushing to the
  pipelined list. Doc-comment the cross-stream rules above on the pipelined
  methods.
- The terminal draw methods hand both lists to `draw_frame` (`:2023` takes one
  `pending_compute` today).

### Step 2 — submission records both streams every frame

- **Graphics CB** (`record_command_buffer`, `pending_compute` params at
  `:1296`/`:1383`/`:1405`): always record `pending_frame_compute` at the top
  (the current record site, minus the `compute_placement` parameter, which goes
  away), then — when non-empty — the automatic barrier:
  ```rust
  let barrier = vk::MemoryBarrier2::default()
      .src_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
      .src_access_mask(vk::AccessFlags2::SHADER_WRITE)
      .dst_stage_mask(vk::PipelineStageFlags2::VERTEX_SHADER
          | vk::PipelineStageFlags2::FRAGMENT_SHADER)
      .dst_access_mask(vk::AccessFlags2::SHADER_READ);
  ```
  Always legal here: this CB is always on the graphics queue. Stage mask
  mirrors the pipelined path's semaphore wait stages.
- **Compute CB**: record + submit `pending_pipelined_compute` when non-empty
  (the current `use_pipelined` branch at `:2101`, minus the `compute_frames > 0`
  gating and the mode flag).
- **Timeline bookkeeping for gap frames**: today `compute_value =
  compute_frames + 1` (`:2099`) assumes a compute submit every frame once
  active. With per-frame emptiness possible, track
  `last_submitted_compute_value` and have graphics wait on it (or it minus
  the sync offset), and keep the CB-reuse pre-wait keyed to actual submits.
  (Watercolor submits every frame, but the renderer must not deadlock or
  mis-wait for apps that don't.)
- Delete `ComputePlacement`, `pipelined_compute`, `enable_pipelined_compute`;
  add `set_pipelined_compute_sync` (field `:174`, init `:455`, method `:853`,
  wait selection `:2158-2173`).

### Step 3 — migrate the examples

- **watercolor** (`examples/watercolor.rs`): all 10 `renderer.dispatch` sites
  in `draw` (one is inside the Jacobi loop) → `dispatch_pipelined`; the
  `compute_barrier` helper (`:229`, 10 call sites) switches its body to
  `memory_barrier_pipelined`; `enable_pipelined_compute()` (`:414`) →
  `set_pipelined_compute_sync(SameFrame)`. This is a pure plumbing change: it
  moves watercolor onto the explicit pipelined stream and requests the
  same-frame wait, which orders the display's cross-queue read of the deposit
  chain (the primary visible race). It does **not** address the in-place-RW
  ping-pong resources (brush / flow-outward use `read_storage`,
  `examples/watercolor.rs:91-93`) — those still need write-forward
  restructuring or the graph's `CrossFrameMode` (04_design.md §8 fact (b),
  §10). The 2-slot `PingPong` (`:77-94`) and parity fields (`:185-187`) are
  unchanged by this step.
- **particles** (`examples/particles.rs`): keep `dispatch()` (frame stream);
  **delete** the manual COMPUTE→VERTEX barrier at `:97` — the renderer now
  guarantees it. Update its comment if any.
- Sweep remaining examples for `dispatch(`/`memory_barrier(` — all other
  users are frame-stream and unchanged.

### Step 4 (follow-up, not this change) — typed domains

Choose the domain at pipeline creation
(`PipelineHandle<Compute, PipelinedDomain>`) so `dispatch_pipelined` only
accepts pipelined handles. Coordinate with
`bda_footguns/03_pipelined_current_read_plan.md`: key domain assignment off
the pipeline rather than the shader kind (03's shader-kind conservatism stays
sound in the interim — see `render-graph/04_design.md` §9).

## Verification

1. `cargo check --all`, `just test`, `just lint`.
2. `timeout 3 just dev watercolor` and `timeout 3 just dev particles` — zero
   validation errors/warnings; watercolor's frame 0 now runs the pipelined
   path (confirm no combined-path fallback remains).
3. Grep-level checks: no remaining references to `ComputePlacement`,
   `enable_pipelined_compute`, or `compute_frames > 0` gating; particles has
   no manual barrier.
4. Behavior: paint in watercolor — strokes appear and spread as before;
   toggle the wet-mask debug view.

## Relationship to the render graph

This is the runtime substrate for `render-graph/04_design.md` §2/§9: graph
sections map 1:1 onto these streams (`.simulation()` → pipelined,
`.rendering()` → frame stream + terminal draw), the `Pipelining::Auto|Off`
build option becomes unnecessary, and the frame stream's automatic barrier is
the manual-mode counterpart of the barrier the graph's hazard model derives
for compute→draw edges.
