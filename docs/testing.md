# Testing

Current as of `main` @ `3c1249e`. Two independent things to check, and they
overlap less than you'd expect:

| | what it covers | command |
|---|---|---|
| **Snapshot tests** | generated Rust and shader reflection JSON. No GPU involved. | `just test` |
| **Validation sweep** | every example actually running, checked for Vulkan validation errors. | `just sweep` |

`just test` passing says nothing about whether the renderer works, and the sweep
says nothing about whether codegen is correct. A renderer change wants both.

## Snapshot tests

Uses [insta](https://insta.rs) for snapshot testing of generated code.

```bash
just test                  # non-interactive (CI)
just insta                 # interactive review
cargo insta test --accept  # re-run and accept every changed snapshot
```

Always run `just test` when changing `src/shaders/build_tasks.rs` or the
`templates/*.askama` files — that's what the snapshots cover.

**NOTE `cargo insta accept` on its own does nothing after `just test`**: that
recipe sets `INSTA_UPDATE=no`, so no `.snap.new` files are written for it to
review. Use `cargo insta test --accept`, which re-runs the tests and writes the
snapshots in one step. Review the diffs `just test` prints before accepting.

**Known open issue:** the snapshots capture *pre-rustfmt* template output while
`just shaders` runs `cargo fmt` afterwards, so a snapshot can never be
byte-identical to its committed generated file. Differences are confined to
import placement and signature wrapping. Not yet fixed; see
`llm_notes/build_reproducibility.md` §2.

## Validation sweep

`scripts/headless-sweep.sh` runs every example under a software Vulkan driver
(lavapipe) with no window and no display, and exits nonzero if any of them emits
Vulkan validation output.

```bash
just sweep                                         # all examples (~10s each + a build)
just sweep sprite_batch                            # just these
just sweep-self-test                               # only prove the detector works
SWEEP_TIMEOUT=30 scripts/headless-sweep.sh         # longer window per example
SWEEP_SKIP=watercolor scripts/headless-sweep.sh    # force-skip by name
SWEEP_LOG_DIR=/tmp/logs scripts/headless-sweep.sh  # where per-example logs go
SWEEP_SELF_TEST=0 scripts/headless-sweep.sh        # skip the self-test
```

Requires `mesa-vulkan-drivers vulkan-validationlayers libvulkan-dev`. It needs no
GPU, no display and no sound card, so it works in a container. The lavapipe ICD
is pinned deliberately even on a machine with a real GPU, so results stay
comparable across machines.

### When to run it

**When a change could affect what the renderer records or destroys.** That means
`src/renderer.rs` — especially command recording, synchronization and teardown —
`src/app.rs`, anything touching descriptors or resource lifetimes, and any time
you add or rework an example. It's fast enough to be the default check on
renderer work.

`just watch EXAMPLE` remains the quicker single-example look when you want to
*watch* one run, but it is not a substitute: it covers one example.

### How it decides

The verdict is each example's **exit code**. `src/renderer/debug.rs` counts
validation messages by the severity Vulkan reports, and `Game::run` reads that
count once the `Renderer` has been dropped — which is after `vkDestroyDevice`
and its leaked-object report, so teardown is included.

| exit | meaning |
|---|---|
| 0 | drew at least one frame, shut down cleanly, no validation output |
| 1 | validation messages, or any other error out of `main` |
| 2 | validation is compiled out — a `--release` build validates nothing |
| 3 | exited without ever drawing a frame |
| 143 / 137 | died on a signal, so `Drop for Renderer` never ran and teardown went unchecked |
| 101 | panic |

Codes 2 and 3 apply only under `VKR_SWEEP=1`, which the script exports; they'd
be wrong interactively, where closing a window immediately is not an error.
Code 1 is **not** sweep-gated, on purpose: an interactive run that emitted any
validation message — including warning severity — exits 1 at window close, so a
`just dev` or `just watch` session fails loudly rather than looking clean.

Because the count keys off severity rather than the log level, `RUST_LOG` can
hide the *detail* of a failure but not the failure itself. The script still
greps each log, but as a cross-check: a log and an exit code that disagree are
reported as `FAIL(detector disagreement)` rather than quietly resolved either
way.

### Traps

Both are measured rather than assumed — the evidence is in
`llm_notes/build_reproducibility.md` §7.3. They share one shape: **a broken
example passes silently**, hiding a failure rather than reporting one. The rest
of what used to be on this list is now enforced by the exit codes above.

- **Never wrap a validation check in `timeout N cargo run`.** That times the
  *compile* as well as the run, so on a cold build the timeout expires during
  compilation, cargo exits 124 with an empty log, and every example looks fine.
  One `touch src/renderer.rs` was once enough to make all 16 report `ok` with 16
  empty logs. Both the sweep and `just watch` build up front, then time the
  binary directly.
- **Don't use `timeout --foreground`** on an example launched through `just` or
  `cargo`. Plain `timeout` signals the whole process group, so the example gets
  SIGTERM; `--foreground` signals only `just`, orphaning the example to run
  forever.

One thing that is *not* a trap, contrary to what older notes in `llm_notes/`
claim: `timeout` does **not** skip `Drop`. It sends SIGTERM, SDL converts that to
`SDL_QUIT`, and the loop exits normally, so `drain_gpu()` and `Drop for Renderer`
both run and `vkDestroyDevice` reports leaked objects. Teardown is covered on
every example on every run. What *does* break it is `timeout -s KILL` or
`SDL_NO_SIGNAL_HANDLERS=1`; the script avoids both, and a run that dies on a
signal anyway now reports 143 rather than looking like a pass. See
`llm_notes/build_reproducibility.md` §7.4 for the measurement matrix.

### Machine-local assets

`toon_link` needs `assets/link/converted`, which is gitignored and derived from a
disc image. The script tests for those assets and skips the example where they're
absent, so the same invocation is correct on a dev machine and in a bare
container — 16 ok / 0 skip locally, 15 ok / 1 skip in a container. Run
`just extract-link && just convert-link` to make it sweepable. Every other
example loads from tracked `textures/`, `models/` or `audio/`.

### If you change the script

Run `just sweep-self-test`. It injects a fault
(`VKR_INJECT_VALIDATION_FAULT=1` makes `Renderer::viewport_width` record an
invalid width) and fails unless the sweep reports it. A full sweep runs the same
check first and aborts if the injected fault goes undetected, since a sweep
whose detector is broken reports a clean pass for everything.

This matters more than it sounds: a sweep that has silently stopped working looks
exactly like a passing one. Detection has been verified against injected faults
at all three points in the lifecycle — device init, per-frame command recording,
and teardown-only — since "when does the error happen" is the axis a
timeout-based sweep is most likely to be blind to. The self-test covers the
command-recording point; the other two are in
`llm_notes/build_reproducibility.md` §7.2.
