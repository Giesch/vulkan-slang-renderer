# Facet-based Editor UI API

This document describes the automatic editor UI system built on the `facet` crate for reflection.

## Overview

The facet_egui API allows games to expose editable state that gets automatically rendered as an egui window. You define a struct with `#[derive(Facet)]`, specify it as your `EditState` type, and the engine generates editable UI widgets for each field.

Key features:
- Automatic UI generation from struct reflection
- Live-updates: changes in egui immediately update your game state
- Only enabled in debug builds (`cfg!(debug_assertions)`)
- Supports `Slider` for bounded f32 values and nested structs

## Quick Start

```rust
use facet::Facet;
use vulkan_slang_renderer::editor::Slider;
use vulkan_slang_renderer::game::Game;

#[derive(Facet)]
struct MyEditState {
    speed: Slider,
    brightness: Slider,
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

// Initialize with bounds:
let edit_state = MyEditState {
    speed: Slider::new(1.0, 0.0, 10.0),      // value, min, max
    brightness: Slider::new(0.5, 0.0, 1.0),
};
```

This creates a "My Game Editor" window with slider controls for `speed` (0-10) and `brightness` (0-1).

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

### Slider Type

| Type | Widget |
|------|--------|
| `Slider` | Slider with min/max bounds (f32) |

The `Slider` type (from `vulkan_slang_renderer::editor`) encodes value bounds:

```rust
use vulkan_slang_renderer::editor::Slider;

#[derive(Facet)]
struct MyEditState {
    volume: Slider,
    speed: Slider,
}

// Create with bounds:
let edit_state = MyEditState {
    volume: Slider::new(0.5, 0.0, 1.0),  // value, min, max
    speed: Slider::new(1.0, 0.0, 10.0),
};
```

### Nested Structs

Structs containing other `Facet` structs render as collapsible sections:

```rust
#[derive(Facet)]
struct AudioSettings {
    master_volume: Slider,
    music_volume: Slider,
    sfx_volume: Slider,
}

#[derive(Facet)]
struct GameSettings {
    audio: AudioSettings,  // Renders as a collapsible "audio" section
    brightness: Slider,
}
```

### Unsupported Types

Fields with unsupported types (bare primitives like `f32`, `bool`, or glam types like `Vec3`) are **silently skipped** and do not render any UI. Wrap numeric values in `Slider` to make them editable.

## Full Example

Based on the `serenity_crt` example:

```rust
use facet::Facet;
use vulkan_slang_renderer::editor::Slider;
use vulkan_slang_renderer::game::{Game, Input};
use vulkan_slang_renderer::renderer::{DrawError, FrameRenderer, Renderer};

#[derive(Facet)]
struct CRTSettings {
    scanline_intensity: Slider,
    scanline_count: Slider,
    brightness: Slider,
    contrast: Slider,
    bloom_intensity: Slider,
    bloom_threshold: Slider,
}

struct MyCRTGame {
    edit_state: CRTSettings,
    // ... other game state
}

impl Game for MyCRTGame {
    type EditState = CRTSettings;

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self> {
        Ok(Self {
            edit_state: CRTSettings {
                scanline_intensity: Slider::new(0.3, 0.0, 1.0),
                scanline_count: Slider::new(480.0, 100.0, 1080.0),
                brightness: Slider::new(1.0, 0.5, 2.0),
                contrast: Slider::new(1.0, 0.5, 2.0),
                bloom_intensity: Slider::new(0.15, 0.0, 1.0),
                bloom_threshold: Slider::new(0.7, 0.0, 1.0),
            },
        })
    }

    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("CRT Settings", &mut self.edit_state))
    }

    fn update(&mut self) {
        // Values are live-updated by the egui window
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        // Access slider values via .value field:
        let brightness = self.edit_state.brightness.value;
        let bloom = self.edit_state.bloom_intensity.value;
        // ... use in rendering
        Ok(())
    }
}
```

## Notes

- **Debug builds only**: egui is initialized only when `cfg!(debug_assertions)` is true
- **Window title**: The window name is specified by the game in `editor_ui()` return value
- **Return value**: `render_facet_ui()` returns `true` if any field was modified
- **Unsupported types**: Fields with unsupported types are silently skipped (not rendered)
- **Unit type**: Use `type EditState = ();` for games that don't need an editor UI
- **Accessing values**: Use `.value` to read the current value from a `Slider`

## Implementation Details

The `render_facet_ui` function in `src/renderer/facet_egui.rs`:
1. Gets the type's `Shape` via reflection
2. Classifies each field as either `Slider` or `Collapsing` (nested struct)
3. For `Slider`, renders an egui slider with the stored min/max bounds
4. For nested structs, renders a collapsing section and recursively processes fields
5. Unsupported field types are skipped (no UI rendered)
6. Returns `true` if any value was modified by user interaction
