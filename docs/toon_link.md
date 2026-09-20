# Toon Link rendering modes

The `toon_link` example compares two independent renderers in one application:

- **GameCube** is the startup default. It preserves the GX TEV/XF material interpretation, UNORM texture handling, destination-alpha facial-feature compositing, controls, diagnostics, and draw ordering.
- **Modern** directly shades normalized interpolated world normals per pixel in linear color space. It has its own material preparation, textures, pipelines, parameters, and draw submission. It does not use `tev_pack`, TEV/XF evaluation, destination-alpha compositing, or the classic `ChannelPerPixel` diagnostic.

The example-local host initializes and retains both games, owns the shared elapsed time, and delegates exactly one draw each frame. Switching keeps the same window, camera, model pose, continuous spin, and skeletal animation playback state. Each mode retains its own session-local settings while inactive. Restarting restores defaults. The debug UI is not present in release builds.

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

## Animation playback

Both modes play the converted Link BCK body animations (`assets/link/animations/converted/catalog.json`, see [`link_animations.md`](link_animations.md)) through one CPU player owned by the host and one shared GPU deformation module (`shaders/source/skinning.slang`) that runs before projection and before either mode's shading. Debug and release both start stopped in the bind pose; the playback controls exist only in the debug UI.

The **Animation** section sits above the mode sections:

- **Search**: case-insensitive substring over `archive/member`. Filtering never changes playback; the selected clip stays selected while its row is hidden. Clips whose labels collide are disambiguated with their catalog entry, resource, and hash.
- **Clip list**: selecting a different clip reads and validates its document on demand, then starts at frame 0 playing. Selecting the active clip does nothing. A clip that fails validation is reported with its identity and offending field; the previous clip, pose, and transport state are kept, and the report stays visible until a later pose-changing command succeeds.
- **Bind pose** clears the active clip and stops. **Play / Pause / Restart** resume, freeze, or restart at frame 0 playing; Play is disabled at a completed Once endpoint, where Restart begins again. **Scrub** sets a fractional frame in `[0, duration]` and pauses.
- **Speed**: 0.1–2× (default 1×). **Loop**: Source (the clip's own attribute, 0 → Once, 2 → Repeat), Repeat, or Once. Speed and loop survive clip changes and Bind pose.
- Readouts: clip, transport, frame, effective policy. Catalog or loading errors appear in red; a pose-evaluation diagnostic appears in amber.

Timing is 30 animation frames per second at 1×, independent of the render rate. Repeat wraps at the duration and keeps the overshoot; Once parks at `duration − 0.001` and stays stopped until Restart. A loop change takes effect at the next advance, so a completed Once clip switched to Repeat resumes from its retained frame on Play. The catalog's five zero-duration clips are valid static frame-0 poses: Play, Restart, and Scrub are disabled, and they are not the bind pose. The host advances the clock in update, then the UI buffers commands. The same frame's pre-draw step consumes those commands exactly once before evaluating and publishing the pose, so the first draw after a command shows the commanded frame without another clock advance.

If a later frame fails to evaluate (nonfinite sample, hierarchy overflow, or a scale-compensation divisor with magnitude at or below `1e-6`), the last valid pose and frame are kept, playback pauses, and the diagnostic names the clip and target frame until a later pose-changing command (select, bind pose, play, restart, or scrub) succeeds; accepting a command alone does not clear it before its requested pose succeeds. Automatic advancement, filtering, and mode switches never clear it, and a pending successful request cannot erase a later command's failure diagnostic. Mode switches apply no commands and therefore preserve playback exactly.

Deformation is `animated_world × inverse_bind` per joint applied to the bind-baked model-space vertices, with per-vertex skin records addressed by `SV_VertexID`. Joint composition follows the model's Maya scaling rule with per-joint scale compensation (`T · inverse(parent local scale) · R · S`), which the model manifest must export explicitly ([`link_model_metadata.md`](link_model_metadata.md)); a manifest without that metadata, or whose skin data fails validation, renders statically, and the Animation section shows the reason with its controls disabled. Normals use the cofactor (inverse-transpose direction) of the blended linear part when its Hadamard conditioning is at least `1e-3` and the plain linear transform otherwise; every normal-dependent stage in both modes normalizes safely, returning zero for vectors shorter than `1e-6`, so no NaN reaches shading. Facial features, materials, UVs, draw ordering, and spin are unchanged by playback.

Not supported: BTP/BTK face animations, crossfades, Basic/Softimage scaling rules, and non-unit bind scales. 30 fps is the NTSC viewer convention, not reproduced hardware pacing.

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

`just sweep` is intentionally unchanged. It starts `toon_link` in the default GameCube mode, stopped in the bind pose, and provides GameCube recording, presentation, validation-layer, and teardown evidence only. It exercises the identity-palette skinning path but no animated pose. A passing sweep is **not** Modern GPU, animation, or visual evidence.

After source changes, run the normal generated-code, Rust, lint, test, and unchanged sweep checks described in `AGENTS.md`. Separately verify Modern interactively with `just dev toon_link`:

- both ramps and every control, including endpoints;
- secondary disabled, enabled, and enabled at zero intensity;
- all six diagnostics and return to Final;
- front, profile, and rear portions of full rotations for pupil placement, feature edges, overlapping/far-side occlusion, bangs, body occlusion, geometry visibility, and the double-sided sleeve;
- repeated switching after inactive intervals, retained independent settings, and continuous pose/spin;
- GameCube startup and appearance after returning from Modern;
- startup and smaller usable window sizes, resize, minimize/restore, and shutdown;
- active-mode shader hot reload in each mode using temporary interface-preserving edits.

Animation-specific checks:

```bash
cargo test -p toon_link                                                     # asset-free sampler, clock, transport, host UI and normal-policy tests
cargo test -p toon_link bck_catalog_runtime_audit -- --ignored --nocapture  # real assets: every catalog BCK must be accepted
just toon_link test-skinning-gpu                                            # production shader deformation/normal oracle under lavapipe
```

Then verify playback interactively in both modes with `just dev toon_link`, recording clip identity, frame, loop policy, and mode for each case: bind pose in both modes (`visual-bind-both`); a fractional frame of a nontrivial clip in both modes (`visual-animated-fractional-both`); a paused clip across a mode switch (`visual-paused-mode-switch`); all five static clips (`visual-static-five`); return to bind pose (`visual-return-bind`); and the existing facial/material controls with a clip playing (`visual-facial-settings-regression`).

Record actual observations and any unperformed cases. Manual checks establish only the viewed model/camera cases, not arbitrary-scene correctness or golden-image equivalence.

## Shader hot reload limitation

Hot reload remains supported for the active mode. Renderer watcher events are consumed while only that frame's queued pipelines are recompiled, so an edit to an inactive mode can be missed. Switching modes alone does not recover it. Re-edit the shader while its mode is active or restart the application. Reliable inactive-pipeline reload is an engine follow-up.
