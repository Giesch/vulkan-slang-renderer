# Removing pipelined compute; collapsing the frame ring to 2 slots

Status: **IMPLEMENTED, 2026-07-28.** All five phases landed, one commit per
phase (Phase 4 split into 4a and 4b/4c). Deviations from the plan as written:

- **Phase 4a needed a fix the plan did not anticipate.** Reordering to
  wait → acquire naively would bump `total_frames` before the acquire, leaving
  a never-signalled timeline gap on every `ERROR_OUT_OF_DATE_KHR` return. One
  gap is survivable (`vkWaitSemaphores` is `>=`), but two in a row — plausible
  during a drag-resize — would leave a later frame waiting forever. Fixed by
  computing `frame_value = total_frames + 1` for the wait and only committing
  the bump after a successful acquire.
- **`VK_LAYER_ENABLE_SYNC_VALIDATION` turned out to be a dead check here.**
  With the layer confirming `Current Enables:
  VK_VALIDATION_FEATURE_ENABLE_SYNCHRONIZATION_VALIDATION_EXT`, stripping
  *every* barrier — the new automatic one and watercolor's eight per-dispatch
  ones — still produced zero hazard reports. Buffer traffic here goes through
  BDA, which the layer cannot track, and the descriptor-bound storage textures
  were not flagged either. It should not be relied on as the barrier check the
  Verification section below claims it is.
- A small refactor rode along: `cmd_memory_barrier2` (`src/renderer.rs`) now
  backs both the app-requested `memory_barrier()` and the two renderer-emitted
  barriers.

Supersedes
[frame_ring_collapse.md](frame_ring_collapse.md) and
[watercolor_race_fixes.md](watercolor_race_fixes.md). Code references verified
against main @ `76ad25c` plus the uncommitted `examples/watercolor.rs` edit;
re-verify before editing.

## Context

`Renderer::enable_pipelined_compute` (`src/renderer.rs:876-887`) splits a frame's
compute dispatches onto a second queue so frame N's simulation overlaps frame N−1's
display. Local experiments — the uncommitted `examples/watercolor.rs` edit that
comments out `enable_pipelined_compute()` and swaps the trailing COMPUTE→COMPUTE
barrier for a COMPUTE→FRAGMENT one — show **no meaningful framerate win**.

That feature is the sole reason for a large amount of complexity:

- A second submission path in `draw_frame`, a second command-buffer set, a second
  queue, and a `ComputePlacement` mode enum whose value silently changes the meaning
  of `dispatch()` and `memory_barrier()`.
- A **3-slot** `PRE_WAIT_RING_LEN` ring alongside the 2-slot `MAX_FRAMES_IN_FLIGHT`
  ring — two indices with differently-shaped correctness proofs
  ([frame_ring_collapse.md](frame_ring_collapse.md)).
- BDA footgun #3, the "pipelined current-read race"
  ([bda_footguns.md](bda_footguns.md) §3), and the whole planned fix for it
  ([bda_footguns/03_pipelined_current_read_plan.md](bda_footguns/03_pipelined_current_read_plan.md),
  domain-marked `Addr<T, S>` types).
- The watercolor same-frame cross-queue display race
  ([render-graph/04_design.md](render-graph/04_design.md) §8), and the
  `CrossFrameMode` / `dispatch_pipelined` machinery designed to fix it
  ([watercolor_race_fixes.md](watercolor_race_fixes.md)).

**Outcome:** compute always runs before graphics in the same command buffer;
graphics reads the *most recent* compute output; `Gpu::previous_addr` stays (compute
can still read the previous frame's output); everything rings across
`MAX_FRAMES_IN_FLIGHT = 2`. Removing pipelining makes footgun #3 and the watercolor
§8 race *not exist*, rather than needing to be designed around.

**What stays:** `GpuOnlyBufferHandle`, `Gpu::current_addr` / `Gpu::previous_addr`,
`write_gpu_only_all_frames`, per-frame buffer/descriptor rings, `frame_timeline`,
`FrameRenderer::dispatch` / `memory_barrier`, watercolor's own parity-indexed
texture ping-pong. Only their ring *length* and their sync argument change.

### Why the ping-pong is safe at 2 slots once pipelining is gone

With compute(N) and graphics(N) in one submit, slot `s(N)` has two readers —
compute(N+1) via `previous_addr`, and graphics(N) via `current_addr` — and its next
writer is compute(N+2).

- **graphics(N) vs compute(N+2):** frame N+2's CPU wait is `frame_timeline >= N`,
  signalled at `ALL_COMMANDS` of submit N. Submit N has fully retired before frame
  N+2 is even recorded. Safe unconditionally.
- **compute(N+1) vs compute(N+2):** *not* covered by any CPU wait. Needs a
  queue-level execution dependency — supplied either by the existing
  `compute_timeline` wait or by Phase 3's leading barrier.

`frame_ring_collapse.md` §"Job 2" concludes 2 slots are unsafe. That conclusion is
specific to async compute, where compute(N+2) waits only on compute(N+1) rather than
on graphics(N)'s retirement; it does not survive this change — and that note says so
itself at L82.

---

## Phase 0 — this note

Land this file before touching code, so the phases below can be checked off against
it, and so the superseding banners in Phase 5 have something to point at.

## Phase 1 — delete the pipelined submission path

All in `src/renderer.rs`.

- Delete fields `compute_queue` (`:171`), `compute_command_buffers` (`:173`),
  `pipelined_compute` (`:176`) and their init at `:463-465`.
- Delete `enable_pipelined_compute` (`:876-887`) and `record_compute_command_buffer`
  (`:1486-1503`).
- Delete `enum ComputePlacement` (`:5476-5481`) and the `compute_placement` parameter
  on `record_command_buffer` (`:1511`, `:1521-1523`) — always record compute at the
  top of the graphics CB.
- In `draw_frame` (`:2259-2436`): drop `use_pipelined` and the whole `if use_pipelined`
  branch; keep the `else` body as the only path.
- Stop requesting a second graphics-family queue: `create_logical_device`
  (`:3368-3380`) always uses `queue_priorities_single`; drop
  `QueueFamilyIndices::graphics_queue_count` (`:3032`, `:3072-3076`) and the queue
  fetch at `:334-338`.
- Delete the commented-out call site `// renderer.enable_pipelined_compute();` at
  `examples/watercolor.rs:413`.

`compute_timeline` / `compute_frames` / `has_compute_pipelines` stay for now
(Phase 3 revisits them).

## Phase 2 — the renderer guarantees compute → graphics ordering

Today the combined path records compute at the top of the graphics CB with **no**
compute→graphics barrier; same-queue submission order gives no memory dependency.
Apps paper over it themselves (`examples/particles.rs:97-102` COMPUTE→VERTEX;
watercolor's new `compute_to_fragment_barrier`). Since "graphics reads the most
recent compute output" is now the renderer's contract, the renderer must emit it.

In `record_command_buffer`, immediately after recording `pending_compute` and only
when that list is non-empty, emit (design lifted from
[watercolor_race_fixes.md](watercolor_race_fixes.md) Step 2):

```rust
let barrier = vk::MemoryBarrier2::default()
    .src_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
    .src_access_mask(vk::AccessFlags2::SHADER_WRITE)
    .dst_stage_mask(vk::PipelineStageFlags2::VERTEX_SHADER
        | vk::PipelineStageFlags2::FRAGMENT_SHADER)
    .dst_access_mask(vk::AccessFlags2::SHADER_READ);
```

Always legal: this CB is always on the graphics queue. This also closes the frame-0
barrier gap `watercolor_race_fixes.md` §"Renderer footguns this closes" §1 describes.

Then remove the now-redundant manual trailing barriers:

- `examples/particles.rs:96-102` — delete the manual COMPUTE→VERTEX barrier.
- `examples/watercolor.rs` — delete the `compute_to_fragment_barrier` helper
  (`:228-236`, uncommitted) and its call at `:1027`, plus the stale
  `// FIXME wrong comment` block at `:1024-1026`. The per-dispatch `compute_barrier`
  (COMPUTE→COMPUTE) calls inside the sim stay.

Also drop the `#[expect(unused)]` `descriptor_sets_for_compute_frame` (`:2153-2164`)
if it is still unused after the deletions.

## Phase 3 — replace the cross-frame compute timeline with a barrier

The combined path currently waits on `compute_timeline >= compute_value - 1`, which
is signalled at `ALL_COMMANDS` of frame N−1's submit — so frame N's compute cannot
begin until *all* of frame N−1, including its graphics, has retired. With pipelining
gone this is the only cross-frame ordering, and it serialises consecutive GPU frames
for every compute app.

Replace it with an execution/memory dependency recorded at the **top** of each
frame's command buffer, before the compute section:

```rust
// orders this frame's compute against everything submitted earlier on this queue
src_stage:  COMPUTE_SHADER | VERTEX_SHADER | FRAGMENT_SHADER
src_access: SHADER_READ | SHADER_WRITE
dst_stage:  COMPUTE_SHADER
dst_access: SHADER_READ | SHADER_WRITE
```

A `vkCmdPipelineBarrier2`'s first synchronization scope covers all commands submitted
earlier in submission order on the same queue, so this orders frame N's compute writes
after frame N−1's compute *and* graphics reads — exactly the WAR guarantee the 2-slot
ping-pong needs in Phase 4, without the full-frame serialisation.

Then delete `compute_timeline` (`:165`, `:3712`, `:3718`, `:2814`), `compute_frames`
(`:167`), and the compute wait/signal arms of the submit (`:2389-2419`).
`has_compute_pipelines` (`:168`, `:1249`) survives only if it still gates something;
otherwise gate on `!pending_compute.is_empty()` and delete it too.

**Fallback if this proves shaky:** keep `compute_timeline` exactly as-is. Phase 4 is
still correct (the timeline wait is strictly stronger than the barrier), at the cost
of no cross-frame GPU overlap. Phase 4 must not land with *neither* mechanism.

## Phase 4 — collapse the ring: `PRE_WAIT_RING_LEN` → `MAX_FRAMES_IN_FLIGHT`

### 4a. Reorder `draw_frame` (prerequisite)

The third slot's other job is that CPU writes happen *before* the timeline wait
(`:2229-2231`). Reorder to the standard vulkan-tutorial sequence
(`frame_ring_collapse.md` Phase 1):

1. `total_frames += 1` / compute `frame_value`
2. `wait_semaphores` on `frame_timeline` (currently `:2239-2246`)
3. `acquire_next_image` (currently `:2208-2224`)
4. `gpu_update(&mut gpu)` (currently `:2232-2237`)

Watch the `ERROR_OUT_OF_DATE_KHR` early return from acquire: it now happens *after*
`total_frames` is bumped and after the wait. `recreate_swapchain` does a
`device_wait_idle` (`:2478`) and does not recreate the timeline semaphores, so the
timeline stays consistent — verify this explicitly.

Expected cost is ~nil (see `frame_ring_collapse.md` §"Job 1"); acquiring later is
mildly better for input latency.

### 4b. Delete `PRE_WAIT_RING_LEN`, use `MAX_FRAMES_IN_FLIGHT` everywhere

Mechanical substitution — every `PRE_WAIT_RING_LEN` becomes `MAX_FRAMES_IN_FLIGHT`
and every `self.ring_slot` becomes `self.flight_slot`; then delete the constant and
the `ring_slot` field (`:162`, init `:458`, advance `:2443`).

Representative sites (not exhaustive):

- `src/renderer/uniform_buffer.rs` — `[RawUniformBuffer; PRE_WAIT_RING_LEN]` arrays.
- `src/renderer/storage_buffer.rs` — same for all three storage handle kinds
  (`StorageBufferHandle`, `ImmutableBufferHandle`, `GpuOnlyBufferHandle`).
- `src/renderer.rs` — `create_uniform_buffers_per_frame` (`:812-815`),
  `create_storage_buffers_per_frame` (`:892-898`), the three `write_*_all_frames`
  loops (`:928`, `:943`, `:958`), descriptor pool sizing (`:3924`, `:3944`) and
  `create_descriptor_sets` (`:3998-4026`), the `.nth(self.ring_slot)` lookups
  (`:1429`, `:2144`, `:2158`, `:2171`), `Gpu` (`:5389`) and all its accessors
  (`:5395-5472`), `create_sync_objects` (`:3690-3703`).
- `previous_addr` (`:5456-5461`) becomes
  `(self.flight_slot + MAX_FRAMES_IN_FLIGHT - 1) % MAX_FRAMES_IN_FLIGHT` — equal to
  `self.flight_slot ^ 1` at 2 slots, but keep the general form.

`GpuOnlyBufferHandle` and the `current_addr` / `previous_addr` split are **kept** —
they are still the only way for compute to read the previous frame's output; only
their ring length changes. Update the invariant doc at
`src/renderer/storage_buffer.rs:41-50`: the type no longer exists to keep CPU writes
off a history slot (there is no pre-wait window any more); it exists for the
current/previous distinction and its WAR proof.

### 4c. `image_available` semaphores

`image_available` (`:147`, created `:3697-3703`, used `:2213`/`:2327`/`:2379`) is
sized by the ring and indexed by `ring_slot`. After 4a acquire happens after the
wait, so indexing by `flight_slot` at length 2 is the standard pattern — do that as
part of 4b.

Note but do **not** fix here: `recreate_swapchain` (`:2477+`) never recreates
`render_finished`, which is sized per swapchain image. If the image count changes on
recreation this panics or leaks. Pre-existing, orthogonal, worth its own commit
(`frame_ring_collapse.md` Phase 4).

---

## Phase 5 — docs, comments, and superseded plans

Last step; do it once the code has settled.

**Load-bearing comments that become actively wrong:**

- `src/renderer.rs:65-74` — the two-constant doc block.
- `src/renderer.rs:150-162` — `flight_slot` / `ring_slot` field docs.
- `src/renderer.rs:2229-2231` — "CPU buffer writes BEFORE the timeline wait".
- `src/renderer.rs:2438-2441` — "advance both frame counters".
- `src/renderer/storage_buffer.rs:41-50` — the `GpuOnlyBufferHandle` invariant.
- `src/renderer/uniform_buffer.rs` header comments referencing the pre-wait ring.
- `examples/particles.rs` and `examples/watercolor.rs` barrier comments.

**Notes to rewrite or mark superseded.** Prefer a dated
`> **SUPERSEDED by remove_pipelined_compute.md** (2026-07-28)` banner over deletion —
these notes already carry status banners and several are historical records worth
keeping intact. Finally, flip this file's own status line from plan to implemented.

*Superseded outright:*

- `bda_footguns/03_pipelined_current_read_plan.md` — domain-marked `Addr<T, S>`
  exists only to police the pipelined/frame domain split. Also drop the references
  to it in `link_rendering/phase_05.md:616` and `link_rendering/follow_up.md:107`.
- `watercolor_race_fixes.md` — the per-dispatch stream split is unneeded; its
  automatic-barrier design survives as Phase 2 above. Say so in the banner.
- `frame_ring_collapse.md` — its Phase 1 reorder is absorbed as Phase 4a; its
  "Job 2 is NOT removable" conclusion is void (see Context above).

*Needs substantive edits:*

- `bda_footguns.md` — rewrite the "Ring model context" paragraph (`:9-13`); mark §3
  (`:33-35`) and its summary-table row (`:154`) **not applicable — removed with
  pipelined compute**; §1 flicker (`:18-22`) becomes a 2-slot problem (halved period,
  *not* fixed); re-check §12 (`:107`) and §15 (`:138`) against the new ring.
- `render-graph/04_design.md` — the densest. §8's watercolor race is **fixed** by
  Phase 2; fact (a) at `:396` no longer applies; decision 4 (`:24`, "the graph owns
  the pipelined/frame split"), the `PRE_WAIT_RING_LEN` note (`:48`), §4's "pipelined
  compute already exists" (`:77-92`), the `PipelinedDomain` annotations (`:259`,
  `:270-271`, `:428-461`), the `CrossFrameMode::SyncWait` row (`:413-422`) and the
  Phase-0.5 entry (`:525`) all change. Its stated hard prerequisite on
  `03_pipelined_current_read_plan.md` (`:8`) dissolves.
- `frame_inputs_api.md:22-24,32-40,73,182-185,272,281` — pipelined-mode framing and
  "N mod 3". The universal graphics-`current()` ban loses its motivation.
- `render-graph/05_multi_draw_rendering.md:476,484,501,536,547-550` — ring arithmetic
  (`R`, `A`, `M`, `PRE_WAIT_RING_LEN = M + 1`) and the `compute_timeline` references.
- `render-graph/02_explicit_parallelism.md:8-10,20` and
  `render-graph/original_compute_shaders_plan.md:5-6` — header claims that Pattern A
  "exists today as `enable_pipelined_compute()`".
- `render-graph/00_PLAN.md:25` — the `MAX_FRAMES_IN_FLIGHT` double-buffering line.

*Footnote only (historical records — do not rewrite):*
`vulkan_1_3_migration.md:92,97,103,147,157`,
`vulkan_1_3_migration/timeline_semaphores.md:43,57,68-72`,
`vulkan_1_3_migration/bda_renderer_plumbing.md:52-53,73`,
`link_rendering.md:143-144`, `link_rendering/phase_04.md:70-71,130,141`,
`offscreen_testing.md:605`.

*`todo.org`:* `:5` "triple-buffering problems" — done; `:17` "3. pipelined
current-read race" — resolved by removal; `:125` "find a strucutured solution for
pipelined compute shader simulations" — re-scope or drop; `:309` and `:504` mention
`MAX_FRAMES_IN_FLIGHT` / per-swapchain-image vs per-in-flight-frame — re-check.

**Stale claims to fix while here:** several notes assert
`examples/watercolor.rs:414` calls `enable_pipelined_compute()`; it is currently
commented out in the working tree, and this plan deletes the line entirely.
`examples/watercolor.rs:765,771` claim the display reads the *previous* frame's
deposit output — `render-graph/04_design.md` §8 already showed that trace is wrong,
and after this change it is unambiguously reading this frame's output.

Final sweep:

```bash
rg -n 'pipelined|PRE_WAIT_RING_LEN|ring_slot|ComputePlacement|compute_timeline|triple' \
   --glob '!target' --glob '!src/generated'
```

---

## Verification

Generated code has no coupling to either ring length (`src/generated/`,
`src/shaders/build_tasks.rs`), so `just shaders` is not required; run `just test`
anyway to confirm no snapshot drift.

```bash
cargo check --all-targets
just lint
just test
cargo fmt
```

Then, per phase, under validation layers (debug builds enable them via
`ENABLE_VALIDATION`, `renderer.rs:61`):

```bash
timeout 3 just dev particles      # the only previous_addr user — the real test
timeout 3 just dev watercolor     # compute + storage buffers + storage textures
timeout 3 just dev sprite_batch   # largest per-frame CPU write (Phase 4a cost check)
timeout 3 just dev gpu_picking    # flight_slot readback path
timeout 3 just dev space_invaders # multiple storage buffers + egui
timeout 3 just dev viking_room    # plain uniform + texture path
```

Automated results: `cargo check --all-targets`, `just lint` and `just test`
(32 + 66 tests, no snapshot drift) all green, and all 16 examples run clean under
validation layers in debug.

Manual checks automation will not catch:

- **`particles` visual correctness — CONFIRMED.** A WAR hazard on the 2-slot history
  ring would show up as intermittent particle-position corruption or stutter, never
  as a validation error. Visually confirmed on the finished branch; also ran 30 s
  debug and 35 s release without validation errors or crashes.
- **Watercolor §8 race — CONFIRMED.** The display legitimately reads this frame's
  simulation output now, and the example renders correctly.
- **~~`VK_LAYER_ENABLE_SYNC_VALIDATION`~~ — not a usable check here.** See the
  status block at the top: with the layer confirming it was enabled, removing every
  barrier in watercolor still produced zero hazard reports. BDA accesses are
  invisible to it. Do not treat a silent sync-validation run as evidence.
- **Resize under load.** Confirmed by eye 2026-08-28. ~~Not separately exercised.~~ Phase 4a changes when acquire
  happens relative to the wait, and the `total_frames` fix above is what a
  repeated-`OUT_OF_DATE` resize storm tests. Worth a drag-resize pass if this area
  is touched again.
- **Framerate.** Not separately measured — watercolor's FPS readout is an on-screen
  egui label, and `Game::frame_delay` caps at ~60 fps
  (`DEFAULT_FRAME_DELAY = 15 ms`, `src/game/traits.rs:9`), so a small regression
  would hide under the cap anyway. The premise that pipelining bought nothing came
  from the pre-existing local experiment, not from a measurement taken here.
