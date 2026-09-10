# Phase 1b — Render Graph Simplification

STATUS: IMPLEMENTED (2026-09-10). Automated gates passed; interactive validation
limits are recorded below.

This work simplifies the combined 07/08-phase-1 branch while preserving the
migration requirements in [`../08_plain_data_graph.md`](../08_plain_data_graph.md).
It does not wire the pure compiler into execution or close later-phase ledger items.

## Constraints

Preserve the public tuple API, generated split types, constructor signatures,
frame types, picking compatibility, and submission-aware cursor commits. Retain
the pure compiler/expander, assembly vocabulary, lowering side tables,
future-feature variants, and their rejection tests.

Preserve declaration-level validation of unused uniform sources and per-command
hazard checks: each draw remains a separate command. The only intended validation
change is rejecting invalid draw push-data references already rejected for compute.
Do not trim the phase-1 test inventory or erase typed resource distinctions.

## Implementation sequence

Run focused checks after each task; group full repository validation at completion.

1. **Share the draw-call input enum.** Replace `DrawCallKind` with
   `lower::LowerDrawCall`, deriving `Clone, Copy`. Delete the field-for-field
   conversion in `DrawNode::lower`. Preserve the canonical description's separate
   `DrawCall`, which carries graph buffer IDs and checked offsets. Retain all
   four-variant lowering and indirect-offset overflow tests.
2. **Move texture declarations into lowering.** Move `resources.decls` into
   `LowerCtx::new`; allocate from `lowered.desc.textures` after all validation.
   Preserve declaration order, names, image counts, and handle ownership.
   Existing extent, call-site, and physical-count tests suffice.
3. **Share description traversal.** Add internal iterators for commands and their
   uniform-plus-push bindings. Yield one command per dispatch or draw, carrying
   name, pipeline, uniform, and optional push. Use these in validation and compiler
   access collection. Preserve order and duplicate accesses, tolerate missing
   uniform IDs during traversal, and validate unused declarations separately.
   Retain independent-draw and shared-source tests; cover missing uniform IDs and
   duplicate writes split between uniform and push bindings if not already tested.
4. **Consolidate command and push validation.** Share pipeline-kind, uniform-ID,
   push-schema, push-data, and binding checks. Keep group, raster-target,
   draw-write, and indirect-buffer checks specific. Draw push data must reference
   an existing `Bytes` value, matching compute. Preserve existing diagnostic order.
   Add paired compute/draw cases for out-of-range IDs, wrong kinds, valid bytes,
   and absent push data.
5. **Share push-assembly insertion.** One private compiler helper appends an
   optional push program and returns its `AsmId`. Keep uniform assemblies first,
   followed by pushes in pass/draw order; preserve explicit layouts and `Deferred`.
   Test mixed compute/raster commands with and without pushes, including IDs and
   program order. Reuse field-layout tests.
6. **Remove tuple-only access collection.** Replace `collect_access`, `NodeAccess`,
   and `PlanCtx::apply_writes` with a binding visitor that commits only graph
   `Write` cursors after dispatch bindings resolve. Keep full pure `LeafAccess`.
   Test ignored reads, previous reads, mutations, external textures, and buffers;
   single-image writes; and writes from separate uniform and push bindings.
   Duplicate writes remain a validation error, not executor deduplication.
7. **Unify compute nodes.** Use `ComputeNode<S, P = ()>` and alias
   `ComputeNodeWithPush<S, B>` to `ComputeNode<S, PushValues<B>>`. Keep dispatch
   constructors and frame types unchanged. Share lowering, staging, and queueing.
   Add default no-op binding visitation to `GraphPush`, overridden by `PushValues`.
   Resolve uniform and push values before any cursor commits; re-resolve pushes
   on every repeat iteration without allocating binding vectors during execution.
   Compile existing particles/watercolor type declarations. Add CPU-only resolver
   coverage with synthetic handles for pre-commit visibility, subsequent rotation,
   and fixed push data. Retain zero-repeat and skipped-optional tests and exercise
   tuple execution in runtime validation.
8. **Classify codegen fields once.** Build a split representation once for name
   collision checks and rendering. Preserve shader-local analysis, transitive
   nested-resource rejection, shared-module names, and synthetic padding. Keep
   classification/collision errors ahead of nested-resource errors. Run existing
   codegen tests and snapshots without updates; add combined-invalid-input
   precedence coverage if missing. Generated Rust must remain unchanged.

## Validation and acceptance

The review baseline is 101 passing render-graph tests, not evidence of visual
parity or CLI correctness. During implementation run the focused renderer tests,
CLI tests with `INSTA_UPDATE=no` after codegen changes, and workspace all-target
checks after public type changes.

Completion gates: `cargo fmt --check`, `cargo check --workspace --all-targets`,
`cargo test -p mltrs-renderer`, `just lint`, `just test`, `just shaders`, and
`just sweep`. Inspect regeneration output rather than accepting unexpected
snapshot or generated-source changes.

Interactive checks: watercolor painting, all pigments, debug views, odd/even
pressure-loop counts, skipped brush work, and texture history; particles normal
animation. Record unavailable checks and Vulkan validation failures explicitly.

Finish with a correctness and Rust-style review, confirm code removal and public
API compatibility, and record task completion and validation evidence here.
Keep the historical phase-1 review intact.

## Implementation evidence

All eight tasks are complete in the sequence above. The facade and lowering share
`LowerDrawCall`; texture declarations move into lowering; `Command` and iterator
helpers share pure traversal; command validation checks compute and draw pushes
consistently; push assembly insertion is shared; tuple cursor updates visit only
writes; compute nodes share the `GraphPush` implementation; and codegen classifies
each field once before checking names and rendering.

The implementation removes 178 production lines relative to the branch before
phase 1b (counted before each file's test module). Seven new tests cover missing
uniform IDs, writes split across uniform/push bindings, compute/draw push data,
mixed-command assembly ordering, cursor commit filtering, synthetic binding
resolution with fixed push data, and codegen diagnostic precedence. The original
phase-1 tests remain. The `ComputeNodeWithPush` alias and defaulted compute push
parameter compile against the existing example type declarations; the added
`GraphPush::visit_bindings` method defaults to a no-op for existing implementations.

Validation completed on 2026-09-10:

| Check | Result |
| --- | --- |
| Focused render-graph tests | 107 passed (baseline: 101) |
| `cargo test -p mltrs-renderer` | 117 passed |
| CLI tests with `INSTA_UPDATE=no` | 52 passed |
| `cargo check --workspace --all-targets` | Passed |
| `cargo fmt --check` and `git diff --check` | Passed |
| `just lint` | Debug and release passed with warnings denied |
| `just test` | Passed; six existing workspace tests ignored |
| `just shaders` | Passed; generated files and snapshots unchanged |
| `just sweep` | 16 passed, zero skipped, zero failed; fault-injection self-test passed |

The shader recipe initially failed because the sandbox could not create Just's
temporary script under `/run/user/1000`; its approved rerun outside the sandbox
passed. No renderer or codegen failure required a workaround.

An X11 session exercised all twelve watercolor pigment keys with brush strokes,
idle frames with brush work skipped, and the Pigments and Wet Area Mask views.
Painted output remained visible during subsequent idle frames. Particles rendered
at two sampled times. Both examples exited cleanly with code 0 and empty warning
logs under `VKR_SWEEP=1` and `RUST_LOG=warn`. These are smoke checks, not a pixel
comparison against the pre-refactor branch. Captures and logs are temporary
artifacts under `/tmp/phase1b-*`, not repository assets.

The live watercolor session used its unchanged compile-time
`JACOBI_ITERATIONS = 2`. Odd counts, zero counts, and skipped optional scopes are
covered by the retained pure expansion tests; repeated push resolution and cursor
rotation also have new CPU-only coverage. An odd-count interactive watercolor run
was not performed. No example source was changed for validation.

Correctness and Rust-style review completed. Later-phase compiler/expander
scaffolding, side tables, rejection tests, public tuple inputs, picking
compatibility, and submission-aware state commits remain intact. This work closes
no later-phase ledger item.
