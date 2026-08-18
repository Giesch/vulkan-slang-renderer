# mltrs Roc platform

A [Roc](https://www.roc-lang.org/) platform that renders with the mltrs
renderer. A Roc app supplies a window title. The host opens the window and
draws the basic triangle.

The host ABI boilerplate, `build.sh`, and the platform module layout come from
[roc-platform-template-rust](https://github.com/lukewilliamboswell/roc-platform-template-rust).
See `LICENSE`.

## Requirements

To build the platform:

- Rust. `rust-toolchain.toml` pins the version.
- `roc` on `PATH`
- `rust_glue` installed for the same roc build: `roc install rust_glue <url>`
- SDL3 build dependencies

To run what it builds:

- A Vulkan loader (`libvulkan.so.1`), `libgcc_s.so.1`, `libm.so.6` and
  `libc.so.6`. All four exist on any desktop Linux that runs a Vulkan game.
- glibc 2.39 or newer. `stubs/generate.sh` sets that floor; see
  [`../llm_notes/tech_debt.md`](../llm_notes/tech_debt.md) §18 for what it
  excludes.

`just roc-platform stubs` additionally needs gcc and binutils. It is the only
recipe that does. Run it only when the link inputs change.

`built_with_roc_version.txt` records the roc build the platform was last
compiled against. Roc installs glue plugins per compiler version, so a
different `roc` needs its own `roc install rust_glue`.
`built_with_toolchain.txt` records the glibc and gcc the committed link inputs
came from.

## Platform API

An app provides a `game` record and gets a window:

```roc
app [game] { pf: platform "../platform/main.roc" }

import pf.Game

game = {
	init!,
}

init! : {} => Game.Init
init! = |_| { window_title: "Basic Triangle from Roc!" }
```

The host calls `init!` once, before it creates the window. `Stdout`, `Stderr`,
and `Stdin` are also exposed.

`platform/InitConfig.roc` names both halves of that boundary:

- `InitConfig.Init` — the alias for the record an app returns. `requires` uses
  it, so the shape has one definition. Aliases are structural, so an app
  writes the record literally and never imports anything.
- `InitConfig` — the nominal type `init_for_host!` wraps that record in before
  the host reads it, so the generated glue names the Rust type. An anonymous
  record reaches Rust as a structural hash (`AnonStruct2fe7803feeace153`), and
  every field added to it renames the Rust type.

The module stays out of `exposes`. An app never names it.

Roc requires a module's top-level to be a nominal type, so `Init` is an
associated alias of `InitConfig` rather than its own module, and it is spelled
`InitConfig.Init` at every use.

What the window draws is fixed: `src/game.rs` holds the basic triangle, ported
from `examples/basic_triangle`. Roc controls the title and nothing else.

## Usage

```bash
just roc-platform build        # build the host archive
just roc-platform run          # run examples/basic_triangle.roc
just roc-platform exe          # build a standalone executable
just roc-platform test         # build and run every example headlessly
just roc-platform bundle       # bundle the platform into dist/
just roc-platform bundle-test  # prove the bundle runs from a URL
just roc-platform stubs        # regenerate the committed link inputs
just roc-platform licenses     # regenerate platform/LICENSES
just roc-platform glue         # regenerate src/roc_platform_abi.rs
just roc-platform shaders      # regenerate src/generated/ from shaders/source/
```

## Shipping

`just roc-platform bundle` writes `dist/<hash>.tar.zst`. The name is a BLAKE3
hash of the content, so every release has a different name. Put the archive at
a public URL, and name that URL in the app header:

```roc
app [game] { pf: platform "https://example.com/<hash>.tar.zst" }
```

The archive is 41 MB. It expands to 154 MiB, because `libhost.a` is 155 MB.

roc keeps a platform package out of the 10 MB per-package limit, so an app
needs no `--max-package-mb` flag. roc applies the 100 MB transitive limit to
the platform package, so an app that names this platform by URL needs one
flag:

```bash
roc --max-transitive-mb=0 main.roc
```

The archive carries `NOTICE` and `LICENSES/`. They record the licence of the
platform and of every redistributed file: `libstdc++.a`, the glibc startup
objects, and the libraries `libhost.a` links statically. `ci/licenses.sh`
regenerates `LICENSES/` from the toolchain and from `cargo metadata`, and
`stubs/generate.sh` calls it.

`just roc-platform bundle-test` proves the archive. It serves `dist/` on
loopback and runs the example in an `ubuntu:24.04` container. That container
holds the Vulkan loader and the lavapipe software driver. It has no rust, no
cargo, no cmake, no gcc, no SDL3, no Vulkan headers and no `libvulkan-dev`.
The test then examines the executable: the interpreter path, the library
list, the symbol versions, the undefined symbols, the copy relocations and
the exported symbols. A green run shows that the executable needs a Vulkan
loader and glibc 2.39, and nothing else.

## Releasing

`.github/workflows/roc-platform-release.yml` builds, tests and publishes the
platform. It runs on a pinned `ubuntu-24.04` runner, which is the floor image:
a host symbol above glibc 2.39 fails the release build at link time.

A pull request that touches `roc-platform/**` runs every job except the
release. A `workflow_dispatch` with a `release_version` of `X.Y.Z` publishes
the tested archive under the tag `roc-platform-X.Y.Z`.

The workflow builds roc from source at the commit in `ci/roc_commit.txt`, then
asserts `roc version` names it. `built_with_roc_version.txt` records a
different hash: it names the dev machine's build, whose branch carries local
commits. Update both files in one commit.

Three checks guard the committed artifacts:

- `ci/expected_sdl_backends.txt` names the SDL backend set. A dev package on
  the runner that the dev machine lacks turns on another backend, and the
  comparison fails with the backend's name.
- `stubs/*.s` must not change when the runner regenerates them. That is the
  signal a new dependency entered the host.
- A change to `platform/targets/x64glibc`, `platform/LICENSES` or
  `built_with_toolchain.txt` prints a warning. Those files are byte copies
  from apt packages, so an Ubuntu point update moves them with no fix
  available from the dev machine. The workflow restores the committed bytes,
  so the release always ships the reviewed inputs.

## Layout

- `platform/main.roc` — the platform header: `requires`, `hosted`, and the
  link inputs for each target.
- `platform/{Stdout,Stderr,Stdin}.roc` — app-facing effect modules.
- `platform/Game.roc` — the `game` record an app provides and its `Init` alias.
- `platform/InitConfig.roc` — internal: the `Init` alias an app returns and the
  nominal type the host reads.
- `platform/Host.roc` — the hosted-effect boundary the modules above wrap.
- `platform/NOTICE`, `platform/LICENSES/` — the licence texts the archive
  ships. `ci/licenses.sh` regenerates `LICENSES/`.
- `platform/targets/x64glibc/` — the link inputs. Committed except `libhost.a`.
- `stubs/generate.sh` — regenerates those link inputs and the licence texts.
- `ci/roc_commit.txt` — the upstream roc commit CI builds.
- `ci/expected_sdl_backends.txt` — the SDL backend set CI asserts.
- `stubs/*_stub.s` — the generated stub sources, committed for review.
- `stubs/forward/` — the C sources behind `libc_forward.a`.
- `src/lib.rs` — allocators, hosted-effect implementations, and `rust_main`.
- `src/game.rs` — the `Game` impl the host runs.
- `src/roc_platform_abi.rs` — generated by `roc glue rust_glue`. Do not edit.
- `shaders/`, `src/generated/` — the same shader workflow the examples use.

## Targets

`x64glibc` only. The host links SDL3, the Vulkan loader, and the C++ runtime
that slang and vk-mem need, all as glibc shared libraries. The musl and macOS
targets the template shipped need a static Vulkan and SDL story that does not
exist here.

`roc` resolves every name in a target's `inputs` list against
`platform/targets/x64glibc/`, including the glibc startup objects and the
system shared libraries. Every one of them is committed except `libhost.a`,
which `build.sh` rebuilds.

- `libc.so`, `libm.so`, `libgcc_s.so` and `libvulkan.so` are **stubs**. Each
  declares the symbols the host archive leaves undefined, carries the real
  library's SONAME, and carries no symbol versions. The executable therefore
  records `libc.so.6` as a plain `DT_NEEDED` and no `GLIBC_2.xx` requirement,
  and the real library supplies every implementation at run time.
- `libstdc++.a` is a committed copy. The host links the C++ runtime
  statically, so `ldd` on a built example lists no `libstdc++.so.6`. A stub
  cannot do this job: 26 of the libstdc++ symbols the host needs are data
  objects, and a stub sizes their copy relocations.
- `libc_forward.a` supplies `atexit` and three other symbols that glibc keeps
  out of `libc.so.6` on every version, by forwarding each to a symbol that
  `libc.so.6` does export.
- `Scrt1.o`, `crti.o` and `crtn.o` are committed copies of the glibc startup
  objects.

`stubs/generate.sh` produces all of that. It measures every link input, assigns
each undefined symbol to the first system library that defines it, and fails
when any symbol has no provider — that is the signal a new dependency entered
the host. Run it with `just roc-platform stubs` and commit what changes.

The `.s` stub sources live in `stubs/`, not in `targets/`, so `roc bundle`'s
glob over `targets/` does not ship them.

## Cargo

This directory is its own cargo workspace, excluded from the root one:

- The host needs `panic = "abort"` and an LTO release profile, and cargo
  applies `[profile.*]` only at a workspace root.
- Building it needs `roc` and generated glue, so the root
  `cargo check --workspace --all-targets` stays runnable without roc.

`Cargo.lock` is committed. It pins `sdl3-src` to 3.2.24; 3.4.14 ships a
`CMakeLists.txt` that calls `add_subdirectory(test)` without a `test/`
directory, so `build-from-source-static` fails.
