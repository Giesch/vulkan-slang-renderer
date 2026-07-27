# Tech debt

Cross-cutting cleanups that don't belong to any one feature plan. Unlike the
per-project follow-up files (e.g.
[`link_rendering/follow_up.md`](link_rendering/follow_up.md)), entries here are
renderer-wide and have no owning phase — they get picked up when someone is
already in the neighborhood, or when the cost of leaving them starts showing up
in debugging time.

Each entry states what's wrong, why it's tolerable today, and what "done" means.

1. [Vulkan objects leak when an init function fails partway](#1-vulkan-objects-leak-when-an-init-function-fails-partway) — cleanup debt, diagnostic cost
2. [Dangling pipeline when a hot reload's `create_graphics_pipeline` fails](#2-dangling-pipeline-when-a-hot-reloads-create_graphics_pipeline-fails) — **correctness bug**, debug builds only
3. [Remove the legacy `disable_depth_test` flag](#3-remove-the-legacy-disable_depth_test-flag) — carries a **behavior-change trap**, see §3.1
4. [Duplicate struct names across shared slang modules resolve by silent last-write-wins](#4-duplicate-struct-names-across-shared-slang-modules-resolve-by-silent-last-write-wins) — latent **silent-wrong-output** hazard in codegen

## 1. Vulkan objects leak when an init function fails partway

**The problem.** The `create_*` / `init_*` family creates Vulkan objects one at
a time with `?` between the steps. Any error after the first successful
creation drops the earlier handles without destroying them. `vk::Pipeline`,
`vk::Buffer` and friends are plain `Copy` handles with no `Drop`, so nothing
reclaims them.

Observed live while testing the missing-vertex-data error path — bailing out of
`init_pipeline` produced three validation errors at teardown:

```
vkDestroyDevice(): VkPipelineLayout 0x290000000029 has not been destroyed.
vkDestroyDevice(): VkPipeline 0x2c000000002c[basic_triangle] has not been destroyed.
vkDestroyDevice(): VkDescriptorSetLayout 0x280000000028 has not been destroyed.
```

Confirmed sites (read, not exhaustively audited — the rest of the `create_*`
family still needs a pass):

- `init_pipeline` (`src/renderer.rs:1263`) — `pipeline_layout` and `pipeline`
  are created up front, then every later `?` and both `anyhow::bail!`s in the
  `vertex_config` match leak them. Widest window in the codebase.
- `create_compute_pipeline` (`src/renderer.rs:1147`) — leaks
  `pipeline_layout` on any later failure, and additionally leaks
  `shader_module` if `create_compute_pipelines` fails, since the explicit
  `destroy_shader_module` sits *after* pipeline creation.
- `create_mesh` (`src/renderer.rs:1002`) — leaks the vertex buffer and its
  allocation if `create_index_buffer` fails.
- `create_texture_image` (`src/renderer.rs:4174`) — leaks the staging buffer if
  `write_to_gpu_buffer`, `create_vk_image` or the layout transition fails.

**Why it's tolerable today.** Every one of these is startup-only and its error
is fatal, so process exit reclaims the memory. Hot reload is *not* affected: it
recreates only the pipeline and retires the old objects through
`self.old_pipelines` (`src/renderer.rs:2648`), never calling `init_pipeline`.
The real cost is diagnostic — a genuine bring-up error arrives buried in
object-tracking noise, which is exactly when you can least afford it.

**Fix.** A scope-guard crate (`scopeguard`, or hand-rolled — it's ~20 lines and
avoids a dependency) so each created object registers its own destructor that
is disarmed once ownership transfers to the returned struct:

```rust
let pipeline_layout = ShaderPipelineLayout::create_from_atlas(&self.device, &*config.shader)?;
let layout_guard = scopeguard::guard(pipeline_layout, |l| l.destroy(&self.device));
// ...fallible steps...
let pipeline_layout = scopeguard::ScopeGuard::into_inner(layout_guard); // disarm
```

Note the borrow-checker friction: the guards close over `&self.device` while
`init_pipeline` takes `&mut self`, so the device handle likely needs cloning
(`ash::Device` is cheap to clone — it's an `Arc`-like handle) or the guarded
sections need to avoid `&mut self`. Worth prototyping on `init_pipeline` alone
before committing to the pattern across the family.

**Done means.** Each init function is leak-free on every error path, verified by
temporarily forcing a failure at each `?` and confirming a clean
`vkDestroyDevice`. A `destroy`-shaped method on `ShaderPipelineLayout` would
help; `destroy_compute_pipeline` (`src/renderer.rs:1247`) already shows the
teardown order.

## 2. Dangling pipeline when a hot reload's `create_graphics_pipeline` fails

**Not debt — an actual correctness bug**, found while surveying §1 and kept
here so it isn't lost. Unlike §1 this one is use-after-free, not a leak: the
renderer keeps drawing with a `vk::Pipeline` it has already queued for
destruction.

**The problem.** In `try_shader_recompile` (`src/renderer.rs:2638-2668`) the
old pipeline handle is retired *before* its replacement exists:

```rust
self.old_pipelines.push((                     // :2648 — queued for destruction
    self.total_frames,
    render_pipeline_mut.pipeline,             // vk::Pipeline is Copy — the field still holds it
    tmp_pipeline_layout.pipeline_layout,
    descriptor_set_layouts,
));
// ...
render_pipeline_mut.pipeline = create_graphics_pipeline(/* ... */)?;   // :2659 — may fail
```

If that `?` returns `Err`, the assignment never happens, so
`render_pipeline_mut.pipeline` still holds the handle now sitting in
`old_pipelines`. A few frames later the deferred cleanup destroys it and the
renderer draws with a dead pipeline.

**Why it hasn't bitten.** Debug builds only (hot reload is
`#[cfg(debug_assertions)]`), and hard to reach: shader compile errors return
early at `:2632`, so you need a shader that compiles cleanly but that Vulkan
then refuses to build a pipeline from. It is also silent when it does happen —
the failed reload logs an error and the corruption shows up frames later as a
GPU hang or garbage output, with no obvious link back to the reload.

**Fix.** Build the new pipeline first; only retire the old handle once the new
one succeeds. That ordering also makes the reload atomic — a failed recompile
leaves the previous pipeline intact and drawing, which is the behavior you want
from hot reload anyway.

**Done means.** A forced `create_graphics_pipeline` failure during reload
leaves the old pipeline live and rendering, with no validation complaints and
no entry orphaned in `old_pipelines`.

## 3. Remove the legacy `disable_depth_test` flag

> **Read §3.1 before starting.** The obvious mechanical migration silently
> changes depth-write behavior in two examples. This is not a pure refactor.

**The problem.** Depth state is now part of `RasterState`
(`src/renderer/pipeline.rs:216`, shipped in P5 of the link-rendering plan), but
the older boolean it replaced is still a field on both `PipelineConfig`
(`:274`) and `PipelineConfigBuilder` (`:332`), still emitted as
`disable_depth_test: false` by the codegen template
(`templates/shader_atlas_entry.rs.askama:132`, so it appears in all 16
generated shader files), and still overrides the raster state at
`src/renderer.rs:1270-1275`:

```rust
// the older, coarser disable_depth_test flag (emitted by generated
// pipeline_config()) wins over the raster state's depth compare
let mut raster_state = config.raster_state;
if config.disable_depth_test {
    raster_state.depth_test = DepthCompare::Disabled;
}
```

Two ways to say the same thing, one silently beating the other. The
`with_raster_state` doc comment (`:315-318`) exists only to warn about this
precedence.

**Consumers.** Exactly two, both setting the field by direct mutation rather
than a builder call:

- `examples/sprite_batch.rs:87`
- `examples/space_invaders.rs:165`

**Fix.** Delete the field from `PipelineConfig` and `PipelineConfigBuilder`,
drop it from the template and the `build()` body (`:349`), delete the override
block at `src/renderer.rs:1270-1275`, trim the precedence warning out of the
`with_raster_state` doc comment, and migrate the two examples per §3.1.

### 3.1 The migration trap: `disable_depth_test` never touched `depth_write`

The flag sets `depth_test = Disabled` and *leaves `depth_write` alone*, so both
examples run today with `Disabled` **plus** `depth_write: true` (the
`RasterState::default()` value). Per the `DepthCompare::Disabled` doc comment
(`src/renderer/pipeline.rs:205-208`) Vulkan still honors depth writes when the
test is off — so these two pipelines currently write depth unconditionally,
which is almost certainly not what anyone intended when they reached for a flag
named "disable depth test".

That makes two different migrations, and they are not the same change:

```rust
// (a) faithful 1:1 — preserves today's behavior exactly, including the
//     probably-unintended depth writes. Correct choice for the removal commit.
.with_raster_state(RasterState {
    depth_test: DepthCompare::Disabled,
    ..Default::default()            // depth_write stays true
})

// (b) what these 2D examples arguably *want* — a real behavior change.
.with_raster_state(RasterState {
    depth_test: DepthCompare::Disabled,
    depth_write: false,
    ..Default::default()
})
```

Do (a) in the removal commit so the change stays reviewable as a pure refactor,
then decide (b) separately. Both examples draw a single pipeline into a depth
buffer that is cleared every frame and never sampled, so (b) should be visually
inert — which is exactly why it must be verified deliberately rather than
assumed: if it *isn't* inert, something else depends on those writes and that is
worth knowing before it's buried in a cleanup diff.

The same trap applies to any future `DepthCompare::Disabled` user, so it may be
worth pairing the removal with a rename or a constructor
(`RasterState::no_depth()` setting both fields) that makes the pairing hard to
get wrong.

**Cost.** Mechanical but wide: `just shaders` regenerates all 16 generated
files and every one of their snapshots changes (one deleted line each), so
review the diff shape once and then `cargo insta test --accept`. Verify with
`timeout 3 just dev sprite_batch` and `space_invaders` — the failure mode to
watch for is sprites vanishing or z-fighting, which would mean the depth state
didn't actually carry over.

## 4. Duplicate struct names across shared slang modules resolve by silent last-write-wins

**The problem.** The codegen resolves Slang struct names through a single flat
namespace with no qualification by module. `reflect_shared_module_types`
(`src/shaders.rs:207-253`) walks every shared (non-shader) `.slang` module and
records each struct it declares:

```rust
let mut type_to_module: HashMap<String, String> = HashMap::new();
for &module_name in module_names {
    // ...
    type_to_module.insert(name.to_string(), module_name.to_string());   // :247
}
```

The key is the bare struct name. If two shared modules declare the same struct
name, the second `insert` silently overwrites the first — no warning, no error.
That map is not cosmetic: `tag_source_modules` (`src/shaders/build_tasks.rs:1347`)
uses it to set each definition's `source_module`, which decides **which
`src/generated/shader_atlas/<module>.rs` owns the generated type** and which
`use` line every consuming shader emits. So a name collision silently relocates a
generated type, and the shader that "lost" gets a `use` pointing at the other
module's definition of a same-named but potentially differently-laid-out struct.

**Why it's tolerable today.** No collision exists. The 11 shared modules in
`shaders/source/` declare 10 struct names, all distinct: `ClosestShape` and
`RayHitDistance` (`ray_march`), `Cube` / `FragInput` / `RayMarchHit`
(`gpu_picking_common`), `FullscreenPosition` (`fullscreen_triangle`),
`MVPMatrices` (`mvp`), `Particle` (`particle`), `Projection` (`projection`),
`RayMarchCamera` (`ray_march_camera`). The remaining modules (`addr`,
`dragon_curve`, `super_sample`, `watercolor_common`) declare no structs.

Note what changed and what didn't when the codegen was made
order-independent (`link_rendering/follow_up.md` §5b): `reflect_slang_module_types`
now sorts its module list, so the collision *winner* is at least reproducible
across machines. But reproducible is not correct — the rule is now "whichever
module name sorts last," which nobody would choose on purpose and which will read
as a bug the first time someone hits it. Sorting removed the
machine-to-machine variation that would have made this *undebuggable*; it did
not remove the hazard.

**The in-repo precedent for the fix.** One level down, the analogous case is
already handled the right way. `collect_shared_modules`
(`src/shaders/build_tasks.rs:1368-1397`) panics when the same shared type turns up
with an incompatible layout in two shaders, and its comment says exactly why:

```rust
// a shared type must have the same layout in every shader
// that uses it; first-definition-wins would silently drop
// one of two diverging layouts
```

§4 is that same principle applied one level up, at module-level name collisions.
It also matches the policy the vec4-array mini-phase settled on
(`link_rendering/follow_up.md` §1): support the honest subset, hard actionable
error otherwise.

**Fix.** Cheap — `HashMap::insert` already returns the displaced value, so detect
the collision instead of discarding it:

```rust
if let Some(prev_module) = type_to_module.insert(name.to_string(), module_name.to_string())
    && prev_module != module_name
{
    anyhow::bail!(
        "struct '{name}' is declared in two shared slang modules \
         ('{prev_module}' and '{module_name}'); generated types are keyed by \
         bare struct name, so rename one",
    );
}
```

The function already returns `anyhow::Result`, so this needs no signature change.

**Done means.** Two shared modules declaring the same struct name fail
`just shaders` with a message naming the type and both modules. Coverage wrinkle:
this case **cannot** live in `shaders/test/` as an atlas fixture, because a
fixture that fails would break `alignment_tests` for every other case in the
directory. It needs a unit test that writes two colliding modules into a temp dir
and calls `reflect_shared_module_types` directly.
