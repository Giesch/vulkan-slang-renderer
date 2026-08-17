# releasable roc platform — plan

Make `roc-platform/` publishable as a `.tar.zst` bundle on a GitHub release.

An app author writes this header and runs `roc main.roc`:

```roc
app [game] { pf: platform "https://.../<hash>.tar.zst" }
```

They install `roc` and nothing else. No rust, no cmake, no SDL3 headers, no
Vulkan headers. SDL3 and the Vulkan loader may resolve things at run time.
They must not be link-time requirements.

## 0. Current state

`roc-platform/build.sh` symlinks nine files out of `/usr/lib/gcc/...` into
`platform/targets/x64glibc/` and copies `libhost.a` beside them:

```
Scrt1.o  crti.o  crtn.o  libc.so  libc_nonshared.a
libgcc_s.so  libm.so  libstdc++.so  libvulkan.so
```

`platform/main.roc` names all ten, plus `app`, in its
`targets.x64glibc.inputs` list — 11 entries.
`.gitignore` excludes them. They point at whatever this machine provides, so
`roc bundle` cannot produce a publishable archive.

`libhost.a` is 155 MB. `cargo build --release --lib` produces it.

## 1. What already works

Four facts make the rest tractable.

- **roc links the executable itself.** LLD is a library inside the `roc`
  binary. `../roc/src/llvm_compile/compile.zig:827` appends `ld.lld`, then
  calls `embedded_lld.link(...)`. The author's machine needs no `cc`, no
  `ld`, and no zig.
- **SDL3 is static.** The `sdl3` dep carries `build-from-source-static`.
  `sdl3-sys/build.rs` sets `SDL_STATIC=ON` and compiles the vendored C
  source. SDL3 `dlopen`s X11 and Wayland at run time, so no windowing
  library appears in the link line.
- **Slang is static.** The `Giesch/slang-rs` fork vendors prebuilt `.tar.xz`
  trees and links `slang-static`, which bundles slang, glslang, miniz, lz4
  and cmark. Nothing downloads. There is no `libslang.so`.
- **`roc bundle` accepts non-`.roc` paths.** `.a`, `.o` and `.so` files go
  into the archive. See `../roc-ray/scripts/bundle.sh`.

## 2. Make the link self-contained

Target state: `targets/x64glibc/` holds only files this repo builds and
commits.

The C++ runtime links statically. Every remaining `.so` becomes a **stub
shared object with the correct SONAME**. The stubs carry no symbol
versions, so the linked binary records no `GLIBC_2.34`-style requirement.
The floor is still **glibc ≥ 2.34** (Ubuntu 22.04+, Debian 12+), for three
reasons.

- glibc 2.34 merges libpthread and libdl into `libc.so.6`. `libhost.a`
  leaves `pthread_create` and `dlopen` undefined, and the binary names
  only `libc.so.6`, so an older glibc has no provider for them.
- glibc 2.33 first exports the `stat` family as dynamic symbols.
  `libhost.a` references `stat`, `fstat64`, `lstat64` and `fstatat64`
  directly.
- A `Scrt1.o` from glibc ≥ 2.34 passes NULL init and fini and relies on
  the 2.34 `__libc_start_main` to run the executable's init array. An
  older `__libc_start_main` skips the init array, and static constructors
  never run. Stripping symbol versions turns the loud
  `GLIBC_2.34 not found` error into that silent failure.

The floor is defined by the final executable's undefined symbol set, not
the stub set. GCC 13's `libstdc++.a` can reference `arc4random`, which is
glibc 2.36. Building the release artifacts inside the floor image (§7)
removes that creep by construction.

No rust changes. `ash` keeps `features = ["linked"]` and
`crates/renderer/src/renderer.rs:280` keeps `Entry::linked()`.

| Input | Fix |
| --- | --- |
| `libvulkan.so` | stub, SONAME `libvulkan.so.1` |
| `libstdc++.so` | replace with a committed `libstdc++.a` |
| `libgcc_s.so` | stub, SONAME `libgcc_s.so.1` |
| `libm.so` | stub, SONAME `libm.so.6` |
| `libc.so` | stub, SONAME `libc.so.6` |
| `libc_nonshared.a` | replace with a committed forwarding archive (see Risks) |
| `Scrt1.o`, `crti.o`, `crtn.o` | commit real copies |

The runtime cost is that `libvulkan.so.1`, `libgcc_s.so.1`, `libm.so.6`
and `libc.so.6` must exist on the player's machine. All four exist on any
desktop Linux that runs a Vulkan game.

### Static libstdc++

The C++ runtime is committed, not stubbed.
`platform/targets/x64glibc/libstdc++.a` is a copy of
`gcc -print-file-name=libstdc++.a`. It is 6.6 MB.

The reason is copy relocations. 26 of the 148 `libstdc++.so.6` symbols are
data objects, and the executable is `ET_EXEC`, so the linker resolves them
with `R_X86_64_COPY`. The copy is sized from **the stub's** declared size.
On a machine whose libstdc++ object is larger, the loader copies the
smaller of the two sizes and prints one line to stderr:

```
<exe>: Symbol `<sym>' has different size in shared object, consider re-linking
```

The `memcpy` stops at the smaller size. The redirection does not. Every
reference, including references inside libstdc++ itself, then points at a
slot too small to hold the object, and libstdc++ reads and writes past the
end of the executable's `.bss`. The failure is silent, and it appears only
on the player's machine.

Three files carry the change.

- `roc-platform/build.sh:56` copies `libstdc++.a` into the target
  directory. Use `cp`, not `ln -sf`. The file is committed and goes into
  the `.tar.zst`. A symlink supports neither.
- `roc-platform/platform/main.roc:17` names `"libstdc++.a"` in
  `x64glibc.inputs`. Keep the position after `libhost.a` and `app`. LLD
  extracts archive members lazily from left to right, and the dependency
  runs one way: `libhost.a` → `libstdc++.a` → `libc.so`.
- `roc-platform/.gitignore:15` drops the `libstdc++.so` line. Nothing else
  in that file matches `libstdc++.a`.

The stub generator measures `libhost.a` **and** `libstdc++.a`.
`libstdc++.a` leaves 201 symbols undefined that it does not define. 12 are
`_Unwind_*` and libgcc helpers. The rest are libc and libm functions:
`arc4random`, `bindtextdomain`, `fdopendir`, `fchmodat`, `fegetround`,
`fopen64`, `pthread_create`, `pthread_rwlock_*`. A rust and slang host
references none of `bindtextdomain`, `arc4random` or `fdopendir`, so a
generator that reads `libhost.a` alone produces a stub set that fails to
link. None of the 201 is a data object in `libc.so.6`, so they cost
nothing beyond `ret` stubs.

Keep the `libgcc_s.so.1` stub. Static libstdc++ with a shared unwinder is
the standard configuration, and those 12 symbols need a provider.

Two risks follow.

- **Two C++ runtimes in one process.** Mesa ICDs link `libstdc++.so.6` —
  `libvulkan_radeon.so`, `libvulkan_intel.so` and `libvulkan_lvp.so` all
  do. The Vulkan loader `dlopen`s the ICD, so the process holds the static
  runtime and Mesa's shared one. Two conditions make that safe. The Vulkan
  API is C, so no C++ object, exception or `type_info` crosses the
  boundary. And roc emits no export flags for an executable link —
  `--export-dynamic-symbol` fires only for shared-library output
  (`../roc/src/cli/linker.zig:255-268`) — so `.dynsym` holds only
  copy-relocated data, the static C++ symbols stay out of it, and a
  `dlopen`ed ICD cannot bind its `operator new` to them. The second
  condition is a property of roc's linker, not a guarantee. Check it per
  build. If it breaks, Mesa allocates through the static `operator new`
  and frees through its own `operator delete`, and the corruption appears
  only on machines with a Mesa driver.
- **The bundle carries one GCC's runtime.** A slang archive that needs a
  libstdc++ symbol newer than the build machine's fails at link time on
  the build machine. That is the right failure: loud, and before release.
  Record the glibc and GCC versions of the floor image beside
  `built_with_roc_version.txt`.

The C++ ABI split is not a risk. `libstdc++.a` defines 2030 `__cxx11`
symbols and 240 old-ABI `basic_string` symbols, so it resolves either ABI,
exactly like the shared object.

The GCC Runtime Library Exception covers redistribution. Name it in the
bundle's license notice, because the archive is a shipped artifact.

### Prior art

The stub technique is sanctioned, not a workaround.

- `../roc/src/build/glibc_stub.zig` — `generateComprehensiveStub` writes a
  `libc_stub.s` of `.globl` symbols. `compileAssemblyStub` builds it as
  `addLibrary(.{ .name = "c", .linkage = .dynamic, .version = .{ .major = 6 } })`
  with `linker_allow_shlib_undefined = true` and `pie = false`.
- `../roc-ray/build.zig` — `generateLibcStub` is the same code with a
  hand-written 313-symbol `libc_stub.s`. Its `libX11.so` stub takes the
  opposite approach: an explicit 7-name list, one comment explaining why
  each name is there. Its data objects (`stdin`, `stdout`, `stderr`,
  `environ`) export size 0 — the copy-relocation bug the generator below
  guards against. Copy the technique, not the file.
- Committed CRT objects: `../roc/test/int/platform/targets/x64glibc/` and
  `../roc-ray/platform/targets/x64glibc/`. roc-ray's carry no `.comment`
  section, so their glibc provenance is unrecoverable. This plan records
  its own.

### Measured symbol set

Against the current `libhost.a`:

| measurement | count |
| --- | --- |
| undefined symbols | 15,505 |
| defined symbols | 88,203 |
| undefined and not defined — the stub set | **548** |

The 548 split by provider, probed with `nm -D --defined-only`:

| provider | count | disposition |
| --- | --- | --- |
| `libvulkan.so.1` | 1 | stub |
| `libstdc++.so.6` | 148 | resolved by the static `libstdc++.a` |
| `libgcc_s.so.1` | 12 | stub |
| `libc.so.6`, `libm.so.6` | 387 | stub |

The static link takes the 148 C++ symbols out of the stub set. 400 remain.
`libstdc++.a` adds its own 201 undefined symbols to the libc, libm and
libgcc_s groups, and the two sets overlap, so measure the union rather than
adding the counts.

The single Vulkan symbol is `vkGetInstanceProcAddr`. `Entry::linked()`
builds its whole dispatch table through it (`ash-0.38.0/src/entry.rs:359`),
so the `libvulkan.so` stub has one entry.

Generate the lists; do not hand-write them. Run `nm --undefined-only` over
`libhost.a` and `libstdc++.a`, subtract what those two archives define,
then assign each remaining symbol to the first provider that defines it, in
priority order `libvulkan.so.1`, `libgcc_s.so.1`, `libm.so.6`,
`libc.so.6`.

Fail the generator on any symbol no provider defines. That is the signal a
new dependency entered the host.

Over-approximating from the whole archive is correct. The linker extracts
archive members lazily, so the stub set is larger than the final link needs.
Extra stub symbols cost nothing.

Keep the generator in `roc-platform/`. Commit its output so a release build
does not need `nm`.

### Data symbols in the libc stub

roc-ray's `libX11.so` stub is 7 functions, so a `.text` body of `ret` is
enough. The libc set is not uniform. Most entries are functions, and a few
— `environ`, `__environ`, `stdin`, `stdout`, `stderr` — are data objects.

roc's linker passes no `-pie`. `../roc/src/cli/linker.zig` names no such
flag. The executable is therefore `ET_EXEC`, and the linker resolves data
references into shared libraries with `R_X86_64_COPY` relocations. The copy
is sized from **the stub's** declared size, so a `ret`-bodied `stdout` gives
the loader a slot too small for glibc's `FILE`. There is no link error.

So the generator emits, per symbol:

- **function** — `.type <sym>, @function` in `.text`, body `ret`.
- **object** — `.type <sym>, @object` in the matching section (`.bss`,
  `.data` or `.rodata`) with `.size <sym>, <N>`. Read `N` from
  `nm -D --print-size` against the real library at generation time.

Find the objects rather than assuming the list. Run `nm -D --defined-only`
over the real `libc.so.6` and `libm.so.6`, and filter the stub set for
`nm` types `B`, `D`, `R` and `G`. Fail on any symbol whose type falls
outside the recognized set — a TLS symbol cannot be stubbed as a function
or an object. Warn on any data object that is not pointer-sized; those
sizes can drift between glibc versions.

Record the glibc version the stubs came from. A future mismatch is then
diagnosable.

### Risks

- **`atexit`.** glibc defines `atexit` only in `libc_nonshared.a`, on
  every version — glibc 2.39's `libc.so.6` exports no `atexit` dynamic
  symbol — and `libhost.a` references it. A stubbed `atexit` links and
  then fails to resolve at run time on every machine. So `inputs` keeps a
  committed one-object archive that forwards `atexit` to
  `__cxa_atexit(fn, NULL, __dso_handle)`. The generator cross-checks the
  libc stub list against the real `libc.so.6` dynamic exports, routes
  symbols defined only in `libc_nonshared.a` (`atexit`, `at_quick_exit`,
  `pthread_atfork`, `__stack_chk_fail_local`) into that archive, and fails
  on any symbol that fits neither.
- **`_Unwind_*`.** `panic = "abort"` is set, but std ships unwinding tables.
  Expect `_Unwind_*` in the `libgcc_s` stub.
- **No rust-side route to static libstdc++.** `cargo:rustc-link-lib=stdc++`
  is a dynamic directive, so nothing puts libstdc++ objects inside
  `libhost.a`, and `link-cplusplus` has no `static` feature — its features
  are `libc++`, `libstdc++` and `nothing`. The `inputs` list in
  `platform/main.roc` is the only place the choice can be made.

### Constraints from roc

- **Static musl is out**, even though it is roc's default target. Static
  musl has no working `dlopen`, and SDL3 and the Vulkan loader both need it.
  Declaring only glibc targets in `main.roc` forces roc onto the glibc path.
- **CPU floor.** The plain `x64` targets assume x86-64-v3 plus AES-NI and
  PCLMULQDQ, which is Haswell 2013 and newer (`../roc/design.md:10399`).
  The audience is PC games, so `x64glibc` alone is enough. `x64v1glibc`
  stays out of scope.
- **Missing files fail; extra files do not.**
  `../roc/src/cli/targets_validator.zig:171-227` checks only that each
  declared input exists. It never enumerates the target directory, and its
  `ExtraFileInTargetsDir` diagnostic is never constructed. `roc bundle`
  fails on a named file that is missing
  (`../roc/src/cli/main.zig:7522-7544`). Keep the stub `.s` sources
  outside `targets/`, beside the generator, so the bundle glob does not
  ship them.
- **Dynamic linker discovery.** At link time, on the machine running roc,
  `../roc/src/cli/libc_finder.zig` locates `libc.so` through
  `gcc|clang|cc -print-file-name=libc.so` and discards the compiler's
  answer for the dynamic linker (`libc_finder.zig:93`, marked TODO). It
  then probes the filesystem: the libc directory first, then `/lib64`,
  `/lib/x86_64-linux-gnu`, `/lib` (`libc_finder.zig:179-224`). The
  hardcoded `/lib64/ld-linux-x86-64.so.2` fallback is in
  `../roc/src/cli/linker.zig:637-643`. A machine with no compiler still
  works. On a Debian-family machine the libc directory wins the probe, so
  `PT_INTERP` becomes `/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2` — a
  path that does not exist on Fedora or Arch, where the shipped game fails
  with `ENOENT`. The fix belongs upstream in roc, or in a
  `patchelf --set-interpreter` step in the author's release process.

## 3. Bundle

Add `roc-platform/bundle.sh`, modelled on `../roc-ray/scripts/bundle.sh`:

```bash
roc bundle platform/*.roc platform/targets/x64glibc/* --output-dir dist
```

The output filename is a base58 BLAKE3 hash of the content, so the release
URL changes every release. Both reference repos automate rewriting their
examples to the new URL.

## 4. Bundle size

`libhost.a` is 155 MB. Slang dominates. `slang-embedded-core-module.cpp.o`
alone is 10 MB. roc's linker extracts members lazily, so the linked
executable is far smaller, but the bundle carries the whole archive.

roc enforces a 10 MB decompressed per-package limit by default
(`--max-package-mb`, `../roc/src/cli/cli_args.zig:104`), but the app's
platform package is exempt:
`../roc/src/compile/package_resolution.zig:854` passes no limit for the
platform edge. An app author needs no flag. Phase 3 confirms this
empirically with a plain `roc run` against the served bundle. `roc bundle`
itself accepts no size flag.

The release profile keeps `strip = "debuginfo"`. `-C strip` applies to
linked artifacts, not a staticlib, so no strip setting shrinks
`libhost.a`. Leave size work — splitting out `egui`/`epaint`, trimming the
slang core modules — until the bundle size becomes annoying.

`roc install <shorthand> <url>` builds once at install time.
`roc run <shorthand>` then needs no compile step and no network. That is a
better story for a shipped game than repeated `roc run main.roc`.

## 5. The host bakes in one game

`roc-platform/src/game.rs` holds a hardcoded triangle. `src/generated/` is
codegen'd from `shaders/source/basic_triangle.shader.slang` and compiled
into `libhost.a`. `src/lib.rs` calls `roc_init()` once, then nulls
`ROC_HOST` before the window opens, so the frame loop never re-enters Roc.

A published platform cannot contain the app's shaders. The triangle is a
deliberate placeholder. None of this blocks §2 or §3.

The Roc-facing game API does not mirror the rust `Game` trait. Its design
is a separate, future plan. Until that plan exists, the platform exposes a
minimal stand-in API on the order of the current window-title value. The
rest of this section is input to that plan, not work in this one.

The target shape is a host that loads the app's shaders at run time.
`libhost.a` already contains the full slang compiler, so the host can
compile `shaders/source/*.slang` at startup or read a `shaders/compiled/`
directory. `roc run main.roc` stays the only command an author types.

That shape is reachable because `ShaderAtlasEntry`
(`crates/renderer/src/shaders/atlas.rs:25`) returns only values derivable
from a `ReflectionJson` plus SPIR-V bytes: vertex binding and attribute
descriptions, `layout_bindings`, `precompiled_shaders`, `pipeline_layout`.
The generated code precomputes them. A runtime implementation backed by a
loaded `ReflectionJson` computes the same values. `crates/renderer` already
recompiles slang at run time for hot reload.

The future plan covers three pieces, and together they are the largest
part of the project:

- A dynamic `ShaderAtlasRoot` and `ShaderAtlasEntry` pair driven by runtime
  reflection instead of `src/generated/`.
- A name-keyed Roc-facing draw API: bind a uniform buffer by name, upload
  vertex bytes against a reflected layout, issue a draw. The rust API is
  typed — `Resources<'a> { matrices_buffer: &UniformBufferHandle<MVPMatrices> }`
  and a `#[repr(C)] Vertex` — and Roc cannot use generated rust.
- Per-frame Roc callbacks. `Game::update` and `Game::draw` must re-enter
  Roc, so `ROC_HOST` stays live for the process lifetime, and
  `panic = "abort"` behaviour across the FFI boundary needs settling.
  `Game::window_title`, `initial_window_size`, `render_scale` and
  `max_msaa_samples` are associated fns
  (`crates/mltrs/src/game/traits.rs:34-68`), so anything Roc supplies for
  them needs the `OnceLock` pattern in `roc-platform/src/game.rs` or a trait
  change.

## 6. `mltrs dev` and `mltrs run`

Do not embed the roc compiler. roc exposes no library API, and the author
installs `roc` anyway. Both commands shell out to it.

- `mltrs dev` always recompiles shaders — and, as the platform grows,
  other assets or artifacts — then execs `roc <main.roc>`. It also watches
  `shaders/source/` for hot reload while the game runs, the way `just dev`
  serves a rust game today. The watcher lives in the renderer
  (`crates/renderer/src/shader_watcher.rs`), so it ships inside
  `libhost.a`; `mltrs dev` needs to enable it, not reimplement it.
- `mltrs run` skips recompilation, uses the precompiled artifacts in
  `shaders/compiled/`, and execs `roc <main.roc>`.

The slang half needs no new work. `mltrs shaders compile` already links
slang statically.

Add `Command::Dev` and `Command::Run` in `crates/cli/src/main.rs` beside
`Shaders`. `Dev` finds `main.roc` and `shaders/`, calls the existing
`build_tasks::write_precompiled_shaders` with
`generate_rust_source: false`, then execs `roc <main.roc>`. `Run` execs
`roc <main.roc>` with no compile step. Reuse the path defaults in
`CompileArgs`.

## 7. Release CI

Build the release artifacts — `libhost.a`, the stubs, the CRT objects, the
bundle — inside the same `ubuntu:22.04` container the tests use. The floor
is then enforced at link time: a symbol above glibc 2.35 fails the release
build, loudly and before publication. The stub generator and the CRT
copies target the floor image, and CI regenerates the committed outputs
and fails on diff.

Copy the workflow shape from
`../roc-platform-template-rust/.github/workflows/release.yml`: build,
bundle, serve the archive over `python3 -m http.server`, test the bundle on
an OS matrix, then `gh release create`.

roc-ray additionally uses the reusable `roc-lang/release-package/actions/*`
suite — `validate-release`, `run-bump-check`, `prepare-bundles`,
`test-bundle`, `make-release-notes`, `publish-release`. `run-bump-check`
compares the new platform's host boundary against the previous release and
gates on API compatibility. The suite is worth adopting whole. Its test
matrix runs `ubuntu-latest` only
(`../roc-ray/scripts/release_helpers.py:17`), so it does not test the
floor; the `ubuntu:22.04` container test stays alongside it.

## Phases

Phases 2 to 4 each get a sub-plan in `llm_notes/roc_platform_release/`.

1. **Static libstdc++.** Commit `libstdc++.a`. Change `build.sh`, the
   `inputs` list in `platform/main.roc`, and `.gitignore`. Confirm the link
   resolves against the system `.so` stubs still in place. This phase
   stands alone and shrinks the next one. No sub-plan: the "Static
   libstdc++" section in §2 is the full spec.
2. **Stub generator.** Sub-plan:
   `llm_notes/roc_platform_release/02_stub_generator.md`. Write the stub
   generator. Commit four `.so`s, their `.s` sources, and three CRT
   objects. Rewrite `build.sh` to drop every `gcc -print-file-name` call.
   Replace `libc_nonshared.a` in `inputs` in `platform/main.roc` with the
   `atexit` forwarding archive. Generate the committed artifacts inside
   the `ubuntu:22.04` floor image (§7): the recorded glibc version is then
   the floor's, and phase 4 does not regenerate them.

   > **Done 2026-08-17, but not as written here or in the sub-plan.** The floor
   > is Ubuntu 24.04 / glibc 2.39 and there is no container; `stubs/generate.sh`
   > runs locally and asserts its own glibc. Five specific claims in the
   > sub-plan's algorithm were wrong. Read the banner at the top of
   > `02_stub_generator.md` before touching any of this, and
   > [`tech_debt.md`](tech_debt.md) §18 for the reach the floor trades away.
   > §2's own "Measured symbol set" table below predicts 400 stub symbols; the
   > real figure at this floor is 475.
3. **Bundle + container proof.** Sub-plan:
   `llm_notes/roc_platform_release/03_bundle.md`. Add `bundle.sh`. Serve
   the `.tar.zst` locally, point an example at the URL, and run it in an
   `ubuntu:22.04` container with lavapipe. Reuse the headless setup in
   `roc-platform/ci/all_tests.sh`. The same image works on the dev
   machine, on GitHub runners, and in a Claude Code web session (an
   Ubuntu 24.04 VM with `docker` and Docker Hub on the default network
   allowlist). This phase owns the `PT_INTERP` decision from §2: a
   `patchelf --set-interpreter` step, an upstream roc fix, or a documented
   limitation.
4. **Release CI (§7).** Sub-plan:
   `llm_notes/roc_platform_release/04_release_ci.md`. Depends only on
   phases 1 to 3. It runs before the game API work, so `run-bump-check`
   takes its compatibility baseline from the triangle platform, and every
   later phase lands on a releasable pipeline.
5. **Roc game API (§5).** Deferred to a future plan.
6. **`mltrs dev` and `mltrs run` (§6).**

## Verification

Phases 1 to 3 are done when all of these hold. Each sub-plan copies its
slice of this list as its done-criteria.

- `just roc-platform build && just roc-platform test` passes headless with
  lavapipe.
- The stub generator reports zero unassigned symbols.
- `ldd ./basic_triangle` lists exactly `libvulkan.so.1`, `libgcc_s.so.1`,
  `libm.so.6`, `libc.so.6`, `linux-vdso` and `ld-linux`. No
  `libstdc++.so.6`.
- `readelf --version-info ./basic_triangle` shows no versioned glibc
  requirement.
- `readelf -l ./basic_triangle` names `/lib64/ld-linux-x86-64.so.2` as the
  interpreter, not a multiarch path.
- `LD_BIND_NOW=1 ./basic_triangle` runs. Eager binding resolves every
  stubbed symbol at startup, so a missing provider fails immediately and
  attributably.
- Inside the `ubuntu:22.04` container, every symbol from
  `nm -D --undefined-only ./basic_triangle` resolves against the
  container's libraries. This pins the glibc floor to the executable, not
  the stub set.
- `readelf -r ./basic_triangle | grep R_X86_64_COPY` lists libc data
  symbols only, and no `_Z`-prefixed name. Each entry's size matches the
  real system library. This check catches a `ret`-stubbed `stdout`.
- `nm -D ./basic_triangle` exports no `_Z`-prefixed symbol. That is the
  interposition check: a `dlopen`ed Mesa ICD must not bind its
  `operator new` to the static libstdc++ inside the executable.
- `./basic_triangle` writes to stdout and survives a thrown-and-caught C++
  exception. That exercises the static C++ runtime against the shared
  `libgcc_s.so.1` unwinder.
- `git status` in `platform/targets/x64glibc/` is clean after `build.sh`,
  except for `libhost.a`.
- The bundled platform runs from a URL, with a plain `roc run` and no
  `--max-package-mb` flag, in an `ubuntu:22.04` container with no rust, no
  cmake, no SDL3, no Vulkan headers and no `libvulkan-dev`. lavapipe links
  `libstdc++.so.6`, so the container test covers the two-runtime case.
