# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`mltrs` is a Rust Vulkan renderer with Slang shader integration. Provides type-safe,
reflection-based interfaces for Slang shaders with hot-reloading capabilities.

This is a cargo workspace. An outside project can depend on `mltrs`, run the `mltrs`
CLI against its own `.slang` files, and get generated bindings that compile in its own
crate — the examples are the first consumers of exactly that workflow.

## Workspace Layout

| path | package | what |
|---|---|---|
| `crates/renderer` | `mltrs-renderer` | Vulkan engine, slang compilation, reflection types, editor widgets |
| `crates/mltrs` | `mltrs` | consumer-facing facade: `Game` trait, app loop, asset helpers |
| `crates/cli` | `mltrs-cli` | slang → Rust codegen; the binary is named `mltrs` |
| `crates/convert-link` | `convert-link` | unrelated Wind Waker asset converter |
| `examples/<name>` | `<name>` | one crate per example, each owning its shaders, assets and generated code |

Dependency direction is `mltrs-renderer ← mltrs ← examples`, with `mltrs-cli` depending
on `mltrs-renderer` only. Nothing depends on `mltrs-cli` — a consumer runs it as a tool.

## Build Commands

```bash
cargo check --workspace --all-targets   # first pass after any rust change
just shaders                            # regenerate ALL examples' bindings
just shaders EXAMPLE                    # regenerate just one
just test                               # snapshot tests via insta
cargo insta test -p mltrs-cli --accept  # accept modified snapshots
just lint                               # clippy, debug + release, warnings denied
timeout 3 just dev EXAMPLE              # boot an example, check for validation errors
just vendor-shaders                     # re-sync engine slang modules into every example
cat examples/EXAMPLE/shaders/compiled/EXAMPLE.json | jq '.'   # inspect reflection json
```

### After changes
- Always run `just shaders EXAMPLE` after modifying that example's `.slang` files.
- Always use `cargo check --workspace --all-targets` when changing rust files as a first pass.
  NOTE `--all-targets`, not just `--workspace`: `--workspace` means "all members" and
  silently skips example *targets*, so a broken example passes. `--all-targets` covers
  examples, benches and `#[cfg(test)]` code. Same distinction applies to clippy
  (`just lint` uses both).
- Always use `just test` when changing `crates/cli/src/build_tasks.rs` or the templates.
- Run `cargo fmt` after a set of rust file changes are complete.

## Shader System

**Workflow (same for examples and outside consumers):**
1. `mltrs shaders init` seeds `shaders/source/` with the engine slang modules
2. Create/edit `shaders/source/*.shader.slang` or `*.compute.slang`
3. `mltrs shaders compile` writes `shaders/compiled/` (SPIR-V + reflection JSON) and
   `src/generated/` (Rust bindings)
4. `mod generated;` in `main.rs`, then use `generated::shader_atlas::…`

`shaders/source` and `shaders/compiled` are fixed conventions relative to the crate.
Generated code locates them via `env!("CARGO_MANIFEST_DIR")`, which expands in the
*consuming* crate — that is what lets hot reload and `include_bytes!` follow each crate.

Compile removes stale outputs it previously wrote, so deleting a `.slang` file no longer
leaves orphaned SPIR-V or generated modules behind.

**Generated code includes:**
- Vertex input structs with Vulkan format annotations
- Parameter block structs (Std140 for uniforms, Std430 for storage)
- Type-safe `Resources` struct and `pipeline_config()` builder

**Consumers need these deps** alongside `mltrs`, because generated bindings name them
directly: `ash`, `serde`, `serde_json`, `glam`.

### CLI

```
mltrs shaders compile [--crate-dir DIR] [--source-dir DIR] [--compiled-dir DIR]
                      [--rust-dir DIR] [--import-root PATH] [--no-rust]
mltrs shaders init    [--dir DIR] [--force]
```

`--import-root` defaults to `mltrs`. The `crate` value is used only by the check_crate
fixture, which must not depend on `mltrs` (that would drag sdl3 and slang into a
`cargo check` fixture).

## Game Trait

Implement this to create an application:
```rust
pub trait Game {
    type EditState: for<'a> Facet<'a> + 'static;

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>;
    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError>;

    // Optional overrides (have default implementations):
    fn update(&mut self) {}
    fn input(&mut self, _input: Input) {}
    fn window_title() -> &'static str;
    fn initial_window_size() -> (u32, u32);
    fn frame_delay(&self) -> Duration;
    fn render_scale() -> Option<f32>;
    fn max_msaa_samples() -> MaxMSAASamples;
    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)>;
    fn run() -> anyhow::Result<()>;  // Entry point
}
```

## Type-Safe Resource Handles

- `PipelineHandle<DrawIndexed>` / `PipelineHandle<DrawVertexCount>`
- `UniformBufferHandle<T>` - Uniform buffers
- `StorageBufferHandle<T>` - Storage buffers
- `TextureHandle` - Textures

## Key Constants (crates/renderer/src/renderer.rs)

- `ENABLE_VALIDATION` - Vulkan validation layers (on in debug builds)
- `ENABLE_SAMPLE_SHADING` - MSAA (off by default)
- `MAX_FRAMES_IN_FLIGHT` - 2 (double buffering)

## Examples

Run with `just dev NAME`:
- basic_triangle - Minimal vertex/index buffer
- depth_texture - Depth testing and textures
- dragon - Dragon curve fractal
- gpu_picking - GPU-side object picking
- koch_curve - Koch curve fractal
- multi_mesh - Shared meshes, raster and blend states
- particles - Compute-driven particle system
- ray_marching - Ray marching SDF rendering
- sdf_2d - SDF rendering (fullscreen quad)
- serenity_crt - CRT shader effect
- space_invaders - Complete game example
- sprite_batch - Sprite rendering with storage buffers
- suzanne - KTX2 mipmapped textures
- viking_room - 3D model loading
- watercolor - Fluid simulation across several compute passes

## Testing

Uses insta for snapshot testing of generated code. Snapshots live in
`crates/cli/src/snapshots/` and are prefixed `mltrs_cli__build_tasks__tests__`.

```bash
just test                  # Non-interactive (CI)
just insta                 # Interactive review
cargo insta test -p mltrs-cli --accept   # Re-run and accept every changed snapshot
```

NOTE `cargo insta accept` on its own does nothing after `just test`: that recipe
sets `INSTA_UPDATE=no`, so no `.snap.new` files are written for it to review.
Use `cargo insta test -p mltrs-cli --accept`, which re-runs the tests and writes the
snapshots in one step. Review the diffs `just test` prints before accepting.

Codegen is snapshotted against a curated corpus in `crates/cli/fixtures/shaders/`, not
against the real examples — it covers every codegen path without double maintenance.
`crates/cli/fixtures/check_crate` is a hand-mirrored stub of the codegen-facing API that
the alignment test compiles generated code against; **keep it in sync whenever a template
or an atlas trait changes**, or that test fails with a missing-trait-item error. Note it
stubs the API rather than using it, so it cannot catch *visibility* mistakes.

## Key Dependencies

- ash - Vulkan bindings
- glam - Math (Vec3, Mat4, etc.)
- sdl3 - Window/input
- shader-slang - Slang compiler (statically linked)
- askama - Template engine for code generation
- clap - CLI argument parsing
