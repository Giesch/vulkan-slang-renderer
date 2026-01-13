# Facet-based Editor UI API

This document describes the automatic editor UI system built on the `facet` crate for reflection.

## Overview

The facet_egui API allows games to expose editable state that gets automatically rendered as an egui window. You define a struct with `#[derive(Facet)]`, specify it as your `EditState` type, and the engine generates editable UI widgets for each field.

Key features:
- Automatic UI generation from struct reflection
- Live-updates: changes in egui immediately update your game state
- Only enabled in debug builds (`cfg!(debug_assertions)`)
- Supports primitives and glam math types

## Quick Start

```rust
use facet::Facet;
use vulkan_slang_renderer::game::Game;

#[derive(Facet, Default)]
struct MyEditState {
    speed: f32,
    show_hitboxes: bool,
}

struct MyGame {
    edit_state: MyEditState,
}

impl Game for MyGame {
    type EditState = MyEditState;

    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("My Game Editor", &mut self.edit_state))
    }

    // ... other Game methods
}
```

This creates a "My Game Editor" window with editable fields for `speed` and `show_hitboxes`.

## API Reference

### Associated Type: `EditState`

```rust
pub trait Game {
    type EditState: for<'a> Facet<'a> + 'static;
    // ...
}
```

The type must implement `Facet`. Use `#[derive(Facet)]` on a struct. For games that don't need an editor UI, use `type EditState = ();`.

### Method: `editor_ui()`

```rust
fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
    None  // default implementation
}
```

Return `Some((window_name, &mut self.edit_state))` to enable the editor window with the specified title, or `None` to disable it for a particular frame (or always, if you never override the default).

## Supported Field Types

### Primitives

| Type | Widget |
|------|--------|
| `f32`, `f64` | Drag value (speed: 0.1) |
| `i32`, `i64`, `u32`, `u64` | Drag value |
| `bool` | Checkbox |

### Glam Types

| Type | Widget |
|------|--------|
| `Vec2` | x/y drag values |
| `Vec3`, `Vec3A` | x/y/z drag values |
| `Vec4` | x/y/z/w drag values |
| `Quat` | Euler angles (x/y/z rotation) |
| `Mat4` | 4x4 grid of drag values |

### Nested Structs

Structs containing other `Facet` structs render as collapsible sections.

### Editor Types

| Type | Widget |
|------|--------|
| `Slider<f32>` | Slider with min/max bounds |

The `Slider` type (from `vulkan_slang_renderer::editor`) allows specifying value bounds:

```rust
use vulkan_slang_renderer::editor::Slider;

#[derive(Facet, Default)]
struct MyEditState {
    volume: Slider<f32>,  // renders as a bounded slider
}

// Create with bounds:
let edit_state = MyEditState {
    volume: Slider::new(0.5, 0.0, 1.0),  // value, min, max
};
```

## Full Example

```rust
use facet::Facet;
use glam::{Vec3, Quat};
use vulkan_slang_renderer::game::{Game, Input};
use vulkan_slang_renderer::renderer::{DrawError, FrameRenderer, Renderer};

#[derive(Facet, Default)]
struct CameraEdit {
    position: Vec3,
    rotation: Quat,
    fov: f32,
}

#[derive(Facet, Default)]
struct GameEditState {
    camera: CameraEdit,
    time_scale: f32,
    paused: bool,
    entity_count: u32,
}

struct MyGame {
    edit_state: GameEditState,
    // ... other game state
}

impl Game for MyGame {
    type EditState = GameEditState;

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self> {
        Ok(Self {
            edit_state: GameEditState {
                camera: CameraEdit {
                    position: Vec3::new(0.0, 5.0, -10.0),
                    rotation: Quat::IDENTITY,
                    fov: 60.0,
                },
                time_scale: 1.0,
                paused: false,
                entity_count: 0,
            },
        })
    }

    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("Game Editor", &mut self.edit_state))
    }

    fn update(&mut self) {
        if !self.edit_state.paused {
            // Use self.edit_state.time_scale, self.edit_state.camera.position, etc.
            // Values are live-updated by the egui window
        }
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        // Render using edit_state.camera settings
        Ok(())
    }
}
```

## Notes

- **Debug builds only**: egui is initialized only when `cfg!(debug_assertions)` is true
- **Window title**: The window name is specified by the game in `editor_ui()` return value
- **Return value**: `render_facet_ui()` returns `true` if any field was modified
- **Unsupported types**: Fields with unsupported types display as "(TypeName)" labels
- **Unit type**: Use `type EditState = ();` for games that don't need an editor UI

## Implementation Details

The `render_facet_ui` function in `src/renderer/facet_egui.rs`:
1. Gets the type's `Shape` via `T::SHAPE`
2. For structs, iterates over fields using `StructType::fields`
3. Uses `shape.type_identifier` to match types by name
4. Renders appropriate egui widgets based on type
5. Updates values in-place via pointer manipulation
