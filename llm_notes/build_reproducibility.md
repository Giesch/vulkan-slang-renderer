# Build & test reproducibility

Status: **plan, 2026-07-26. Not yet implemented.** Every problem below was hit
while implementing [`link_rendering/phase_07.md`](link_rendering/phase_07.md)
in a fresh cloud container, and every diagnosis here was verified on this
machine against `main` @ `3c36467`.

Companion documents: [`offscreen_testing.md`](offscreen_testing.md) — its
`just headless-all` design is what §7 finally makes reachable;
[`tech_debt.md`](tech_debt.md) — renderer-wide cleanups, same "no owning
phase" character as this note.

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

---

## 1. The generated shader atlas is filesystem-order dependent

**The problem.** `write_precompiled_shaders` collects the shader sources with
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

**One-time churn to expect.** Sorting changes the committed order, so the
`generated_files` / `alignment_tests` atlas snapshots and
`src/generated/shader_atlas.rs` all need one deliberate regeneration. After
that the order is alphabetical everywhere and matches what rustfmt was already
doing to the module list, so the two stop disagreeing.

**Gate.** `just shaders` twice in a row, from different working-directory
states, produces byte-identical output; `just test` green; regenerating after
`touch`-ing a `.slang` file produces no atlas diff.

---

## 2. Snapshots capture unformatted output, so they can never match the files

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

Plus, for headless verification (§7): `mesa-vulkan-drivers` (lavapipe ICD) and
`vulkan-validationlayers`.

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

## 7. Headless example runs — **proven working, worth wiring up**

[`offscreen_testing.md`](offscreen_testing.md) designs `just headless-all` (a
validation sweep under a software driver) and argues correctly that no cloud
GPU is needed. It is marked "design, not yet implemented." **The driver stack
it needs is now confirmed to work**, which removes the main unknown.

Starting point on this container: every example died at SDL init with
`Error: No available video device`, so §P7's entire validation sweep was
recorded as un-runnable. Walking it forward:

| step | result |
|---|---|
| default | `Error: No available video device` |
| `SDL_VIDEODRIVER=dummy` | `Vulkan support ... not available in current SDL video driver (dummy)` |
| `SDL_VIDEODRIVER=offscreen` | past SDL; `Installed Vulkan doesn't implement the VK_KHR_surface extension` |
| + `mesa-vulkan-drivers` (lavapipe ICD) | `missing required layer: VK_LAYER_KHRONOS_validation` |
| + `vulkan-validationlayers` | **runs clean** |

Final confirmed invocation:

```sh
SDL_VIDEODRIVER=offscreen \
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
timeout 15 cargo run --example basic_triangle
```

`basic_triangle` ran the full timeout (exit 124), renderer initialized with the
debug-utils callback live, **zero validation errors**. So examples are runnable,
and validation *is* observable, in a plain container with no GPU.

**The fix.** Implement `offscreen_testing.md`'s sweep now that its premise is
confirmed: a recipe that sets the three environment variables, runs each
example under a timeout, and fails on any validation output. Note the design
doc's point that `timeout`'s signal skips `Drop`, so a leak-checking variant
needs a clean-exit path rather than a kill — that part is unchanged by this
finding.

**Out of scope here:** golden images. `offscreen_testing.md` is explicit that
lavapipe does not solve image comparison against frames blessed on real
hardware, and nothing found this session changes that.

**Gate.** `just headless-all` runs every example and exits nonzero if any
emits validation output — demonstrated by deliberately introducing one.

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

§1 and §2 together (they touch the same function and share one snapshot
regeneration — doing them separately means regenerating twice), then §3+§4+§5
as a "clean clone builds" unit, then §6, then §7 as its own piece against the
existing design doc.

## Risks

1. **The §1+§2 snapshot regeneration is large and must be reviewed as noise.**
   That is exactly the situation where a real change hides. Mitigation: land
   §1 and §2 as two commits within the PR, and verify each diff is
   *mechanically* explainable — §1 pure reordering (every line present on both
   sides, as checked with a sort-and-compare), §2 pure rustfmt output
   (re-derivable by piping the old snapshot through `rustfmt`).
2. **Formatting at generation time makes `prepare_shaders` depend on an
   external binary.** Mitigation: hard error with an actionable message; it is
   already a de-facto dependency via the `cargo fmt` in `just shaders`.
3. **`.cargo/config.toml [env]` may not express `$PWD`.** Mitigation is stated
   in §5 — fall back to recipe-level sourcing, which is known to work because
   Windows already does it.
4. **Sorting could theoretically change behavior if anything depended on atlas
   order.** Checked: nothing derives an index from it — no `enumerate()` over
   the atlas, no positional lookup; the struct is addressed by field name.
   Worth re-confirming while implementing rather than trusting this note.
