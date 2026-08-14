# Testing

Two checks cover different things. A renderer change needs both.

| check            | what it covers                                               | command      |
| ---------------- | ------------------------------------------------------------ | ------------ |
| snapshot tests   | generated Rust and shader reflection JSON. No GPU.           | `just test`  |
| validation sweep | every example running, checked for Vulkan validation output. | `just sweep` |

`just test` says nothing about whether the renderer works. The sweep says
nothing about whether codegen is correct.

## Snapshot tests

[insta](https://insta.rs) holds the snapshots of the generated code.

```bash
just test                              # non-interactive (CI)
just insta                             # interactive review
cargo insta test --workspace --accept  # re-run and accept every changed snapshot
```

Run `just test` after changing `crates/cli/src/build_tasks.rs` or
`crates/cli/templates/*.askama`. Those are what the snapshots cover.

`cargo insta accept` does nothing after `just test`. The recipe sets
`INSTA_UPDATE=no`, so `just test` writes no `.snap.new` files for it to review.
Use `cargo insta test --workspace --accept`. It re-runs the tests and writes the
snapshots in one step. Read the diffs `just test` prints before you accept.

The snapshots capture template output before rustfmt.
`just shaders` runs `cargo fmt` after codegen. The differences are limited to
import placement and signature wrapping.
See `llm_notes/build_reproducibility.md` §2.

## Validation sweep

`scripts/headless-sweep.sh` runs every example under the lavapipe software
driver, with no window and no display. It exits nonzero if any example emits
Vulkan validation output.

```bash
just sweep                                         # all examples (~10s each, plus a build)
just sweep sprite_batch                            # only the named examples
just sweep-self-test                               # only prove the detector works
SWEEP_TIMEOUT=30 scripts/headless-sweep.sh         # seconds per example (default 10)
SWEEP_SKIP=watercolor scripts/headless-sweep.sh    # force-skip by name
SWEEP_LOG_DIR=/tmp/logs scripts/headless-sweep.sh  # per-example logs (default /tmp/sweep-logs)
SWEEP_SELF_TEST=0 scripts/headless-sweep.sh        # skip the self-test
```

The sweep needs `mesa-vulkan-drivers vulkan-validationlayers libvulkan-dev`. It
needs no GPU, no display and no sound card, so it runs in a container. The
script pins the lavapipe ICD even on a machine with a real GPU, so results stay
comparable across machines.

### When to run it

Run it when a change could affect what the renderer records or destroys:

- `crates/renderer/src/renderer.rs`, in particular command recording,
  synchronization and teardown
- `crates/mltrs/src/app.rs`
- anything that touches descriptors or resource lifetimes
- any new or reworked example

`just watch EXAMPLE` is the quicker way to watch one run. It covers one
example, so it is not a substitute.

### How it decides

The verdict is each example's exit code.
`crates/renderer/src/renderer/debug.rs` counts validation messages by the
severity Vulkan reports. `Game::run` reads that count after the `Renderer` is
dropped. The drop happens after `vkDestroyDevice` and its leaked-object report,
so the count includes teardown.

| exit      | meaning                                                                        |
| --------- | ------------------------------------------------------------------------------ |
| 0         | drew at least one frame, shut down cleanly, no validation output               |
| 1         | validation messages, or any other error out of `main`                          |
| 2         | validation is compiled out — a `--release` build validates nothing             |
| 3         | exited without drawing a frame                                                 |
| 101       | panic                                                                          |
| 143 / 137 | died on a signal, so `Drop for Renderer` never ran and teardown went unchecked |

Codes 2 and 3 apply only under `VKR_SWEEP=1`, which the script exports.
Interactively they would be wrong: closing a window at once is not an error.
Code 1 is not sweep-gated, on purpose. An interactive run that emits any
validation message — warning severity included — exits 1 at window close, so a
`just dev` or `just watch` session fails loudly.

The script also fails an example that exits 0 before its window ends. An early
clean exit means most of the run went unobserved.

The count keys off severity rather than the log level. `RUST_LOG` can hide the
detail of a failure. It cannot hide the failure. The script still greps each
log, as a cross-check. A log and an exit code that disagree report as
`FAIL(detector disagreement)`.

### Traps

Both traps share one shape: a broken example passes silently.
`llm_notes/build_reproducibility.md` §7.3 holds the measurements.

- Never wrap a validation check in `timeout N cargo run`. That times the
  compile as well as the run. On a cold build the timeout expires during
  compilation, cargo exits 124, and the log is empty. All 16 examples then
  report `ok` with 16 empty logs. One edit to `renderer.rs` is enough to
  trigger it. The sweep and `just watch` build up front, then time the binary
  directly.
- Never use `timeout --foreground` on an example launched through `just` or
  `cargo`. Plain `timeout` signals the whole process group, so the example gets
  SIGTERM. `--foreground` signals `just` only, which orphans the example to run
  forever.

`timeout` does not skip `Drop`. It sends SIGTERM, SDL converts SIGTERM to
`SDL_QUIT`, and the loop exits normally. `drain_gpu()` and `Drop for Renderer`
both run, and `vkDestroyDevice` reports leaked objects. Teardown is covered on
every example on every run. `timeout -s KILL` and `SDL_NO_SIGNAL_HANDLERS=1` do
break this, and the script uses neither. A run that dies on a signal reports 143. `llm_notes/build_reproducibility.md` §7.4 holds the measurement matrix.

### Machine-local assets

`toon_link` needs `examples/toon_link/assets/link/converted`. That directory is
gitignored and derived from a disc image. The script tests for the assets and
skips the example where they are absent, so one invocation is correct on a dev
machine and in a bare container: 16 ok / 0 skip locally, 15 ok / 1 skip in a
container. Run `just toon_link extract-link && just toon_link convert-link` to
make the example sweepable. Every other example loads from tracked assets
inside its own `examples/<name>/` crate.

### If you change the script

Run `just sweep-self-test`. It sets `VKR_INJECT_VALIDATION_FAULT=1`, which
makes `Renderer::viewport_width` record an invalid width. The self-test fails
unless the sweep reports the fault. A full sweep runs the same check first, and
aborts if the injected fault goes undetected.

A sweep whose detector is broken reports a clean pass for everything, and looks
exactly like a passing sweep. Detection is checked against injected faults at
three points in the lifecycle: device init, per-frame command recording, and
teardown. "When does the error happen" is the axis a timeout-based sweep is
most likely to be blind to. The self-test covers command recording.
`llm_notes/build_reproducibility.md` §7.2 covers the other two.
