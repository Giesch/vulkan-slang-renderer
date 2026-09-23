# mltrs Roc platform

A [Roc](https://www.roc-lang.org/) platform that renders with the mltrs
renderer. A Roc app supplies a window title and a render graph built from its
own shaders as compile-time constants. The host opens the window and executes
the graph every frame.

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

An app provides a nominal `Game`, constructed from `init!`, `draw`, and `graphs`:

```roc
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.RenderGraph
import pf.Graphs
import Generated/BasicTriangle
import Generated/Mltrs

game : Game
game = Game.new({ init!, draw, graphs })

graphs = Graphs.or_crash(Graphs.single(RenderGraph.draw_indexed(triangle)))

init! : {} => Game.Init
init! = |_| { window_title: "Basic Triangle from Roc!" }

draw = |frame| graphs.draw(mvp_for(frame.aspect_ratio))

triangle : RenderGraph.IndexedPipeline(BasicTriangle.Vertex, Mltrs.MvpMatrices)
triangle = RenderGraph.indexed_pipeline({
	name: "basic_triangle",
	shader: BasicTriangle.shader,
	vertices,
	indices: [0, 1, 2],
})
```

The host calls `init!` once, before it creates the window. It reads the
game's graph definitions once at setup, to create every graph's pipelines and
uniform buffers, and calls the game's `draw` once per frame with a
`Game.Frame` containing the window's current `aspect_ratio` and `elapsed` in
seconds since the host began game setup. The app's `draw` uses its
module-level `graphs`: a single-graph collection exposes
`draw(values)` directly. For multiple graphs, define a selection at module
scope with `graphs.select(|registered| registered.triangle)`, then call the
selection's `draw(values)`. Both paths supply concrete local
types for inference. Their opaque
`Graphs.Submission(g)` result retains the collection type, which `Game.new`
unifies with the registered `Graphs(g)`. Only the stored host callback erases
that type to `Graphs.Draw`; the nominal `Game` and host ABI stay monomorphic.
This checks type equality, not collection identity: two collections with the
same type can contain different definitions or orderings. Use the same
module-level collection for registration and drawing. The selector can also
capture a graph from another collection; collection identity is not proved.
Each selected graph retains its own packer, so frame values carry no slots
that could accidentally refer to another node or graph.
The game is a constant: `Graphs.single`
runs during compile-time constant evaluation and returns
`Err(InvalidRenderGraph(message))` for an invalid render graph, where `message` is
the same aggregated message the Rust validator reports. `Graphs.or_crash` turns
that `Err` into a compile-time error, so an invalid graph fails `roc check`
with the message. The `game : Game` annotation is load-bearing:
`roc check` evaluates annotated constants only, and an unannotated game is
first evaluated by `roc build`. The example builds its projection from the frame's aspect ratio, so
resizing the window keeps the triangle's proportions.

The render-graph API uses `RenderGraph`, `Graphs`, the internal `ValidatedRenderGraph`,
and `Game.new`. Names are for messages only; tuple position determines draw
and payload order.

- `RenderGraph` holds every declaration. `RenderGraph.indexed_pipeline` and
  `RenderGraph.vertex_count_pipeline` declare pipelines from a generated `shader`
  record. Its `shader.uniform` holds the generated binding for the constant
  buffer the pipeline writes each frame. Codegen adds this field only for
  exactly one constant buffer with a supported packer. The binding carries the
  buffer's position in descriptor-set-layout order, its size, and the packer
  for its type, so the app never names a uniform or orders a binding. An
  indexed pipeline takes logical vertices of the shader's vertex type and
  packs them with the shader's own layout. `shader.vertex` is a
  `VertexInput` or `NoVertexInput` marker from `ShaderReflection`,
  so a shader that reads vertices only fits `indexed_pipeline` and a shader
  that reads none only fits `vertex_count_pipeline`.
  `RenderGraph.draw_indexed` and `RenderGraph.draw_vertex_count` are the nodes. Each
  returns an opaque `RenderGraph(t)`: one declaration plus a packer consuming
  its uniform value. `RenderGraph.from_tuple_2` through `from_tuple_12` combine
  a tuple of render graphs into a render graph consuming the corresponding tuple
  of frame values. `RenderGraph.empty` consumes `{}` and emits no payloads.
- `ValidatedRenderGraph.new` takes a `RenderGraph`, checks each node's uniform against its
  shader's reflection and its mesh, lowers the nodes through
  `RenderGraphLower`, validates with `RenderGraphValidate`, and returns
  `Try(ValidatedRenderGraph(frame), [InvalidRenderGraph(Str)])`. Each node owns its
  pipeline and its uniform buffer, so one pipeline declaration can be drawn by
  two nodes with different values. `ValidatedRenderGraph` is internal to the platform.
- `Graphs` is the collection the app hands to the host. `Graphs.single`
  validates one `RenderGraph` and returns `Try(Graphs(ValidatedGraph(frame)), [InvalidRenderGraph(Str)])`.
  `Graphs.or_crash` unwraps that `Try`; `Graphs.map2` combines two
  collections (or render graphs) into one. The only way to build a `Graphs` is
  through validation, so every graph the host registers passed it.
- `Game.new` takes `{ init!, draw, graphs }`, checks that `draw` and `graphs`
  share the same generic collection type through `Graphs.Submission(g)`.
  It returns an opaque `Game` with a type-erasing draw closure.
- `Graphs.select` takes a collection and a selector, and returns an opaque
  `SelectedGraph(g, frame)`. Its `draw(values)` accepts `frame` and returns
  `Submission(g)`. A module-level selection stores the registered graph's
  packer and ordinal without retaining the selector or host assets.
- A single-graph collection exposes `draw(values)` as a convenience. This
  registers the packer at base ordinal zero on each call; it does not rebuild
  or upload host assets. To retain the registration, define
  `selected = graphs.select(|graph| graph)` at module scope.

A graph with two or more nodes uses a tuple of render graphs. Tuple position is
both the draw order and the frame-value order:

```roc
graphs = Graphs.or_crash(Graphs.single(RenderGraph.from_tuple_2((
	RenderGraph.draw_indexed(mesh_pipeline),
	RenderGraph.draw_vertex_count(sky_pipeline, 3),
))))

draw = |frame| graphs.draw((mvp_for(frame), light_for(frame)))
```

The collection and selected-graph `draw` methods require the exact frame type
of the render graph. Missing or extra tuple elements and incorrect uniform types
fail `roc check` at `draw`. Each leaf produces exactly one payload, using the
packer retained by that graph; callers do not supply node indices or packed
value lists. Swapping two values of the same type still type-checks, but each
position always writes its own distinct node. Each leaf checks its packer's
output length against the declared uniform size.

Tuple constructors accept one tuple argument and support arities 2 through
12. Nest tuple render graphs for larger graphs; frame tuples have the same
nesting. A single-node render graph consumes its uniform value directly.

For a collection containing multiple graphs, select each one at module scope:

```roc
graphs = Graphs.or_crash({ triangle: triangle_graph, sky: sky_graph }.Graphs)
triangle = graphs.select(|registered| registered.triangle)
sky = graphs.select(|registered| registered.sky)

draw = |frame| {
    mvp = mvp_for(frame.aspect_ratio)
    triangle.draw(mvp)
}
```

Both selections retain the full collection type. A frame may return either
selection's submission even when their uniform shapes differ. A submission
from a differently typed collection fails at `Game.new`.

The current `.Graphs` record builder supports two fields. A flat record with
three or more fields fails because the intermediate `Try(Graphs(...), ...)`
has no `to_graphs` method. Nest validated two-field collections for larger
collections; selections can follow nested fields, such as
`graphs.select(|registered| registered.pair.triangle)`.

`RenderGraphDesc`, `RenderGraphLower`, and `RenderGraphValidate` port the
description, lowering, and validation of `crates/render-graph`. The platform
covers graphics draws with one uniform block each; textures, buffers, push
blocks, compute, and scopes keep their description types but have no builders
yet. `roc test platform/RenderGraphValidate.roc` and
`roc test platform/RenderGraphLower.roc` run their expectations.
`examples/basic-triangle/two_draws.roc` is a two-node fixture whose
expectations `roc test` runs.

`Stdout`, `Stderr`, and `Stdin` are also exposed. `ShaderReflection` exposes
the reflection schema, the logical shader value types, and the GPU byte
packing helpers that generated Roc shader modules use.

`Game.Init` and `Game.Frame` are nominal records defined directly in
`platform/Game.roc`. The app returns `Game.Init` from `init!`, and the host
passes `Game.Frame` to `draw`. An app can write the initialization record
literally under its `Game.Init` annotation; no separate wrapper is needed.
The generated host glue names these types `GameInit` and `GameFrame`.
Nominal types keep those Rust names stable when fields change; anonymous
records instead receive names containing a structural hash. `Graphs.Draw`
and the types it holds are nominal for the same reason.

`Game.HostConfig` holds the static host configuration. `Game.new` builds it
from `Graphs.definitions`, which returns the collection's validated graph
definitions. The platform provides this configuration through `roc_config`;
the generated Rust type is `GameHostConfig`.

`src/game.rs` turns a `Draw` into live resources: a `RuntimeShader` per
pipeline from the app's SPIR-V and reflection JSON, a byte-sized uniform
buffer per uniform, and one erased draw node per draw. It builds and prepares
a Rust render graph from those nodes and executes it each frame with the
bundle's values. The host has no shaders of its own. A bundle whose structure
differs from the one setup built from stops the frame loop with an error.

`mltrs shaders compile` generates the app side when a project has `main.roc`
(or passes `--language roc`): typed logical values, reflection, SPIR-V and
reflection JSON imports, a `shader` record per graphics shader, and a
`<Type>.to_bytes` packer plus `<Type>.gpu_size` per struct with a GPU layout.
See [`../docs/roc_shader_codegen.md`](../docs/roc_shader_codegen.md).

## Usage

```bash
just roc-platform build                      # build the host archive
just roc-platform run                        # run examples/basic-triangle/main.roc
just roc-platform exe                        # build a standalone executable
just roc-platform test                       # build and run every example headlessly
just roc-platform bundle                     # bundle the platform into dist/
just roc-platform bundle-test                # prove the bundle runs from a URL
just roc-platform stubs                      # regenerate the committed link inputs
just roc-platform licenses                   # regenerate platform/LICENSES
just roc-platform glue                       # regenerate src/roc_platform_abi.rs
just roc-platform shaders                    # regenerate every example's Roc shader modules and SPIR-V
just roc-platform shaders basic-triangle     # regenerate one example's Roc shader modules and SPIR-V
```

Each Roc example lives in `examples/<name>/`, with `main.roc`,
`shaders/source/`, `shaders/compiled/`, and `Generated/`.
`run` and `exe` accept the directory name (default `basic-triangle`); `exe`
writes `examples/<name>/main`. Files beside the example are fixtures,
not runnable examples, and `ci/all_tests.sh` runs them:

- `invalid_graph.roc` must fail `roc check` with the validation message.
- `invalid_values.roc`, `invalid_selected_values.roc`,
  `invalid_extra_values.roc`, and `invalid_value_type.roc` must fail with a
  type mismatch at `draw`, covering missing, extra, and mistyped frame values.
- `invalid_vertex_count.roc` rejects a shader that requires vertices when
  constructing a vertex-count pipeline.
- `invalid_game.roc` rejects a draw callback whose collection type differs
  from the registered collection.
- `invalid_packer.roc` checks that packing rejects an incorrect byte length.
- `two_draws.roc`, `local_graphs.roc`, and `tuple_render_graphs.roc` must pass
  `roc test`. They cover tuple packing order, inferred graph selection,
  distinct graph ordinals, all tuple arities, mixed uniform types, nested and
  empty render graphs, and retention of each graph's own packer.

Constant evaluation packs the mesh and the frame at compile time. A large
mesh slows `roc check`; converted assets belong in imported files.

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
- `platform/Game.roc` — the game an app provides and its nominal `Init` and
  `Frame` records.
- `platform/Host.roc` — the hosted-effect boundary the modules above wrap.
- `platform/ShaderReflection.roc` — the reflection schema, logical shader
  value types, and GPU byte packing helpers that generated Roc shader modules
  import.
- `platform/RenderGraph.roc` — opaque draw declarations and typed tuple packers.
- `platform/Graphs.roc` — graph collections, selections, and the `Draw` bundle.
- `platform/ValidatedRenderGraph.roc` — internal validated graph definitions.
- `platform/RenderGraphDesc.roc`, `RenderGraphLower.roc`,
  `RenderGraphValidate.roc` — the description, lowering, and validation port.
- `platform/NOTICE`, `platform/LICENSES/` — the licence texts the archive
  ships. `ci/licenses.sh` regenerates `LICENSES/`.
- `platform/targets/x64glibc/` — the link inputs. Committed except `libhost.a`.
- `stubs/generate.sh` — regenerates those link inputs and the licence texts.
- `ci/roc_commit.txt` — the upstream roc commit CI builds.
- `ci/expected_sdl_backends.txt` — the SDL backend set CI asserts.
- `stubs/*_stub.s` — the generated stub sources, committed for review.
- `stubs/forward/` — the C sources behind `libc_forward.a`.
- `src/lib.rs` — allocators, hosted-effect implementations, and `rust_main`.
- `src/game.rs` — the `Game` impl that renders the app's `Draw` bundle.
- `src/roc_platform_abi.rs` — generated by `roc glue rust_glue`. Do not edit.
- `examples/basic-triangle/` — the Roc app, shader sources, compiled SPIR-V
  and reflection JSON, generated Roc modules, and the invalid-graph fixture.

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
