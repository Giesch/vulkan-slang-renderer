# Timeline Semaphores: What They Are and How Phase 3 Uses Them

Companion note to [../vulkan_1_3_migration.md](../vulkan_1_3_migration.md). Context: Phase 3 replaced the renderer's fence/binary-semaphore frame sync with two timeline semaphores. This note explains the primitive itself, where it can and cannot be used, and maps the old sync objects to the new scheme.

## The primitive

A timeline semaphore (core in Vulkan 1.2, `VK_KHR_timeline_semaphore` before that) is a single sync object containing a **monotonically increasing 64-bit counter**. Every signal operation sets the counter to a specific value; every wait operation blocks until the counter reaches **at least** a specific value. That "at least" is the heart of the design.

Compare the two older primitives it subsumes:

- A **binary semaphore** is a one-shot flag between exactly one GPU signal and exactly one GPU wait. The wait consumes the signal. It cannot be waited on by the CPU, cannot be reset from the host, and a signaled-but-never-waited binary semaphore is stuck — the only way out is destroying it.
- A **fence** is a GPU→CPU flag: the GPU signals it at submit completion, the CPU waits and must then explicitly reset it. One fence per frame-in-flight, plus the "create pre-signaled" trick so frame 1's wait doesn't hang.

A timeline semaphore does both jobs at once, N times over:

- **GPU→GPU and GPU→CPU with one object.** `SemaphoreSubmitInfo::value()` signals/waits it in a submit; `vkWaitSemaphores` waits it from the host; `vkGetSemaphoreCounterValue` polls it. One timeline replaces an *array* of fences and an *array* of binary semaphores.
- **Waits don't consume anything.** Any number of waiters — on different queues, or on the CPU — can wait on the same value. In this renderer, one compute signal (value `v`) satisfies both the next compute submit's wait and the next graphics submit's wait; previously that took two separate binary semaphores signaled side by side.
- **No reset, no stuck state.** The counter only moves forward. Waiting on a value the counter has already passed returns immediately. Waiting on `0` always succeeds — which is what deletes both bootstrap machinery (frame 1 waits on "frame 0", trivially satisfied) and the pre-signaled-fence trick.
- **Wait-before-signal is legal**, including submitting a wait for a value no submit has promised yet (the driver resolves it when the signal arrives). Host signals (`vkSignalSemaphore`) exist too, though this renderer doesn't need them.
- **Skipped values are fine.** Signals must be strictly increasing but need not be consecutive; waits are `≥`. If an error path skips a frame number, the next successful signal satisfies the older wait. (With fences, an error between `reset_fences` and submit meant a guaranteed deadlock on the next wait.)

The mental model shift: instead of "did *this frame's slot* finish?" (an array of flags you must index, reset, and bootstrap), you ask "has the GPU reached frame N−2 yet?" (a single monotonic clock you compare against).

## Where they cannot be used

The one hard boundary is **WSI (the swapchain)**:

- `vkAcquireNextImageKHR` can only signal a **binary** semaphore (or fence).
- `vkQueuePresentKHR` can only wait on **binary** semaphores.

So the two swapchain-adjacent semaphores stay binary: `image_available` (signaled by acquire, waited by the graphics submit) and the per-swapchain-image `render_finished` (signaled by the graphics submit, waited by present). This is a spec limitation, not a renderer choice — `VK_KHR_swapchain_maintenance1` improves adjacent pain points but presents still can't wait timelines.

Binary and timeline entries mix freely within one `SubmitInfo2`: each `SemaphoreSubmitInfo` names its semaphore, and `.value()` is simply ignored for binary ones. That's what makes the hybrid scheme below clean.

Two softer caveats: timeline semaphores cannot be used with `VK_KHR_external_semaphore_fd`'s SYNC_FD type (relevant only for cross-API interop), and drivers may take a slightly different submission path for them — no practical impact here.

## What Phase 3 actually did

Before → after, object by object:

| Old object | Count | Replaced by |
|---|---|---|
| `frames_in_flight` fences | 2, created pre-signaled | `frame_timeline` (one semaphore) |
| `compute_fences` | 2, created pre-signaled | `compute_timeline` CPU wait |
| `compute_finished` binary sems | 3 | `compute_timeline` |
| `compute_to_graphics_sem` binary sems | 3 | `compute_timeline` (same signal, second waiter) |
| `compute_bootstrapped` flag + three-way submit branch | — | deleted (wait on 0 is trivially satisfied) |
| recreate-swapchain destroy/recreate of compute sems | — | deleted (see below) |
| `image_available` binary sems | 3 | **kept** — WSI acquire |
| `render_finished` binary sems | per swapchain image | **kept** — WSI present |

### `frame_timeline`

The graphics submit for frame N signals value N, where N = `total_frames` after its per-frame increment. The old `wait_for_fences`/`reset_fences` pair at the top of `draw_frame` became one call:

```
wait_semaphores(frame_timeline >= N.saturating_sub(MAX_FRAMES_IN_FLIGHT))
```

Frames 1 and 2 wait on ≤0 and pass immediately — exactly what the pre-signaled fences simulated. Everything gated behind the old fence wait (command-buffer reuse, picking readback, egui texture frees) sits behind this wait unchanged.

### `compute_timeline` and the `compute_frames` counter

The k-th compute-signaling submit signals value k, tracked by a dedicated `compute_frames: u64` counter — **deliberately not the frame number** (a deviation from the original migration plan). Reason: `has_compute_pipelines` can flip true mid-run, at the first `create_compute_pipeline` call. If compute values were frame numbers, the first compute submit at frame N would wait on `≥ N−1` — a value nothing ever signals, since values 1..N−1 were skipped. Deadlock. With a dedicated counter, the first compute submit waits on `≥ 0`, trivially.

Per frame with compute active (`compute_value = compute_frames + 1`):

- **Non-pipelined** (one combined command buffer): the submit waits `compute_timeline ≥ compute_value − 1` at `VERTEX|FRAGMENT|COMPUTE_SHADER`, and signals `render_finished` (binary), `frame_timeline = N`, and `compute_timeline = compute_value`. Waiting and signaling the same timeline in one submit is legal because the waited value is lower.
- **Pipelined** (separate compute submit, possibly on a dedicated queue): the compute submit waits `≥ compute_value − 1` at `COMPUTE_SHADER` and signals `compute_value`; the graphics submit waits the *same* `compute_value − 1` at `VERTEX|FRAGMENT|COMPUTE_SHADER` — the one-frame-behind read that `compute_to_graphics_sem` used to carry. Two waiters, one signal.
- The compute command-buffer-reuse fence became a CPU wait `compute_timeline ≥ compute_value − MAX_FRAMES_IN_FLIGHT`. This is exact because once compute is active every frame signals the compute timeline (both branches), so the CB slot's previous user — same frame parity, two frames ago — signaled precisely that value.

A second small deviation: `use_pipelined` now requires `compute_frames > 0`, replacing the `compute_bootstrapped` flag with a derived check. The first compute frame still goes through the combined command buffer, so graphics sees that frame's compute output rather than uninitialized buffers — same behavior as before, no managed state.

### The deleted recreate-swapchain dance

The old code destroyed and recreated `compute_finished` + `compute_to_graphics_sem` inside `recreate_swapchain`, because after `device_wait_idle` some binary semaphores could be left signaled with no pending waiter — and a signaled binary semaphore cannot be reset, only destroyed. Timeline semaphores have no such stuck state, so the block is gone. The inverse rule now applies and is commented at the site: the timelines must **not** be recreated there — a fresh semaphore restarts the counter at 0, and the next frame's wait on `≥ N−2` would hang forever. Values stay monotonic across swapchain recreation; `device_wait_idle` guarantees every signaled value has been reached before the swapchain is rebuilt.

## Rules of thumb (gotchas encoded in this code)

- Signals must be **strictly increasing** per timeline. Never signal the same value twice — fold multiple consumers into one signal with multiple waiters instead.
- Waits are `≥`, and waiting on an already-passed or zero value returns immediately — lean on this instead of bootstrap flags and pre-signaled objects.
- Derive wait values from **monotonic counters that advance exactly when their signal is submitted** (`total_frames` for graphics, `compute_frames` for compute). The audit point is early-return paths: an acquire failure returns before the increment (nothing submitted, nothing advanced); a present failure returns after both (signal is in flight; `recreate_swapchain`'s idle wait absorbs it).
- Keep WSI semaphores binary; mix them into the same `SubmitInfo2` as timeline entries.
- `u64` values at ~one per frame will not wrap in any human timescale (2^64 frames ≈ 10^12 years at 60 fps).

## Search terms and reading

- `VK_KHR_timeline_semaphore`, `vkWaitSemaphores`, `vkGetSemaphoreCounterValue`, `SemaphoreTypeCreateInfo`
- **Khronos blog: "Vulkan Timeline Semaphores"** — the canonical introduction, including the design rationale and WSI caveat.
- **Vulkan spec §7.4 (Semaphores)** — normative rules for strictly-increasing signals, wait-before-signal, and host operations.
- **Vulkan-Samples `timeline_semaphore`** (KhronosGroup) — a runnable minimal example.
- The **synchronization2** notes pair well: timeline entries ride the same `SemaphoreSubmitInfo`/`SubmitInfo2` structures Phase 1 introduced.
