# mltrs Roc platform

A [Roc](https://www.roc-lang.org/) platform that renders with the mltrs
renderer. A Roc app declares its GPU resources and render graphs in pure Roc
and picks a graph and its values each frame. The host opens the window,
creates the resources, and executes the graphs.

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

An app provides a `game`, built by `Game.new` from an `init!`, a set of
validated render graphs, and a `draw` function:

```roc
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.GraphSet
import pf.RasterState
import pf.RenderGraph
import pf.ResourcePlanner

import Generated/BasicTriangle
import Generated/ShaderAtlas

game = Game.new({ init!, graphs: graph, draw })

init! : {} => Game.Init
init! = |_| {
	window_title: "Basic Triangle from Roc!",
	shader_hot_reload: Bool.False,
	shaders_source_dir: ShaderAtlas.shaders_source_dir,
}

draw : Game.Frame -> GraphSet.Submission(_)
draw = |frame| graph.draw({ mvp: mvp_with_aspect_ratio(frame.aspect_ratio) })

(planner1, vertices) = ResourcePlanner.singleton_buffer(ResourcePlanner.empty, "vertices", BasicTriangle.Vertex.packer, triangle)
(planner2, indices) = ResourcePlanner.index_buffer(planner1, "indices", [0, 1, 2], 3)
(planner3, params) = ResourcePlanner.uniform_buffer(planner2, "params", BasicTriangle.params)
(planner4, pipeline) = ResourcePlanner.graphics_pipeline(planner3, { name: "basic_triangle", shader: BasicTriangle.shader, uniforms: { params: params }, raster: RasterState.default })

node = RenderGraph.draw_indexed(pipeline, indices, params, { vertices: vertices.addr() })

Ok(graph) = RenderGraph.new(planner4, node).map_ok(GraphSet.single)
```

`examples/basic-triangle/main.roc` is the full app. A graph that fails
validation fails `roc check` at its top-level `Ok` destructure, so an invalid
graph never reaches the host.

- `init!` runs once, before the host opens the window. `Game.Init` carries
  the window title and the shader hot reload settings;
  `Game.Init.titled(title)` leaves hot reload off.
- `draw` runs every frame with `Game.Frame`: `aspect_ratio` (width over
  height) and `elapsed` (seconds since startup).
- `shader_hot_reload: Bool.True` makes the release host recompile the app's
  slang sources from `shaders_source_dir` when they change and swap the
  rebuilt pipelines. `VKR_SHADER_HOT_RELOAD=1`/`=0` overrides it at launch. A
  relative dir resolves against the working directory, then the executable's
  directory; a dir found in neither leaves hot reload off.

[`../docs/roc_render_graph.md`](../docs/roc_render_graph.md) documents the
resource planner, the nodes, validation, the `HostGraph` wire form, and how
the host executes it. `Stdout` and `Stderr` are also exposed.
`ShaderReflection` exposes the reflection schema and the logical shader value
types that generated Roc shader modules import as `pf.ShaderReflection`.

`mltrs shaders compile` generates an app's Roc shader modules when a project
has `main.roc` (or passes `--language roc`): a tool-owned
`Generated/ShaderAtlas.roc` tree with typed shader descriptors, block schemas,
GPU byte packers, and reflection, importing SPIR-V from `shaders/compiled/`.
The host has no shaders of its own; it builds every pipeline from the SPIR-V
and reflection JSON a graph carries.

The host-facing types (`Game.Init`, `Game.HostConfig`, `Game.Frame`,
`GraphSet.Draw`, and everything a `HostGraph` holds) are nominal, so the
generated glue gives each a stable Rust name. An anonymous record reaches Rust
as a structural hash, and every field added to it renames the Rust type.

## Usage

```bash
just roc-platform build                      # build the host archive
just roc-platform run                        # run examples/basic-triangle/main.roc from its directory
just roc-platform exe                        # build a standalone executable
just roc-platform test                       # host CPU tests, Roc fixtures, and every example headlessly
just roc-platform roc-test                   # format-check and test the platform's Roc modules
just roc-platform bundle                     # bundle the platform into dist/
just roc-platform bundle-test                # prove the bundle runs from a URL
just roc-platform stubs                      # regenerate the committed link inputs
just roc-platform licenses                   # regenerate platform/LICENSES
just roc-platform glue                       # regenerate src/roc_platform_abi.rs
just roc-platform roc-shaders                # regenerate every example's Roc shader modules and SPIR-V
just roc-platform roc-shaders basic-triangle # regenerate one example's Roc shader modules and SPIR-V
```

Each Roc example lives in `examples/<name>/`, with `main.roc`,
`shaders/source/`, `shaders/compiled/`, and `Generated/ShaderAtlas.roc`.
`run` and `exe` accept the directory name (default `basic-triangle`); `exe`
writes `examples/<name>/main`. `basic-triangle` draws an indexed triangle by
vertex pulling. `creative-coding-sep-23` draws three triangles whose color
pulses with `frame.elapsed`, with shader hot reload on; its `just watch`
restarts it on Roc edits.

The fixtures beside `examples/basic-triangle/main.roc` (`two_draws.roc`,
`invalid_*.roc`, and the rest) are not examples: `ci/all_tests.sh` checks that
each valid one passes `roc test` and each invalid one fails `roc check` for its
one intended reason.

`roc glue rust_glue` generates `src/roc_platform_abi.rs`, including the
`roc_init_owned`, `roc_config_owned`, and `roc_draw_owned` wrappers that
release every Roc allocation on drop. Run `just roc-platform glue` after
changing a type `platform/main.roc` exposes to the host.

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

The workflow downloads the exact prebuilt Roc nightly in `ci/roc_nightly.txt`
using the same pinned setup action as roc-ray. The installer verifies the
archive's SHA-256 against GitHub release metadata and installs it into the
runner tool cache, keyed by nightly tag and platform. Reuse is runner-local;
fresh GitHub-hosted runners download the archive instead of compiling Roc.
The workflow then asserts that `roc version` matches the pin.

Use the pinned nightly on `PATH` to reproduce CI locally; `ci/all_tests.sh`
prints the selected version but does not enforce the pin.
`built_with_roc_version.txt` separately records the compiler used to generate
the committed host glue; changing the CI pin does not regenerate that glue.
When updating the pin, run both `ci/all_tests.sh` and `ci/bundle_test.sh`:
the latter also checks the downloaded compiler in the Ubuntu 24.04 container.

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
- `platform/{Stdout,Stderr}.roc` — app-facing effect modules.
- `platform/Game.roc` — `Game.new`, `Game.Init`, `Game.Frame`, and the
  `Game.HostConfig` the host reads.
- `platform/{ResourcePlanner,RenderGraph,GraphSet,RasterState}.roc` — the
  render graph API apps use; `RenderGraphDesc`, `RenderGraphLower`,
  `RenderGraphValidate`, and `ValidatedRenderGraph` lower and validate it.
- `platform/Host.roc` — the hosted-effect boundary the modules above wrap.
- `platform/ShaderReflection.roc` — the reflection schema and logical shader
  value types that generated Roc shader modules import.
- `platform/NOTICE`, `platform/LICENSES/` — the licence texts the archive
  ships. `ci/licenses.sh` regenerates `LICENSES/`.
- `platform/targets/x64glibc/` — the link inputs. Committed except `libhost.a`.
- `stubs/generate.sh` — regenerates those link inputs and the licence texts.
- `ci/roc_nightly.txt` — the prebuilt Roc nightly used in CI.
- `ci/expected_sdl_backends.txt` — the SDL backend set CI asserts.
- `stubs/*_stub.s` — the generated stub sources, committed for review.
- `stubs/forward/` — the C sources behind `libc_forward.a`.
- `src/lib.rs` — allocators, hosted-effect implementations, and `rust_main`.
- `src/game.rs` — `RocGame`, the `Game` impl the host runs: resource creation
  and per-frame graph execution.
- `src/host_graph.rs` — the owned copy of each Roc `HostGraph`.
- `src/graph_lifecycle.rs` — the setup and frame control flow.
- `src/shaders_source_dir.rs` — resolves an app's `shaders_source_dir`.
- `src/roc_platform_abi.rs` — generated by `roc glue rust_glue`. Do not edit.
- `tests/lifecycle/` — a separate cargo workspace that tests the host's CPU
  logic without linking Roc.
- `platform-tests/` — the platform's Roc tests and their test app.
- `examples/<name>/` — the Roc apps, shader sources, compiled SPIR-V, and
  generated Roc modules.

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
  declares the symbols the host archive leaves undefined and carries the real
  library's SONAME, and the real library supplies every implementation at run
  time. A symbol with a single version in its provider stays unversioned and
  adds no `GLIBC_2.xx` requirement. A symbol the provider also exports at a
  compat version carries a pin to the default version, because ld.so binds an
  unversioned reference to the oldest version node — the compat
  implementation. glibc 2.39 binds an unversioned `realpath` to
  `realpath@GLIBC_2.2.5`, which rejects a null resolved buffer with `EINVAL`.
  Every pinned version is a version of the floor glibc.
- `libstdc++.a` is a committed copy. The host links the C++ runtime
  statically, so `ldd` on a built example lists no `libstdc++.so.6`. A stub
  cannot do this job: 26 of the libstdc++ symbols the host needs are data
  objects, and a stub sizes their copy relocations.
- `libc_forward.a` supplies `atexit` and three other symbols that glibc keeps
  out of `libc.so.6` on every version, by forwarding each to a symbol that
  `libc.so.6` does export.
- `force_extract.o` holds a strong reference to `__cxa_pure_virtual`. Every
  host reference to it is weak, a weak undefined reference does not extract
  the definition from `libstdc++.a`, and no `DT_NEEDED` library exports it,
  so without this object the linker resolves it to address 0 and a
  pure-virtual dispatch jumps to null.
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
