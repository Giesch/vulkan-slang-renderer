# Toon Link animation playback options

Research date: 2026-09-15. This is an architectural investigation, not an implemented feature or an approved implementation spec. Current reference documentation lives in [docs](../../docs/); these notes are a historical snapshot.

## Goal

Play the extracted Wind Waker Link animations in the Toon Link example, allow animation selection in the debug UI, and share playback and deformation between Modern and GameCube rendering modes.

## Recommendation

Start with **Rust evaluating animation tracks and the joint hierarchy, then Slang skinning vertices**. Put one animation player above the Modern/GameCube split and have both vertex shaders call a shared skinning function before their respective shading work.

The locally converted model has 42 joints and 1,754 vertices. A GPU animation sampler would introduce considerable complexity for a small workload. A 42-joint palette of full 4×4 f32 matrices is approximately 2.6 KiB, excluding separate normal-transform data.

## Existing assets and runtime

The locally converted catalog contains:

| Format | Clips | Purpose |
|---|---:|---|
| BCK | 594 | Joint translation, rotation, and scale |
| BTP | 458 | Stepped texture selection, including facial patterns |
| BTK | 383 | Texture-matrix transforms |

These are preservation-format tracks, not a GPU-ready or fully specified runtime playback format. Sampling, playback timing, and loop behavior remain runtime responsibilities. Nothing in the extraction work currently feeds the example's run path. See [link_animations.md:13-14](../../docs/link_animations.md#L13-L14), [link_animations.md:94-95](../../docs/link_animations.md#L94-L95), and [animation_manifest.rs:7-34](../../crates/gx/src/animation_manifest.rs#L7-L34).

Assets are gitignored and machine-local, generated with `just toon_link extract-link-animations` and `just toon_link convert-link-animations`. The catalog supports loading clips on demand rather than loading all 1,435 clip documents eagerly. See [justfile:92-103](../../examples/toon_link/justfile#L92-L103) and [animation_manifest.rs:106-116](../../crates/gx/src/animation_manifest.rs#L106-L116).

Both rendering modes currently draw static vertices baked into model-space bind pose. The converter emits a 42-joint skeleton and per-vertex skinning influences, but the runtime does not consume them. The skinning file stores up to four `(u8 joint, f32 weight)` influences per vertex, 20 bytes per vertex. See [pose.rs:44-99](../../crates/convert-link/src/pose.rs#L44-L99), [output.rs:276-284](../../crates/convert-link/src/output.rs#L276-L284), and [model_manifest.rs:540-553](../../crates/gx/src/model_manifest.rs#L540-L553).

## Playback approaches

| Approach | Rust / CPU | Slang / GPU | Assessment |
|---|---|---|---|
| CPU pose evaluation + vertex-shader skinning | Sample tracks, optionally blend poses, evaluate hierarchy, produce palette | Deform vertex positions and normals | Recommended: fits existing buffer APIs and supports straightforward CPU tests |
| CPU pose evaluation + CPU skinning | Evaluate pose and deform vertices | Render the deformed mesh | Useful as a correctness reference, but production playback needs a dynamic-mesh update path that the renderer does not currently expose |
| CPU pose evaluation + compute skinning | Produce palette | Compute writes deformed vertices; rendering consumes them | Useful when multiple passes reuse deformation; adds output-buffer and synchronization work |
| GPU pose evaluation + GPU skinning | Supply playback controls and clip/instance state | Sample packed tracks or baked poses, evaluate hierarchy, deform vertices | Potential future crowd-rendering option; requires GPU-oriented animation data and more complex shader logic |

Current meshes use one-shot device-local vertex/index uploads, while per-frame storage/immutable buffer writes and vertex-stage buffer reads already exist. See [renderer.rs:1298-1336](../../crates/renderer/src/renderer.rs#L1298-L1336), [renderer.rs:5704-5734](../../crates/renderer/src/renderer.rs#L5704-L5734), and [gpu_picking.shader.slang:15](../../examples/gpu_picking/shaders/source/gpu_picking.shader.slang#L15).

GPU sampling is not forbidden by the preservation-format contract. The format simply is not directly arranged for GPU sampling; packing or baking would be additional work.

### Original tracks versus baked poses

This is independent of the CPU/GPU split:

- **Original tracks** retain source keys, timing, and tangents, but require a sampler.
- **Uniformly sampled baked poses** simplify interpolation and GPU data access at the cost of additional conversion output, memory, and an explicit approximation policy.

Recommendation: sample the original tracks in Rust first. GPU-oriented baking can be added later if a measured workload justifies it.

## Recommended Rust responsibilities

A shared animation player should:

1. Load the catalog and lazily load/cache selected clips.
2. Own clip selection, playback position, speed, pause, and loop policy.
3. Sample BCK translation, rotation, and scale tracks.
4. Optionally blend local poses if transitions are added later.
5. Evaluate the joint hierarchy and construct the skinning palette.
6. Upload the palette for the current frame.

### Transform and track correctness

Existing vertices are already in model-space bind pose. Each joint's deformation must be `animated_joint_world * inverse_bind_joint_world`, not merely the animated joint-world transform. Runtime inverse binds can be derived from the exported skeleton; verify bind-pose identity explicitly. The converter validates its source inverse binds and supplies a reference for transform composition. See [pose.rs:62-99](../../crates/convert-link/src/pose.rs#L62-L99) and [pose.rs:149-176](../../crates/convert-link/src/pose.rs#L149-L176).

BCK joint ordinals bind to model JNT1 joint ordinals. Tracks are axis-major SRT, not grouped as all scales followed by all rotations and translations. See [animation_manifest.rs:229-241](../../crates/gx/src/animation_manifest.rs#L229-L241) and [link_animations.md:151-153](../../docs/link_animations.md#L151-L153).

Rotation values are stored as i16 angles with 65,536 units per turn, with a separate per-clip rotation decimal shift. Track kinds include default, constant, and keyed; keyed tracks retain tangent data for Hermite-style sampling. A nonzero tangent-type word means split tangents, including values greater than one. See [animation_manifest.rs:14-29](../../crates/gx/src/animation_manifest.rs#L14-L29), [animation_manifest.rs:159-205](../../crates/gx/src/animation_manifest.rs#L159-L205), and [animation_manifest.rs:222-227](../../crates/gx/src/animation_manifest.rs#L222-L227).

The converter's verified bind-transform convention is `T * (Rz * Ry * Rx)`; its current model has unit joint scale. Animated scale must be handled deliberately rather than assuming every clip is rotation/translation only. See [pose.rs:149-176](../../crates/convert-link/src/pose.rs#L149-L176).

The schema preserves raw loop attributes and frame durations; it does not establish the runtime tick rate. Confirm the intended tick rate, end-frame behavior, loop modes, and interpolation details during implementation rather than treating them as settled by this investigation. See [animation_manifest.rs:30-34](../../crates/gx/src/animation_manifest.rs#L30-L34) and [animation_manifest.rs:222-227](../../crates/gx/src/animation_manifest.rs#L222-L227).

## Recommended Slang responsibilities

A shared skinning module should read joint indices/weights, fetch palette entries, and return deformed position and normal. The two mode-specific vertex shaders then retain their own shading behavior.

GameCube must use the deformed normal/position before raster-color and texgen evaluation. Modern consumes the same deformation before its shading setup. See [toon_link.shader.slang:95-109](../../examples/toon_link/shaders/source/toon_link.shader.slang#L95-L109) and [toon_link_modern.shader.slang:67-75](../../examples/toon_link/shaders/source/toon_link_modern.shader.slang#L67-L75).

Joint influences can be additional vertex attributes or an indexed storage-buffer sidecar. Neither choice is locked by this investigation. Nonuniform animated scale requires an explicit normal-transform policy; do not assume rotation-only matrices.

## Shared player and debug UI

`ToonLinkHost` already owns both rendering modes and a shared clock, and delegates rendering to the active mode. Only the selected mode receives its regular update. The host is therefore the natural owner of one animation player: switching render modes must not pause or restart a mode-specific copy of playback. See [main.rs:1183-1313](../../examples/toon_link/src/main.rs#L1183-L1313) and [toon_link.md:8](../../docs/toon_link.md#L8).

Conceptual data flow:

```text
Shared debug controls
         |
Shared Rust animation player -> joint palette
                                  |
                         Shared Slang skinning
                           /              \
                  GameCube shading    Modern shading
```

Suggested controls:

- Searchable clip picker with archive/path-qualified labels.
- Play/pause and restart.
- Playback speed and frame scrubbing.
- Loop policy and bind-pose option.

594 skeletal clips are too many for radio buttons. The host already has a custom egui section suitable for a picker; a new generic editor-widget framework is not inherently required. See [main.rs:1198-1223](../../examples/toon_link/src/main.rs#L1198-L1223) and [editor.rs:12-175](../../crates/renderer/src/editor.rs#L12-L175).

Recommended initial behavior: rendering-mode switches preserve clip, time, and pose; clip switches restart immediately. Crossfading is a possible follow-up, not part of the confirmed request.

## Facial animation is a separate layer

BTP/BTK animate material state, not skeletal deformation. Their sampling can be shared in Rust and produce texture-selection and UV-transform overrides, but applying those values needs mode-specific integration.

Bind targets by material name, not the preserved author-time `material_remap` value. BTP rows may repeat materials; do not discard row identity by blindly collecting into a material-name map. BTK also carries texgen selection, transform centers, and matrix-calculation metadata. See [animation_manifest.rs:255-316](../../crates/gx/src/animation_manifest.rs#L255-L316).

GameCube has a material/texgen path, while Modern uses a curated face-composition path. Modern's current shader does not implement the full GameCube texgen path. Full BTK parity therefore requires deliberate mapping or added support, not simply reusing a matrix unchanged. Modern's texture-loading subset also needs checking for BTP-selected textures. See [modern.rs:128-134](../../examples/toon_link/src/modern.rs#L128-L134), [modern.rs:574-604](../../examples/toon_link/src/modern.rs#L574-L604), and [tev.slang:363-385](../../examples/toon_link/shaders/source/tev.slang#L363-L385).

Current material buffers are upload-once singletons. Animated material state needs a per-frame path, such as small override buffers or mutable per-frame material tables. Shared playback does not require identical material shaders. See [main.rs:1069](../../examples/toon_link/src/main.rs#L1069), [modern.rs:683](../../examples/toon_link/src/modern.rs#L683), and [storage_buffer.rs:54-58](../../crates/renderer/src/renderer/storage_buffer.rs#L54-L58).

Recommendation: begin with BCK selection/playback in both modes, while leaving room for BTP/BTK. Whether animated faces belong in the first increment remains an operator scope decision.

## Verification considerations for future implementation

- Test track decoding/sampling, time boundaries, and loop behavior on CPU.
- Verify bind-pose skinning reproduces the current mesh, then verify a nontrivial animated pose.
- Verify mode switches preserve playback and both modes deform identically before shading.
- Define behavior when the machine-local animation directory is missing.
- Run `cargo check --workspace --all-targets` for Rust changes and `just shaders toon_link` for shader changes; do not edit generated bindings manually. See [AGENTS.md:69-83](../../AGENTS.md#L69-L83).
- Run `just sweep` for recording/resource-lifecycle changes, but do not count it as Modern validation: the documented sweep covers GameCube, and Modern needs its own visual checks. See [AGENTS.md:128](../../AGENTS.md#L128) and [toon_link.md:56-74](../../docs/toon_link.md#L56-L74).

## Investigation boundary

The findings were obtained by read-only inspection of local documentation, converter/schema code, locally converted assets, renderer buffer APIs, host/UI code, and both shaders. No playback was implemented, no performance benchmark was run, and no visual correctness claim was established. GPU crowd playback, crossfades, sound trailers, and full facial-animation parity are not approved scope.
