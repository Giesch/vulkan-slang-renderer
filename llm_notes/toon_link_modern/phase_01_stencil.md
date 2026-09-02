# Phase 1: renderer stencil support

Detailed plan for step 1 of [toon_link_modern](../toon_link_modern.md).
Goal: the renderer can allocate a stencil-capable depth buffer and bake
stencil test/write state into pipelines. The feature is opt-in per game and
off by default, so no existing example changes behavior.

## Current state (verified against the code)

- `find_depth_format` (`crates/renderer/src/renderer.rs:5070`) already lists
  the stencil formats as fallbacks: `[D32_SFLOAT, D32_SFLOAT_S8_UINT,
  D24_UNORM_S8_UINT]`. Every desktop driver supports `D32_SFLOAT`, so the
  stencil formats are unreachable today.
- `find_depth_format` runs twice: once in `Renderer::init` (renderer.rs:375,
  stored as `self.depth_format`), and again inside
  `create_depth_buffer_image` (renderer.rs:5006). The two calls agree only by
  construction.
- `create_depth_buffer_image` (renderer.rs:4996) creates the image with
  `DEPTH_STENCIL_ATTACHMENT` usage but creates the view with
  `AspectFlags::DEPTH` only (renderer.rs:5025).
- `create_graphics_pipeline` (renderer.rs:4052) hardcodes
  `.stencil_test_enable(false)` (:4136) and sets
  `depth_attachment_format` but no `stencil_attachment_format` (:4139-4141).
- The main pass binds a depth attachment only (renderer.rs:2092-2102). The
  clear value already zeroes stencil (:2062-2066).
- Stencil aspects are already handled where formats are inspected:
  `has_stencil_component` (:5083), the per-frame depth barrier (:2013-2014),
  and `transition_image_layout` (:4863-4865).
- `Renderer::init` has one caller: `Game::run` at
  `crates/mltrs/src/game/traits.rs:120`.
- Unaffected paths: the picking pipeline passes `None` as depth format
  (renderer.rs:1325); the egui overlay is a separate color-only pass recorded
  by `egui_ash_renderer` (:2384-2414); shader hot reload re-runs
  `create_graphics_pipeline` with the stored `raster_state` (:2934), so a new
  `RasterState` field carries through with no extra work.

## Design

### Consistency rule that shapes the change

Dynamic rendering requires the bound pipeline's `stencilAttachmentFormat` to
match the render pass instance's stencil attachment: the real format when one
is bound, `UNDEFINED` when none is. There is no per-pipeline choice. So one
predicate, `has_stencil_component(self.depth_format)`, must drive both sides:

- every main-pass pipeline declares `stencil_attachment_format` iff the depth
  format has stencil, and
- `begin_rendering` binds a stencil attachment iff the depth format has
  stencil.

All main-pass pipelines flow through `create_graphics_pipeline` with
`Some(self.depth_format)`, so both sides key off the format argument that is
already passed in.

### Static reference, no dynamic state

The example needs one reference value (1). Bake compare mask, write mask, and
reference into the pipeline via `vk::StencilOpState`. No dynamic stencil
state, no new command recording per draw.

## Changes

### 1. `crates/mltrs/src/game/traits.rs`

- Add to `trait Game`, next to `max_msaa_samples` (:64-68):

  ```rust
  /// Override to request a stencil-capable depth buffer.
  fn needs_stencil() -> bool {
      false
  }
  ```

- Pass `Self::needs_stencil()` into `Renderer::init` (:120).

### 2. Format selection (`crates/renderer/src/renderer.rs`)

- `Renderer::init` (:262) gains `needs_stencil: bool` after
  `max_msaa_samples`.
- `find_depth_format` (:5070) gains `needs_stencil: bool`. When true, the
  candidate list is `[D32_SFLOAT_S8_UINT, D24_UNORM_S8_UINT]`. When false,
  the list is unchanged. The existing
  `.expect("no supported depth format available")` stays the failure mode.
- `create_depth_buffer_image` (:4996): replace the internal
  `find_depth_format` call (:5006) with a `depth_format: vk::Format`
  parameter; drop the now-unused `instance` and `physical_device` parameters.
  Callers pass the single source of truth: the init path passes the format
  found at :375, the resize path (:2801) passes `self.depth_format`. This
  removes the duplicated format derivation.
- View aspect (:5025): `DEPTH | STENCIL` when
  `has_stencil_component(depth_format)`, else `DEPTH`. A view bound as a
  combined depth-stencil attachment must include both aspects.
- No barrier changes: `transition_image_layout` and the per-frame depth
  barrier already widen the aspect mask for stencil formats.

### 3. `StencilMode` (`crates/renderer/src/renderer/pipeline.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StencilMode {
    /// No stencil test, no stencil writes.
    Disabled,
    /// Every fragment that passes the depth test writes `reference`.
    Write { reference: u8 },
    /// Draw only where the stencil buffer equals `reference`. No writes.
    TestEqual { reference: u8 },
}
```

> **Superseded during implementation.** A public enum lets a game bake an
> enabled stencil mode without `needs_stencil()`; Vulkan treats a stencil
> test with no stencil attachment as always passing and discards the
> writes, so the mistake fails silently. The shipped API closes that hole:
> `StencilMode` is an opaque struct, `StencilMode::DISABLED` is the only
> public constructor, and `Renderer::stencil_support()` returns
> `Option<StencilSupport>` (`Some` iff the depth format has stencil). The
> token's `write(reference)` and `test_equal(reference)` methods build the
> enabled modes, so an enabled mode proves the attachment exists.

- Add `pub stencil: StencilMode` to `RasterState` (:282); `Disabled` in
  `Default` (:291).
- The type re-exports automatically through `pub use pipeline::*`
  (renderer.rs:53-54), so examples reach it as `mltrs::renderer::StencilMode`.

### 4. Pipeline baking (`crates/renderer/src/renderer.rs`)

- Add a pure helper next to `vk_blend_state` (:4027), same testable shape as
  `vk_cull_mode` / `vk_depth_compare` / `vk_color_write_mask`:

  ```rust
  /// Returns (stencil_test_enable, front-and-back StencilOpState).
  fn vk_stencil_state(stencil: StencilMode) -> (bool, vk::StencilOpState)
  ```

  - `Disabled` → `(false, StencilOpState::default())`.
  - `Write { reference }` → compare `ALWAYS`, `pass_op: REPLACE`,
    `fail_op: KEEP`, `depth_fail_op: KEEP`, compare/write mask `0xFF`.
    `depth_fail_op` must stay `KEEP`: a fragment behind opaque geometry must
    not mark stencil, that is the whole point of z-testing the mask pass.
  - `TestEqual { reference }` → compare `EQUAL`, all ops `KEEP`,
    compare mask `0xFF`, write mask `0`.
- `create_graphics_pipeline` (:4130-4136): feed the helper's result into
  `stencil_test_enable`, `.front()`, and `.back()` on
  `PipelineDepthStencilStateCreateInfo`.
- Rendering info (:4139-4141): when `depth_format` is `Some(f)` and
  `has_stencil_component(f)`, add `.stencil_attachment_format(f)`. The
  picking pipeline passes `None` and is untouched.

### 5. Begin rendering (`crates/renderer/src/renderer.rs:2092-2102`)

When `has_stencil_component(self.depth_format)`, add
`.stencil_attachment(&depth_attachment)` to the `RenderingInfo`. The same
`RenderingAttachmentInfo` serves both slots: same image view and layout,
`load_op` CLEAR (the clear value already zeroes stencil), `store_op`
DONT_CARE. The egui pass binds no depth or stencil attachment and does not
change.

### 6. Ripple: `examples/toon_link/src/main.rs:703`

`raster_state()` lists every `RasterState` field so that a new field is a
compile error (comment at :701). Add `stencil: StencilMode::Disabled` and the
import. This is the only edit to toon_link in the whole project.

`multi_mesh` and the picking path construct `RasterState` with
`..Default::default()` and need no change.

## Tests

- Extend `raster_state_default_matches_original_hardcoded_pipeline`
  (renderer.rs:6260) to assert `vk_stencil_state(default.stencil) ==
  (false, vk::StencilOpState::default())`.
- Add `vk_stencil_state` unit tests beside the `vk_color_write_mask` tests
  (:6240): Write bakes REPLACE with KEEP on depth-fail, TestEqual bakes EQUAL
  with write mask 0.

## Verification

- `cargo check --workspace --all-targets`, then `just lint`, `cargo fmt`.
- `cargo test -p mltrs-renderer` for the pure-helper tests.
- `just sweep`. Command recording changes, so the sweep is mandatory
  (docs/testing.md). `needs_stencil` is false for every existing example, so
  the depth format stays `D32_SFLOAT` and the sweep output must not move. A
  diff here means the opt-out path is not a no-op — stop and fix.
- The stencil-enabled path gets live validation coverage in phase 3, when
  `toon_link_modern` opts in and the sweep runs it.

## Risks

1. Format availability. Desktop Vulkan guarantees that at least one of
   `D32_SFLOAT_S8_UINT` and `D24_UNORM_S8_UINT` supports depth-stencil
   attachment. The existing `expect` in `find_depth_format` fails loudly if
   neither does.
2. Depth precision. An opted-in game can land on `D24_UNORM_S8_UINT` where
   the driver lacks `D32_SFLOAT_S8_UINT`. Toon Link's depth range (near 0.1,
   far 20.0) is far inside 24-bit precision.
3. MSAA interaction. The depth image is created at `msaa_samples`; the
   stencil aspect shares the allocation and needs no separate handling.
