# Toon Link rendering modes

The `toon_link` example compares two independent renderers in one application:

- **GameCube** is the startup default. It preserves the GX TEV/XF material interpretation, UNORM texture handling, destination-alpha facial-feature compositing, controls, diagnostics, and draw ordering.
- **Modern** directly shades normalized interpolated world normals per pixel in linear color space. It has its own material preparation, textures, pipelines, parameters, and draw submission. It does not use `tev_pack`, TEV/XF evaluation, destination-alpha compositing, or the classic `ChannelPerPixel` diagnostic.

The example-local host initializes and retains both games, owns the shared elapsed time, and delegates exactly one draw each frame. Switching keeps the same window, camera, model pose, and continuous spin. Each mode retains its own session-local settings while inactive. Restarting restores defaults. The debug UI is not present in release builds.

Converted assets are machine-local at `examples/toon_link/assets/link/converted`. Create them with:

```bash
just toon_link extract-link
just toon_link convert-link
```

## Debug controls

Use the top-level **GameCube / Modern** radio buttons to select the active renderer. Initially, GameCube is selected and expanded, and Modern is collapsed. Changing the selection opens the selected mode's section and closes the other. Both headers remain available for manual expansion or collapse; manual choices persist until the next mode change. The nested GameCube and Modern sections keep their settings independently; inactive settings do not affect the selected draw.

Modern starts with the **Analytic** ramp. Its controls are:

- **Ramp**: Analytic or Texture. Texture samples the existing two-dimensional `toonex` data texture.
- **Band center**: moves the analytic light/shadow boundary.
- **Band softness**: changes analytic transition width. Zero is a defined hard step.
- **LUT ambient**: offsets only the texture-ramp lookup.
- **Shadow / Lit**: authored sRGB endpoint colors, decoded once to linear values before upload.
- **Secondary**: enables an additive secondary light.
- **Secondary tint / intensity**: authored sRGB tint and linear scale. Disabled and enabled-with-zero-intensity produce the same parameters and output.
- **Secondary azimuth / elevation**: radians in model space. The direction rotates with Link; azimuth is around model Y and elevation is above the model XZ plane.

Ramp-specific values remain retained when the other ramp is selected. Disabled secondary direction and tint values are retained but cannot affect rendering.

## Modern diagnostics

Modern exposes six shader outputs before normal target encoding and blending:

- **Final**: direct linear toon shading.
- **World Normals**: normalized world normal mapped with `normal * 0.5 + 0.5`.
- **UV0**: primary untransformed `(u, v, 0)` coordinates.
- **N·L**: clamped main-light cosine in grayscale.
- **Band Only**: the selected ramp's main weight in grayscale.
- **Albedo Only**: sampled linear base color, including pupil multiplication, without lighting.

Coverage discard, depth, culling, and stencil behavior remain active in every diagnostic. Transparent facial decals therefore blend the selected diagnostic color rather than becoming opaque rectangles. Returning to Final restores the retained shading settings.

## Color and facial-feature policy

Modern loads albedo and pupil textures as sRGB and the `toonex` ramp as UNORM data. Texture RGB is decoded by sampling exactly once; texture alpha remains coverage data. UI RGB is decoded once on the CPU. Normals, UVs, ramp values, and alpha are never color-decoded. Modern emits linear RGB to the sRGB target and does not apply the classic shader's compensating `srgbDecode`.

Modern validates its supported manifest roles, required texture references, batch/material mapping, `toonex` substitution, paired facial-feature geometry, and the supported pupil texture/UV transform before using them. Erase submissions are classified and validated but omitted because stencil is cleared each frame.

Facial masks write feature-specific stencil references only where nonzero alpha survives depth testing. Bangs draw after masks. Source-alpha feature passes test the matching stencil reference. Other opaque geometry, including the face, participates in depth occlusion. The implementation uses no GX destination-alpha compositing.

## Verification boundary

`just sweep` is intentionally unchanged. It starts `toon_link` in the default GameCube mode and provides GameCube recording, presentation, validation-layer, and teardown evidence only. A passing sweep is **not** Modern GPU or visual evidence.

After source changes, run the normal generated-code, Rust, lint, test, and unchanged sweep checks described in `AGENTS.md`. Separately verify Modern interactively with `just dev toon_link`:

- both ramps and every control, including endpoints;
- secondary disabled, enabled, and enabled at zero intensity;
- all six diagnostics and return to Final;
- front, profile, and rear portions of full rotations for pupil placement, feature edges, overlapping/far-side occlusion, bangs, body occlusion, geometry visibility, and the double-sided sleeve;
- repeated switching after inactive intervals, retained independent settings, and continuous pose/spin;
- GameCube startup and appearance after returning from Modern;
- startup and smaller usable window sizes, resize, minimize/restore, and shutdown;
- active-mode shader hot reload in each mode using temporary interface-preserving edits.

Record actual observations and any unperformed cases. Manual checks establish only the viewed model/camera cases, not arbitrary-scene correctness or golden-image equivalence.

## Shader hot reload limitation

Hot reload remains supported for the active mode. Renderer watcher events are consumed while only that frame's queued pipelines are recompiled, so an edit to an inactive mode can be missed. Switching modes alone does not recover it. Re-edit the shader while its mode is active or restart the application. Reliable inactive-pipeline reload is an engine follow-up.
