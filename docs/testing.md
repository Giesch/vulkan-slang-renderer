# Testing

Three checks cover different things. A renderer change needs the normal tests
and validation sweep; Roc codegen changes also need the explicit Roc gate.

| check             | what it covers                                                       | command                 |
| ----------------- | -------------------------------------------------------------------- | ----------------------- |
| normal tests      | generated Rust/JSON and compiler-independent Roc codegen. No GPU/Roc. | `just test`             |
| Roc codegen gate  | generated modules, imports, values, bytes, and formatting in real Roc | `just roc-codegen-test` |
| validation sweep  | generated GPU readback and examples, checked for Vulkan validation output | `just sweep`         |

`just test` says nothing about whether the renderer works. The sweep checks
generated readback on Vulkan but does not replace the codegen suite. It starts
`toon_link` in its default GameCube mode; it does not select Modern and is not
Modern GPU or visual evidence. See [`toon_link.md`](toon_link.md) for the
separate interactive coverage matrix.

## Link local checks

`just toon_link link-verify-model` checks the raw asset hashes, converts into a
temporary directory, checks every entry in `link_converted.sha256`, and runs
the P1/P2/P3 oracle comparisons and ignored real-file tests. It requires the
extracted raw assets and `uv` with access to the pinned gclib dependency. Missing
or changed assets fail the check; prepare them with `just toon_link extract-link`
and `TWW_DIR` set. For only the raw and converted hashes, run
`just toon_link link-verify-goldens`. Neither command updates the golden files.

`just toon_link verify-assets` runs `link-verify-model` and
`link-verify-animations`. The animation gate extracts the animation raws on
every run, so it requires `TWW_DIR` and the `dtk` binary in addition to the
model prerequisites above.

`just test` does not run these gates: Cargo skips ignored real-file tests, and
the hash checks are separate commands. P1/P2/P3 alone also do not check the
converted golden hashes.

## Pre-commit hook

Install the local hook with `just setup-precommit`. `just pre-commit` builds
the workspace graph with `just _workspace-graph`, which reduces
`cargo metadata --no-deps` with `jq` to one `{name, dir, dependencies}` object
per package. It passes the graph to `scripts/pre-commit-checks.rs` as the
first argument and pipes the staged paths into it. The script prints `just`
arguments, one per line, and the recipe passes them to `just` unchanged. The
script always names at least one recipe:

- `_pre-commit-skip` alone, when no path needs a check.
- Otherwise `_pre-commit-shaders` and `lint` first, then
  `toon_link::verify-assets` when the Link asset gates are needed, then
  `_roc-codegen-test-if-available`, then `test-crates` followed by every
  selected package name. `test-crates` takes a variadic parameter, so it and
  its packages are always last.

The script selects packages as follows:

- A path under a package directory from the graph selects that package and
  every workspace package that depends on it, directly or transitively. A
  change under `crates/render-graph/` therefore tests the renderer, `mltrs`,
  and every example.
- A path under `examples/toon_link/`, `crates/convert-link/`, or `crates/gx/`
  also selects the asset gates. Dependents do not: a change to `mltrs` tests
  `toon_link` but does not run the asset gates.
- `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `justfile`, `scripts/`,
  `.cargo/`, and any path under `crates/` or `examples/` that no package
  directory owns select every package and the asset gates.
- An empty staged list (for example `git commit --amend` with nothing staged)
  selects every package.
- `.md` and `.org` files, `.gitignore`, `.github/`, and any other path select
  nothing.

The script rejects a graph that lacks `toon_link`, `convert-link`, or `gx`,
the packages that select the asset gates.

Renames check both old and new paths. Like the build checks, the hook validates
the working tree, so stage the intended changes before committing. The script
requires nightly Cargo with `-Zscript` support and `jq` on the path. Its
inline tests use a fixture graph and run with
`cargo +nightly -Zscript test --manifest-path scripts/pre-commit-checks.rs`.

## Link animations

The toon_link animation pipeline carries its own two-level checks
([link_animations.md](link_animations.md) has the full picture):

- `just toon_link link-test-animations` — asset-free: Rust parser/schema
  unit tests, CLI integration tests on synthetic raw inventories, and
  Python unittests that drive the extraction script against a fake `dtk`
  with generated RARC/J3D fixtures. Needs `uv` (it resolves the pinned
  gclib dependency) but no game assets.
- `just toon_link link-verify-animations` — real-asset gate: raw-tree and
  golden-hash checks, byte-identical parity between the Rust converter and
  the independent Python oracle over every clip, conversion repeatability,
  tamper detection, and a model-gate isolation check. Fails loudly when
  the assets or prerequisites are missing; it is never cargo-discovered.

## Snapshot tests

[insta](https://insta.rs) holds the snapshots of the generated code.

```bash
just test                              # non-interactive (CI)
just insta                             # interactive review
cargo insta test --workspace --accept  # re-run and accept every changed snapshot
```

Run `just test` after changing `crates/cli/src/build_tasks.rs`,
`crates/cli/src/roc_codegen.rs`, or `crates/cli/templates/*.askama`. Normal tests
cover Roc manifests, generated-source snapshots, schema values, CLI selection,
name/path validation, and replacement safety without invoking a Roc compiler.

`just roc-codegen-test [ROC]` is separate from `test`. `just pre-commit` runs it
when `roc` is on `PATH` and skips it with a message otherwise. It reports the
selected compiler path/version, generates fresh fixtures, checks and formats
every generated module through Roc, and evaluates imported SPIR-V bytes and
reflection values from a consumer. It also runs missing-file and false-byte
negative controls, plus a compile-failure check for identically shaped vertex
types from different shader modules. `ROC` defaults to `roc` on `PATH`; a missing or incompatible
compiler is a failure, not a skip. Generation itself never invokes Roc. Roc
shader codegen was verified locally with `/home/danknutson/.local/bin/roc`,
`release-fast-42fbc4b0`. Compatibility with the platform's recorded
`release-fast-62a50c46` build and CI source pin `40fe7ddc…` remains unverified;
this gate does not change or upgrade either platform pin.

`cargo insta accept` does nothing after `just test`. The recipe sets
`INSTA_UPDATE=no`, so `just test` writes no `.snap.new` files for it to review.
Use `cargo insta test --workspace --accept`. It re-runs the tests and writes the
snapshots in one step. Read the diffs `just test` prints before you accept.

The templates emit rustfmt-clean rust, so a snapshot is byte-identical to the
file an example commits. `generated_rust_source_is_rustfmt_clean` enforces this:
it runs `rustfmt --check --edition 2024` over the generated code. Keep the
templates matching rustfmt rather than relying on a later `cargo fmt`.

Two rules the templates carry, because rustfmt applies them and a template
cannot:

- A `mod` list and a `use` group are sorted. `super::` sorts before `crate::`.
- A single-name import list loses its braces.

Line width is decided in rust, not in the template. See
`ShaderAtlasField::init_line` and `RUSTFMT_MAX_WIDTH` in
`crates/cli/src/build_tasks.rs`.

The `*.roc.askama` templates emit `roc fmt` canonical Roc in 4-space units.
Each template renders its first line at column 0; the embedding site
re-indents continuation lines. `canonical_roc_indentation` in
`crates/cli/src/roc_codegen.rs` converts each leading group of 4 spaces to a
tab at publication. `just roc-codegen-test` enforces the result with
`roc fmt --check`.

## Extracted render-graph tests

`cargo test -p mltrs-render-graph` runs the logical/compiler regression suite,
four doctests, and `graph_dependency_boundary`, which checks the resolved
transitive dependency closure for renderer, ash, vk-mem, SDL, and shader-slang.
The deterministic fake backend exercises real preparation and execution:
extent rejection before allocation, physical image counts, keepalive drops,
partial preparation failure, current/previous addresses, dropped buffers,
upload capacity, command plans, and submission-time cursor commits.

Renderer unit tests separately prove the opaque Vulkan indirect ABI and buffer
range checks. Graph-driven upload regressions exercise the production adapter's
batch preflight helper with forged storage capacity, wrong uniform payload sizes,
unmapped/missing destinations, and incompatible upload access kinds. Invalid
batches must fail before any write, submission, or cursor commit. The renderer
directly sequences the flight wait, writes, queue submission, cursor commit, and
presentation in `draw_frame`. These CPU tests complement, rather than replace,
the real Vulkan validation sweep below.

## Render-graph API compile checks

`cargo test -p mltrs-renderer` also runs a compile harness for the public
render-graph API. It runs inside `just test` and `just pre-commit`.

- The driver is `crates/renderer/tests/render_graph_api_compile.rs`.
- Each case is one bin of the fixture crate `crates/renderer/fixtures/api_compile`.
- The harness checks each bin independently with `cargo check` against the
  real `mltrs-renderer` crate.
- Positive cases must compile. A negative case must fail at its intended
  operation. The harness requires the expected error code, the expected type
  names, and the case file name in the diagnostic. A case that fails on an
  unresolved import or a missing dependency fails the harness.
- Compute builder cases cover omitted, wrong, and duplicate parameter bindings,
  both attachment orders with push constants, and parameter blocks with unit bindings.
- Trait cases check that public graph bounds satisfy backend APIs and that
  backend traits cannot be imported through either renderer path.
- Extraction cases cover direct graph imports and facade type identity (including
  bindless-to-sampled conversion), opaque indirect construction and private
  fields, and private erased-request layout metadata. Two distinct types both
  implement `IndexedIndirectArgs`: exact backend matches compile for direct and
  nested tuple/repeat/optional graphs; mismatches fail at `prepare`'s compatibility
  bound. A wrong prepared-frame family and an external `CompatibleWith`
  implementation also fail. Expected codes and snippets must occur in diagnostics
  located in the intended case file, not merely somewhere in cargo output.
- No case constructs a `Renderer` or allocates a GPU. Every case type-checks
  a function that takes renderer handles as parameters.

The harness generates the fixture's shader bindings, checks every case against
them, and deletes the generated output again. This is the shape `alignment_tests`
uses for the CLI stub fixture, so nothing generated is committed and no justfile
recipe maintains it. The committed tree holds the slang sources and the cases,
nothing built from them.

- `shaders/source/*.slang` are the inputs. The harness writes the vendored
  `mltrs.slang` beside them, then generates `shaders/compiled/` and
  `src/generated/`, then removes all three.
- Generation uses `--import-root mltrs_renderer` instead of the default
  `mltrs`, so the cases reach the renderer API with no engine crate in between.
- The case types come from the generated modules. Nothing in the fixture
  mirrors a codegen template, so the cases cannot drift from what the generator
  emits.
- `mltrs-cli` is a dev-dependency of `mltrs-renderer` for this. It does not
  depend on the renderer, so this adds no cycle. It does mean the slang
  compiler is needed to run the renderer's tests.
- The test is `#[cfg(not(windows))]`, like every other codegen test here.

The fixture crate sits outside the workspace (root `Cargo.toml` `exclude`)
because its negative bins never compile and its `src/generated` exists only
while the test runs. The harness points `CARGO_TARGET_DIR` at the workspace
`target/`, so the checks reuse the workspace build of `mltrs-renderer`.

The fixture's `Cargo.lock` is a pruned copy of the root lock. It pins the same
dependency versions, which is what lets the shared target directory be reused;
a fresh resolve picks different `sdl3` versions and rebuilds SDL from source.
Unlike `check_crate`, the harness passes `--locked`, so drift fails loudly
rather than silently diverging. To recover: copy the root `Cargo.lock` into the
fixture, then run `cargo metadata --offline --format-version 1 --manifest-path
crates/renderer/fixtures/api_compile/Cargo.toml` to re-prune it. This maintenance
command omits `--locked`; the compile harness keeps `--locked`. Check the lock
diff for unexpected registry-version changes, then rerun the harness. Metadata
does not require the generated fixture sources to exist.

The harness runs `cargo`, which takes the build-directory lock. A cargo build
running at the same time (bacon, a second shell) makes the harness wait for
that build to finish.

`cargo fmt` does not reach the fixture, because it is outside the workspace and
its `src/lib.rs` names a module that exists only during the test. Format the
cases directly:

```bash
rustfmt --edition 2024 crates/renderer/fixtures/api_compile/cases/*/*.rs
```

The CLI stub fixture (`crates/cli/fixtures/check_crate`) compiles generated
code against stub renderer types, which is fast and needs no graphics stack.
It cannot prove that real renderer API calls are safe. This harness compiles
the same generated code against the real renderer.

## Validation sweep

`scripts/headless-sweep.sh` checks generated GPU readback and runs every example
under the lavapipe software driver, with no window and no display. It exits
nonzero on a readback failure or if any example emits Vulkan validation output.

```bash
just sweep                                         # readback + all examples (~10s each, plus builds)
just sweep sprite_batch                            # only the named examples
just sweep readback                                # only GPU readback
just sweep readback sprite_batch                   # readback + the named examples
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

### Generated GPU readback

A full `just sweep` runs the ignored `gpu_readback` renderer integration test
before the examples. Naming examples selects only those examples; add `readback`
to include the diagnostic. `just sweep readback` runs only the diagnostic,
without building examples or running fault injection. `SWEEP_SKIP` only skips
examples; `SWEEP_SELF_TEST=0` only skips fault injection. The standalone
`just sweep-self-test` runs only fault injection.

The readback test generates SPIR-V, reflection, and Rust bindings from the
fixture sources in a temporary crate, then executes it against the real renderer.
It needs Slang and the normal renderer build dependencies, seeds resolution from
the workspace lockfile, and runs the temporary crate with offline Cargo.
Successful runs remove the temporary crate; failures retain it for inspection.
The sweep saves output to `$SWEEP_LOG_DIR/gpu-readback.log` and fails on missing
prerequisites, generation/build failures, mismatched output, validation warnings
or errors, or a missing completion marker. Ordinary `just test` still skips it.

Two one-thread workgroups write records covering a nested struct, two enum
variants, vectors, a fixed array, and a nonsymmetric nonidentity matrix. A second
dispatch writes invalid enum value 99 and must produce a checked decoder error.
Validation counts include renderer destruction, independently of the log filter.
Row-major Slang matrix storage decodes into glam columns, matching the inverse
raw upload representation; explicit column assertions are separate from the
shader-computed matrix/vector product. This covers software Vulkan execution,
not hardware drivers, animation skinning, or the frame loop.

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
container. Run `just toon_link tww-assets` to extract, convert, and verify all
Wind Waker assets and make the example sweepable (see
[`link_animations.md`](link_animations.md#prerequisites) for prerequisites).
Every other example loads from tracked assets
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
