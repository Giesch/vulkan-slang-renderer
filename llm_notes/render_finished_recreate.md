# Recreate `render_finished` with the swapchain

> **STATUS: PLAN.** One standalone commit. Line numbers are as of `95bcc37`.

Origin: [`archived/frame_ring_collapse.md`](archived/frame_ring_collapse.md)
Phase 4 and [`archived/remove_pipelined_compute.md`](archived/remove_pipelined_compute.md)
Phase 4c both record this bug and defer it. The decoupled acquire-semaphore
design from Phase 4 is deferred again here; see §5.

## 1. The bug

`render_finished: Vec<vk::Semaphore>` (`crates/renderer/src/renderer.rs:180`)
holds one binary semaphore per swapchain image. It is sized once in
`create_sync_objects` from `swapchain_images.len()` (`:4214-4217`) and
destroyed once in `Drop` (`:3065-3067`).

`recreate_swapchain` (`:2735`) rebuilds the swapchain, image views, depth
image, MSAA image, resolve images and picking images. It does not touch
`render_finished`.

`create_swapchain` requests `min_image_count + 1` clamped to
`max_image_count` (`:3758-3766`). The driver returns any count ≥ the request.
Surface capabilities are re-queried on every recreation, so the returned
count can change on resize, fullscreen toggle, monitor move, or present-mode
change.

- Count grows: `self.render_finished[image_index]` (`:2666`, `:2699`) indexes
  past the vec. Rust panics on the first acquire of a new image.
- Count shrinks: the surplus semaphores stay alive until `Drop`. Each
  recreation that shrinks the count leaks nothing new, but the vec keeps stale
  entries that nothing signals or waits on.

Semaphore reuse across recreation with an unchanged count is safe today:
`recreate_swapchain` begins with `device_wait_idle` (`:2736`), and the
presentation engine releases a binary semaphore once its present is
processed.

## 2. The fix

In `recreate_swapchain`, after `self.swapchain_images` is refreshed
(`:2782-2783`):

1. Destroy every semaphore in `self.render_finished`.
2. Create `self.swapchain_images.len()` new semaphores and assign the vec.

`device_wait_idle` at the top of the function makes step 1 safe: no submit or
present still references the old semaphores.

Factor the creation loop at `:4214-4217` into a helper so
`create_sync_objects` and `recreate_swapchain` share it:

```rust
fn create_render_finished_semaphores(
    device: &ash::Device,
    swapchain_images: &[vk::Image],
) -> Result<Vec<vk::Semaphore>, anyhow::Error>
```

Change list:

- `create_sync_objects` (`:4195`): call the helper.
- `recreate_swapchain` (`:2735`): destroy the old vec, call the helper.
- No field, signature, or call-site change elsewhere.

## 3. Out of scope

- `image_available` stays `[vk::Semaphore; MAX_FRAMES_IN_FLIGHT]` indexed by
  `flight_slot`. The frame-timeline wait at `:2593-2598` proves the previous
  waiter on that slot's semaphore retired. That guarantee holds at
  `MAX_FRAMES_IN_FLIGHT = 2`.
- The timeline semaphores are monotonic and must not be recreated; the
  existing comment at `:2738-2739` stays.

## 4. Verification

- `cargo check --workspace --all-targets`, `just lint`, `just test`.
- `just sweep` 16/16.
- Manual, at the window: drag-resize `basic_triangle` and `watercolor`
  through a resize storm, toggle fullscreen, and close mid-resize. No panic,
  no validation message, no VMA leak at exit.
- Forced count change: temporarily set `desired_image_count` to
  `min_image_count + 2` in `create_swapchain` after the first creation, resize,
  confirm the new vec length matches `swapchain_images.len()`, then revert.
  This is a local check, not a committed test.

## 5. Deferred: per-image acquire semaphores

The decoupled design sizes `image_available` per swapchain image, acquires
with a spare semaphore, and swaps it into `image_available[image_index]`
after `acquire_next_image` returns. Both rings then share one lifetime and one
recreation path.

Deferred because:

- The minimal fix above closes the bug on its own.
- The design touches the acquire path (`:2601-2616`), which the
  `draw_frame` comments mark as a place that has broken before.
- It becomes necessary only if `MAX_FRAMES_IN_FLIGHT` exceeds the swapchain
  image count, or if the frame-timeline wait stops guarding `image_available`.

Revisit when something else changes the acquire path.
