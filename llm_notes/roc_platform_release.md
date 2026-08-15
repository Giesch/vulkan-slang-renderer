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
`platform/targets/x64glibc/`:

```
Scrt1.o  crti.o  crtn.o  libc.so  libc_nonshared.a
libgcc_s.so  libm.so  libstdc++.so  libvulkan.so
```

`platform/main.roc` names all nine in its `targets.x64glibc.inputs` list.
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

Every `.so` becomes a **stub shared object with the correct SONAME**. The
stubs carry no symbol versions, so the linked binary records no
`GLIBC_2.34`-style requirement and runs on any glibc that has the symbols.

No rust changes. `ash` keeps `features = ["linked"]` and
`crates/renderer/src/renderer.rs:280` keeps `Entry::linked()`.

| Input | Fix |
| --- | --- |
| `libvulkan.so` | stub, SONAME `libvulkan.so.1` |
| `libstdc++.so` | stub, SONAME `libstdc++.so.6` |
| `libgcc_s.so` | stub, SONAME `libgcc_s.so.1` |
| `libm.so` | stub, SONAME `libm.so.6` |
| `libc.so` | stub, SONAME `libc.so.6` |
| `libc_nonshared.a` | drop; let the `libc.so` stub declare its symbols |
| `Scrt1.o`, `crti.o`, `crtn.o` | commit real copies |

The runtime cost is that `libvulkan.so.1`, `libstdc++.so.6`,
`libgcc_s.so.1`, `libm.so.6` and `libc.so.6` must exist on the player's
machine. All five exist on any desktop Linux that runs a Vulkan game.

### Prior art

The stub technique is sanctioned, not a workaround.

- `../roc/src/build/glibc_stub.zig` — `generateComprehensiveStub` writes a
  `libc_stub.s` of `.globl` symbols. `compileAssemblyStub` builds it as
  `addLibrary(.{ .name = "c", .linkage = .dynamic, .version = .{ .major = 6 } })`
  with `linker_allow_shlib_undefined = true` and `pie = false`.
- `../roc-ray/build.zig` — `generateLibcStub` is the same code with a
  hand-written 313-symbol `libc_stub.s`. Its `libX11.so` stub takes the
  opposite approach: an explicit 7-name list, one comment explaining why
  each name is there.
- Committed CRT objects: `../roc/test/int/platform/targets/x64glibc/` and
  `../roc-ray/platform/targets/x64glibc/`.

### Measured symbol set

Against the current `libhost.a`:

| measurement | count |
| --- | --- |
| undefined symbols | 15,505 |
| defined symbols | 88,203 |
| undefined and not defined — the stub set | **548** |

The 548 split by provider, probed with `nm -D --defined-only`:

| provider | count |
| --- | --- |
| `libvulkan.so.1` | 1 |
| `libstdc++.so.6` | 148 |
| `libgcc_s.so.1` | 12 |
| `libc.so.6`, `libm.so.6` | the rest |

The single Vulkan symbol is `vkGetInstanceProcAddr`. `Entry::linked()`
builds its whole dispatch table through it (`ash-0.38.0/src/entry.rs:359`),
so the `libvulkan.so` stub has one entry.

Generate the lists; do not hand-write them. Run `nm --undefined-only` over
`libhost.a`, subtract what `libhost.a` defines, then assign each remaining
symbol to the first provider that defines it, in priority order
`libvulkan.so.1`, `libstdc++.so.6`, `libgcc_s.so.1`, `libm.so.6`,
`libc.so.6`.

Fail the generator on any symbol no provider defines. That is the signal a
new dependency entered the host.

Over-approximating from the whole archive is correct. The linker extracts
archive members lazily, so the stub set is larger than the final link needs.
Extra stub symbols cost nothing.

Keep the generator in `roc-platform/`. Commit its output so a release build
does not need `nm`.

### Data symbols and copy relocations

roc-ray's `libX11.so` stub is 7 functions, so a `.text` body of `ret` is
enough. The C++ set is different. About 26 of the 148 `libstdc++.so.6`
symbols are data objects.

| group | count | `nm` type | size |
| --- | --- | --- | --- |
| `_ZSt4cout`, `_ZSt4cerr` | 2 | `B` | 0x110 each |
| `_ZSt11__once_call`, `_ZSt15__once_callable` | 2 | `B` | 8 each |
| `_ZSt7nothrow` | 1 | `R` | 1 |
| `_ZTV…` vtables | 13 | `V` | 0x20–0x80 |
| `_ZTT…` VTTs | 5 | `V` | 0x20–0x50 |
| `_ZTI…` typeinfo | 3 | `V` | 0x10–0x18 |

roc's linker passes no `-pie`. `../roc/src/cli/linker.zig` names no such
flag. The executable is therefore `ET_EXEC`, and the linker resolves data
references into shared libraries with `R_X86_64_COPY` relocations. The copy
is sized from **the stub's** declared size.

A zero-size or `.text`-declared `_ZSt4cout` corrupts `std::cout` and breaks
RTTI and exception dispatch at run time. There is no link error.

So the generator emits, per symbol:

- **function** — `.type <sym>, @function` in `.text`, body `ret`.
- **object** — `.type <sym>, @object` in the matching section (`.bss`,
  `.data` or `.rodata`) with `.size <sym>, <N>`. Read `N` from
  `nm -D --print-size` against the real library at generation time.

The sizes are stable. `std::ostream` layout and the `__cxxabiv1` vtable
shapes are fixed by the C++11 ABI.

Record the glibc and libstdc++ versions the stubs came from. A future
mismatch is then diagnosable.

Apply the same function-versus-object split to the libc and libm stubs.
Most entries are functions. A few — `environ`, `__environ`, `stdin`,
`stdout`, `stderr` — are objects on the same copy-relocation path.

### Risks

- **`atexit`.** glibc supplied it from `libc_nonshared.a` before 2.34 and
  from `libc.so.6` after. A stub that declares it fails to resolve at load
  time on an older glibc. Fallback: commit a one-object archive that
  forwards `atexit` to `__cxa_atexit(fn, NULL, __dso_handle)`, and keep the
  archive in `inputs`.
- **`libstdc++.so.6`.** If the stub proves unreliable, commit `libstdc++.a`
  and name it in `inputs`. That is fully self-contained and costs ~5 MB. The
  GCC Runtime Library Exception covers redistribution.
  `cargo:rustc-link-lib=stdc++` is a dynamic directive, so nothing puts
  libstdc++ objects inside `libhost.a` on its own, and `link-cplusplus` has
  no `static` feature. Its features are `libc++`, `libstdc++` and `nothing`.
- **`_Unwind_*`.** `panic = "abort"` is set, but std ships unwinding tables.
  Expect `_Unwind_*` in the `libgcc_s` stub.

### Constraints from roc

- **Static musl is out**, even though it is roc's default target. Static
  musl has no working `dlopen`, and SDL3 and the Vulkan loader both need it.
  Declaring only glibc targets in `main.roc` forces roc onto the glibc path.
- **CPU floor.** The plain `x64` targets assume x86-64-v3 plus AES-NI and
  PCLMULQDQ, which is Haswell 2013 and newer (`../roc/design.md:10391`).
  The audience is PC games, so `x64glibc` alone is enough. `x64v1glibc`
  stays out of scope.
- **No undeclared files.** `../roc/src/cli/targets_validator.zig` rejects
  any file in a target directory that `inputs` does not name.
  `roc bundle` fails on a named file that is missing. The directory must
  match the list exactly.
- **Dynamic linker discovery.** On the glibc path roc runs
  `gcc|clang|cc -print-file-name=...` on the player's machine to locate the
  dynamic linker (`../roc/src/cli/libc_finder.zig:93`). It falls back to a
  filesystem search, then to a hardcoded `/lib64/ld-linux-x86-64.so.2`. A
  machine with no compiler still works. Confirm this in the container test.

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
(`--max-package-mb`, `../roc/src/cli/cli_args.zig:104`). An app author
passes `--max-package-mb=<N>` on every `roc run`. There is no config-file
escape. Accept the flag for now and raise the limit upstream later.

Take `strip = "symbols"` in the release profile because it costs nothing.
Leave the rest — splitting out `egui`/`epaint`, trimming the slang core
modules — until the flag becomes annoying.

`roc install <shorthand> <url>` builds once at install time.
`roc run <shorthand>` then needs no compile step and no network. That is a
better story for a shipped game than repeated `roc run main.roc`.

## 5. The host bakes in one game

`roc-platform/src/game.rs` holds a hardcoded triangle. `src/generated/` is
codegen'd from `shaders/source/basic_triangle.shader.slang` and compiled
into `libhost.a`. `src/lib.rs` calls `roc_init()` once, then nulls
`ROC_HOST` before the window opens, so the frame loop never re-enters Roc.

A published platform cannot contain the app's shaders. The triangle is a
deliberate placeholder, and the API grows by iteration. None of this blocks
§2 or §3.

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

Three pieces remain, and together they are the largest part of the project:

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

## 6. `mltrs dev`

Do not embed the roc compiler. roc exposes no library API, and the author
installs `roc` anyway. `mltrs dev` shells out to it.

The slang half needs no new work. `mltrs shaders compile` already links
slang statically.

Add `Command::Dev` in `crates/cli/src/main.rs` beside `Shaders`. It finds
`main.roc` and `shaders/`, calls the existing
`build_tasks::write_precompiled_shaders` with
`generate_rust_source: false`, then execs `roc <main.roc>`. Reuse the path
defaults in `CompileArgs`.

## 7. Release CI

Copy the workflow shape from
`../roc-platform-template-rust/.github/workflows/release.yml`: build,
bundle, serve the archive over `python3 -m http.server`, test the bundle on
an OS matrix, then `gh release create`.

roc-ray additionally uses the reusable `roc-lang/release-package/actions/*`
suite — `validate-release`, `run-bump-check`, `prepare-bundles`,
`test-bundle`, `make-release-notes`, `publish-release`. `run-bump-check`
compares the new platform's host boundary against the previous release and
gates on API compatibility. The suite is worth adopting whole.

## Sequence

1. Write the stub generator. Commit five `.so`s, their `.s` sources, and
   three CRT objects. Rewrite `build.sh` to drop every
   `gcc -print-file-name` call. Drop `libc_nonshared.a` from `inputs` in
   `platform/main.roc`.
2. Add `bundle.sh`. Serve the `.tar.zst` locally, point an example at the
   URL, and run it in an Ubuntu 20.04 container with lavapipe. Reuse the
   headless setup in `roc-platform/ci/all_tests.sh`.
3. Iterate the Roc-facing game API (§5). Plan separately.
4. Add `mltrs dev` (§6).
5. Add release CI (§7).

## Verification

Steps 1 and 2 are done when all of these hold.

- `just roc-platform build && just roc-platform test` passes headless with
  lavapipe.
- The stub generator reports zero unassigned symbols.
- `ldd ./basic_triangle` lists exactly `libvulkan.so.1`, `libstdc++.so.6`,
  `libgcc_s.so.1`, `libm.so.6`, `libc.so.6`, `linux-vdso` and `ld-linux`.
- `readelf --version-info ./basic_triangle` shows no versioned glibc
  requirement.
- `readelf -r ./basic_triangle | grep R_X86_64_COPY` lists every data
  symbol, and each entry's size matches the real system library. This check
  catches a `ret`-stubbed `std::cout`.
- `./basic_triangle` writes to stdout and survives a thrown-and-caught C++
  exception. That exercises the copy-relocated `std::cout` and the RTTI
  vtables.
- `git status` in `platform/targets/x64glibc/` is clean after `build.sh`,
  except for `libhost.a`.
- The bundled platform runs from a URL in a container with no rust, no
  cmake, no SDL3, no Vulkan headers and no `libvulkan-dev`.
