# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo check --all-targets  # Check source AND examples for type errors
just shaders               # Generate shader bindings (MUST run after .slang changes)
just test                  # Run tests (snapshot testing via insta)
cargo insta test --accept  # accept all modified snapshots
just lint                  # Clippy with warnings as errors
timeout 3 just dev EXAMPLE # run one example in a window, watch for validation errors
scripts/headless-sweep.sh  # run EVERY example headlessly, fail on validation output
cat shaders/compiled/EXAMPLE.json | jq '.' # inspect shader reflection json
```

### After changes
- Always run `just shaders` after modifying any `.slang` files to regenerate Rust bindings.
- Always use `cargo check --all-targets` when changing rust files as a first pass.
  NOTE `--all-targets`, not `--all`: `--all` means "all workspace members" and
  silently skips examples, so a broken example passes. `--all-targets` covers
  examples, benches and `#[cfg(test)]` code. Same distinction applies to clippy
  (`just lint` uses `--all-targets`).
- Always use `just test` when making changes to shaders/build_tasks.rs
- Run `cargo fmt` after a set of rust file changes are complete
- Never edit `src/generated/` by hand — `just shaders` regenerates it.

## Shader System

**Workflow:**
1. Create/edit `shaders/source/*.shader.slang`
2. Run `just shaders`
3. Generates: SPIR-V bytecode + reflection JSON + Rust bindings in `src/generated/`

## Testing

Uses insta for snapshot testing of generated code:
```bash
just test                  # Non-interactive (CI)
just insta                 # Interactive review
cargo insta test --accept  # Re-run and accept every changed snapshot
```

NOTE `cargo insta accept` on its own does nothing after `just test`: that recipe
sets `INSTA_UPDATE=no`, so no `.snap.new` files are written for it to review.
Use `cargo insta test --accept`, which re-runs the tests and writes the
snapshots in one step. Review the diffs `just test` prints before accepting.

## Vulkan validation sweep

`just test` does not touch the GPU. To check for Vulkan validation errors, run
`scripts/headless-sweep.sh` — it runs every example under a software driver
(lavapipe) with no window and no display, and exits nonzero if any of them emits
validation output. Not yet wired to a `just` recipe.

```bash
scripts/headless-sweep.sh                    # all examples (~10s each + a build)
scripts/headless-sweep.sh sprite_batch       # just these
SWEEP_TIMEOUT=30 scripts/headless-sweep.sh   # longer window per example
SWEEP_SKIP=watercolor scripts/headless-sweep.sh   # force-skip by name
```

**Run it when a change could affect what the renderer records or destroys** —
`src/renderer.rs` (especially command recording, synchronization or teardown),
`src/app.rs`, anything touching descriptors or resource lifetimes, and after
adding or reworking an example. It is fast enough to be the default check on
renderer work; `timeout 3 just dev EXAMPLE` remains the quicker single-example
look when you also want to *see* the output.

Requires `mesa-vulkan-drivers vulkan-validationlayers libvulkan-dev`. It needs
no GPU, no display and no sound card, so it works in a container.

Things that will mislead you if you don't know them (all measured — see
`llm_notes/build_reproducibility.md` §7.3):

- **An example's exit code says nothing about validation.** The debug callback
  returns `VK_FALSE`, so a run with 500 errors still exits 0, and a run that
  used its whole window exits 124. The script greps the log; don't judge a
  hand-run example by its status.
- **Don't check validation with `--release`.** `ENABLE_VALIDATION` is
  `cfg!(debug_assertions)`, so a release run validates nothing and passes
  everything.
- **`RUST_LOG` must be set to `warn` or lower**, which the script does itself.
  Unset, env_logger keeps only `error!` and WARNING-severity validation
  disappears; set to another module (as `.env` does) everything disappears.
- **Never wrap a validation check in `timeout N cargo run`** — that times the
  *compile* too, so on a cold build the timeout expires during compilation,
  cargo exits 124 with an empty log, and every example looks fine. The script
  builds first, then times the binary.
- `toon_link` skips itself where its gitignored assets are absent, and sweeps
  normally where `just extract-link && just convert-link` has been run.

**If you change the script, re-check that it still detects a fault** — e.g. set
a viewport width to `1e9` in `record_command_buffer`, confirm
`VUID-VkViewport-width-01771` is reported, and revert. A sweep that has silently
stopped working looks exactly like a passing one.
