# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Where the docs are

- **`docs/`** — current reference material, kept up to date. Trust it, and update
  it when you change what it describes.
- **`llm_notes/`** — historical plans and phase records, written before or during
  a piece of work. **Treat as possibly out of date**: much of it is a snapshot of
  what was believed at the time, some of it was superseded by the work it
  describes, and a few claims turned out to be wrong (those are annotated in
  place rather than deleted, so the record stays honest). Useful for _why_ a
  thing is the way it is; verify against the code before acting on it.

## Workspace layout

- `crates/slang-reflection` (package `mltrs-slang-reflection`) — the slang
  compile/reflect machinery and the `json` reflection format. **The only crate
  in the workspace that may depend on `shader-slang`**, and no `shader_slang`
  type appears in its public API: `OptimizationLevel` and `ShaderStage` are
  ours, not re-exports. Keep it free of `ash`, `vk-mem` and `sdl3` — that is
  what lets `mltrs shaders compile` build without a graphics stack.
- `crates/renderer` (package `mltrs-renderer`) — the renderer, editor widgets,
  env_config, and the shader watcher. `shaders.rs` is a façade over
  `mltrs-slang-reflection` plus the vulkan-facing pieces: the `atlas` traits,
  and the `ReflectionLayoutBindings` / `ToVk` / `VkCreate` / `SpvBytes`
  extension traits (the reflected types are defined in the other crate, so
  these cannot be inherent impls).
- `crates/mltrs` — the consumer-facing engine crate: `Game` trait, app loop,
  asset helpers; re-exports the renderer modules.
- `crates/cli` (package `mltrs-cli`, binary `mltrs`) — shader codegen
  (`mltrs shaders compile`) and project seeding (`mltrs shaders init`);
  owns the askama templates, the vendored engine slang modules, and the
  snapshot-test fixtures. Depends on `mltrs-slang-reflection`, _not_ the
  renderer — keep it that way.
- `crates/gx` — GameCube manifest schema shared by `convert-link` and the
  `toon_link` example.
- `crates/convert-link` (binary `convert_link`) — Wind Waker asset converter.
- `examples/<name>/` — one crate per example, each with its own
  `shaders/source/`, committed `shaders/compiled/` + `src/generated/`
  bindings, and its own assets. The examples are the first consumers of the
  `mltrs` CLI workflow.

Keep the root justfile for workspace-wide tasks only (`shaders`, `sweep`,
`test`, `lint`, `dev`, `pre-commit`); per-example recipes live in the example's
own justfile (see `examples/CLAUDE.md`).

## Build Commands

`just --list` shows every workspace recipe and every example's own recipes.

```bash
cat examples/EXAMPLE/shaders/compiled/EXAMPLE.json | jq '.' # inspect reflection json
```

### After changes

- Always run `just shaders EXAMPLE` after modifying an example's `.slang`
  files (`just shaders` regenerates all of them).
- Always run `just EXAMPLE textures` after adding or replacing an example's
  source image, and commit the regenerated `.ktx2` alongside it. See
  [`docs/textures.md`](docs/textures.md).
- Always use `cargo check --workspace --all-targets` when changing rust files
  as a first pass. NOTE both flags: `--workspace` covers every member crate
  (each example is one), `--all-targets` covers tests, benches and
  `#[cfg(test)]` code. Same for clippy (`just lint` uses both).
- Always use `just test` when making changes to `crates/cli/src/build_tasks.rs`
  or `crates/cli/templates/`.
- Run `cargo fmt` after a set of rust file changes are complete
- Never edit `examples/*/src/generated/` by hand — `just shaders` regenerates it.
- Never call `std::env::var` outside `crates/renderer/src/env_config.rs`.
  Every variable is parsed once at startup into `EnvConfig` and passed down
  from there.

## Shader System

**Workflow:**

1. Create/edit `examples/<name>/shaders/source/*.shader.slang`
2. Run `just shaders <name>`
3. Generates: SPIR-V bytecode + reflection JSON in `shaders/compiled/`, and
   Rust bindings in `src/generated/` — all inside the example's crate

The engine slang modules (`addr`, `mvp`, `projection`, `fullscreen_triangle`,
`super_sample`) are vendored in `crates/cli/vendor/`: a top-level `mltrs.slang`
prelude re-exports the modules under `vendor/mltrs/`, with every declaration
inside `namespace mltrs`. Shaders write `import mltrs;` and qualified
references (`mltrs::MVPMatrices`, `mltrs::Addr<T>`, …). `just vendor-shaders`
re-seeds every example's copies (`shaders/source/mltrs.slang` +
`shaders/source/mltrs/`). Shared example modules (`ray_march.slang`, …) are
intentionally duplicated between examples and stay un-namespaced.

The namespace is ergonomics, not isolation: reflection records type names
unqualified into a flat map, so every public struct/enum name must still be
unique across all of a crate's `shaders/source/`.

**Consumer workflow** (what the examples model):

```bash
cargo add mltrs            # path/git dep for now
mltrs shaders init         # seeds shaders/source with mltrs.slang + mltrs/
# write shaders/source/my_game.shader.slang
mltrs shaders compile      # emits shaders/compiled + src/generated (imports `mltrs::…`)
# src/main.rs: mod generated; impl Game for MyGame; MyGame::run()
```

## Textures

See [`docs/textures.md`](docs/textures.md) before adding a texture or changing
the encode flags.

## Testing

`cargo insta test --workspace --accept` accepts every changed snapshot.

Run `just sweep` when a change could affect what the renderer records or destroys.

See [`docs/testing.md`](docs/testing.md) before writing a validation check or accepting a snapshot.
