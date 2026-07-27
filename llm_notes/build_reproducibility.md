# Build & test reproducibility

Status: **implemented, 2026-07-27.** The diagnosis was written on 2026-07-26
against a clean checkout of `main` @ `3c36467` in a fresh cloud container,
while implementing [`link_rendering/phase_07.md`](link_rendering/phase_07.md).

§1 landed ahead of the rest, in `e080d72` ("address cross-machine differences
in codegen/tests"), and is kept below as the record of *why* — it is the one
item whose absence produced failing tests rather than a slow start. Everything
else landed with this note.

Companion documents: [`offscreen_testing.md`](offscreen_testing.md) — §7 here
implements the validation half of its `just headless-all` design, as a shell
sweep rather than the in-process harness that note designs;
[`tech_debt.md`](tech_debt.md) — renderer-wide cleanups, same "no owning
phase" character as this note.

---

## Why

A fresh clone of this repo could not build, and its test suite could not pass,
until a fair amount of undocumented local knowledge was applied. That cost was
paid by every new contributor and every agent session, *before* any useful work
started. Worse, two of the problems were not environmental at all — they were
latent nondeterminism in the codegen, producing spurious diffs and spurious
test failures on **any** machine that isn't the one the files were last
generated on.

The failure modes were also *misleading*: a snapshot test failing with a
pure-reordering diff reads like "your change broke codegen," and the only way
to learn otherwise is to stash everything and re-run on a pristine tree.

Ordered by value: §1 and §2 fixed real latent bugs, §3–§6 fixed
time-to-first-build, §7 unlocked a class of verification that was previously
assumed to need a GPU.

---

## 1. The generated shader atlas was filesystem-order dependent — fixed in `e080d72`

**The problem.** `write_precompiled_shaders` collected the shader sources with
an unsorted `std::fs::read_dir`. That order flowed straight into
`src/generated/shader_atlas.rs`: the `pub mod` lines, the `ShaderAtlas` struct
fields, and the `init()` body.

**Why it was survivable, and why that made it confusing.** `just shaders` runs
`cargo fmt` afterward, and rustfmt's `reorder_modules` (default on) sorts the
`pub mod` declarations — but not struct fields. So the committed file was
*half* normalized, and the half that wasn't stayed invisible until someone
regenerated on a different machine. On `main` @ `3c36467`: module list sorted,
`ShaderAtlas` fields not.

**Observed consequence.** `generated_files` and `alignment_tests` both failed
on a clean checkout on that machine (18 passed, 2 failed), with a diff in which
every line was present on both sides. Any contributor running `just shaders`
also got an unrelated `shader_atlas.rs` diff mixed into their real change.

**What shipped.** `collect_slang_file_names` sorts, and so do
`reflect_slang_module_types`' module list, the `alignment_tests` check-crate
`mod.rs` construction, and the `field_size_tripwire` mismatch list. A
`slang_file_names_are_sorted` test pins the first of those. The fix already had
precedent in the same file: `shader_branching_snapshots` sorted its `read_dir`
and was, not coincidentally, the one order-sensitive snapshot that did *not*
fail.

Nothing derives an index from atlas order — it is addressed by field name
throughout, `init()` is a named-field struct literal, and there is no
`enumerate()` over it — so sorting was a pure reordering.

---

## 2. Snapshots captured unformatted output, so they could never match the files

**The problem.** The snapshot tests generate into a temp directory and snapshot
the raw template output. `just shaders` generates into `src/generated/` and
*then* runs `cargo fmt`. Nothing formatted the temp-directory output, so the
snapshot and the committed file were formatted differently by construction:

```
snapshot:  pub use super::mvp::{MVPMatrices};      (braces retained)
on disk:   pub use super::mvp::MVPMatrices;        (braces stripped, hoisted)

snapshot:  pub fn pipeline_config(
               self,
               resources: Resources<'_>,
           ) -> IndexedPipelineConfig<'_, Vertex> {
on disk:   pub fn pipeline_config(self, resources: Resources<'_>) -> IndexedPipelineConfig<'_, Vertex> {
```

**Why it mattered.** The snapshots are the review surface for codegen changes.
They showed something nobody's compiler ever saw, a formatting-only change in
the templates showed up as snapshot churn that looked semantic, and a real
change could hide inside re-wrapping noise.

**What shipped.** `rustfmt_source` in `src/shaders/build_tasks.rs` pipes each
generated file through `rustfmt --edition 2024` inside `write_generated_file`,
so the temp-dir output and `src/generated/` go through the identical path. A
missing `rustfmt` is a hard error with an actionable message rather than a
warned-and-skipped fallback, which would reintroduce exactly the
two-different-formattings problem: it ships with every standard toolchain and
was already a de-facto dependency via the `cargo fmt` in `just shaders`. The
edition is passed explicitly, since rustfmt.toml discovery is relative to the
current directory.

`cargo fmt` stays in the `just shaders` recipe as belt-and-braces; it is now a
no-op for the generated tree.

**Churn.** Snapshots only. The committed `src/generated/` files were already
`cargo fmt`-clean, so they are byte-identical before and after — verified by
piping each through `rustfmt --edition 2024` and diffing against the file.
The `.snap` diffs are pure rustfmt output, re-derivable the same way.

---

## 3. `just build-slang` failed from a clean checkout on Linux

The unix recipe configured slang with `--preset default -DSLANG_LIB_TYPE=STATIC`
and nothing else, which fails:

```
CMake Error at FetchContent.cmake:1679 (message):
  Build step for optix_8_0 failed: 1
Call Stack:
  external/slang-rhi/cmake/FetchPackage.cmake:49 (FetchContent_MakeAvailable)
  external/slang-rhi/CMakeLists.txt:558 (FetchPackage)
```

slang-rhi's CMake unconditionally fetches the OptiX headers, which needs
network access it doesn't get behind a proxy — and which nothing here uses; we
link `slang-compiler`, `compiler-core` and `core`, never `gfx` or slang-rhi.

**What shipped.** The unix recipe now passes the flags the **Windows recipe
already passed**: `-DSLANG_ENABLE_SLANG_RHI=OFF -DSLANG_ENABLE_TESTS=OFF`. That
also settles the standing "these flags are likely removable" comment above the
Windows recipe in the other direction — on Linux they are required — so the
comment now covers both recipes and says why.

The comment also records that the static build produces no `libslang.a`, only
`libslang-compiler.a`, `libcompiler-core.a` and `libcore.a`, which is exactly
what `slang-sys/build.rs` links under its `static` feature. Time was lost
waiting for a `libslang.a` that was never going to appear; the README says so
too.

---

## 4. Undocumented system dependencies

README listed rust/cargo, just, direnv, clang, cmake. Missing, each discovered
by a build failing partway through a long compile:

| package | needed by | failure if absent |
|---|---|---|
| `libasound2-dev` | `alsa-sys` ← `rodio` | `pkg-config` fails; build script panics |
| `libvulkan-dev` | link step | `rust-lld: error: unable to find library -lvulkan` |
| `ninja-build` | slang's cmake preset | preset configure fails |

Plus, for the headless sweep (§7): `mesa-vulkan-drivers` (the lavapipe ICD) and
`vulkan-validationlayers` — without the latter the renderer bails outright with
`missing required layer: VK_LAYER_KHRONOS_validation`. No audio package is
needed; `sdf_2d`'s missing-device abort was fixed in code (§7.1).

**What shipped.** README documents both sets, and `just install-deps-debian` /
`just install-deps-headless-debian` install them. Both are explicitly
best-effort and distro-specific, so they stay a convenience rather than a
promise.

---

## 5. Environment variables needed direnv, with no non-interactive path

`.envrc` sources `.env` to export `SLANG_LIB_DIR`, `SLANG_INCLUDE_DIR` and
`SLANG_EXTERNAL_DIR`. That works in an interactive shell with direnv hooked. It
does nothing in a non-interactive one — a CI step, a `bash -c`, an agent
session — where every `cargo` invocation then died with:

```
The environment variable SLANG_INCLUDE_DIR, SLANG_DIR, or VULKAN_SDK must be set
```

**What shipped.** Both options the plan weighed, because they cover different
processes:

- **`.cargo/config.toml [env]`** sets the three paths for everything cargo
  launches, including a bare `cargo test`. `relative = true` resolves them
  against the repo root — verified, that is the directory containing `.cargo`,
  not `.cargo` itself — which is what the `$PWD` in `.env` exists for. `force`
  is left false, so a value already in the environment (direnv,
  `load-env.ps1`, `load-env.sh`) still wins.
- **`scripts/load-env.sh`**, the unix mirror of the existing `load-env.ps1`,
  for processes cargo does *not* launch. `scripts/headless-sweep.sh` sources
  it, since it runs `target/debug/examples/<name>` directly.

It parses `.env` rather than sourcing it, so `$PWD` expands to the repo root
regardless of the caller's working directory — the same thing `load-env.ps1`
does.

---

## 6. `cargo-insta` was documented but not installed by anything

`CLAUDE.md` documents `cargo insta test --accept` and `just insta`, and the
snapshot workflow depends on them, but `cargo-insta` was not a checked-in
dependency and nothing installed it. Without it the only path is hand-renaming
`.snap.new` files, which is what the P7 session had to do.

**What shipped.** `just install-tools` (`cargo install cargo-insta`), noted in
both README and CLAUDE.md next to the first mention.

---

## 7. Headless example runs — the validation sweep

[`offscreen_testing.md`](offscreen_testing.md) designs `just headless-all` — a
validation sweep under a software driver — and argues correctly that no cloud
GPU is needed. What ships here is the *shell* half of that design:
`scripts/headless-sweep.sh`, wired to `just headless-all`. None of the
in-process machinery that note describes (`HeadlessConfig`, validation
counters, virtual clock, frame capture) is implemented, and goldens remain out
of scope, per that note's own reasoning.

### 7.1 Getting a window-less example to run at all

| step | result |
|---|---|
| default | `Error: No available video device` |
| `SDL_VIDEODRIVER=dummy` | `Vulkan support ... not available in current SDL video driver (dummy)` |
| `SDL_VIDEODRIVER=offscreen` | past SDL; `Installed Vulkan doesn't implement the VK_KHR_surface extension` |
| + `mesa-vulkan-drivers` (lavapipe ICD) | `missing required layer: VK_LAYER_KHRONOS_validation` |
| + `vulkan-validationlayers` | **runs clean** |

One more dependency, found by the sweep rather than by reading: **`sdf_2d`
initializes audio** (rodio/cpal) and aborted on a machine with no sound card —
`Failed to get the config for the given device`. It is the only example that
does.

**Fixed here.** The audio there is playback only: the visuals are driven by
`beats` (a JSON of timestamps) plus elapsed time, and the stream was held in a
field purely to keep it alive (`#[expect(unused)]`). So a missing output device
is now non-fatal — `start_audio()` returns `Result`, `setup` degrades to
`None`, and the example prints `sdf_2d: no audio (...); rendering silently` and
renders normally.

That message uses `eprintln!` rather than `log::warn!` on purpose: per 7.3.2, a
`warn!` would be invisible with `RUST_LOG` unset, which is exactly the
configuration of the machines that take this path.

### 7.2 Does it actually catch errors?

Faults were injected on purpose to confirm the sweep notices them — the point
being that "when in the lifecycle does the error happen" is the axis a
timeout-based sweep is most likely to be blind to. Measured on this container,
one fault at a time:

| fault | when it fires | result | how it was caught |
|---|---|---|---|
| zero-size buffer, first `create_memory_buffer` | once, during `setup` | `FAIL(exit 1)`, `Error: Initialization of an object has failed` | exit code |
| the real `sprite_batch` MSAA bug (§7.5), fix reverted | every frame | `FAIL(validation, 95 lines)`, `VUID-VkRenderingAttachmentInfo-imageView-06861` | log grep |
| skipped `destroy_image_view` in `Drop` | teardown only | `FAIL(validation, 1 lines)`, `VUID-vkDestroyDevice-device-05137` | log grep |

Two things worth noting. **A single error, occurring once at teardown, is not
lost** — one line in an otherwise empty log still fails the sweep. And the
zero-size buffer never reached the log here: VMA returned
`ERROR_INITIALIZATION_FAILED`, the example bailed, and the *exit-code* branch
caught it. Both branches carry weight; a sweep that only grepped the log would
have passed a run that never drew a frame. Uninjected examples in the same run
report `ok`, so the sweep blames the right example.

### 7.3 Five ways the sweep can silently pass a broken example

These are why the sweep must **own** its environment rather than inherit it.
Each was confirmed by running an injected fault and watching the detection go
to zero.

1. **An inherited `RUST_LOG` hides everything.** With the teardown fault
   active, `RUST_LOG=off` → **0** validation lines and a green `ok` on a
   visibly broken example. A `RUST_LOG` naming some other module does the same
   thing by the same mechanism, since the filter is applied per target. Note
   `.env` sets `RUST_LOG`, so the sweep would otherwise inherit whatever a
   developer last put there — `scripts/load-env.sh` is sourced *before* the
   sweep sets its own.
2. **`RUST_LOG` unset drops WARNING-severity validation.** Measured with a
   probe logging at all three levels: unset → only the `error!` line;
   `RUST_LOG=warn` → `error!` and `warn!` both. The debug callback routes
   `Severity::WARNING → warn!` (`renderer/debug.rs:35`), so every
   warning-severity and PERFORMANCE-type message is invisible by default.
   **The sweep sets `RUST_LOG=warn` explicitly.**
3. **`--release` validates nothing.** `ENABLE_VALIDATION` is
   `cfg!(debug_assertions)` (`renderer.rs:61`), so a release build never
   installs the layer or the callback and passes everything. No `--release` in
   the script.
4. **`SIGKILL`, or disabling SDL's signal handlers, hides teardown errors.**
   See 7.4 — this one also corrects a claim in the existing docs.
5. **`timeout N cargo run` times the *compile* as well as the run**, and this
   one makes the entire sweep vacuous rather than hiding a single example.
   Measured: after one `touch src/renderer.rs`,
   `timeout -s TERM 5 cargo run --example basic_triangle` expired during
   compilation, exit **124** — indistinguishable from "the example ran its
   whole window" — with a 201-byte log containing cargo's `Compiling` line and
   zero validation lines. Every example would report `ok` and the sweep would
   exit 0. The script therefore runs `cargo build --examples` up front, fails
   loudly if that build fails, and then times
   `target/debug/examples/<name>` directly, so the budget covers execution
   only.

The general shape of 1–5: **an empty log and a green sweep look identical to a
clean pass.** Every one of these was a case where the signal never reached the
log, not a case where detection misread it.

Also: **the exit code is not a signal.** The debug callback returns
`vk::FALSE`, so nothing propagates — a run with hundreds of validation errors
still exits 124 like a clean one. Detection greps the log. (Exit codes are
still checked for the *separate* case of a crash or an early bail; `toon_link`
correctly exits 1 with its "run `just convert-link`" message, which the sweep
reports distinctly from a validation failure.)

### 7.4 Correction: `timeout` does **not** skip `Drop`

`offscreen_testing.md` stated that "`timeout` SIGKILLs the process, so
`drain_gpu()` and `Drop for Renderer` never run", and
[`link_rendering/phase_07.md`](link_rendering/phase_07.md)'s test plan repeated
it ("`timeout`'s SIGTERM skips `Drop`, so the leak check needs a manual
close"). Both are wrong, and it matters because it is the stated reason
teardown leaks supposedly can't be automated.

Measured, with the leaked-image-view fault active:

| invocation | exit | teardown VUID seen |
|---|---|---|
| `timeout -s TERM` (the default) | 124 | **yes** |
| `timeout -s KILL` | 137 | no |
| `timeout -s TERM` + `SDL_NO_SIGNAL_HANDLERS=1` | 124 | no |

`timeout` sends **SIGTERM**, not SIGKILL, and SDL installs a handler that
converts SIGTERM into an `SDL_QUIT` event — so the event loop exits normally,
`Drop` runs, and `vkDestroyDevice` reports leaked objects. That makes the
`tech_debt.md` §1 leak class **automatable today**, with no clean-exit
machinery to build.

The dependency is real but fragile: it holds only while SDL's signal handlers
are left enabled. The sweep therefore `unset`s `SDL_NO_SIGNAL_HANDLERS` and
says why, and the two documents above are corrected.

### 7.5 The first full sweep found a real bug

Running the sweep over every example on a clean tree turned up **`sprite_batch`
emitting validation errors** — 55 lines in the original session, 95 here, the
count being a function of how many frames the run gets through:

```
VUID-VkRenderingAttachmentInfo-imageView-06861
vkCmdBeginRendering(): pRenderingInfo->pColorAttachments[0].imageView must not
have a VK_SAMPLE_COUNT_1_BIT when resolveMode is VK_RESOLVE_MODE_AVERAGE_BIT
```

**Root cause.** `record_command_buffer` set
`.resolve_mode(vk::ResolveModeFlags::AVERAGE)` **unconditionally**, but
`get_max_usable_sample_count` falls back to `TYPE_1` when the requested count
isn't in `framebuffer_color_sample_counts & framebuffer_depth_sample_counts`.
One sample plus a resolve is a spec violation.

`sprite_batch` is the only example that requests a non-default level
(`MaxMSAASamples::Max2`), and `Max2`'s descending option list is `[TYPE_2]`
alone — so if 2× isn't supported there is no fallback but 1×. lavapipe reports
`SAMPLE_COUNT_1_BIT | SAMPLE_COUNT_4_BIT` and **not** 2× (measured directly in
`offscreen_testing.md` §9, and confirmed here by the fallback firing at all).
Other examples use the default and land on 4×, so they never hit it.

This is a **genuine latent renderer bug in the MSAA fallback path**, not a
lavapipe quirk: it fires on any device that doesn't support the requested
sample count. It was invisible only because the dev GPU supports 2×.

**Fixed here.** With one sample there is nothing to resolve, so
`record_command_buffer` renders straight into `resolve_image_views[flight_slot]`
— which the upscale blit reads either way — with `store_op: STORE` and no
resolve attachment at all. The resolve image is already transitioned to
`COLOR_ATTACHMENT_OPTIMAL` with `COLOR_ATTACHMENT_WRITE` by the existing
`resolve_barrier`, so the single-sample path needs no new synchronization. The
multisampled path is byte-for-byte what it was.

Worth noting for review: **on a GPU that supports 2× MSAA this changes
nothing.** `sprite_batch` keeps taking the multisampled branch there, so the
new branch is dead code on the dev machine and only activates where the
requested count is unavailable. Both branches are exercised by the sweep in
this container — `sprite_batch` at 1× and every other example at 4×.

Caveat on the verification: this confirms the path is **spec-clean**, not that
the pixels are right, since the sweep has no golden images. The reasoning for
correctness is that the blit's source image is now written directly with
`STORE` instead of receiving an undefined resolve, which is strictly
better-defined than what it replaced.

It is also a fair illustration of the one caveat: **a software driver's limits
differ from a real GPU's.** Here that difference surfaced a real bug in an
untested path, which is the good outcome; the same mechanism could equally
produce a `FAIL` that doesn't reproduce on the dev machine. Triage a sweep
failure by reading the VUID before assuming either.

### 7.6 What is left

- Decide whether the sweep belongs in CI. It needs no GPU, so the only real
  cost is build time.
- `toon_link` is skipped via a `SWEEP_SKIP` list: it cannot run anywhere
  without `assets/link/converted`, which is gitignored and disc-image-derived
  ([`link_rendering/follow_up.md`](link_rendering/follow_up.md)), and it bails
  with a helpful message rather than crashing. `SWEEP_SKIP=` sweeps it anyway
  on a machine where `just convert-link` has been run.
- The in-process harness `offscreen_testing.md` designs — counters, a real exit
  code, deterministic frame counts, capture — is still unbuilt, and is what
  golden images would need.

**Out of scope:** golden images. `offscreen_testing.md` is explicit that
lavapipe does not solve comparison against frames blessed on real hardware, and
nothing here changes that.

---

## Scope boundaries

- **No renderer behavior changes except §7.5**, which only affects devices that
  fall back to one sample. §2 changes the *formatting* of generated code, never
  its meaning.
- **No converter changes**, so `scripts/link_converted.sha256` stays untouched —
  the same gate P6/P7 used.
- **`toon_link` stays un-runnable in CI**: its assets are machine-local and
  disc-image-derived.
