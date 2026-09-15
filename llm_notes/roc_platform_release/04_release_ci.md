# phase 4 — release CI

> **Implemented 2026-08-18. Read this banner before acting on anything below.**
>
> The phase shipped close to this spec. Six things differ, and four of them are
> defects in the spec rather than choices.
>
> 1. **The regen-diff had to restore the tree.** §Job `build-and-test` step 4
>    regenerates the committed artifacts and never puts them back.
>    `stubs/generate.sh` rewrites nine tracked binaries and
>    `built_with_toolchain.txt`, and `ci/bundle_test.sh:323` then hard-asserts
>    that directory is git-clean. Without a restore, the byte drift this spec
>    calls a warning fails the job, and the released bundle carries runner
>    bytes rather than the reviewed committed ones. The workflow adds a
>    "Restore the committed tree" step.
> 2. **The gate reads `git status`, not `git diff`.** A brand-new `.s` arrives
>    untracked, and `git diff` cannot see it. The spec's pathspec was also
>    wrong: every step runs with the working directory `roc-platform`, and a
>    git pathspec is relative to the working directory, so
>    `-- roc-platform/stubs` matches nothing and exits 0.
> 3. **The SDL backend assert is an exact-set comparison.** §Open question 2
>    asks for the X11 and Wayland symbol names, and a presence check cannot see
>    an *extra* backend. The `ubuntu-24.04` runner image ships `libgbm-dev`,
>    which turns on KMSDRM, so an extra backend is the likely skew.
>    `ci/expected_sdl_backends.txt` holds the twelve `*_bootstrap` symbols, and
>    the workflow removes the packages that would add more.
> 4. **The availability check sits inside an `if`.**
>    `git ls-remote --exit-code` exits 2 when it finds nothing, which is the
>    success case, so the spec's bare call fails its own step under `set -e`.
> 5. **`release_version` reaches the shell through `env`.** A `${{ }}`
>    expression inside a `run:` body is expanded into the script text before
>    the shell parses it.
> 6. **The notice covers `libhost.a`, not only the toolchain copies.**
>    §License notice names `libstdc++.a` and the CRT objects. `libhost.a` also
>    links SDL3 (Zlib), slang (Apache-2.0 with LLVM exception) and about 140
>    rust crates, and SDL's licence requires attribution in the distribution.
>    `ci/licenses.sh` copies every licence text the build already has into
>    `platform/LICENSES/`, and `stubs/generate.sh` calls it.
>
> Two smaller corrections. CI runs `zig build build-release`, not
> `zig build -Doptimize=ReleaseFast`: that is the step the dev machine uses,
> and it is musl-static, which `ci/bundle_test.sh` needs because it bind-mounts
> the binary as one file. The bundle filename comes from the `dist/*.tar.zst`
> glob, because `bundle.sh` prints no `Created:` line.
>
> All four open questions are answered. See §Open questions.

Sub-plan of [`roc_platform_release.md`](../roc_platform_release.md) §7. That
document holds the rationale: why the floor is enforced at build time, why
the container proof runs beside the release pipeline. This document is the
implementation spec.

Two corrections to the main plan apply throughout:

- **There is no build container.** §7 says to build the release artifacts
  inside the `ubuntu:22.04` container. Phase 2 moved the floor to Ubuntu
  24.04 / glibc 2.39 with no container (see the banner in
  [`02_stub_generator.md`](02_stub_generator.md) and
  [`tech_debt.md`](../tech_debt.md) §18). The workflow runs on a pinned
  `ubuntu-24.04` runner instead. The runner is the floor image: its glibc
  2.39 and gcc 13 satisfy the exact-version asserts in
  `stubs/generate.sh`, and a symbol above glibc 2.39 fails the release
  build at link time, loudly and before publication.
- **The release-package suite is not adopted.** §7 calls the
  `roc-lang/release-package/actions/*` suite worth adopting whole. This
  platform ships one bundle for one target, so `prepare-bundles` and the
  test matrix add machinery without coverage. The suite's `publish-release`
  also creates bare `X.Y.Z` git tags, and tags in this monorepo need a
  namespace. The workflow is hand-rolled on the shape of
  `../roc-platform-template-rust/.github/workflows/release.yml`. Two suite
  features carry over by hand: the tag-availability check, and release
  notes that embed the platform URL in a form the next release can parse.
  The `roc bump` compatibility gate needs a previous release as its
  baseline, so it enters at the second release, not this phase.

Deliverables:

- `.github/workflows/roc-platform-release.yml` — the repo's first
  workflow: build, test, container proof, bundle, publish.
- `roc-platform/ci/roc_commit.txt` — the upstream roc commit CI builds.
- `roc-platform/rust-toolchain.toml` — pinned to the rustc version in
  `built_with_toolchain.txt`.
- A license notice that ships inside the bundle.

## Result state

| file | status |
| --- | --- |
| `.github/workflows/roc-platform-release.yml` | new |
| `roc-platform/ci/roc_commit.txt` | new |
| `roc-platform/ci/expected_sdl_backends.txt` | new |
| `roc-platform/ci/licenses.sh` | new |
| `roc-platform/rust-toolchain.toml` | `channel = "1.97.1"` |
| `roc-platform/platform/NOTICE` | new, named in `bundle.sh` |
| `roc-platform/platform/LICENSES/` | new, 24 generated files |
| `roc-platform/bundle.sh` | bundles `NOTICE` and `LICENSES/*` |
| `roc-platform/stubs/generate.sh` | calls `ci/licenses.sh` |
| `roc-platform/justfile` | `licenses` recipe added |
| `roc-platform/README.md` | "Releasing" section added |

On landing, annotate the main plan the way phase 2's done-note does: the
phase-4 bullet, and any §7 or §Verification claim this document corrects.

## Triggers and concurrency

- `workflow_dispatch` with one input, `release_version`. This is the only
  path to publication.
- `pull_request`, filtered to paths `roc-platform/**` and the workflow file
  itself. A PR run performs every job except `release`.
- Concurrency: dispatch runs share one global group and never cancel; PR
  runs group per ref with `cancel-in-progress`. Copy the conditional group
  expression from `../roc-ray/.github/workflows/release.yml:20-22`.

## The roc toolchain

CI builds roc from source at a pinned upstream commit.

- `roc-platform/ci/roc_commit.txt` holds one line: the full hash of a
  commit on `roc-lang/roc` `main`. Initial value:
  `40fe7ddce9ca4fcaa0745eb63e701c91979666bc`.
- The pin is a separate file from `built_with_roc_version.txt` because the
  two answer different questions. `built_with_roc_version.txt` records the
  hash of the dev machine's build, whose branch carries local commits that
  exist nowhere upstream. Those commits touch only a justfile, so the
  compiler at the pin is the same compiler. The bump procedure keeps the
  two in step: update `ci/roc_commit.txt` and run
  `just roc-platform record-roc-version` in the same commit.
- The workflow checks out `roc-lang/roc` at the pin, installs zig 0.16.0
  (`mlugg/setup-zig@v2`; `minimum_zig_version` in roc's `build.zig.zon`),
  runs `zig build -Doptimize=ReleaseFast`, and caches the resulting binary
  with `actions/cache` keyed on the pin and the zig version. A cache hit
  skips the checkout and build.
- After the build or cache restore, assert `roc version` names the pinned
  hash. This catches a stale cache and a pin that does not match the
  binary.

## Job `build-and-test`

Runner: `ubuntu-24.04`, pinned. Steps:

1. Checkout.
2. `apt-get install` the SDL3 build dependencies and
   `mesa-vulkan-drivers`. The exact package list is open question 1. The
   list must include the X11 and Wayland dev packages: SDL3 compiles a
   video backend only when its headers are present, so a missing package
   ships a host that cannot open windows on that display server, with no
   build error.
3. Install rust (the pinned `rust-toolchain.toml` selects the version) and
   the cached roc build with its version assert.
4. Regen-diff: run `bash stubs/generate.sh`, then
   `git diff --exit-code -- roc-platform/stubs`. The `.s` sources are
   textual and deterministic, and they carry the drift signal: a new
   undefined symbol or a changed data-object size changes them, and
   `generate.sh` itself fails on any symbol no provider defines. Diffs
   under `platform/targets/x64glibc/` and in `built_with_toolchain.txt`
   are printed, not failed: those files are byte-copies from apt packages
   and version-string records, so an Ubuntu point update or a
   dev-machine/runner package skew would hold a byte-diff red with no fix
   available from the dev machine. tech_debt §18's caveat stands: this
   step covers the happy path only, because the runner sits at the floor.
5. `bash ci/all_tests.sh` — builds the host, runs the floor-probe
   self-test and the headless examples.
6. SDL backend assert: `nm --defined-only` over `libhost.a` finds the X11
   and Wayland video bootstrap symbols. This guards the package list in
   step 2. The exact symbol names are open question 2.
7. `bash ci/bundle_test.sh`, unchanged. It rebuilds (cargo-cached), checks
   `platform/targets/x64glibc/` is git-clean except `libhost.a`, bundles,
   serves `dist/` on loopback, and runs the container proof. It needs no
   prebuilt-bundle mode: the rebundle is byte-identical, measured in
   phase 3.
8. Upload `roc-platform/dist/*.tar.zst` as the workflow artifact
   (`if-no-files-found: error`) and expose the bundle filename as a job
   output. The glob is unambiguous: `bundle.sh` asserts exactly one file.

Cargo caching (`Swatinem/rust-cache` or `actions/cache` over
`roc-platform/target/`) is an implementation option, not a requirement.
The first uncached run pays for roc, SDL3 and slang; later runs pay for
the diff.

## Job `release`

Runs only on `workflow_dispatch`. Needs `build-and-test`. Permissions:
`contents: write` on this job alone.

1. Validate `release_version` against `^[0-9]+\.[0-9]+\.[0-9]+$`. The tag
   and the release title are both `roc-platform-<version>`.
2. Availability check: `git ls-remote --exit-code --tags origin <tag>`
   must find nothing, and `gh release view <tag>` must fail.
3. Download the bundle artifact. The tested bytes are the released bytes.
4. Write the notes file:
   - a `roc` code block with the full app header:
     `app [game] { pf: platform "https://github.com/Giesch/vulkan-slang-renderer/releases/download/roc-platform-<version>/<hash>.tar.zst" }`.
     The URL sits in a fixed section so a future `roc bump` gate can parse
     the previous release's URL out of the notes, the way roc-ray's
     release flow does.
   - the `--max-transitive-mb=0` requirement
     ([`tech_debt.md`](../tech_debt.md) §19).
   - the glibc ≥ 2.39 floor ([`tech_debt.md`](../tech_debt.md) §18).
   - the Debian-family `PT_INTERP` limitation
     ([`roc_interp_fix.md`](../roc_interp_fix.md)).

     > **Removed 2026-08-18.** The roc fix is merged, so the limitation no
     > longer exists and the notes template no longer carries it.
5. `gh release create roc-platform-<version> <bundle> --title roc-platform-<version> --notes-file <file> --target "$GITHUB_SHA"`.

## Compression

The bundle keeps roc's default compression level 3. This closes the open
decision in [`03_bundle.md`](03_bundle.md) §`bundle.sh`. The hash is
reproducible at level 3, size work is deferred (main plan §4), and the
release ships the tested artifact regardless of level. Revisit when the
download size becomes annoying.

## License notice

The bundle redistributes `libstdc++.a` and the CRT objects. Main plan §2
requires the GCC Runtime Library Exception in the bundle's license notice.
Add `roc-platform/platform/NOTICE`: the platform's own license, the GCC
Runtime Library Exception for `libstdc++.a` and `libgcc` pieces, and the
glibc licenses for the CRT objects, each with the source package versions
from `built_with_toolchain.txt`. Name the file in the `bundle.sh` command
so it ships in the archive. Whether `roc bundle` accepts an extensionless
path is open question 3; the fallback is `NOTICE.txt`.

## Done criteria

- A PR touching `roc-platform/**` or the workflow file runs
  `build-and-test` green.
- `workflow_dispatch` with a version produces a GitHub release whose asset
  is the tested `.tar.zst` and whose notes contain the platform URL, the
  `--max-transitive-mb=0` requirement, the floor, and the `PT_INTERP`
  limitation.

  > **Amended 2026-08-18.** The `PT_INTERP` limitation is fixed in roc and
  > dropped from the notes template; the other three items stand.
- The CI-built `roc version` matches `ci/roc_commit.txt`.
- Regen-diff: a change to any `stubs/*.s` fails the job;
  `built_with_toolchain.txt` drift only warns.
- `roc unbundle` on the released archive shows the notice file.
- Post-release smoke, manual, once: `roc run` against the published URL
  with `--max-transitive-mb=0`, inside the `ubuntu:24.04` image from
  `ci/bundle_test.Dockerfile`.

## Open questions

Verify each at the start of implementation. None is expected to block.

1. The SDL3 build-dependency package list for `ubuntu-24.04`. Start from
   SDL's `docs/README-linux.md` and prune against the backend assert.
2. The X11 and Wayland bootstrap symbol names in `libhost.a`. Confirm with
   `nm` on the dev machine before writing the assert.
3. Whether `roc bundle` accepts an extensionless `NOTICE` path as a
   positional input.
4. Whether the `ubuntu-24.04` runner image's default `gcc` is major 13.
   `stubs/generate.sh` asserts it; the image ships several compilers.

> **All four answered.**
>
> 1. **The list mirrors the dev machine, and a removal list guards it.** SDL3
>    compiles a backend only when its headers are present, and the generator
>    derives the stub set from `libhost.a`, so a package skew changes
>    `stubs/*.s`. The workflow installs `cmake ninja-build pkg-config
>    libasound2-dev libdbus-1-dev libdrm-dev libegl1-mesa-dev libudev-dev
>    libwayland-dev wayland-protocols libx11-dev libxcursor-dev libxext-dev
>    libxfixes-dev libxi-dev libxkbcommon-dev libxrandr-dev libxss-dev
>    libvulkan1 mesa-vulkan-drivers`, then removes `libgbm-dev` and nine other
>    packages the dev machine lacks. `libvulkan1` is for `stubs/generate.sh`,
>    which runs `gcc -print-file-name=libvulkan.so.1`.
> 2. **`X11_bootstrap`, `Wayland_bootstrap`, `Wayland_preferred_bootstrap` and
>    `OFFSCREEN_bootstrap`**, plus eight audio, camera and evdev names. The
>    assert compares the whole set against `ci/expected_sdl_backends.txt`
>    rather than probing for two names.
> 3. **Yes.** `roc bundle` applies no extension rule, and
>    `pathHasUnbundleErr` (`../roc/src/unbundle/unbundle.zig:296`) rejects only
>    absolute paths, traversal, Windows reserved names and reserved characters.
>    `roc unbundle` on the local archive lists `NOTICE` and `LICENSES/`.
> 4. **Unverified offline, and the workflow no longer depends on the answer.**
>    An early step asserts glibc 2.39 and gcc major 13 before the twenty-minute
>    build. It installs `gcc-13`, `g++-13` and four `/usr/local/bin` shims when
>    the default major is not 13. cc-rs calls `cc` and `c++`, so all four names
>    need the shim.
