# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rust-based Vulkan renderer with Slang shader integration. Provides type-safe, reflection-based interfaces for Slang shaders with hot-reloading capabilities.

## Build Commands

```bash
cargo check --all    # Check source and examples for type errors
just shaders         # Generate shader bindings (MUST run after .slang changes)
just test            # Run tests (snapshot testing via insta)
cargo insta accept   # accept all modified snapshots
just lint            # Clippy with warnings as errors
```

### After changes
- Always run `just shaders` after modifying any `.slang` files to regenerate Rust bindings.
- Always use `cargo check --all` when changing rust files as a first pass
- Always use `just test` when making changes to shaders/build_tasks.rs

## Architecture

### Core Modules (src/)
- **app.rs** - Application event loop, SDL integration
- **game.rs** - Game trait definitions and input system
- **renderer.rs** - Main Vulkan rendering engine (~131KB)
- **shaders.rs** - Slang compilation interface
- **shader_watcher.rs** - Hot reload for shaders (debug builds only)
- **generated/** - Auto-generated shader bindings (don't edit manually)

### Shader System

**Workflow:**
1. Create/edit `shaders/source/*.shader.slang`
2. Run `just shaders` (sets `GENERATE_RUST_SOURCE=true`)
3. Generates: SPIR-V bytecode + reflection JSON + Rust bindings in `src/generated/`

**Generated code includes:**
- Vertex input structs with Vulkan format annotations
- Parameter block structs (Std140 for uniforms, Std430 for storage)
- Type-safe `Resources` struct and `pipeline_config()` builder

### Game Trait

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

### Type-Safe Resource Handles

- `PipelineHandle<DrawIndexed>` / `PipelineHandle<DrawVertexCount>`
- `UniformBufferHandle<T>` - Uniform buffers
- `StorageBufferHandle<T>` - Storage buffers
- `TextureHandle` - Textures

### Key Constants (src/renderer.rs)

- `ENABLE_VALIDATION` - Vulkan validation layers (on in debug builds)
- `ENABLE_SAMPLE_SHADING` - MSAA (off by default)
- `MAX_FRAMES_IN_FLIGHT` - 2 (double buffering)

## Examples

Run with `just dev NAME`:
- basic_triangle - Minimal vertex/index buffer
- depth_texture - Depth testing and textures
- dragon - Dragon curve fractal
- koch_curve - Koch curve fractal
- ray_marching - Ray marching SDF rendering
- sdf_2d - SDF rendering (fullscreen quad)
- serenity_crt - CRT shader effect
- space_invaders - Complete game example
- sprite_batch - Sprite rendering with storage buffers
- viking_room - 3D model loading

## Testing

Uses insta for snapshot testing of generated code:
```bash
just test           # Non-interactive (CI)
just insta          # Interactive review
```

## Key Dependencies

- ash - Vulkan bindings
- glam - Math (Vec3, Mat4, etc.)
- sdl3 - Window/input
- shader-slang - Slang compiler
- askama - Template engine for code generation
