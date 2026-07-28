# Collapsing the pre-wait ring: 3 slots → 2

> **SUPERSEDED by [remove_pipelined_compute.md](remove_pipelined_compute.md)**
> (2026-07-28), which did collapse the ring — but on different grounds, and
> further than this note thought possible.
>
> Its **Phase 1** (reorder `draw_frame` to wait → acquire → write) was absorbed
> as that plan's Phase 4a and is implemented.
>
> Its **"Job 2 is NOT removable"** conclusion is **void**. That conclusion is
> specific to async compute, where compute(N+2) waits only on compute(N+1)
> rather than on graphics(N)'s retirement. With pipelining gone, compute(N) and
> graphics(N) share one submit, so graphics(N) vs compute(N+2) is covered
> unconditionally by frame N+2's `frame_timeline >= N` wait, and compute(N+1)
> vs compute(N+2) is covered by a barrier at the top of each command buffer.
> `PRE_WAIT_RING_LEN` and `ring_slot` are gone; there is one ring of length
> `MAX_FRAMES_IN_FLIGHT`, indexed by `flight_slot`. This note says as much
> itself at L82.

Status: **plan, 2026-07-26.** Companion to
[bda_footguns.md](bda_footguns.md) (§1 occasional-write flicker, §3 pipelined current-read race).
Code references verified against main @ `bd60578`; re-verify before editing.

## Context

`src/renderer.rs:65-74` declares two ring counts:

```rust
const MAX_FRAMES_IN_FLIGHT: usize = 2;
const PRE_WAIT_RING_LEN: usize = MAX_FRAMES_IN_FLIGHT + 1;
```

- `flight_slot` (mod 2, `renderer.rs:155`) indexes things touched **after** the `frame_timeline`
  wait: graphics + compute command buffers, resolve images, picking readback, egui texture frees.
- `ring_slot` (mod 3, `renderer.rs:160`) indexes uniform buffers, storage buffers, per-slot
  descriptor sets, and the `image_available` acquire semaphores.

Two indices with differently-shaped correctness proofs is a standing source of reasoning error —
`storage_buffer.rs:44-47` encodes one of those proofs into the type system (`GpuOnlyBufferHandle` is
"the only handle that can mint a `Gpu::previous_addr` history pointer"), and `bda_footguns.md`
catalogues the places where the proofs leak into user-facing API.

The goal is to reduce to a single wait-guarded index wherever that is free, and to leave a third
slot **only** where the frames-in-flight arithmetic genuinely requires it.

## The two jobs the third slot is doing

`PRE_WAIT_RING_LEN = 3` is not one requirement. It is two, with very different costs to remove.

### Job 1 — CPU writes land before the timeline wait (removable, ~free)

In `draw_frame`, `gpu_update` (`renderer.rs:2207-2212`) runs **before** `wait_semaphores`
(`renderer.rs:2216-2221`). At that point the strongest guarantee available is that frame N-3 has
retired — the most recent wait executed was frame N-1's wait for value N-3. Frame N-2 may still be
executing, so a 2-slot ring would let a CPU memcpy land in a buffer the GPU is still reading.
`renderer.rs:2204-2206` states exactly this.

`acquire_next_image` (`renderer.rs:2184-2199`) is likewise pre-wait, which is why `image_available`
is sized 3 (`renderer.rs:145`, `renderer.rs:3671-3677`).

**Fix:** move both after the timeline wait — the standard vulkan-tutorial ordering.

**Cost on desktop/Mac: effectively zero.** The reasoning matters more than the number:

- **GPU-bound:** the wait blocks, but the CPU has slack anyway. Losing the write/wait overlap raises
  per-frame CPU latency without lowering framerate.
- **CPU-bound:** the wait does not block, so the reorder costs nothing.
- Only where CPU and GPU frame times are near-balanced does it add real time, and only the memcpy
  duration. The largest per-frame buffers in-tree are `examples/sprite_batch.rs` (8192 sprites) and
  `examples/particles.rs` (4096 particles) — hundreds of KB into write-combined BAR memory, tens of
  microseconds.

Moving `acquire_next_image` later is likely a small **improvement**: acquiring as late as possible
is standard practice for input latency.

### Job 2 — `previous_addr` ping-pong (NOT removable)

`Gpu::previous_addr` (`renderer.rs:5418-5424`) hands frame N a read pointer to slot N-1 while frame
N writes slot N. Work out when a slot is overwritten:

- **3 slots:** `s(N)` is rewritten by frame N+3, whose `frame_timeline` wait is for value N+1 —
  exactly the retirement of graphics(N+1), the reader of `s(N)`. Safe, with precisely one slot of
  margin.
- **2 slots:** `s(N)` is rewritten by frame N+2, whose wait is only for value N. graphics(N+1) is
  still reading `s(N)`. **Write-after-read hazard.**

General rule: with F frames in flight and readers lagging writers by one generation, F+1 live
generations are required. Splitting the ping-pong into two standalone dedicated buffers does not
escape this — generation N+2 still lands on generation N's buffer.

The escapes are all worse:

1. Add a `frame_timeline >= N+1` wait to the compute submit. Legal, but that is exactly the
   serialization `Renderer::enable_pipelined_compute` (`renderer.rs:869-880`) exists to avoid:
   compute(N+2) would stall on graphics(N+1), destroying the compute/graphics overlap. Also the
   worst option on macOS, where MoltenVK emulates timeline semaphores.
2. Drop to one frame in flight. Not wanted.

Note the **non-pipelined path already works at 2 slots** for ping-pong, since compute and graphics
share a command buffer and are ordered by barriers (`ComputePlacement::BeforeGraphics`). The cost is
specific to async compute — currently only `examples/particles.rs:109`.

### Platform note

The desktop/Mac-vs-mobile axis barely bears on this. Frames-in-flight is independent of swapchain
image count, and the swapchain side is already handled separately (`min_image_count + 1` at
`renderer.rs:3278`, per-image `render_finished` at `renderer.rs:148`). Mobile's preference for deeper
buffering is about the presentation stack and tiler latency tolerance, not this ring. Dropping mobile
buys nothing specific here.

The one macOS caution runs the *opposite* way from intuition: MoltenVK emulates timeline semaphores
over Metal primitives, so host-side `vkWaitSemaphores` is relatively more expensive there. That is an
argument against escape (1) above, not against the reorder.

### Memory is not an argument

3 → 2 on uniform buffers, storage buffers, and descriptor sets saves single-digit MB at current
example sizes. Do not justify this work on memory.

## Design

Keep two indices, but change what the second one covers and what it is called:

| Resource | Today | After |
|---|---|---|
| command buffers (gfx + compute), resolve images, picking readback, egui frees | `flight_slot` (2) | unchanged |
| uniform buffers | `ring_slot` (3) | `flight_slot` (2) |
| storage + immutable buffers | `ring_slot` (3) | `flight_slot` (2) |
| descriptor sets | `ring_slot` (3) | `flight_slot` (2) |
| GPU-only ping-pong buffers | `ring_slot` (3) | `history_slot` (3) |
| `image_available` semaphores | `ring_slot` (3) | see Phase 4 |

Rename `PRE_WAIT_RING_LEN` → `HISTORY_RING_LEN` and `ring_slot` → `history_slot`, scoped to
`GpuOnlyBufferHandle` only.

**Why this is still a complexity win despite keeping two indices:** the surviving second index covers
one narrow, already-type-distinguished case (`GpuOnlyBufferHandle`), and the subtle "CPU write before
the wait" safety argument — the one that leaks into `bda_footguns.md` §2 (address stashing) and
§1 (occasional-write flicker) — disappears entirely.

Descriptor sets follow uniform buffers rather than storage buffers because
`create_descriptor_sets` (`renderer.rs:3968-4087`) only binds uniform buffers, textures, and storage
images; storage buffers are reached by BDA, not descriptors. So there is no cross-ring aliasing to
worry about.

**Side effect worth recording:** `bda_footguns.md` §1 (occasional-write flicker) goes from a 3-slot
to a 2-slot problem. It is *not* fixed — a dirty-flag write still leaves slots holding different
generations — but the flicker period halves. Do not claim it as a fix.

## Phases

Each phase should compile and run cleanly on its own.

### Phase 1 — reorder `draw_frame` (no count changes)

In `Renderer::draw_frame` (`renderer.rs:2150+`), move steps in this order:

1. `wait_semaphores` on `frame_timeline` (currently step 3, `renderer.rs:2214-2221`)
2. `acquire_next_image` (currently step 1, `renderer.rs:2184-2199`)
3. `gpu_update(&mut gpu)` (currently step 2, `renderer.rs:2207-2212`)

`self.total_frames += 1` (`renderer.rs:2201`) must stay ordered so `frame_value` is computed before
the wait that uses it. Keep the picking readback (`renderer.rs:2224-2227`) and egui texture frees
(`renderer.rs:2229-2232`) after the wait where they already are.

Watch the early-return paths: `acquire_next_image` returning `ERROR_OUT_OF_DATE_KHR` currently
returns before `total_frames` is bumped. After the reorder the wait has already happened; confirm
`recreate_swapchain` (which does a `device_wait_idle`, `renderer.rs:2459`) still leaves the timeline
consistent. The note at `renderer.rs:2461-2462` about not recreating timeline semaphores still holds.

Rewrite the comments at `renderer.rs:65-74` and `renderer.rs:2204-2206` — they are the load-bearing
documentation of the old invariant and will be actively wrong.

Phase 1 alone is behavior-preserving and independently testable. **Land and validate it before
touching any counts.**

### Phase 2 — move CPU-written buffers and descriptor sets to `flight_slot`

- `UniformBufferStorage` (`renderer/uniform_buffer.rs:23,32,47,64,68`): array length
  `PRE_WAIT_RING_LEN` → `MAX_FRAMES_IN_FLIGHT`.
- `StorageBufferStorage` (`renderer/storage_buffer.rs:75`) and the `add`/`take` signatures for
  `StorageBufferHandle` and `ImmutableBufferHandle` — but **not** the `*_gpu_only` variants
  (`storage_buffer.rs:168-206`), which stay at the history length.
- `create_uniform_buffers_per_frame` (`renderer.rs:805-808`) and
  `create_storage_buffers_per_frame` (`renderer.rs:882-916`) — the latter is shared by all three
  storage handle kinds, so it needs to become length-parameterised (const generic or two functions).
- `write_storage_all_frames` / `write_immutable_all_frames` (`renderer.rs:918-944`) loop bounds;
  `write_gpu_only_all_frames` (`renderer.rs:946-959`) keeps the history length.
- Descriptor pool sizing: `create_descriptor_pool_from_layouts` (`renderer.rs:3898`) and
  `create_descriptor_pool` (`renderer.rs:3918`), plus `create_descriptor_sets`
  (`renderer.rs:3989,4000`).
- `descriptor_sets_for_frame`, `descriptor_sets_for_compute_frame`,
  `picking_descriptor_sets_for_frame` (`renderer.rs:2109-2148`) and the inline compute lookup at
  `renderer.rs:1415-1418`: `.nth(self.ring_slot)` → `.nth(self.flight_slot)`.
- `Gpu` (`renderer.rs:5350-5354`) gains both indices; `write_uniform` / `write_storage` /
  `write_immutable` / `addr` / `current_immutable_addr` use the flight index, while `current_addr` /
  `previous_addr` use the history index.

### Phase 3 — rename the surviving ring

`PRE_WAIT_RING_LEN` → `HISTORY_RING_LEN`, `ring_slot` → `history_slot`. Update the doc comment to
state the real reason (F+1 generations for 1-frame-late reads at F frames in flight) rather than the
now-obsolete pre-wait reason. Update `storage_buffer.rs:41-49`: the `GpuOnlyBufferHandle` doc still
justifies itself by "no CPU write can land before the frame_timeline wait", which is no longer the
argument — the type survives for the current/previous distinction and the ping-pong hazard, not for
CPU-write race freedom.

Also update the ring-model paragraph in `llm_notes/bda_footguns.md` (lines 9-13), which states the
old model and cites `renderer.rs:5185` for `previous_addr` (now `renderer.rs:5419`).

### Phase 4 — acquire semaphores (separable; consider doing independently)

`image_available` is sized `PRE_WAIT_RING_LEN` and indexed by `ring_slot`
(`renderer.rs:145`, `renderer.rs:3671-3677`, `renderer.rs:2188`). Tying acquire semaphores to *any*
frame ring is the known-shaky part of the tutorial pattern: `vkAcquireNextImageKHR` can return images
out of order, so a frame-indexed acquire semaphore can be recycled while its acquire is still
pending.

**Do not simply shrink this from 3 to 2.** Prefer decoupling it: one acquire semaphore per swapchain
image, as `render_finished` already is (`renderer.rs:148`, `renderer.rs:3679-3683`).

Blocking prerequisite: `recreate_swapchain` (`renderer.rs:2458-2551`) does **not** currently recreate
`render_finished`, even though the swapchain image count can change across recreation. If the count
grows, `self.render_finished[image_index]` panics; if it shrinks, semaphores leak. Any move toward
per-swapchain-image acquire semaphores must fix that first. This is a pre-existing latent bug and a
reasonable standalone commit.

## Verification

Generated code has no coupling to either ring length (grepped `src/generated/` and
`shaders/build_tasks.rs` — clean), so `just shaders` is not required.

```bash
cargo check --all
just lint
just test            # insta snapshots; expect no diffs, since codegen is uncoupled
cargo fmt
```

Then run each example under validation layers (debug builds enable them via `ENABLE_VALIDATION`,
`renderer.rs:61`). Priority order — these exercise the changed paths:

```bash
timeout 3 just dev particles      # ONLY previous_addr user + pipelined compute; the real test
timeout 3 just dev watercolor     # compute + storage buffers + storage textures
timeout 3 just dev sprite_batch   # largest per-frame CPU write
timeout 3 just dev gpu_picking    # flight_slot readback path, unchanged but adjacent
timeout 3 just dev space_invaders # multiple storage buffers + egui
timeout 3 just dev viking_room    # plain uniform + texture path
```

Manual checks that automated tests will not catch:

- **Resize under load.** Drag-resize each of `particles` and `watercolor` continuously for several
  seconds. Swapchain recreation interacts with the reordered acquire and with slot advancement
  (`renderer.rs:2413-2418`, whose comment explains why counters advance before present).
- **Ping-pong visual correctness.** `particles` should show smooth continuous motion. A WAR hazard
  from an incorrect history ring shows up as intermittent particle position corruption or stutter,
  not as a validation error — validation layers will not catch it.
- **Sync validation.** Run `particles` with `VK_LAYER_ENABLE_SYNC_VALIDATION` if available; this is
  the one check that can actually catch a mis-sized history ring.
- **Frame pacing.** Compare a rough frame-time readout on `sprite_batch` before and after Phase 1 to
  confirm the reorder cost is in the noise, as predicted.
