# Packed per-frame uniform buffer for Roc graphs

## Context

- On every frame the Roc host allocates and copies the frame values twice. The host-side cost is N + 1 heap allocations for N draws.
  - Roc side: each leaf packer returns `[bytes]`, which is a `List(List(U8))`. Each tuple level joins those lists.
  - Host side: `FrameSubmission::fetch` copies every inner list into a `Vec<Vec<u8>>`. `PreparedRenderGraph<Vec<DynDrawNode>>` then consumes that `Vec<Vec<u8>>`.
- `PlanCtx::stage_uniform_bytes` copies the bytes into the staging buffer anyway (`crates/render-graph/src/runtime.rs:1576`). The frame value only has to be readable during `plan`.
- Goal: Roc sends one `List(U8)` per frame. The host splits it at offsets that are fixed when the graph is created. The host makes no per-frame heap allocations after the first frame.

## Approach

### Offsets are static

- Each node's uniform size (`gpu_size`) is fixed when the graph is created. The offset of each node is the sum of the sizes of the nodes before it.
- The wire format carries no offsets. Roc sends only the concatenated bytes.
- The host checks one value per frame: total length == sum of `gpu_size`.

### render-graph (`crates/render-graph/src/runtime/dynamic.rs`)

- Add `DynDrawList`. It holds `Vec<DynDrawNode>` and `total_size: usize`, which the constructor computes.
- `impl GraphNode for DynDrawList`:
  - `type Frame = Vec<u8>`.
  - `lower` lowers each node in order.
  - `plan` checks `frame.len() == total_size`, splits the buffer by each node's `gpu_size`, and plans each node with its slice.
- Move the body of `DynDrawNode::plan` into a private `plan_bytes(&self, bytes: &[u8], cx)`. `DynDrawNode::plan` calls it.
- Add `impl Sealed<B> for DynDrawList`.
- Move the tests in `dynamic.rs` and `backend_dynamic_draw_plan` in `backend_tests.rs` to `DynDrawList`. The tests cover a short buffer, a long buffer, and "nothing submitted".
- Open decision: keep `impl GraphNode for Vec<N>`, or remove it. After this change its only users are tests.

### Roc platform

- `roc-platform/platform/RenderGraph.roc`:
  - `pack : frame -> List(U8)`.
  - `make_graph` returns `bytes`, not `[bytes]`.
  - The tuple packers keep the same text: `[pack0(v0), ...].join()` now joins `List(List(U8))` into `List(U8)`.
- `ValidatedRenderGraph.roc` and `GraphSet.roc`: change the packer and `Draw.values` annotations to `List(U8)`.
- Run `just glue` in `roc-platform/` to regenerate `src/roc_platform_abi.rs`.
- `roc-platform/src/game.rs`:
  - Store `frame_bytes: Vec<u8>` on the game struct. Each frame, call `clear` and then `extend_from_slice(owned.values.as_slice())`.
  - Change the graph type to `PreparedRenderGraph<DynDrawList>`. Build the `DynDrawList` in `prepare`.
- `GraphLifecycle::frame` is generic over `V`, so it needs no change.

### Docs

- `docs/render_graph.md`: the `DynDrawNode` section and the paragraph about `Vec<N>`.
- `roc-platform/README.md`: the description of frame values at about lines 87 and 120.

## Stack placement

- Put the change on a new branch above `roc-graphs/graph-set-example`. That branch sees only the final state, so the PRs below it get no conflicts.
- Before you branch, run `gh stack rebase`. `dynamic-nodes` (#36) was rewritten, but #37–#44 still sit on the old commits. #42 contains `4d8e2f1`, which duplicates `c3beebe` on #36.

## Out of scope

- Zero-copy frames: `Frame` would need a lifetime parameter.
- Append-style Roc packers, so that Roc allocates one list per frame. This needs a change to the `roc-codegen` templates.

## Verification

- `cargo check --workspace --all-targets`, `just lint`, and `just test`.
- `cargo test -p mltrs-render-graph` (the dynamic and backend tests).
- `roc-platform/ci/all_tests.sh`: the invalid fixtures still fail `roc check`, and the examples build.
- Run `basic-triangle/two_draws.roc` and `creative-coding-sep-23` to see that the frames render.
