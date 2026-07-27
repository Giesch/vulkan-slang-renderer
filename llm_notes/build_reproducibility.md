# Build & test reproducibility

Status, per section, re-checked against `main` @ `caaf66a`:

| § | topic | state |
|---|---|---|
| 1 | shader-atlas ordering nondeterminism | ✅ **done on `main`** in `e080d72`, independently of this branch |
| 2 | snapshots capture pre-rustfmt output | ⬜ open — re-verified unchanged |
| 3 | `just build-slang` fails on Linux | ⬜ open |
| 4 | undocumented system packages | ⬜ open |
| 5 | env vars need direnv | ⬜ open |
| 6 | `cargo-insta` documented, not installed | ⬜ open |
| 7 | headless sweep | ✅ **implemented on this branch**, green, and adversarially tested |

Every problem below was hit while implementing
[`link_rendering/phase_07.md`](link_rendering/phase_07.md) in a fresh cloud
container, and every diagnosis was verified on that machine — originally
against `main` @ `3c36467`, and re-checked after merging `caaf66a`.

Two of the things this note reported have since been fixed on `main` by other
work (§1's ordering fix, and the `sprite_batch` MSAA bug §7.5 found), in both
cases more thoroughly than proposed here. Their sections say so and the
superseded code was dropped from this branch rather than left to conflict.

Companion documents: [`offscreen_testing.md`](offscreen_testing.md) — its
`just headless-all` design is what §7 finally makes reachable;
[`tech_debt.md`](tech_debt.md) — renderer-wide cleanups, same "no owning
phase" character as this note, and now home to the codegen-namespace hazard
that §1 surfaced.

---

## Why

A fresh clone of this repo cannot build, and its test suite cannot pass, until
a fair amount of undocumented local knowledge is applied. That cost is paid by
every new contributor and every agent session, and it is paid *before* any
useful work starts. Worse, two of the problems are not environmental at all —
they are latent nondeterminism in the codegen that produces spurious diffs and
spurious test failures on **any** machine that isn't the one the files were
last generated on.

The P7 session spent a large fraction of its time on this rather than on the
phase. The failure modes were also *misleading*: a snapshot test failing with a
pure-reordering diff reads like "your change broke codegen," and the only way
to learn otherwise is to stash everything and re-run on a pristine tree.

Ordered by value: §1 and §2 fix real latent bugs, §3–§6 fix
time-to-first-build, §7 unlocks a class of verification that was previously
assumed to need a GPU.

**§7 has since been prototyped, adversarially tested, and is green** — faults
were injected on purpose to confirm the sweep notices them, which turned up
five ways it can silently pass a broken example, a correction to what the
existing docs say about `timeout` and `Drop`, and one real pre-existing
renderer bug. Of the two examples the sweep flagged, the `sdf_2d` audio abort
(§7.1) is fixed on this branch, and the `sprite_batch` MSAA fallback (§7.5) was
fixed on `main` in `b260973`. Either way the full sweep now exits 0.
§2–§6 remain plan-only.

---

## 1. ~~The generated shader atlas is filesystem-order dependent~~ — DONE

**Landed on `main` in `e080d72`** ("address cross-machine differences in
codegen/tests"), independently of this branch, and it went further than this
section asked: the two copy-pasted walks became one sorted
`collect_slang_file_names` helper, three further unsorted iterations in the same
file were found and sorted, and a `slang_file_names_are_sorted` regression test
guards it — with a non-empty assertion so it can't pass vacuously on an empty
directory. Write-up in `link_rendering/follow_up.md` §5b.

**Verified here after merging `main`:** the full test suite is now **23 passed,
0 failed** on this container. Before the merge it was 18/2, failing exactly the
two atlas snapshots described below. That is the cross-machine reproduction
this section was written from, now closed.

One related item was promoted rather than fixed: the `reflect_slang_module_types`
row in the table below (same-named structs in two shared modules resolving by
last-write-wins) is now `tech_debt.md` §4 as a **silent-wrong-output** hazard.
Sorting made it deterministic; it is still a footgun, just a predictable one.

The original diagnosis is kept below, since §2 builds on it.

**The problem.** `write_precompiled_shaders` collected the shader sources with
an unsorted directory read:

- `src/shaders/build_tasks.rs:28` — `slang_file_names`
- `src/shaders/build_tasks.rs:42` — `compute_slang_file_names`

`std::fs::read_dir` returns entries in whatever order the filesystem hands
back, which differs between machines and even between two states of the same
directory. That order flows straight into `src/generated/shader_atlas.rs`:
the `pub mod` lines, the `ShaderAtlas` struct fields, and the `init()` body.

**Why it has been survivable so far, and why that made it confusing.**
`just shaders` runs `cargo fmt` after generating, and rustfmt's
`reorder_modules` (default on) sorts the `pub mod` declarations. So the
*on-disk* file's module list is alphabetical and looks stable. rustfmt does
**not** reorder struct fields, so the `ShaderAtlas` struct body keeps the raw
filesystem order. Verified on committed `main`:

```
pub mod list in src/generated/shader_atlas.rs   → sorted
ShaderAtlas struct fields in the same file      → NOT sorted
```

So the committed file is *half* normalized, and the half that isn't is
invisible until someone regenerates on a different machine.

**Observed consequences.**

- `shaders::build_tasks::tests::generated_files` and
  `shaders::build_tasks::tests::alignment_tests` **both fail on a clean
  checkout of `main`** on this machine (18 passed, 2 failed), with a diff in
  which every line is present on both sides — pure reordering. Confirmed by
  stashing every unrelated change and re-running on a pristine tree.
- Any contributor running `just shaders` gets an unrelated
  `src/generated/shader_atlas.rs` diff mixed into their real change, which
  they must either notice and revert, or commit as noise.

**The fix.** Sort by file name before use. This is not a new idea in this
file: `shader_branching_snapshots` (build_tasks.rs:2060) already does exactly
this (`entries.sort_by_key(|e| e.file_name())`) and is, not coincidentally,
the one order-sensitive snapshot that does *not* fail here.

Sort all four unsorted sites:

| line | what | why it matters |
|---|---|---|
| 28 | `slang_file_names` | **the bug** — drives atlas order |
| 42 | `compute_slang_file_names` | same, for the compute half |
| 1326 | `reflect_slang_module_types` | if two shared modules declared the same type name, the `HashMap` winner is order-dependent |
| 1748 | `field_size_tripwire` mismatches | only affects the order of a failure message, but it's a test assertion |

Also sort the `mod.rs` construction in the `alignment_tests` check-crate helper
(build_tasks.rs:1555, :1568) so that the temp crate it compiles is itself
reproducible.

**One-time churn expected, and taken.** Sorting changed the committed order, so
the two atlas snapshots and `src/generated/shader_atlas.rs` were regenerated
once in `e080d72` — and the diffs were checked to be pure line reorderings by
comparing sorted added/removed line sets rather than by eye, which is the right
way to review that particular change.

---

## 2. Snapshots capture unformatted output, so they can never match the files

**Still open after `e080d72`** — re-verified against current `main`, output
below is unchanged. §1's fix made the *order* deterministic; it did not touch
*formatting*, and the two are independent. `follow_up.md` §5b now notes in
passing that the snapshots are taken pre-rustfmt and that this is what let the
scrambled `init()` body hide behind a rustfmt-tidied module list — so the
mechanism is already documented on `main`; what is missing is the fix.

**The problem.** The snapshot tests generate into a temp directory and snapshot
the raw template output. `just shaders` generates into `src/generated/` and
then runs `cargo fmt` over it. Nothing formats the temp-directory output, so
the snapshot and the committed file are formatted differently by construction.

Verified — diffing the committed `toon_link.rs` snapshot body against the
committed `src/generated/shader_atlas/toon_link.rs`:

```
snapshot:  pub use super::mvp::{MVPMatrices};      (line 18, braces retained)
on disk:   pub use super::mvp::MVPMatrices;        (line 12, braces stripped, hoisted)

snapshot:  pub fn pipeline_config(
               self,
               resources: Resources<'_>,
           ) -> IndexedPipelineConfig<'_, Vertex> {
on disk:   pub fn pipeline_config(self, resources: Resources<'_>) -> IndexedPipelineConfig<'_, Vertex> {
```

**Why it matters.** The snapshots are the review surface for codegen changes —
they are what a reviewer reads to see what the generator now emits. Today they
show something nobody's compiler ever sees. It also means a formatting-only
change in the templates shows up as snapshot churn that looks semantic, and a
real change can hide inside re-wrapping noise.

**The fix.** Format at generation time, inside `write_precompiled_shaders`,
before writing each `.rs` file — so the temp-dir output and `src/generated/`
go through the identical path and the snapshots match the committed files
byte for byte. Shell out to `rustfmt --edition 2024` (matching the repo's
`rustfmt.toml`, which sets exactly that and nothing else), or pipe the source
through it on stdin.

Keep `cargo fmt` in the `just shaders` recipe as belt-and-braces; it becomes a
no-op for the generated tree.

**Decide during implementation:** whether a missing `rustfmt` should be a hard
error or a warned-and-skipped fallback. Recommendation is **hard error** —
rustfmt ships with every standard toolchain, and a silent fallback reintroduces
exactly the two-different-formattings problem this section exists to remove.

**Gate.** For every generated file, the snapshot body is byte-identical to the
corresponding file under `src/generated/`; `cargo fmt --check` is clean
immediately after `cargo run --bin prepare_shaders`, with no separate format
step.

---

## 3. `just build-slang` fails from a clean checkout on Linux

**The problem.** The unix recipe (justfile:113) is:

```sh
cd slang && cmake --preset default -DSLANG_LIB_TYPE=STATIC && cmake --build --preset release
```

Configuring with just that fails:

```
CMake Error at FetchContent.cmake:1679 (message):
  Build step for optix_8_0 failed: 1
Call Stack:
  external/slang-rhi/cmake/FetchPackage.cmake:49 (FetchContent_MakeAvailable)
  external/slang-rhi/CMakeLists.txt:558 (FetchPackage)
```

slang-rhi's CMake unconditionally fetches the OptiX headers, which needs
network access it does not get behind a proxy (and which nothing in this repo
uses — we link `slang-compiler`, `compiler-core` and `core`, never `gfx` or
slang-rhi).

**The fix.** Pass the flags the **Windows recipe already passes**
(justfile:127): `-DSLANG_ENABLE_SLANG_RHI=OFF -DSLANG_ENABLE_TESTS=OFF`.
Verified working on this machine; the resulting build produces everything
`shader-slang-sys` links.

This also settles the standing question in the comment above the Windows recipe
("these flags are likely removable; keeping them until that's verified on a
Windows machine") in the other direction: on Linux they are **required**, so
they should be in both recipes rather than removed from either.

**Note for whoever implements this:** the static build produces no
`libslang.a`. It produces `libslang-compiler.a`, `libcompiler-core.a` and
`libcore.a`, which is exactly what `slang-sys/build.rs` links under its
`static` feature. Time was lost waiting for a `libslang.a` that was never going
to appear — worth a sentence in the README.

**Gate.** From a clean clone: `just init-submodules && just build-slang &&
just shaders` succeeds with no network access beyond the git fetch.

---

## 4. Undocumented system dependencies

**The problem.** README lists rust/cargo, just, direnv, clang, cmake. Missing,
each discovered by a build failing partway through a long compile:

| package | needed by | failure if absent |
|---|---|---|
| `libasound2-dev` | `alsa-sys` ← `rodio` | `pkg-config` fails; build script panics |
| `libvulkan-dev` | link step | `rust-lld: error: unable to find library -lvulkan` |
| `ninja-build` | slang's cmake preset | preset configure fails |

Plus, for headless verification (§7): `mesa-vulkan-drivers` (the lavapipe ICD),
`vulkan-validationlayers` (without it the renderer bails outright —
`missing required layer: VK_LAYER_KHRONOS_validation`), and optionally
`vulkan-tools` for `vulkaninfo` when triaging a driver-limit question. No audio
package is needed: `sdf_2d`'s missing-device abort was fixed in code (§7.1).

**The fix.** Document them in README's setup section, split into "to build" and
"to run headless". Add a `just install-deps-debian` (or a
`scripts/install-deps.sh`) that installs the set on Debian/Ubuntu — explicitly
best-effort and distro-specific, so it stays a convenience rather than a
promise.

**Gate.** A container with only rust + git + the documented packages gets
through `just build-slang && just shaders && just test`.

---

## 5. Environment variables need direnv, with no non-interactive path

**The problem.** `.envrc` sources `.env` to export `SLANG_LIB_DIR`,
`SLANG_INCLUDE_DIR` and `SLANG_EXTERNAL_DIR`. That works in an interactive
shell with direnv hooked. It does nothing in a non-interactive shell — a CI
step, a `bash -c`, an agent session — where every `cargo` invocation then dies
with:

```
The environment variable SLANG_INCLUDE_DIR, SLANG_DIR, or VULKAN_SDK must be set
```

The Windows recipes already solve this by sourcing explicitly
(`. ./scripts/load-env.ps1`). Unix has no equivalent.

**The fix.** Add `scripts/load-env.sh` (the unix mirror of the existing
`load-env.ps1`) and have it be the documented way in. Options for wiring it in,
to decide during implementation:

1. **`set -a; . ./scripts/load-env.sh; set +a` inside the unix recipes** that
   shell out to cargo — mirrors Windows exactly, no new dependency, but touches
   several recipes.
2. **A `.cargo/config.toml` `[env]` block** — cargo would set the vars itself
   for every invocation including bare `cargo test`, which is strictly better
   than recipe-level plumbing. Needs checking whether the `$PWD` expansion the
   `.env` comment specifically calls out can be expressed there; if not, the
   paths would have to be relative, which `[env]` supports via
   `relative = true`.

Recommendation: try (2) first, since it fixes bare `cargo` too, and fall back
to (1).

**Gate.** `bash -c 'cargo test'` in a fresh shell, with no direnv, works from a
clean clone.

---

## 6. `cargo-insta` is documented but not present

`CLAUDE.md` documents `cargo insta test --accept` and `just insta`, and the
snapshot workflow depends on them, but `cargo-insta` is not a checked-in
dependency and is not installed by anything. Without it the only path is
hand-renaming `.snap.new` files, which is what the P7 session had to do.

**The fix.** Either add a `just install-tools` recipe
(`cargo install cargo-insta`), or note the install in README/CLAUDE.md next to
the first mention. Low effort; mostly a documentation accuracy fix.

---

## 7. Headless example runs — **implemented and proven, including its failure modes**

[`offscreen_testing.md`](offscreen_testing.md) designs `just headless-all` (a
validation sweep under a software driver) and argues correctly that no cloud
GPU is needed. It is marked "design, not yet implemented." The driver stack it
needs now works, the sweep has been prototyped
(`scripts/headless-sweep.sh`), and — the point of this section — it has been
**tested by deliberately breaking things and checking that it notices**.

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

**Fixed in this PR.** The audio there is playback only: the visuals are driven
by `beats` (a JSON of timestamps) plus elapsed time, and the stream is held in
a field purely to keep it alive (`#[expect(unused)]`). So a missing output
device is now non-fatal — `start_audio()` returns `Result`, `setup` degrades to
`None`, and the example prints `sdf_2d: no audio (...); rendering silently` and
renders normally. Verified with no `~/.asoundrc` and no sound card present.

That message uses `eprintln!` rather than `log::warn!` on purpose: per 7.3.2, a
`warn!` would be invisible with `RUST_LOG` unset, which is exactly the
configuration of the machines that take this path.

### 7.2 Does it actually catch errors? Yes — at all three points in the lifecycle

Three faults were injected behind an `INJECT_FAULT` env var, chosen to land at
different times, since "when does the error happen" is the axis a
timeout-based sweep is most likely to be blind to. Injected into two of four
examples per run, to check the sweep blames the right ones.

| injected fault | VUID | when it fires | caught? |
|---|---|---|---|
| zero-size buffer at device init | `VUID-VkBufferCreateInfo-size-00912` | once, before frame 1 | ✅ 1 line |
| viewport width `1e9` in command recording | `VUID-VkViewport-width-01771` | every frame | ✅ ~478 lines |
| skipped `destroy_image_view` in `Drop` | `VUID-vkDestroyDevice-device-05137` | teardown only | ✅ 1 line |

In every run the two uninjected examples reported `ok`. No false positives, no
false negatives, and a **single** error occurring once before the first frame
is not lost in the noise.

### 7.3 Five ways the sweep silently passes a broken example

These are the reason the sweep must **own** its environment rather than
inherit it. Each was confirmed by running an injected fault and watching the
detection go to zero.

1. **An inherited `RUST_LOG` that names another module hides everything.**
   With the viewport fault active: `RUST_LOG=my_other_crate=debug` → **0**
   validation lines. `RUST_LOG=off` → **0**. The example is visibly broken and
   the sweep says `ok`. Note `.env` sets `RUST_LOG`, so with direnv active the
   sweep would inherit whatever a developer last put there.
2. **`RUST_LOG` unset drops WARNING-severity validation.** A probe logging at
   all three levels with `RUST_LOG` unset emits only `PROBE_ERROR_LEVEL` —
   env_logger's default keeps `error!` and nothing else. The debug callback
   routes `Severity::WARNING → warn!` (`renderer/debug.rs:35`), so every
   warning-severity and PERFORMANCE-type message is invisible by default.
   `RUST_LOG=warn` restores them (verified: error + warn both appear).
   **The sweep must set `RUST_LOG` explicitly, at `warn` or lower.**
3. **`--release` validates nothing.** `ENABLE_VALIDATION` is
   `cfg!(debug_assertions)` (`renderer.rs:61`). A release sweep with the
   viewport fault active reports **0** validation lines and a clean pass.
4. **`SIGKILL`, or disabling SDL's signal handlers, hides teardown errors.**
   See 7.4 — this one also corrects a claim in the existing docs.
5. **`timeout N cargo run` times the *compile* as well as the run**, and this
   one makes the entire sweep vacuous rather than hiding a single example. On a
   cold build the timeout expires during compilation; cargo is killed with exit
   **124** — indistinguishable from "the example ran its whole window" — and
   the log is empty. Every example reports `ok` and the sweep exits 0.
   Hit for real while verifying the fixes below: one `touch src/renderer.rs`
   before a sweep was enough, and 16/16 reported `ok` with 16 empty logs.
   The script therefore runs `cargo build --examples` up front, fails loudly if
   that build fails, and then times `target/debug/examples/<name>` directly so
   the budget covers execution only.

   Note the general shape of 1–5: **an empty log and a green sweep look
   identical to a clean pass.** Every one of these was a case where the signal
   never reached the log, not a case where detection misread it.

Also: **the exit code is not a signal.** Every run above — clean, one error,
478 errors — exited **124**. The debug callback returns `vk::FALSE`, so nothing
propagates. Detection must grep the log. (Exit codes are still worth checking
for the *separate* case of a crash or an early bail; `toon_link` correctly
exits 1 with its "run `just convert-link`" message, which the sweep reports
distinctly from a validation failure.)

### 7.4 Correction: `timeout` does **not** skip `Drop`

`offscreen_testing.md` states that "`timeout` SIGKILLs the process, so
`drain_gpu()` and `Drop for Renderer` never run", and
[`link_rendering/phase_07.md`](link_rendering/phase_07.md)'s test plan repeats
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
are left enabled. The sweep should `unset SDL_NO_SIGNAL_HANDLERS` and say why,
and the two docs above should be corrected.

### 7.5 The first full sweep found a real bug

Running the sweep over all 16 examples on a clean tree: 14 `ok`, `toon_link`
exits 1 on its missing assets (expected), and **`sprite_batch` emits 55
validation errors** —

```
VUID-VkRenderingAttachmentInfo-imageView-06861
vkCmdBeginRendering(): pRenderingInfo->pColorAttachments[0].imageView must not
have a VK_SAMPLE_COUNT_1_BIT when resolveMode is VK_RESOLVE_MODE_AVERAGE_BIT
```

Not an artifact of the injection work — reproduced after `git checkout` of the
injected file, with no `INJECT_FAULT` code present in the tree.

**Root cause.** `record_command_buffer` sets
`.resolve_mode(vk::ResolveModeFlags::AVERAGE)` **unconditionally**
(`renderer.rs:1781`), but `get_max_usable_sample_count` (`renderer.rs:5011`)
falls back to `TYPE_1` when the requested count isn't in
`framebuffer_color_sample_counts & framebuffer_depth_sample_counts`. One sample
plus a resolve is a spec violation.

`sprite_batch` is the only example that requests a non-default level
(`MaxMSAASamples::Max2`), and `Max2`'s descending option list is `[TYPE_2]`
alone — so if 2× isn't supported there is no fallback but 1×. lavapipe reports
`SAMPLE_COUNT_1_BIT | SAMPLE_COUNT_4_BIT` and **not** 2×, which is exactly the
case. Other examples use the default and land on 4×, so they never hit it.

This is a **genuine latent renderer bug in the MSAA fallback path**, not a
lavapipe quirk: it fires on any device that doesn't support the requested
sample count. It has simply been invisible because the dev GPU supports 2×.

**Fixed on `main` in `b260973`** (PR #8, "Add `MaxMSAASamples::Off` and use it
in the sprite_batch example"), and more thoroughly than the version this branch
originally carried. Both make the attachment conditional — render straight into
`resolve_image_views[flight_slot]` with `store_op: STORE` and no resolve when
there is one sample — but `main` also turns the multisampled color image into
an `Option<MsaaColorImage>` that **isn't allocated at all** when MSAA is off,
which this branch had left as allocated-but-unused and noted as out of scope.
`main`'s version supersedes it, so the `renderer.rs` change was dropped from
this branch on merge.

Checked while dropping it, because it is the part that could quietly regress:
`create_color_image` returns `None` based on the **resolved** `msaa_samples`
(`renderer.rs:5038`), which is `get_max_usable_sample_count`'s output including
its `TYPE_1` fallback — not on the requested `MaxMSAASamples` enum. So the
original root cause (device lacks the requested count → fallback → invalid
resolve) is genuinely fixed, not merely sidestepped by switching `sprite_batch`
to `Off`. Re-verified: the full sweep is green on this container with this
branch carrying no renderer change at all.

Caveat on the verification, unchanged: the sweep confirms the path is
**spec-clean**, not that the pixels are right, since there are no golden images
(§7.6).

It is also a fair illustration of the one caveat: **a software driver's limits
differ from a real GPU's.** Here that difference surfaced a real bug in an
untested path, which is the good outcome; the same mechanism could equally
produce a `FAIL` that doesn't reproduce on the dev machine. Triage a sweep
failure by reading the VUID before assuming either.

### 7.6 What to implement

`scripts/headless-sweep.sh` is a working prototype encoding all five controls
from 7.3, each with a comment saying why it is there. **The full sweep is
green**: 15 `ok`, 1 skip, exit 0.

The skip is `toon_link`, via a `SWEEP_SKIP` list. It cannot run anywhere
without `assets/link/converted`, which is gitignored and disc-image-derived
(`link_rendering/follow_up.md`), and it bails with a helpful message rather
than crashing. `SWEEP_SKIP=` sweeps it anyway on a machine where
`just convert-link` has been run.

Remaining work:

- wire the script to `just headless-all`;
- document the container packages from §4 (and drop the `~/.asoundrc` step,
  now unnecessary — see 7.1);
- correct the `timeout`/`Drop` claim in `offscreen_testing.md` and
  `link_rendering/phase_07.md` (7.4);
- decide whether the sweep belongs in CI. It needs no GPU, so the only real
  cost is build time.

Re-validated after all of the above: with the viewport fault reintroduced, the
sweep still fails both examples it is pointed at. Worth repeating whenever the
script changes — a sweep that has quietly stopped detecting anything is
indistinguishable from a healthy one.

**Out of scope:** golden images. `offscreen_testing.md` is explicit that
lavapipe does not solve comparison against frames blessed on real hardware, and
nothing here changes that.

**Gate.** `just headless-all` exits 0 on a clean tree and nonzero when any of
the three faults from 7.2 is reintroduced.

---

## Scope boundaries

- **No renderer behavior changes.** §1 and §2 change the *order and formatting*
  of generated code, never its meaning; the one-time snapshot churn should be
  reviewable as pure reordering plus rustfmt output.
- **No converter changes**, so `scripts/link_converted.sha256` must stay
  untouched — same gate P6/P7 used.
- **`toon_link` stays un-runnable in CI** regardless of §7: its assets are
  machine-local and disc-image-derived. §7 makes the *other* examples sweepable;
  the toon_link line still only means something where `just convert-link` has
  run. (`link_rendering/follow_up.md`.)
- **Golden-image testing stays out**, per §7.

## Suggested order

Originally: §1 and §2 together, since they touch the same function and share
one snapshot regeneration. §1 has since landed on its own, so that pairing is
moot — §2 now costs its own regeneration pass, which is the one thing the
pairing was meant to avoid. Not a problem, just no longer free.

Remaining: **§2** on its own (one deliberate snapshot regeneration, reviewable
by piping the old snapshots through `rustfmt` and diffing), then **§3+§4+§5**
as a "clean clone builds" unit, then **§6**. §7 is done on this branch.

## Risks

1. **The §2 snapshot regeneration is large and must be reviewed as noise.**
   That is exactly the situation where a real change hides. Mitigation: verify
   the diff is *mechanically* explainable — pure rustfmt output, re-derivable
   by piping the old snapshots through `rustfmt --edition 2024` and diffing.
   §1 already did the equivalent for its own churn (sorted added/removed line
   sets, not eyeballing), which is the pattern to copy.
2. **Formatting at generation time makes `prepare_shaders` depend on an
   external binary.** Mitigation: hard error with an actionable message; it is
   already a de-facto dependency via the `cargo fmt` in `just shaders`.
3. **`.cargo/config.toml [env]` may not express `$PWD`.** Mitigation is stated
   in §5 — fall back to recipe-level sourcing, which is known to work because
   Windows already does it.
4. ~~**Sorting could change behavior if anything depended on atlas order.**~~
   Settled: nothing derives an index from it — the struct is addressed by field
   name — and `e080d72` has since shipped the sort with the test suite green.
