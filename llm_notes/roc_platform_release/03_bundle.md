# phase 3 — bundle + container proof

> **Implemented 2026-08-17. Read this banner before acting on anything below.**
>
> The phase shipped close to this spec. Four things differ, and one of them is
> a new limitation on the shipped artifact.
>
> 1. **roc applies a second size limit, and platforms are not exempt from it.**
>    §Done criteria and the parent's §4 track `--max-package-mb` only. That
>    exemption is real, but `--max-transitive-mb` (default 100 MB) also applies,
>    and `checkTransitiveLimits` never consults `platform_exempt`. The platform
>    expands to 161,198,326 bytes, so an app that names it by URL needs
>    `--max-transitive-mb=0`. Recorded in [`../tech_debt.md`](../tech_debt.md)
>    §19. `ci/bundle_test.sh` measures this on every run rather than assuming
>    it.
> 2. **The container side builds first, then checks, then runs.** §Container
>    side orders the interpreter run first. The app exits on SIGTERM only, so a
>    passing run always consumes its whole timeout window, and putting a 155 MB
>    download and link inside that window costs minutes on every green run. The
>    cache is still empty at the first resolution, so the download proof is
>    unchanged.
> 3. **`roc <app>.roc` reports 143 on a clean shutdown.** §Container side step 2
>    calls for the `timeout --signal=TERM` ladder from `ci/all_tests.sh`. `roc`
>    runs the app as a child process, and `timeout` signals the whole process
>    group, so the app does receive SIGTERM and shuts down cleanly. `roc`
>    installs no SIGTERM handler, so it dies on the default disposition, and
>    `--preserve-status` reports `roc`'s status rather than the app's. The run
>    therefore reports 143 against the local platform too. The interpreter run
>    has its own ladder where 143 passes; the two executable runs verify
>    teardown.
> 4. **Both open questions are answered.** See §Open questions.
>
> The parts that were right and are worth keeping: no staging directory, the
> `cd platform` before bundling, `--network=host` with a loopback server, the
> read-only mounts, the image contents, and the decision to record `PT_INTERP`
> rather than assert it.

Sub-plan of [`roc_platform_release.md`](../roc_platform_release.md) §3 and
§4. That document holds the rationale: why a bundle, why the size is
acceptable, why the container proof pins the floor to the executable. This
document is the implementation spec.

Two corrections to the main plan apply throughout:

- **The container image is `ubuntu:24.04`, not `ubuntu:22.04`.** Phase 2
  moved the floor to Ubuntu 24.04 / glibc 2.39 (see the banner in
  [`02_stub_generator.md`](02_stub_generator.md) and
  [`tech_debt.md`](../tech_debt.md) §18). A 22.04 container lacks thirteen
  symbols the host references, so the proof cannot run there. Every
  `ubuntu:22.04` reference in the main plan's §3, §7 and §Verification
  reads `ubuntu:24.04` for this phase.
- **The `PT_INTERP` decision this phase owns is: defer to a roc fix.**
  roc records a Debian-family dynamic-linker path when it links on a
  Debian-family machine, so shipped executables fail on Fedora, Arch and
  SteamOS. The fix belongs in roc and has its own plan:
  [`roc_interp_fix.md`](../roc_interp_fix.md). This phase adds no
  `patchelf` machinery and documents the limitation in
  `roc-platform/README.md`. The main plan's verification item
  "`readelf -l` names `/lib64/ld-linux-x86-64.so.2`" is waived until that
  fix lands; the container test records the observed interpreter path
  instead of asserting it.

Deliverables:

- `roc-platform/bundle.sh` — produce `dist/<hash>.tar.zst` with
  `roc bundle`.
- `roc-platform/ci/bundle_test.sh` and
  `roc-platform/ci/bundle_test.Dockerfile` — serve the bundle over HTTP,
  run an app against the URL inside an `ubuntu:24.04` container with
  lavapipe, and run the executable checks there.
- Justfile recipes `bundle` and `bundle-test`.
- A "Shipping" section in `roc-platform/README.md` with the Debian-family
  limitation.

## Result state

| file | status |
| --- | --- |
| `roc-platform/bundle.sh` | new |
| `roc-platform/ci/bundle_test.sh` | new |
| `roc-platform/ci/bundle_test.Dockerfile` | new |
| `roc-platform/justfile` | `bundle` and `bundle-test` recipes added |
| `roc-platform/.gitignore` | `dist/` added |
| `roc-platform/README.md` | "Shipping" section added |
| `roc-platform/dist/` | gitignored bundle output |

On landing, annotate the main plan the way phase 2's done-note does: the
phase-3 bullet, and the §Verification items this document corrects.

## `bundle.sh`

The platform directory content equals the bundle content, so there is no
staging directory. roc-ray stages because it ships four targets in two
package variants from one source tree; this platform ships one target.

The script:

1. Checks the nine committed inputs and `libhost.a` exist in
   `platform/targets/x64glibc/`. Reuse the `COMMITTED_INPUTS` list shape
   from `build.sh`. On a missing file, print the hint
   `just roc-platform build` and exit 1.
2. Empties `dist/` so the output glob is unambiguous.
3. Runs, from inside `platform/`:

   ```bash
   roc bundle *.roc targets/x64glibc/* --output-dir ../dist
   ```

   The cd matters: `roc bundle` takes the paths as given, and
   roc-ray's script cds into its stage directory before bundling, so the
   archive root must be the platform directory. Confirm this on the first
   run; it is open question 1.
4. Prints the output filename and its size.

`roc bundle --compression` defaults to 3. Local proofs keep the default.
The release compression level is a phase 4 decision.

`.gitignore` already excludes `*.tar.zst`; the added `dist/` line keeps the
directory itself out of `git status`.

## Container proof: `ci/bundle_test.sh`

The proof is one script with a host side and a container side.

Host side:

1. Run `build.sh`, then `bundle.sh`.
2. Serve `dist/` with `python3 -m http.server` bound to `127.0.0.1` on an
   ephemeral port. Kill the server on exit.
3. Write the test app to a scratch directory: `examples/basic_triangle.roc`
   with its header rewritten to
   `app [game] { pf: platform "http://127.0.0.1:<port>/<hash>.tar.zst" }`.
   The committed example keeps its relative-path header; only the test copy
   names the URL.
4. Build the image from `ci/bundle_test.Dockerfile` and run it with
   `docker run --network=host`, mounting the `roc` binary and the scratch
   directory read-only. Host networking lets the container reach the
   `127.0.0.1` server. The image comes from Docker Hub, so the proof also
   runs in a Claude Code web session.

The image is `ubuntu:24.04` plus `libvulkan1`, `mesa-vulkan-drivers` and
`binutils`. `binutils` exists for the `nm`/`readelf` checks only.
Deliberately absent: rust, cargo, cmake, gcc, SDL3, Vulkan headers,
`libvulkan-dev`. The `roc` binary is statically linked, so the host's copy
runs in the container unmodified.

Container side:

1. Export `SDL_VIDEODRIVER=offscreen` and point `VK_ICD_FILENAMES` at the
   lavapipe ICD, the same setup as `ci/all_tests.sh`.
2. Run the test app with a plain `roc <app>.roc` under the
   `timeout --signal=TERM` clean-shutdown pattern from `ci/all_tests.sh`.
   This is the §4 exemption proof: the platform package downloads and runs
   with no `--max-package-mb` flag. Every container run starts with an
   empty roc cache, so the download path is always exercised.
3. Run `roc build` on the same app and execute the checks in §Done criteria
   against the resulting binary, including one plain run and one
   `LD_BIND_NOW=1` run.
4. Print the binary's `PT_INTERP` path. Do not assert it (see the
   correction above).

## Done criteria

The phase-3 slice of the main plan's §Verification, with this document's
corrections applied. `just roc-platform bundle-test` performs all of them.

- The bundled platform runs from a URL, with a plain `roc run` and no
  `--max-package-mb` flag, in an `ubuntu:24.04` container with no rust, no
  cmake, no SDL3, no Vulkan headers and no `libvulkan-dev`.
- lavapipe links `libstdc++.so.6`, so the container run covers the
  two-C++-runtimes case.
- Inside the container, against the `roc build` output:
  - `ldd` lists exactly `libvulkan.so.1`, `libgcc_s.so.1`, `libm.so.6`,
    `libc.so.6`, `linux-vdso` and `ld-linux`. No `libstdc++.so.6`.
  - `readelf --version-info` shows no versioned glibc requirement.
  - `LD_BIND_NOW=1` runs cleanly, so every stubbed symbol resolves eagerly
    at startup.
  - Every symbol from `nm -D --undefined-only` resolves against the
    container's libraries.
  - `readelf -r | grep R_X86_64_COPY` lists libc data symbols only, no
    `_Z`-prefixed name, and each entry's size matches the container's
    library.
  - `nm -D` exports no `_Z`-prefixed symbol.
- `git status` in `platform/targets/x64glibc/` is clean after `build.sh`,
  except `libhost.a`.

One main-plan item is deferred, not waived silently: "survives a
thrown-and-caught C++ exception". Nothing in the triangle host exercises
the unwinder on demand. Runtime slang compilation (main plan §5) will, and
the `LD_BIND_NOW` and lavapipe coverage stand in until then.

## Open questions

Verify each at the start of implementation. None is expected to block.

1. `roc bundle` path handling: whether archived paths are relative to the
   working directory. roc-ray's cd-before-bundle pattern says yes.
2. Whether a platform URL accepts plain `http`. roc-ray's release flow
   serves its bundle test over `python3 -m http.server`, which says yes.

> **Both answered.**
>
> 1. **Yes.** The archive root is the platform directory: `main.roc` and
>    `targets/x64glibc/libhost.a`, with no `platform/` prefix, so
>    `inputs_dir: "targets/"` resolves. Two further facts about `roc bundle`
>    that this document does not anticipate: `--output-dir` opens the directory
>    rather than creating it, so `bundle.sh` runs `mkdir -p dist`; and roc takes
>    the *first* `.roc` path as its module-discovery entry point, so a bare
>    `*.roc` glob leads with `Game.roc`. The command is
>    `roc bundle main.roc *.roc targets/x64glibc/* --output-dir ../dist`, and
>    roc sorts and deduplicates the repeated `main.roc`.
> 2. **Yes, on loopback only.** `../roc/src/base/url.zig:290 isSafeUrl` accepts
>    `https://`, `http://localhost`, `http://127.0.0.1` and `http://[::1]`.
>    Nothing else. `--network=host` is therefore required rather than
>    convenient: a bridge network needs the host LAN address, which that gate
>    rejects.

## Measured result

Bundle: 41,624,754 bytes compressed, 161,199,170 uncompressed, ratio 3.87:1 at
the default compression level 3. The hash is reproducible across runs.

Container run, `ubuntu:24.04` with `libvulkan1`, `mesa-vulkan-drivers` and
`binutils`:

- `DT_NEEDED` is exactly `libc.so.6`, `libgcc_s.so.1`, `libm.so.6`,
  `libvulkan.so.1`. No `libstdc++.so.6`.
- No versioned glibc requirement.
- 378 undefined symbols, all resolved against the container's libraries.
- Zero `R_X86_64_COPY` relocations, so that check stays vacuous. Phase 2
  measured the same.
- No `_Z`-prefixed export.
- Plain run and `LD_BIND_NOW=1` run both exit 0.
- `PT_INTERP` is `/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2`, the
  Debian-family multiarch path. Recorded, not asserted, as this document
  specifies.
