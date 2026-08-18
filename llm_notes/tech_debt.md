# Tech debt

Cross-cutting cleanups that don't belong to any one feature plan. Unlike the
per-project follow-up files (e.g.
[`link_rendering/follow_up.md`](link_rendering/follow_up.md)), entries here are
renderer-wide and have no owning phase — they get picked up when someone is
already in the neighborhood, or when the cost of leaving them starts showing up
in debugging time.

Each entry states what's wrong, why it's tolerable today, and what "done" means.

> **Path note (2026-08, workspace split):** entries written before the cargo
> workspace migration cite monolith paths. Mapping: `src/renderer*` and
> `src/shaders.rs` → `crates/renderer/src/…`; `src/shaders/build_tasks.rs` →
> `crates/cli/src/build_tasks.rs`; `src/game/traits.rs` and `src/app.rs` →
> `crates/mltrs/src/…`; `shaders/source/` → per-example
> `examples/<name>/shaders/source/`. Line numbers have drifted; grep for the
> named item.

1. [Vulkan objects leak when an init function fails partway](#1-vulkan-objects-leak-when-an-init-function-fails-partway) — cleanup debt, diagnostic cost
2. [Dangling pipeline when a hot reload's `create_graphics_pipeline` fails](#2-dangling-pipeline-when-a-hot-reloads-create_graphics_pipeline-fails) — **correctness bug**, debug builds only
3. [Remove the legacy `disable_depth_test` flag](#3-remove-the-legacy-disable_depth_test-flag---done) — **done**
4. [Duplicate struct names across shared slang modules resolve by silent last-write-wins](#4-duplicate-struct-names-across-shared-slang-modules-resolve-by-silent-last-write-wins) — latent **silent-wrong-output** hazard in codegen
5. [Validation and fault injection cannot run in release builds](#5-validation-and-fault-injection-cannot-run-in-release-builds) — release builds are unsweepable, coverage gap
6. [Fresh-environment builds: friction log from the workspace-migration session](#6-fresh-environment-builds-friction-log-from-the-workspace-migration-session) — reproducibility gaps, some already planned, two new
7. [The Y-down clip space flip is applied in three different places, none documented](#7-the-y-down-clip-space-flip-is-applied-in-three-different-places-none-documented) — undocumented convention, easy to get wrong in a new example
8. [The `fixtures/shaders` corpus is hand-maintained copies of example shaders](#8-the-fixturesshaders-corpus-is-hand-maintained-copies-of-example-shaders) — lockstep edits, undetected drift weakens snapshot coverage
9. [`pipeline_config` consumes the atlas entry, so a shader can build only one pipeline](#9-pipeline_config-consumes-the-atlas-entry-so-a-shader-can-build-only-one-pipeline---done) — **done**
10. [The codegen emits unformatted Rust and relies on the caller running `cargo fmt`](#10-the-codegen-emits-unformatted-rust-and-relies-on-the-caller-running-cargo-fmt) — consumer-visible churn, only the justfile hides it
11. [Vendored slang modules conflate engine API with example-shared conveniences](#11-vendored-slang-modules-conflate-engine-api-with-example-shared-conveniences) — public API surface wider than intended, and no mechanism for the modules actually being duplicated
12. [The sweep's fixed timeout fails the two pipeline-heaviest examples on a GPU-less box](#12-the-sweeps-fixed-timeout-fails-the-two-pipeline-heaviest-examples-on-a-gpu-less-box) — **false red**, and one of its two spellings accuses §1
13. [The engine cannot screenshot itself, so the gate for validation-invisible bugs is external tooling](#13-the-engine-cannot-screenshot-itself-so-the-gate-for-validation-invisible-bugs-is-external-tooling) — verification depends on the desktop environment, not the repo
14. [Multiple `ParameterBlock` globals are supported end-to-end but never exercised](#14-multiple-parameterblock-globals-are-supported-end-to-end-but-never-exercised) — untested path, an ordering contract held together by a comment
15. [`Game::draw` takes `&mut self`, so a frame-scoped GPU address can be stashed across frames](#15-gamedraw-takes-mut-self-so-a-frame-scoped-gpu-address-can-be-stashed-across-frames) — latent **silent-wrong-data** hazard, nothing in the type system prevents it
16. [Typed device-address minting is split across two layers](#16-typed-device-address-minting-is-split-across-two-layers) — an invariant held by convention that could be held by the compiler
17. [Picking is a second rendering path rather than a pass, so every new capability must be re-implemented or refused](#17-picking-is-a-second-rendering-path-rather-than-a-pass-so-every-new-capability-must-be-re-implemented-or-refused) — recurring per-feature carve-outs; the cost lands on whoever adds the *next* feature
18. [The roc platform's glibc 2.39 floor excludes SteamOS, Debian stable and Ubuntu 22.04 LTS](#18-the-roc-platforms-glibc-239-floor-excludes-steamos-debian-stable-and-ubuntu-2204-lts) — **shipped-artifact reach**, and the Steam Deck number is unmeasured

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

The confirming half of that is already automated: `scripts/headless-sweep.sh`
exits every example through `timeout`'s SIGTERM, which SDL turns into
`SDL_QUIT`, so `Drop` runs and `vkDestroyDevice` reports any survivors. Verified
non-vacuous by skipping a `destroy_image_view` and watching
`VUID-vkDestroyDevice-device-05137` appear (`build_reproducibility.md` §7.2/§7.4).
Only the forcing-a-failure-at-each-`?` half is still manual.

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

## 3. Remove the legacy `disable_depth_test` flag — **done**

Removed. The flag is gone from `PipelineConfig`, `IndexedPipelineConfig`,
`PipelineConfigBuilder`, the codegen template, and the `create_pipeline`
override that let it silently beat `RasterState.depth_test`. Depth state now
has exactly one spelling.

The two consumers (`examples/sprite_batch.rs`, `examples/space_invaders.rs`)
migrated to a new `RasterState::no_depth()` constructor. That was a deliberate
behavior change, not a 1:1 port: the old flag set the depth *test* to disabled
and never touched `depth_write`, so both examples had been writing depth
unconditionally (Vulkan honors writes with the test off). `no_depth()` sets
both, and exists so that pairing is hard to get wrong for the next caller.

Kept numbered so §4/§5 references from other notes stay valid.

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

## 5. Validation and fault injection cannot run in release builds

**The problem.** Validation is gated by a single compile-time switch,
`ENABLE_VALIDATION: bool = cfg!(debug_assertions)` (`src/renderer.rs:61`), and
fault injection piggybacks on it via the `cfg!(debug_assertions) &&` conjunct
in `Renderer::viewport_width` (`src/renderer.rs:1441`). The consequence is that
a release build validates nothing and cannot be swept: `game/traits.rs:98`
self-reports `exit_code::VALIDATION_DISABLED` under `VKR_SWEEP`, and
`scripts/headless-sweep.sh` is structurally debug-only anyway (since the
workspace split it builds the per-example packages and runs
`target/debug/<example>`, still with no release path). Release builds get zero
validation coverage — a release-only regression (e.g. from the release half of
the debug/release `create_from_atlas` pair, `src/renderer.rs:5026`) would ship
silently.

**Why it's tolerable today.** Nothing ships from this repo; release builds are
used mostly for `just lint`'s second clippy pass and `just paper-texture`
(which never constructs a `Renderer`). The debug sweep covers the shared code
paths, and the debug/release divergences are few.

**Fix.** Three pieces:

1. **EnvConfig setting.** Add a `validation` field to `EnvConfig` (e.g.
   `VKR_VALIDATION`, the design sketched in `offscreen_testing.md` §11):
   default on in debug, off in release, overridable either way. Replace the
   five `ENABLE_VALIDATION` consumers (`get_required_layers`,
   `check_required_layers` transitively, the `push_next` at
   `src/renderer.rs:291`, `maybe_create_debug_messager_extension` at
   `renderer/debug.rs:80`, and `Drop` at `src/renderer.rs:2739`) with the
   runtime value. **Trap:** the const is what keeps messenger creation and
   `Drop` in agreement; a runtime value must be read once and stored (on
   `Renderer`, which already holds `env: EnvConfig`) or `Drop` will destroy a
   null messenger / leak a real one (flagged in `offscreen_testing.md:684`).
   The message-counting path (`renderer/debug.rs` statics,
   `game/traits.rs:145`) is already unconditional and needs no change.
2. **Fault injection in release.** Drop the `cfg!(debug_assertions) &&`
   conjunct at `src/renderer.rs:1441` — the branch is `cfg!` (an expression),
   so the code already compiles in release and the env var is already parsed
   there; only the conjunct blocks it.
3. **Sweep release flag.** Teach `scripts/headless-sweep.sh` (and
   `just sweep`) a `--release` flag: build with `--release`, run
   `target/release/<example>`, and update the `VALIDATION_DISABLED` check at
   `game/traits.rs:98` to key off the runtime setting rather than the const.

Watch for: `VK_LAYER_KHRONOS_validation` becomes a *runtime* requirement of any
release run that opts in (`check_required_layers` bails if the layer package is
missing — see `build_reproducibility.md:348`); the shader-`println` device
features (`src/renderer.rs:3211`, `:3228`) are gated on the same
`cfg!(debug_assertions)` but are logically independent — committed SPIR-V using
`println` on a release device without those features can itself trip
validation, so decide whether they follow the new setting or stay debug-only;
and `docs/testing.md`'s exit-code table plus the `env_config.rs` doc comments
("Debug builds only, like validation itself") both assert the old model and
must be updated.

**Done means.** `just sweep --release` runs every example under lavapipe with
validation active and passes; `just sweep-self-test --release` proves the
injected fault is still detected in a release binary; a plain release run with
the setting unset behaves exactly as today (no layer loaded, no messenger, no
new runtime dependency).

---

## 6. Fresh-environment builds: friction log from the workspace-migration session

**Context.** The 2026-08 workspace migration ran in a fresh container (no
direnv, no display, restricted network via proxy, a fixed disk allowance, 4
cores, lavapipe only). Everything below was hit in one session. Items marked
*known* re-confirm an existing entry in
[`build_reproducibility.md`](build_reproducibility.md); the disk and
sweep-timing items are new.

**Hit again, exactly as documented (annotations added there):**

- ***known*, §3 — slang-rhi's OptiX fetch.** `cmake --preset default
  -DSLANG_LIB_TYPE=STATIC` failed configuring: slang-rhi unconditionally
  fetches the OptiX headers from GitHub, which a restricted proxy 403s.
  Fixed with the flags the Windows recipe already passes:
  `-DSLANG_ENABLE_SLANG_RHI=OFF -DSLANG_ENABLE_TESTS=OFF`. **The unix
  `build-slang` recipe now passes them too** (applied during this session, as
  §3 prescribed). Also re-confirmed: the static build produces no
  `libslang.a` — `libslang-compiler.a` + `libcompiler-core.a` + `libcore.a`
  are the artifacts to wait for.
- ***known*, §4 — undocumented system deps.** `libasound2-dev` (alsa-sys ←
  rodio) was the first build failure; the sweep set
  (`mesa-vulkan-drivers vulkan-validationlayers libvulkan-dev`) was needed
  exactly as listed. One wrinkle worth a word in §4: on a stale image the
  pinned package index 404s, so it's `apt-get update` *then* install.
- ***known*, §5 — env vars without direnv.** Every non-interactive shell needs
  the `SLANG_*` exports by hand. A sharper footgun surfaced: `.env` computes
  the paths from `$PWD`, so sourcing it (or exporting inline) from a
  subdirectory silently bakes a wrong absolute path, and the eventual
  `shader-slang-sys` build-script panic points at a nonsense nested path far
  from the actual mistake. A `load-env.sh` that resolves relative to its own
  file location (not `$PWD`) would remove the trap; `.cargo/config.toml
  [env] relative = true` (§5's option 2) would too.
- ***known*, §6 — `cargo-insta` not installed by anything.** Needed
  `cargo install cargo-insta` before the snapshot-rename step could run.

**New — disk footprint of the debug workspace build.** A full
`cargo test --workspace` at the default dev profile overflowed a ~30 GB
allowance mid-link (`ld terminated with signal 7`, `No space left on
device`): `target/` alone reached ~21 GB, ~12 GB of which was
`target/debug/examples` — each example binary statically links slang + SDL
and carries full debuginfo, several hundred MB apiece, and the workspace
split doubles the count of linked test/bin artifacts. The slang build tree
adds ~7 GB (~1.3 GB of it deletable `*.o` files). Session workaround:
`CARGO_PROFILE_DEV_DEBUG=0`, which shrank the example binaries by an order
of magnitude with no effect on `debug_assertions` (validation, hot reload
and the sweep all still work). Candidate real fixes, undecided: workspace
`[profile.dev] debug = "line-tables-only"` (keeps usable backtraces), or
`split-debuginfo`, or accepting the cost and documenting the disk
requirement. Whoever picks one should re-run `just sweep` and a
backtrace-bearing failure to check the trade.

**New — watercolor vs. the sweep window on slow machines.** Under lavapipe
on 4 cores, watercolor's first frame (10 compute pipeline creations) can
exceed the default `SWEEP_TIMEOUT`, producing `FAIL(no frames): watercolor`
— a false failure that reads exactly like the real no-frames regression the
exit code exists to catch. `SWEEP_TIMEOUT=60` passed reliably. Options:
per-example timeout overrides in the sweep script, a longer default (costs
~every example's window), or a note in `docs/testing.md` telling a slow-box
operator to retry the failing example with a bigger window before treating
it as red. The last is cheapest and probably enough.

> **Promoted to [§12](#12-the-sweeps-fixed-timeout-fails-the-two-pipeline-heaviest-examples-on-a-gpu-less-box) (2026-08).** Re-measured in a later container
> session, and the paragraph above understates it in two ways: `multi_mesh`
> is affected too, and below ~15s the failure is not `no frames` at all but
> `FAIL(no clean teardown) … exit 137`, which accuses §1. The numbers there
> supersede the ones here.

**Done means.** §3/§4/§5/§6 of `build_reproducibility.md` get their fixes
(the §3 recipe half landed with the workspace migration); the disk footprint
has a chosen policy recorded in the workspace `Cargo.toml` or the README;
and a slow-machine sweep either passes or fails with a message that says
"widen the window", not "no frames".

---

## 7. The Y-down clip space flip is applied in three different places, none documented

**Context.** Surfaced while upgrading glam 0.30.8 → 0.33.2 (2026-08). The
upgrade itself is done and is *not* the debt — this entry records what the
upgrade exposed and deliberately did not fix.

> **Correction (same session).** A first draft of this entry claimed the flip
> was absent from the shared code and hand-rolled per example. That was wrong,
> and it was wrong in the direction that would have caused damage: acting on it
> would have double-flipped six examples. The flip *is* centralized — in two
> vendored slang modules, below. The draft also read `Vec3::Z` in
> `viking_room`/`depth_texture` as clip-space compensation; it is the model's
> orientation. Kept visible rather than silently rewritten, since the
> mischaracterization is the kind a reader might repeat.

**The problem.** The repo's convention is a **Y-up (`directx`-style)
projection matrix on the CPU, flipped to Vulkan's Y-down clip space downstream.**
That is a coherent choice, and consistency with it is the goal. What's missing
is that "downstream" means three unrelated mechanisms, and no document names
any of them:

1. **`crates/cli/vendor/mltrs/mvp.slang`** — `MVPMatrices::project` computes
   `mul(reflectY, this.proj)`, with the Vulkan-tutorial citation inline. This
   covers `basic_triangle`, `suzanne`, `multi_mesh`, `viking_room`,
   `depth_texture` and `toon_link`.
2. **`crates/cli/vendor/mltrs/fullscreen_triangle.slang`** — a *second*,
   independent `reflectY` producing `centeredCoords`, annotated "OpenGL-style
   Y-up" against the same struct's Y-down `svPosition`/`texCoord`. Sixteen
   examples read it; only `gpu_picking` and `ray_marching` invert a projection
   against it, and for them it is what makes the Y-up matrix correct. The other
   consumers are 2D SDFs that simply want Y-up math.
3. **Swapped orthographic bounds** — `examples/sprite_batch/src/main.rs:141`
   and `examples/space_invaders/src/main.rs:380` pass
   `bottom = height, top = 0.0`. `Projection` (`projection.slang`) applies no
   flip of its own, so for these two the swap *is* the flip.

Not a mechanism, listed because a first read mistakes it for one:
`viking_room`/`depth_texture` pass `Vec3::Z` as the up vector because
`viking_room.obj` is Z-up — the same reason the spin is `Mat4::from_rotation_z`.
Unrelated to clip space, and it must not be "fixed".

Whoever writes the next example has to work out which of the three applies
before their first frame is the right way up, and the answer depends on
something invisible from the Rust side: whether the shader went through
`MVPMatrices` or bare `Projection`.

**Why it's tolerable today.** Every example looks right, `just sweep` covers
them, and the convention is at least *consistent* even if it is unwritten.
Nothing ships from this repo, so the cost is the next author's time.

**What the glam upgrade changed.** glam 0.33.2 deprecated the whole
view/projection family (`Mat4::look_at_rh`, `Mat4::perspective_rh`,
`Mat4::orthographic_lh`, …) and moved it to free functions under
`glam::camera::{lh,rh}::{view,proj}`, where the `proj` sub-module *names the
NDC convention*:

| module    | NDC Z   | NDC Y |
|-----------|---------|-------|
| `opengl`  | [-1, 1] | Up    |
| `directx` | [0, 1]  | Up    |
| `vulkan`  | [0, 1]  | Down  |

`vulkan::perspective` is `directx::perspective` with the Y row negated. The old
`Mat4::perspective_rh` was Z ∈ [0,1] **Y-up** — i.e. `directx`. The migration
therefore used `directx::perspective` / `directx::orthographic` /
`view::look_at_mat4`. Each call site carries a short comment naming *which* of
the three mechanisms above flips it, because `directx` in a Vulkan renderer
reads as a mistake otherwise.

Checked rather than assumed: replaying every example's actual arguments through
both the deprecated method and its replacement gives bit-identical matrices in
12 of 13 cases. The exception is `ray_marching`'s `far = 1000.0` perspective,
where the two depth terms differ by exactly 1 ULP (`-1.0001` vs `-1.0000999`,
relative 1.19e-7). The formula was rearranged — old:
`r = far / (near - far)`; new: `z_range_inv = 1.0 / (far - near)` then
`-far * z_range_inv`. Same value mathematically, different rounding. Worst-case
NDC depth error across that frustum is 1.19e-7, which is one ULP of `f32` near
1.0 — at the representational floor of the depth buffer, and `ray_marching` only
feeds the matrix through `(proj * view).inverse()` to reconstruct ray
directions. Not worth compensating for; worth knowing before someone diffs a
matrix and thinks something moved.

**Decision (2026-08): keep the `directx`-style convention.** glam 0.33.2 ships
`camera::rh::proj::vulkan::*`, which would move the flip into the projection
matrix and let all three mechanisms go away. Considered and declined — the goal
is internal consistency, and the repo is already consistently Y-up-plus-
downstream-flip. Switching would mean deleting `reflectY` from `mvp.slang`
(a vendored engine module, so every consumer's contract, not just the
examples'), and either growing `FullscreenPosition` with a Y-down centered
coordinate or flipping `centeredCoords` for the six 2D SDF examples that
legitimately want it Y-up. Large blast radius, no behavioral win.

**Fix — documentation, not code.** The debt is that the convention is unwritten,
so record it in `docs/`: CPU builds Y-up (`directx`) projections; the flip is
applied by `mvp.slang` for `MVPMatrices` shaders, by `fullscreen_triangle.slang`
for shaders that reconstruct rays from `centeredCoords`, and by swapped
orthographic bounds for bare-`Projection` 2D shaders. State which one a new
example inherits, and that a Y-up projection paired with the wrong one is an
upside-down first frame with no other symptom.

Worth doing at the same time: `fullscreen_triangle.slang` and `mvp.slang`
each define their own private `reflectY` with no cross-reference, so neither
tells a reader the other exists.

**Done means.** `docs/` has a section a new example's author can follow to get
the right orientation on the first frame without reading either slang module;
the two `reflectY` definitions reference it; and the per-call-site comments in
`examples/*/src/main.rs` point at it rather than restating it.

**Adjacent, deliberately out of scope.** glam 0.33 made its non-`f32` types
optional (`default = ["std", "all-types"]`), so the workspace pin could set
`default-features = false` and drop `f64` plus unused integer widths for a
compile-time win — the used surface is only `Vec2/3/4`, `UVec4`, `IVec4`,
`Mat3`, `Mat4`, `Quat`. Separately, `crates/mltrs/Cargo.toml:17` declares glam
but `crates/mltrs/src/**` contains zero references to it. Neither is worth a
numbered entry; both are free wins for whoever is next in the neighborhood.

The layout half of the glam contract is *not* debt and must not be disturbed by
any of the above — see Phase 6 of
[`vulkan_1_3_migration.md`](vulkan_1_3_migration.md) for the model (sizes
stable, alignments feature-dependent, never substitute `Vec3A`, and the
`align_of::<glam::Vec4>() == 16` assert the templates emit to catch a
transitively-enabled `scalar-math`). The 0.33.2 upgrade left it intact:
`just shaders` regenerated every example with no diff.

## 8. The `fixtures/shaders` corpus is hand-maintained copies of example shaders

**Context.** Surfaced while moving the engine slang modules under a `mltrs`
namespace (2026-08). The refactor is done and is not the debt — this entry
records the cost it exposed.

**The problem.** The `generated_files` test
(`crates/cli/src/build_tasks.rs:1784`) points `shaders_source_dir` at
`manifest_path(["fixtures", "shaders"])` with `import_root: "crate"`, i.e. one
synthetic consumer crate. Every `.slang` file in that corpus is a byte-identical
copy of a file that lives somewhere else in the repo — nothing there is unique
to the fixtures:

| fixture files | copy of | real source of truth |
|---|---|---|
| `mltrs.slang` + `mltrs/*.slang` (6) | `crates/cli/vendor/`, and every example's seeded copy | `crates/cli/vendor/` |
| `basic_triangle.shader.slang`, `sdf_2d.shader.slang`, `gpu_picking.shader.slang`, `gpu_picking_common.slang`, `particle.slang`, `particle_render.shader.slang`, `particles.compute.slang`, `ray_march_camera.slang` (8) | one example each (`ray_march_camera.slang` also exists identically in 3 examples) | the example crate |

The copies are maintained by hand, and nothing checks that a copy still matches
its origin. The namespace refactor had to rewrite every engine import twice —
once across the fixtures, once across the 16 examples — and the two passes were
kept in sync only by remembering to do both. Drift doesn't fail: it silently
weakens what the snapshots cover, since the corpus would keep exercising the
codegen paths, just not the ones any real consumer uses.

Note the corpus *was* deliberately curated, not accidentally accumulated —
`mltrs_workspace.md:462-470` specifies "a curated set exercising every codegen
path" and lists them by the path each covers (vertex-buffer graphics,
vertex-less fullscreen, compute + shared module, cross-module import). That
intent is still right. What's missing is anything that keeps the files honest to
it.

**Why it's tolerable today.** The copies are currently identical (verified
file-by-file, 2026-08), the corpus is 14 files, and the failure mode is degraded
coverage rather than wrong output. Neither `crates/cli/fixtures/alignment/` (21
purpose-built std140/std430/pointer files) nor `fixtures/check_crate/` has this
problem — alignment fixtures are self-contained and named for the codegen branch
they cover, which is arguably the shape this corpus should have too.

**Fix — pick one; the tradeoff is real in both directions.**

- **(a) Replace the duplicates with purpose-built fixtures.** Follow the
  `fixtures/alignment` model: name each file for the codegen branch it exercises
  rather than the example it was lifted from, and let it diverge from any
  example. Keeps cli snapshots insulated from cosmetic example edits — which is
  the reason for having a separate corpus at all — at the cost of the "fixtures
  mirror real usage" property.
- **(b) Make the example crates the source of truth and regenerate.** A `just
  sync-fixtures` recipe that copies the 8 example-owned files from their example
  homes, plus a test (or `pre-commit` step) that fails when they drift.
  Guarantees the corpus tracks real usage; couples cli snapshot churn to every
  example shader edit, so a purely visual tweak in `sdf_2d` starts producing
  snapshot diffs in `mltrs-cli`.

(a) is the better end state for the reason the corpus exists — stable,
branch-complete coverage — and the pattern is already proven one directory over.
(b) is the cheaper stopgap if the drift guarantee is wanted before anyone has
time for the rewrite.

**Cheap partial win, independent of that choice.** The 6 engine files already
have both a source of truth and a seeding mechanism:
`mltrs shaders init --dir crates/cli/fixtures/shaders --force` is the same
command `just vendor-shaders` (`justfile:62-67`) already runs for every example.
Adding the fixtures dir to that loop retires 6 of the 14 hand-maintained copies
today, whichever way the other 8 go.

**Done means.** No file under `crates/cli/fixtures/shaders/` is a
hand-maintained copy of another file in the repo: each is either purpose-built
for a named codegen path, or regenerated by a recipe that fails when it drifts.
`just test` still passes with the corpus covering every branch it does now —
graphics with vertex buffers, vertex-less fullscreen, compute, shared module,
cross-module import, BDA pointers, enums.

## 9. `pipeline_config` consumes the atlas entry, so a shader can build only one pipeline — **done**

Fixed with option (a) below, `&self` + `Clone`. `pipeline_config` now takes
`&self` and boxes `self.clone()`; `Clone` is derived on the generated entry
struct in both templates and across the reflection JSON tree
(`crates/renderer/src/shaders/json.rs` and its two submodules, plus the
`check_crate` fixture's stub of the same types). The lifetime is spelled out
rather than elided — with `&self` present, elision would tie the returned
config to the atlas entry instead of to the `Resources` it actually borrows —
so `config_return_type` (`crates/cli/src/build_tasks.rs`) emits `'a`.

`watercolor` lost all 17 `ShaderAtlas::init()` repeats and the comment that
explained them; it now builds every parity variant from the single `shaders`
atlas it is handed. `multi_mesh` and `toon_link` lost their per-pipeline
`Shader::init()` too — `toon_link`'s `build_material_pipelines` takes a
`&Shader` parameter instead.

Option (b) (`Box` → `Arc` across the four config structs and two pipeline
structs) was not taken and stays available as a pure renderer-side refactor.
The reason to reach for it is memory, not ergonomics: (a) unblocked the call
sites, but each pipeline still holds its own copy of the reflection data, so
`watercolor`'s 21 pipelines carry 21 copies. That trade was worth taking here
because (a) is template-local and (b) is not.

Verified beyond the usual `just test` / `just sweep`: hot reload is the
consumer that keeps the boxed entry alive for the pipeline's whole life, so
both reload paths were exercised headlessly by editing a `.slang` file
mid-run — `watercolor`'s `wc_gaussian_blur.compute.slang` (one shader backing
three pipelines) and `basic_triangle`'s graphics shader (the vertex-description
readback). Both recompiled and exited 0, i.e. with no validation messages.

Kept numbered so references from other notes stay valid. The original entry
follows.

**Context.** Surfaced while reading `examples/watercolor/src/main.rs` (2026-08).
Not a bug — the by-value signature is load-bearing today; this entry records why
it doesn't have to be.

**The problem.** The generated `pipeline_config` takes `self` and boxes it
(`crates/cli/templates/shader_atlas_entry.rs.askama:137`,
`shader_compute_entry.rs.askama:112`):

```rust
pub fn pipeline_config(self, resources: Resources<'_>) -> ComputePipelineConfig<'_> {
    // ...
    ComputePipelineConfig { shader: Box::new(self), /* ... */ }
}
```

The owned box is not just a creation-time convenience — `create_compute_pipeline`
stores it in `ComputeRendererPipeline { shader: config.shader }`
(`crates/renderer/src/renderer.rs:1238`), and hot reload reads it back for the
pipeline's whole life: `source_file_name()` (`renderer.rs:1402`, `:1843`,
`:2630`), `create_from_atlas` on recompile (`:2514`, `:2570`), and the graphics
path's `vertex_binding_descriptions()` / `vertex_attribute_descriptions()`
(`:2551-2552`). So the config does need an owned `Box<dyn …>`; taking `self` is
just the cheapest way to get one.

The consequence at the call site is that `shaders.wc_divergence_compute` is a
**field move out of the atlas**. Fine once per shader; watercolor builds two
pipelines per compute shader (ping/pong parity variants) and so re-inits the
whole atlas to get a second copy — 17 `ShaderAtlas::init()` calls
(`main.rs:487`, `:532`, `:552-553`, `:576-577`, `:600-601`, `:630-631`, `:652`,
`:665-666`, `:696`, `:730-731`, `:763`), each re-parsing all 10 reflection JSONs
(~84 KB), i.e. ~1.4 MB of `serde_json` at startup where ~84 KB would do.
`multi_mesh:372` and `toon_link:789` hit a milder form of the same thing with a
bare `Shader::init()`.

**Why it's tolerable today.** It is correct, and the workaround is one line at
each site with a comment explaining it (`watercolor/src/main.rs:32`). The
startup cost is real but small in absolute terms, and only watercolor pays it at
this scale. (§6 notes watercolor's first frame can exceed `SWEEP_TIMEOUT` on a
4-core lavapipe box, but attributes that to the 10 compute pipeline creations,
not to JSON parsing — the redundant parses contribute, they are not the cause.)

**Fix — pick one.**

- **(a) `&self` + `Clone`.** `Box::new(self.clone())`. The entry structs hold
  nothing but `reflection_json`, so this needs `Clone` derived on
  `ReflectionJson` / `ComputeReflectionJson`
  (`crates/renderer/src/shaders/json.rs:11,27`, currently
  `Debug, Serialize, Deserialize`) and on the generated entry struct in both
  templates. Cheapest change, strictly less work than the `init()` it replaces,
  but every pipeline still holds its own copy of the reflection data.
- **(b) `Arc<dyn …>`.** Change the four config structs and the two pipeline
  structs from `Box` to `Arc` (`crates/renderer/src/renderer/pipeline.rs:174`,
  `:289`, `:320`, `:377`, `:425`, `:472`) and have `pipeline_config` clone the
  `Arc`. No duplicated reflection data, wider blast radius, and it changes the
  atlas entry's own shape (`Arc<Self>` receiver or an `Arc` field).

Either way the signature needs an explicit lifetime, since elision currently
ties the return type to `self`'s position:

```rust
pub fn pipeline_config<'a>(&self, resources: Resources<'a>) -> ComputePipelineConfig<'a>
```

(a) is the better first move: it's a template-local change that unblocks the
call sites, and (b) stays available afterwards as a pure renderer-side refactor.

**Done means.** `examples/watercolor/src/main.rs` builds all its parity-variant
pipelines from the single `shaders` atlas with no `ShaderAtlas::init()` repeats,
and the comment at `main.rs:32` is deleted rather than reworded. `just shaders`
regenerates every example, `just test` passes with the `mltrs-cli` snapshots
re-accepted (`cargo insta test --workspace --accept`), `cargo check --workspace
--all-targets` is clean, and `just sweep` still passes — hot reload is the
consumer that keeps the boxed entry alive, so a manual edit to a watercolor
`.slang` file while it runs is worth confirming too.

## 10. The codegen emits unformatted Rust and relies on the caller running `cargo fmt`

**Context.** Surfaced while adding the `just mltrs` passthrough recipe
(`justfile:82`), which runs the cli with arbitrary arguments and — unlike
`just shaders` — does not append `cargo fmt`. Running
`just mltrs shaders compile --crate-dir examples/sdf_2d` immediately produced a
three-file diff in `examples/sdf_2d/src/generated*` against the committed,
formatted output. Nothing semantic changed: a missing newline at EOF in all
three files, two blank lines the templates emit, and one line rustfmt wraps
(`const SHADERS_SOURCE_DIR: &'static str = concat!(…)`).

**The problem.** `write_precompiled_shaders` renders askama templates and writes
the result verbatim — `write_generated_file`
(`crates/cli/src/build_tasks.rs:1256`) is a `create_dir_all` plus an
`fs::write` with no formatting pass. The committed files under
`examples/*/src/generated/` are formatted only because the two recipes that
invoke the cli follow it with a whole-workspace `cargo fmt`
(`justfile:69`, `:77`, and the windows arm at `:103`).

That makes formatting a property of *how the cli was invoked*, not of the cli.
A consumer following the documented workflow in `CLAUDE.md` —
`cargo add mltrs`, `mltrs shaders init`, `mltrs shaders compile` — gets
unformatted files with no newline at EOF, and then gets a spurious diff in
generated code the first time they run `cargo fmt` on their own crate. The
generated module is committed by design (that is the whole point of checking in
`src/generated/`), so the churn lands in their version control.

**Why it's tolerable today.** Only the examples consume the cli, and both
recipes that drive it format afterwards, so nothing in-repo is visibly wrong.
The output is valid Rust either way — this is cosmetic churn, not miscompiled
code.

**Fix — Option A: shell out to `rustfmt` from `write_generated_file`.**

Pipe `source_file.content` into `rustfmt --edition 2024 --emit stdout` on
stdin and write what comes back. Best-effort: if the binary is missing (rustup's
minimal profile omits the `rustfmt` component) or exits non-zero, write the raw
content and warn rather than failing the compile. Piping instead of passing a
path means a rustfmt failure can never leave a half-written file on disk.

Details that matter:

- **Not `cargo fmt`.** It formats the consumer's entire crate, which is the
  collateral damage this entry is about — the cli must touch only the files it
  generates.
- **`--edition` must be explicit.** rustfmt defaults to edition 2015; the
  workspace is 2024 (`Cargo.toml:8`).
- **No env-var override.** Cargo honors `RUSTFMT` for the binary path, but
  adopting that would collide with the repo rule against `std::env::var` outside
  `crates/renderer/src/env_config.rs`. Plain `PATH` lookup is the fit here.
- Once this lands, `just shaders` and `just vendor-shaders` can drop their
  trailing `cargo fmt`, and `just mltrs` inherits correct output for free.

**Rejected alternatives**, recorded so they aren't re-litigated:

- **`prettyplease` + `syn`** (in-process, hermetic, no toolchain dependency) —
  ruled out because it drops non-doc `//` comments, and the templates carry
  load-bearing ones: the `repr(int)` UB explanation
  (`crates/cli/templates/shader_atlas_entry.rs.askama:51-54`), the glam
  scalar-math warning, and the descriptor-set-order `NOTE` at `:142`.
- **Fixing the templates alone** (askama whitespace control `{%-`/`-%}` plus a
  trailing newline) — kills the specific diffs above with zero dependencies, but
  cannot match rustfmt's line-wrapping, so a consumer running `cargo fmt` still
  gets churn. Worth doing anyway as the quality floor for Option A's
  rustfmt-unavailable fallback path.

**The cost to accept, and it is the reason this is an entry rather than a
commit.** The snapshot tests glob generated files off disk
(`crates/cli/src/build_tasks.rs:1806`, `:1973`, both
`insta::glob!(…, "**/*.{rs,json}")`), so formatting at write time changes every
`.rs` snapshot — a one-time `cargo insta test -p mltrs-cli --accept` — and
afterwards those snapshots encode the *local* rustfmt's output. A rustfmt
version bump could then fail CI on formatting alone. For generated code this
simple that is unlikely, but whoever takes this should decide knowingly. The
alternative that avoids it entirely is to snapshot `GeneratedFile.content`
before the format pass instead of reading the files back from disk, which is a
larger change to the test's shape.

**Done means.** `mltrs shaders compile` in a bare consumer crate — no justfile,
no `cargo fmt` — writes files that a subsequent `cargo fmt` leaves byte-identical.
`just shaders` produces no diff against the committed `examples/*/src/generated/`
with its `cargo fmt` removed. A `PATH` with no `rustfmt` still compiles
successfully, with a warning and unformatted-but-valid output.

## 11. Vendored slang modules conflate engine API with example-shared conveniences

**The problem.** `crates/cli/vendor/` holds five slang modules, and
`mltrs shaders init` writes all five into every consumer's `shaders/source/`
(`VENDORED_MODULES`, `crates/cli/src/main.rs:65-84`; `just vendor-shaders`,
`justfile:71-77`, re-seeds all 16 examples). They are delivered as one
undifferentiated set. They are not one thing:

| module | example crates using it | engine coupling |
|---|---|---|
| `addr.slang` | 7 — dragon, gpu_picking, particles, ray_marching, space_invaders, sprite_batch, watercolor | **hard**: mirrors `crates/renderer/src/renderer/addr.rs` |
| `mvp.slang` | 16 (all) | soft: the `columnMajor` extern |
| `projection.slang` | 5 — dragon, gpu_picking, ray_marching, space_invaders, sprite_batch | soft: the same extern |
| `fullscreen_triangle.slang` | 7 — dragon, gpu_picking, koch_curve, ray_marching, sdf_2d, serenity_crt, watercolor | none |
| `super_sample.slang` | 2 — ray_marching, sdf_2d | none |

`addr.slang` is genuine engine API: its three typealiases have to stay in lockstep
with `Addr`/`ReadAddr`/`ImmutableAddr` (`crates/renderer/src/renderer/addr.rs:9`,
`:64`, `:128`), the comments in both files say so, and a consumer could not write
them correctly on their own. The other four are conveniences that the examples
happen to share. `MVPMatrices` bakes in one particular model/view/proj triple and
one Y-flip convention (§7); `superSample` is a twenty-line unrolled loop that
touches nothing engine-owned. Shipping them through `shaders init` presents them
as engine contract, which means a change to a convenience utility is a breaking
change to every consumer's source tree.

The mirror image of the same gap: the modules that examples *actually* share are
duplicated by hand with no mechanism at all. `ray_march.slang` lives in dragon and
ray_marching; `ray_march_camera.slang` in dragon, gpu_picking and ray_marching.
Byte-identical today (md5-verified, 2026-08), but nothing makes them stay that way.
The single-crate shared modules — `dragon_curve`, `gpu_picking_common`, `particle`,
`tev`, `watercolor_common` — are not duplicated and are not part of this.

So one mechanism vendors things that don't need vendoring, and nothing covers the
things that are in fact being copied.

**Why it's tolerable today.** Nothing ships from this repo and there is no outside
consumer, so the oversized public surface costs nothing yet. Every example's seeded
`mltrs/` copy is byte-identical to `crates/cli/vendor/` (verified 2026-08), and the
hand-duplicated example modules are two files across three crates. The bill arrives
the first time an external consumer pins against `MVPMatrices`, or the first time
one of those two files is edited in one crate and not the others.

**Two things a split does *not* fix, both worth knowing before starting.**

1. **`columnMajor` stays engine API even if `MVPMatrices` doesn't.** Both
   `mvp.slang:8` and `projection.slang:8` declare
   `extern static const bool columnMajor`, and the value comes from a module the
   renderer *generates at compile time*: `load_cpu_constants_module`
   (`crates/renderer/src/shaders.rs:23-35`) emits
   `export static const bool columnMajor = …` inside `namespace mltrs`, driven by
   `MATRIX_LAYOUT` (`shaders.rs:14`). Demoting those two modules to the examples
   side does not decouple them — it just moves the coupling somewhere less
   visible. Either the extern becomes documented public API in its own right, or
   `mvp`/`projection` keep a foot in the engine set.
2. **It buys no namespace isolation.** Reflection records type names *unqualified*
   into a flat map, so every public struct/enum name must still be unique across a
   crate's entire `shaders/source/` regardless of which side it came from — see §4,
   which is the same flat-map hazard one level up. The win here is API surface and
   churn control, not scoping.

**Fix — two delivery paths instead of one.**

- **Engine set**, staying in `VENDORED_MODULES` with `mltrs shaders init` as its
  delivery: `addr.slang`, plus whatever the `columnMajor` extern forces to come
  along. This is the set the engine promises to keep stable.
- **Examples set**, delivered by a separate recipe (`just sync-example-shaders` or
  similar) from a single in-repo home, never written into a consumer project. It
  should cover both the demoted engine modules and the already-hand-duplicated
  `ray_march.slang` / `ray_march_camera.slang` — absorbing those is the concrete
  payoff, since they get a source of truth they have never had.

Mechanical consequences to plan for: `VENDORED_MODULES` becomes two lists, and
`LEGACY_MODULES` (`crates/cli/src/main.rs:88-94`) — which *deletes* stale
pre-namespace copies — needs a story for files that change sides, since a demoted
module left behind in a consumer's `shaders/source/` is exactly the case it exists
to clean up. The `mltrs.slang` prelude (`crates/cli/vendor/mltrs.slang`)
`__exported import`s all five, so dropping any changes what `import mltrs;` yields
and every affected example's imports move with it. `just vendor-shaders` grows a
second loop rather than being replaced — the examples need both mechanisms.

Whichever way it lands, the new recipe wants the drift check that §8(b) describes:
copies with a source of truth and no verification are what this entry is about.

**Interaction with §8.** §8's cheap partial win is adding
`crates/cli/fixtures/shaders` to the `shaders init` loop, retiring 6 of its 14
hand-maintained copies. If modules leave the init set that win shrinks to whatever
stays vendored, and the fixture copies of the demoted modules have to come from the
new mechanism instead. Doing §8 first is fine; doing this first means §8's partial
win has to be re-scoped. Related: `fullscreen_triangle.slang` carries its own
`reflectY` (`:26-29`) independent of `mvp.slang`'s, already flagged in §7 — if
those two modules end up on opposite sides of the split, §7's "neither module tells
a reader the other exists" gets meaningfully worse, so land §7's documentation
first or at the same time.

**Open question, left visible rather than resolved.** Whether `mvp`/`projection`
can honestly be demoted at all given the `columnMajor` coupling. The clean split
may be `addr` + `mvp` + `projection` as engine, `fullscreen_triangle` +
`super_sample` as examples — which is a smaller win, but an honest one.

**Done means.** `mltrs shaders init` in a bare consumer crate writes only the
modules the engine is prepared to keep stable, and each one's docs say why it is
engine API. The example-shared modules have exactly one in-repo source of truth and
a recipe that regenerates every copy from it. Editing a copy in place is caught —
by that recipe or by `just pre-commit` — rather than committed. `just shaders` and
`just test` pass with the examples' imports updated for whatever left the `mltrs`
prelude.

## 12. The sweep's fixed timeout fails the two pipeline-heaviest examples on a GPU-less box

**Context.** Measured in a fresh container (4 cores, no GPU, lavapipe
`llvmpipe (LLVM 20.1.2, 256 bits)` as the only working ICD) while investigating
a reported watercolor failure. Sharpens the `SWEEP_TIMEOUT` paragraph in §6,
which saw one of the two failure modes on one of the two affected examples.
Nothing is wrong with either example — this is a measurement-harness entry.

**The problem.** `scripts/headless-sweep.sh` gives every example the same
`SWEEP_TIMEOUT` (default 10s) to reach its first frame. That budget is tuned for
a real GPU. Under lavapipe every pipeline is JIT-compiled by LLVM at creation,
and debug builds additionally slang-compile every shader at startup through the
hot-reload path (`create_from_atlas`). A full sweep at the default gives
**13 ok / 1 skip / 2 fail**, both failures spurious:

| example | pipelines | verdict at 10s | at 16s | at 18s |
|---|---|---|---|---|
| `watercolor` | 21 (11 shaders × ping/pong parity) | `no clean teardown`, exit 137 | `no frames` | ok |
| `multi_mesh` | 17 (1 shader, `P_CUBE`…`P_GRAY_UNORM`) | `no frames` | ok | ok |
| every other example | 1–2 | ok | ok | ok |

The driver is **pipeline count, not shader count** — `multi_mesh` has a single
`.shader.slang` and is the second-slowest to first frame. Slang compilation is
the smaller half: compiling watercolor's 11 shaders via `mltrs shaders compile`
takes ~4.8s of its ~17-18s startup (`basic_triangle`, 1 shader: ~1.0s), so the
remaining ~12s is pipeline creation.

**Why the 10s spelling is the bad one.** Where SIGTERM lands decides which
failure you get, and the early one points at the wrong entry:

- **before SDL's event loop exists** (watercolor at 10s) — SDL never converts
  SIGTERM to `SDL_QUIT`, so `-k 5` SIGKILLs it 5s later. Exit 137 hits the
  `143 | 137` arm (`headless-sweep.sh:282`), whose message —
  `FAIL(no clean teardown): watercolor died on a signal` — was written to catch
  a process that skipped `drain_gpu` and `Drop for Renderer`, i.e. **§1**. The
  log is empty at `RUST_LOG=warn`, so there is nothing in it to contradict the
  reading. A process that never finished *starting* is indicted for a teardown
  bug it does not have.
- **after the loop is up, before the first frame** (both at 15-16s) — clean
  exit, zero frames, `VKR_SWEEP` exit 3, `FAIL(no frames)`. This is §6's
  spelling: still false, but at least it names the thing that went wrong.

Both are false reds, and the sweep's whole value is that a red means something.

**Why it's tolerable today.** On a developer box with a real GPU the margin is
wide and the sweep is green. The knob already exists and needs no code change —
`SWEEP_TIMEOUT=25 ./scripts/headless-sweep.sh` passes clean. The cost is
paid by whoever runs the sweep somewhere new: two failures, one of which sends
them reading `Drop for Renderer`.

**Adjacent, same cause, smaller margin.** The self-test's own budget is
`SWEEP_SELF_TEST_TIMEOUT` (5s) against `basic_triangle`. It SIGKILLed once on a
cold page cache immediately after a build — `FAIL: self-test: exit 137, but not
from the injected viewport fault`, empty log — then passed at 5s, 10s and 15s
once warm. That message aborts the entire sweep by design (a broken detector is
worse than no sweep), so the flakiest budget in the script is also the one that
takes everything else down with it.

**Fix — pick one; they are not exclusive.**

1. **Per-example overrides.** An associative array of exceptions in the script
   (`watercolor=30 multi_mesh=25`), defaulting to today's 10s. Cheapest, keeps
   the fast examples fast, but hard-codes a list that drifts as examples grow —
   the next pipeline-heavy example is red until someone edits the script.
2. **Scale the budget when the ICD is software.** The script already resolves
   `$lvp_icd` and pins `VK_ICD_FILENAMES` to it (`:44-53`), so it knows it is on
   lavapipe before it runs anything: multiply the default there. Self-adjusting,
   one line, and it targets the actual variable — a GPU box keeps the tight
   window that makes a hang obvious.
3. **Make the diagnosis honest regardless of the budget.** Independent of 1/2
   and the most valuable of the three: have the `137` arm distinguish
   "died before presenting a frame" from "died during teardown" before blaming
   the latter. The renderer already counts presented frames for `VKR_SWEEP`'s
   exit 3; a marker line at first present would let the script say
   *"never reached the first frame in Ns — widen `SWEEP_TIMEOUT`"* instead of
   accusing §1. Note the sequencing constraint: raising the default (1 or 2)
   *hides* the 137 spelling without fixing it, so if only one lands, land this.

**Done means.** `just sweep` passes on a GPU-less 4-core box with no env vars
set. An example that genuinely hangs, and one that genuinely leaks at teardown,
still fail — with different messages, neither of which says `no clean teardown`
for a startup that ran out of time. `just sweep-self-test` does not depend on
page-cache warmth.

## 13. The engine cannot screenshot itself, so the gate for validation-invisible bugs is external tooling

**Context.** Written after Phase 6 of
[`bindless_textures.md`](bindless_textures.md) (2026-08), which is the fourth
piece of work in this repo whose *actual* verification was a screenshot. Not a
bug — the renderer is correct; this entry is about the verification path being
outside the repo.

**The problem.** There is a whole class of change here that validation, the
sweep, the snapshots and the compile-time layout asserts all pass cleanly on,
and that is only falsifiable by looking at pixels:

- **BDA layouts** — `vulkan_1_3_migration.md:147` says it outright: "visual
  confirmation via window screenshots (**the real gate** — BDA layout bugs are
  invisible to validation)".
- **Bindless heap slots** — a wrong slot samples the wrong texture with no
  validation output at all (bindless_textures.md Phase 6).
- **Per-draw material indices** — bindless_textures.md Phase 9 states "a green
  sweep is weak evidence here" and specifies a four-point visual A/B as the
  real check.
- **Asset conversion correctness** — `link_rendering/` phases 6-9 are built
  almost entirely on screenshot comparison against a noclip.website oracle.

For every one of those, the engine's contribution is nothing. `just sweep`
answers "did it emit validation errors and present a frame", which is a
different question. The capture has to come from whatever the developer's
desktop happens to provide, which makes the gate a property of the machine
rather than of the project — and it silently degrades to "not checked" on any
box where the tooling doesn't work.

**Measured, this session.** The recipe recorded as reusable in
`link_rendering/phase_07.md:497-500` —
`cosmic-screenshot --interactive=false --modal=false --notify=false -s DIR` —
**failed**, twice, with `Error taking screenshot: Portal request didn't
succeed: Other`. The XDG desktop portal needs an interactive session and this
one wasn't. Of `grim`, `scrot`, `maim`, `gnome-screenshot` and `spectacle`,
**none** were installed; only ImageMagick's `import` and `ffmpeg` were. What
worked was a three-step workaround that has nothing to do with the renderer:

```bash
SDL_VIDEODRIVER=x11 ./target/debug/depth_texture &     # force an XWayland window
W=$(DISPLAY=:1 xwininfo -root -tree | grep '"Depth Texture"' | awk '{print $1}')
DISPLAY=:1 import -window "$W" out.png
```

It captures the window including its letterboxed black margins, at whatever
moment the compositor happens to hand over, and it is specific to
Wayland-with-XWayland. On a pure-Wayland box without XWayland, or in a
container with no compositor, there is no fallback at all — which is precisely
where `link_rendering/phase_07.md:508-518` records the whole visual gate being
deferred to "a machine with a GPU", weeks after the code landed.

**Why it's tolerable today.** It has always eventually worked, because the
person doing the work has been on a desktop. The cost is that every session
re-derives the capture method (this one burned four tool probes before finding
a working path), the recorded recipe rots, and the evidence that a phase was
visually verified lives in prose rather than in an artifact anyone can re-run.

**The important scoping point: the blocked thing is not the needed thing.**
[`offscreen_testing.md`](offscreen_testing.md) §9 already designs the capture
machinery in detail — `src/renderer/capture.rs`, where the
`cmd_copy_image_to_buffer` goes (right after the `resolve_to_blit_src` barrier,
pre-upscale and pre-egui), the `BufferMemory::Readback` allocation, the
BGRA→RGBA swizzle, and the three prerequisite fixes (`format_block_info` must
learn BGRA; two barrier stage masks must widen `BLIT` → `ALL_TRANSFER` or the
copy is itself a sync-validation error; `recreate_swapchain` must drop the
capture). But that is filed under "**Phase 2 — golden images**", and Phase 2 is
deferred on §12's genuinely hard, deliberately-open question: *which driver do
you bless goldens on?*

**Capture does not depend on that question.** Golden *comparison* needs
determinism, a virtual clock (§10), reproducible SPIR-V (§11) and a blessed
driver. Writing the current frame to a PNG on request needs none of them — a
human looks at it. Bundling the two is what has kept a ~150-line feature behind
a research problem for a year. Splitting them off is the entire content of this
entry.

**The in-repo precedent is already load-bearing.** `picking.rs` does exactly
this copy every frame — `cmd_copy_image_to_buffer` from the picking image into
per-flight-slot readback buffers (`crates/renderer/src/renderer.rs:1868`).
A screenshot is that same call with a full-extent region instead of a 1×1 one,
and single-buffered instead of per-flight-slot. The synchronization is *easier*
than picking's, not harder: picking tolerates two frames of staleness by design,
while a capture can simply be read after the `drain_gpu()` that `run_loop`
already performs.

**Fix — the smallest useful version.**

1. `src/renderer/capture.rs` per `offscreen_testing.md` §9, including its three
   prerequisite fixes. Land the two barrier-mask widenings first and confirm a
   clean `just sweep`, as §9 advises — the sweep makes that check free.
2. Trigger it two ways: a **key binding** (F12, alongside the existing debug
   keys) for interactive use, and a **`VKR_CAPTURE_FRAME=N`** env setting on
   `EnvConfig` (`crates/renderer/src/env_config.rs` — the only place allowed to
   read env vars) that captures frame N and exits, for scripted and headless
   use. The latter is what makes a capture work under
   `SDL_VIDEODRIVER=offscreen`, i.e. in a container with no compositor at all,
   which no external tool can do.
3. PNG via the `image` crate — already a workspace dependency with the `png`
   feature (`Cargo.toml:28`), though currently only `convert-link` pulls it in,
   so `mltrs-renderer` gains a dependency.

Explicitly **not** in scope: golden comparison, blessing, a virtual clock, and
`VKR_VALIDATION` (§5 owns that one). Those stay with `offscreen_testing.md`
Phase 2 and its open §12. If capture lands here, Phase 2 shrinks to exactly the
determinism problem, which is the honest shape of what's actually hard.

**Done means.** `VKR_CAPTURE_FRAME=60 ./target/debug/depth_texture` writes a PNG
of frame 60 and exits 0 under `SDL_VIDEODRIVER=offscreen` with lavapipe, on a
box with no compositor and no screenshot tool installed. F12 in a windowed run
writes the same thing. The captured pixels are pre-upscale and pre-egui at
`render_extent`, so two runs at different window sizes are comparable.
`link_rendering/phase_07.md`'s tooling note and this entry both get replaced by
a pointer at the engine feature, and the next phase that needs a visual A/B
produces artifacts a reviewer can open rather than a paragraph asserting someone
looked.

## 14. Multiple `ParameterBlock` globals are supported end-to-end but never exercised

**The problem.** Reflection accepts any number of `ParameterBlock<T>` globals
per shader, and the whole pipeline behind that is deliberately multi-set aware —
yet **no shader in the workspace declares more than one**. Counted across every
`examples/*/shaders/source/` and every `crates/cli/fixtures/` shader: the
maximum is 1. So the N-block path has never run in a test, a snapshot, or a
sweep, and nothing in CI would notice if it broke.

It is not vestigial. Probed directly (throwaway test, two sibling blocks each
holding a `float4` and a `Sampler2D`):

```
GLOBALS: 2
SETS: [ {binding 0 constantBuffer, binding 1 combinedTextureSampler},
        {binding 0 constantBuffer, binding 1 combinedTextureSampler} ]
TEXTURES: ["atex", "btex"]
BUFFERS:  ["pa_buffer", "pb_buffer"]
```

Two descriptor sets, and `Resources` gains one field per texture plus one buffer
per block, in set order. The renderer consumes it correctly too:
`create_descriptor_sets` (`crates/renderer/src/renderer.rs:4310`) walks sets in
layout order carrying a running index *per resource kind*, so set 0 takes
`pa_buffer`/`atex` and set 1 takes `pb_buffer`/`btex`.

**The design is intentional**, on four pieces of in-repo evidence:
`create_descriptor_sets` carries a comment diagramming
`frame_0_set_0_binding_0 … frame_0_set_1_binding_1`;
`DescriptorSetLayoutBuilder::reserve_slot` cites slang's nested-parameter-block
ordering rules and reserves a slot to keep indices correct; `build()` handles a
`None` slot for "a ParameterBlock that ended up only containing other
ParameterBlocks"; and the generated `pipeline_config` carries "NOTE each of
these must be in descriptor set layout order in the reflection json" — a
contract that is vacuous with one set.

**Why it's tolerable today.** One block per shader is what every example wants,
and the single-block path is covered by 25+ fixtures.

**Why it is nonetheless a hazard.** The ordering contract between codegen's flat
per-kind vectors and the renderer's running per-kind indices is currently held
together by that comment alone. A single-block shader cannot distinguish a
correct implementation from several wrong ones, so any refactor of
`collect_parameter_block`, `resources_struct` or `create_descriptor_sets` is
unverifiable in that dimension. There is also a live subtlety no test pins:
within a block, codegen pushes textures *before* the block's uniform buffer,
while the layout puts the buffer at binding 0 and the textures after. That
reordering is harmless only because the two kinds live in separate vectors with
independent indices — order matters within a kind, not across. Nothing states
that, and nothing would catch someone merging the vectors.

**On removing support instead — the slang docs argue against it.** There is no
explicit multi-block example in the slang documentation; every sample shows one
(`docs/language-guide.md:81`, `docs/user-guide/09-reflection.md:359,657`). But
the stated *rationale* only pays off with more than one.
`docs/user-guide/a2-01-spirv-target-specific.md:196-200`: "a `ParameterBlock<T>`
introduces a new descriptor set ID … designed specifically for
D3D12/Vulkan/Metal/WebGPU, so that parameters defined in `T` can be placed into
an independent descriptor table/descriptor set … This allows the user
application to create and pre-populate the descriptor set and reuse it during
command encoding". Independent pre-population and reuse *is* the
per-update-frequency split — per-frame view params, per-material, per-object —
and that is the canonical reason to have several. Removing support would
foreclose the pattern parameter blocks exist to enable, to delete a loop that
already works. Not recommended.

**Fix.** Cover it rather than remove it, cheapest first:

1. An alignment fixture with two blocks, each carrying a texture and distinct
   field layouts. `alignment_tests` discovers fixtures automatically, snapshots
   the two-set reflection, and `cargo check`s the generated `Resources` against
   `fixtures/check_crate` — this pins the codegen half, including the flat-vector
   ordering, for the price of one `.slang` file.
2. Write the ordering contract down where it can be checked: a debug assert in
   `create_descriptor_sets` that each per-kind index lands exactly at the end of
   its vector once every set is walked. A miscount currently writes the wrong
   resource into a descriptor with no validation error at all.
3. Only if an example ever wants it: a real multi-block example, which is what
   would actually exercise the renderer half under `just sweep`.

**Done means.** A two-block fixture is committed and snapshotted, and
`create_descriptor_sets` fails loudly rather than silently mis-binding when the
flat vectors and the set layouts disagree. The `Resources` ordering comment in
`shader_atlas_entry.rs.askama` can then point at the fixture instead of asking
the reader to take it on faith.

## 15. `Game::draw` takes `&mut self`, so a frame-scoped GPU address can be stashed across frames

**Context.** Raised while landing Phase 7c of
[`bindless_textures.md`](bindless_textures.md) (2026-08-09), as a question about
whether a `&self` receiver could have replaced the `flight_slot` assert that
phase added. It cannot — see "What this is *not*" below — but the underlying
hazard it points at is real and unowned, so it is recorded here.

**The problem.** `Game::draw` takes `&mut self`
(`crates/mltrs/src/game/traits.rs:32`):

```rust
fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError>;
```

Every device address the engine hands a game is valid for **one frame only**.
`create_immutable_buffer` (`renderer.rs:1035`) allocates one buffer per flight
slot, so `Gpu::current_immutable_addr` and its `FrameRenderer`
twin return a different `u64` depending on `flight_slot`, which cycles with
`MAX_FRAMES_IN_FLIGHT = 2`. A game that caches one in a field and reuses it next
frame reads the *other* slot's buffer — stale data, not a crash. Nothing in the
type system says so: `ImmutableAddr<T>` is an 8-byte `Copy` newtype whose
`to_raw()` is public, and `&mut self` lets a game write it straight into its own
state.

The failure mode is the worst kind: the address is a live, mapped, correctly
aligned allocation of the right type, so there is no validation message, no
fault, and no crash — just data one frame out of date, alternating every frame.

**Why it's tolerable today.** No example does it. The only address-bearing
example is `sprite_batch`, which mints inside the `submit_draws` closure and
writes the result straight into the param struct it is building. And a stale
address is *self-consistent* in the common case — a buffer the CPU rewrites with
similar data every frame looks fine when read one slot late, which is precisely
why this would be found by staring at a diff rather than by a tool.

**What this is *not*.** It is not the invariant the Phase 7c assert
(`renderer.rs:2481`) protects. That one is entirely renderer-internal: it checks
that `flight_slot` is unchanged between `FrameRenderer` reading it at queue time
and `Gpu` being built from it later in the same `Renderer::draw_frame`. The game
is not a participant, so no receiver on `Game::draw` can detect or prevent it.
The two entries share a subject (per-frame addresses) and nothing else.

**Fix — and `&self` alone is not it.** Three options, weakest first:

1. **`fn draw(&self, …)`.** Blocks the obvious `self.cached = addr`. It does not
   block `Cell`/`RefCell`, a `static`, or `addr.to_raw()` into a plain `u64`
   field. It also **breaks the entire write API**: `write_uniform`,
   `write_storage` and `write_immutable` all take `&mut` *handles*
   (`renderer.rs:5399`, `:5420`, and the `get_mapped_mem_for_frame_*` family in
   `renderer/storage_buffer.rs`), and those
   handles live on the game struct — `sprite_batch` does
   `gpu.write_uniform(&mut self.params_buffer, params)` inside its closure. Every
   example would have to move its handles behind interior mutability, which is
   the same escape hatch the change was meant to close. Not recommended on its
   own.
2. **A lifetime brand: `ImmutableAddr<'f, T>`,** tied to the `Gpu<'f>` /
   `FrameRenderer<'f>` borrow, so the address cannot outlive the frame. This is
   the mechanism that actually makes stashing a compile error. The cost is that
   the generated param structs hold `ImmutableAddr<T>` fields and are `#[repr(C)]`
   PODs memcpy'd into mapped memory, so the lifetime propagates through
   `gather_struct_defs` (`crates/cli/src/build_tasks.rs`) into every generated
   struct and every example that names one. Worth pricing before committing.
   Note it must *not* forbid the legitimate case — writing the address into a
   param struct that outlives the closure is the whole point (the same
   realization that moved `bindless_handle` off `Gpu` in Phase 5 of
   `bindless_textures.md`: "a handle written into a param struct outlives the
   draw closure regardless").
3. **Remove the hazard for the buffers that don't need the ring.**
   [`bindless_textures/phase_07d.md`](bindless_textures/phase_07d.md) proposes a
   non-ringed handle for upload-once static data: one allocation, one stable
   address for the process's life. An address minted from that type is safe to
   stash by construction, which shrinks this entry to the genuinely per-frame
   buffers rather than solving it. Cheapest real progress, and already planned.

**Widened by Phase 7c, worth knowing.** Before 7c, addresses could only be minted
inside the `submit_draws` closure. `FrameRenderer::current_immutable_addr` and
`::current_immutable_addr_at` now mint at queue time too, where `&mut self` is
plainly in scope and the natural place to put the result is a local — or a field.
The surface is wider than when this was last implicitly safe.

**Done means.** Either a game cannot hold a frame-scoped address past the frame
(option 2), or every address a game *can* hold is one whose validity does not
expire (option 3 covering the static case, with the per-frame remainder
documented at the accessors). Failing both, at minimum: `addr.rs` and the four
`current_*_addr*` accessors say in their doc comments that the value is valid for
exactly one frame and must not be cached, which today none of them do.

## 16. Typed device-address minting is split across two layers

**Context.** Phase 7c of [`bindless_textures.md`](bindless_textures.md)
(2026-08-09) needed `ImmutableAddr` minting from *two* surfaces — `Gpu` inside
the submit closure and `FrameRenderer` at queue time — and de-duplicating them
pushed the `ImmutableAddr::from_raw` wrap down into `StorageBufferStorage`
(`immutable_addr_for_frame` / `immutable_element_addr_for_frame`,
`renderer/storage_buffer.rs:186`). That was the right move for those two, and it
left the codebase with the wrap applied in two different layers depending on
which pointer type you ask for.

**The problem.** `u64` → typed wrapper now happens in two places:

| accessor | mints in | via |
|---|---|---|
| `Gpu::addr` (`renderer.rs:5435`) | `Gpu` | `Addr::from_raw` |
| `Gpu::current_addr` (`:5446`) | `Gpu` | `Addr::from_raw` |
| `Gpu::previous_addr` (`:5457`) | `Gpu` | `ReadAddr::from_raw` |
| `Gpu::current_immutable_addr{,_at}` | `StorageBufferStorage` | already moved |
| `FrameRenderer::current_immutable_addr{,_at}` | `StorageBufferStorage` | already moved |

Three raw-mint sites remain, all in `Gpu`, and they are the only callers of
`get_device_address_for_frame` (`storage_buffer.rs:104`) and
`get_device_address_for_frame_gpu_only` (`:243`) outside the module.

The cost is not the split itself — it is what the split prevents.
`Addr::from_raw`, `ReadAddr::from_raw` and `ImmutableAddr::from_raw` are all
`pub(super)` in `renderer::addr`, i.e. callable from *anywhere* in `renderer` and
its descendants. `addr.rs:136-138` states the actual rule in a comment:

```rust
// pub(crate): minting is restricted to Renderer/Gpu accessors that take
// an ImmutableBufferHandle, which upholds the never-GPU-written invariant
// Access.Immutable requires.
```

"Restricted to accessors that take a handle" is exactly the kind of claim a
visibility modifier can enforce, and today it is enforced by nobody. A future
accessor that fabricates an address from arithmetic — the thing Phase 7c's
blocker 2 exists to prevent — compiles fine.

**Why it's tolerable today.** Three call sites, all correct, all in one `impl`
block, and the immutable family (the one with the strongest invariant, and the
one Phase 9's push-constant work will lean on) is already consolidated. Nothing
is wrong; the wrap is just applied inconsistently.

**Fix.** Move the remaining three, then tighten the visibility — the second half
is the point, and doing only the first half buys tidiness and nothing else.

1. Add `addr_for_frame`, `gpu_only_addr_for_frame` and
   `gpu_only_read_addr_for_frame` to `StorageBufferStorage`, alongside the two
   immutable ones. The `Gpu` accessors become single-line forwarders, as
   `current_immutable_addr` already is.
2. `previous_addr` is the only one with logic to place: the
   `(flight_slot + MAX_FRAMES_IN_FLIGHT - 1) % MAX_FRAMES_IN_FLIGHT` step
   (`renderer.rs:5458`). Leave it in `Gpu` and pass the resolved frame — the
   storage layer has no business knowing about ping-pong semantics, which
   `GpuOnlyBufferHandle`'s own doc comment frames as a property of the *handle
   kind*, not of the slab.
3. Then narrow all three `from_raw`s from `pub(super)` to
   `pub(in crate::renderer::storage_buffer)`, and rewrite the `addr.rs` comment
   above from a promise into a description of what the compiler now checks. Also
   consider making the raw `get_device_address_for_frame*` getters private to the
   module once nothing outside calls them.

**Two things this does not cover**, named so they don't read as oversights:

- `From<Addr<T>> for ReadAddr<T>` and `From<ImmutableAddr<T>> for ReadAddr<T>`
  (`addr.rs:84`, `:152`) build the struct literally rather than through
  `from_raw`. They convert an address that was already minted legitimately, so
  they do not weaken the invariant — but they do mean `addr.rs` keeps a
  construction path of its own, and "one layer" is precise only about *raw u64*
  entry.
- `TextureHandle::bindless_handle` mints a `BindlessHandle` from a heap slot
  (`renderer/bindless.rs`, `renderer/texture.rs`). Same shape of idea, but the
  raw is a `u32` slot rather than a device address and the owner is the texture
  slab, not the buffer slab. If a general "one place mints typed handles" rule is
  wanted it should follow this entry, not be folded into it.

**Done means.** `Addr`, `ReadAddr` and `ImmutableAddr` can only be constructed
from a raw `u64` inside `renderer/storage_buffer.rs`, enforced by their
`from_raw` visibility rather than asserted in a comment; every `Gpu` and
`FrameRenderer` address accessor is a forwarder with no `from_raw` of its own;
and `just sweep` still passes, since this is behaviour-preserving throughout.

## 17. Picking is a second rendering path rather than a pass, so every new capability must be re-implemented or refused

**Context.** Phase 8 of [`bindless_textures.md`](bindless_textures.md)
(2026-08-11) added per-draw push constants, and could not give them to picking.
The result is an `anyhow::ensure!` in `create_picking_pipeline`
(`renderer.rs:1283`) rejecting any picking shader that declares a push block.
That check is correct and cheap — but it is the *third* time a feature has had to
carve picking out, and the carve-outs are the symptom rather than the problem.

**The problem.** Picking is not a pass in the rendering system; it is a parallel
copy of one. Grep `picking` in `renderer.rs` and the pattern is unmistakable —
almost every core concept has a picking-shaped twin:

| the main path | picking's twin |
|---|---|
| `PipelineHandle<T>` (`pipeline.rs:88`) | `PickingPipelineHandle` (`:104`) |
| `PipelineStorage::add` (`:117`) | `add_picking` (`:128`), `get_picking` (`:142`) |
| `create_pipeline` (`renderer.rs:1209`) | `create_picking_pipeline` (`:1262`) |
| `descriptor_sets_for_frame` (`:2419`) | `picking_descriptor_sets_for_frame` (`:2517`) |
| the `pending_draws` queue | `PickingDrawConfig` (`:6037`), threaded as an `Option` through `draw_frame` and `record_command_buffer` |
| `DrawCallConfig` | a hardcoded `cmd_draw(3, 1, 0, 0)` (`:1880`) |
| `submit_draws` | `draw_vertex_count_with_picking` (`:5972`) |

The two descriptor-set accessors are **byte-identical** apart from how they
resolve the pipeline — a duplication `original_compute_shaders_plan.md:431`
already flagged when compute threatened to add a third copy. And both handle
kinds index the *same* `PipelineStorage`; the split is purely at the API surface,
not in the storage.

The compounding cost is what the parallel path forces on each new feature:

- **Multi-draw:** picking and the draw queue are mutually exclusive, enforced by
  `debug_assert!(self.pending_draws.is_empty())` (`:5983`). Deferred in
  [`link_rendering.md`](link_rendering.md) §4.5.
- **Push constants:** refused outright, the `ensure!` above. Reopening it is
  Phase 13 of `bindless_textures.md`, and the reason it is not trivial is that
  the main and picking pipelines are different shaders, so the entry point would
  need *two* independent payloads.
- **Next feature:** whatever it is, it inherits the same decision.

None of these is expensive alone. The pattern is what costs — each one is
individually cheap enough to defer, so the asymmetry never gets paid down, and
the bill lands on whoever adds the feature after next.

**Why it's tolerable today.** Everything about picking *works*, and one example
uses it (`gpu_picking`). The bespoke path is small, self-contained, and its
limitations are all guarded rather than silent: the mutual exclusion is a
`debug_assert!`, the push refusal is an `Err` at pipeline creation. Nothing is
wrong; it is just built beside the system instead of inside it.

**Fix — deliberately deferred to a declarative/graph API.** Do not restructure
picking on its own. Every twin above exists because the current API has exactly
one shape for "render the frame", and picking does not fit it; a piecemeal fix
would invent a second abstraction to sit beside the first, which is the thing
that already went wrong. The right moment is whenever the render-graph work in
[`render-graph/`](render-graph/) lands, where picking is naturally just another
node: its own color target and format, one draw, a readback edge.
`original_compute_shaders_plan.md:170` already assumes this — it calls picking's
migration into a unified `PipelineKind::Graphics` "a trivial migration", which is
true of the *pipeline* and not of the six other twins above.

**One thing that happens sooner, and is not this entry's win.** Phase 8b of
`bindless_textures.md` threads the push block type through `PipelineHandle`,
which turns the `ensure!` at `:1283` into a compile error at the call site and
deletes the runtime check. That is a real improvement, but it removes a
*diagnostic*, not the asymmetry — picking still has no push-constant channel.
Do not read 8b landing as this entry being addressed.

**Done means.** Picking is expressed with the same vocabulary as any other pass:
no `PickingPipelineHandle` distinct from `PipelineHandle`, no second
`create_*_pipeline`, no duplicated descriptor-set accessor, no `Option<PickingDrawConfig>`
threaded through the record path, and no mutual exclusion with the draw queue.
The `debug_assert!` at `:5983` and the `ensure!` at `:1283` both delete
themselves rather than being relocated. `gpu_picking` still picks, and
`just sweep` still passes.

## 18. The roc platform's glibc 2.39 floor excludes SteamOS, Debian stable and Ubuntu 22.04 LTS

**Context.** Recorded when `roc-platform/stubs/generate.sh` landed. This is the
one deliberate trade that phase made, and it is a property of the *shipped*
artifact rather than of the repo, so it needs a home outside the phase plan.

**The problem.** `stubs/generate.sh` derives the committed `libc.so` and
`libm.so` stubs from the build machine's own libraries, and asserts that machine
is glibc 2.39. Both the symbol set and the data-object sizes come from there. So
every executable a Roc app author builds against the published platform requires
glibc ≥ 2.39 at run time.

The floor is not forced. Three things pin a hard lower bound at **2.34**: the
libpthread/libdl merge into `libc.so.6`, the `stat` family becoming dynamic
exports, and `Scrt1.o` relying on 2.34's `__libc_start_main` to run the
executable's init array. 2.39 sits five releases above that bound.

What the extra distance buys is real but small: no build container, no SDL3 apt
dependency list, and no allowlist for symbols the build machine references and
an older libc lacks. What it costs:

| distro | glibc | in? |
|---|---|---|
| Ubuntu 24.04+, Debian 13, Fedora 40+, RHEL 10, Arch | ≥ 2.39 | yes |
| Ubuntu 22.04 LTS — supported to 2027, ESM to 2032 | 2.35 | no |
| Debian 12 bookworm — current stable | 2.36 | no |
| RHEL / Rocky / Alma 9 | 2.34 | no |
| Linux Mint 21.x | 2.35 | no |
| SteamOS 3.x | unmeasured | probably not |

**SteamOS is the one that matters, and its number is unmeasured.** The audience
for this platform is PC games, which makes the Steam Deck the most likely target
and so the most likely failure. SteamOS 3 shipped glibc 2.33 in the 2022 3.3
era; 3.7 moved to a newer Arch base, and Valve's release notes do not state the
version. Measure it before treating the floor as settled — in desktop mode,
`ldd --version`. Below 2.39, a Deck build fails at startup with a
`GLIBC_2.39 not found` error from the loader: loud and attributable, but only on
the player's machine.

**Why the exact number is 2.39 and not, say, 2.36.** Thirteen symbols in the
host archive come from newer glibc headers than 2.35: eight `__isoc23_*`
redirects plus `strlcpy`, `strlcat`, `wcslcpy`, `wcslcat` (all glibc 2.38, from
SDL3's `SDL_string.c`, `SDL_cpuinfo.c` and `SDL_hidapi.c`), and `arc4random`
(2.36, from gcc 13's `libstdc++.a`). No LTS base image sits at 2.38, so the
choice is effectively binary: 24.04 and none of them, or 22.04 and all thirteen
need an allowlist plus a container to build in.

**Why it's tolerable today.** Nothing ships from this repo yet, and the current
goal is a roc platform that builds and releases at all. Lowering the floor later
is mechanical — regenerate the stubs against an older glibc — not a redesign.

`roc-platform/ci/all_tests.sh` tests that the generator refuses a mismatched
glibc. Phase 4's regen-diff runs on an `ubuntu-24.04` runner, so it covers the
happy path only.

**Fix — three routes, cheapest first.**

1. **Regenerate in an `ubuntu:22.04` container.**
   [`roc_platform_release/02_stub_generator.md`](roc_platform_release/02_stub_generator.md)
   specifies this in full: `ci/floor.Dockerfile`, a
   `stubs/generate_in_container.sh` wrapper, and a `stubs/above_floor.txt`
   allowlist carrying the thirteen symbols so local development still links.
   The allowlist needs two assertions to be safe — each entry absent from the
   floor `libc.so.6`, *and* absent from the container-measured symbol set —
   or it becomes a way to silence a genuine floor violation. Costs a rust
   toolchain and the SDL3 build dependencies inside the image, plus a docker
   round-trip per regeneration.
2. **Build the host against a floor sysroot, with no container.**
   `cargo-zigbuild --target x86_64-unknown-linux-gnu.2.35` pins the glibc
   version directly, which would make the local and released archives
   identical and retire the allowlist along with the container. The unknown is
   SDL3: its C sources go through cmake and cc-rs rather than the rust linker,
   so the override has to reach `CC`/`CXX` for SDL's feature detection to see
   2.35 headers. Worth a spike before being ruled in or out.
3. **Ship a second target.** roc's `inputs` list is per-target, so a low-floor
   target could carry its own stub set beside `x64glibc`. Most machinery, best
   reach.

**Done means.** The floor is chosen against measured data rather than
convenience: SteamOS, Debian stable and Ubuntu 22.04 LTS each have a recorded
glibc version and an explicit in-or-out decision. If any of them is in,
`REQUIRED_GLIBC` in `stubs/generate.sh` names that lower floor, the stubs are
regenerated against it, and `roc-platform/README.md` states the new number.

## 19. An app that names the roc platform by URL needs `--max-transitive-mb=0`

**Context.** Recorded when `roc-platform/bundle.sh` landed. It is a property of
the shipped artifact, so it lives outside the phase plan.

**The problem.** roc applies two size limits to a downloaded dependency, and
they treat platforms differently:

| limit | flag | default | platform exempt? |
| --- | --- | --- | --- |
| per-package expanded size | `--max-package-mb` | 10 MB | yes |
| per-direct-dependency transitive size | `--max-transitive-mb` | 100 MB | **no** |

The exemption is one boolean, `platform_exempt`. It is set from
`dep.is_platform` in `../roc/src/compile/package_resolution.zig:696`, and read
in exactly one place, `:854`, which is the per-package check.
`checkTransitiveLimits` (`:918`, called at `:479`) never consults it, and
`groupKeyForDep` (`:1004`) never branches on `is_platform`. A platform named by
URL therefore joins the transitive tally like any package.

The platform expands to 161,198,326 bytes against a 104,857,600-byte default.
`ci/bundle_test.sh` measures this on every run. The observed diagnostic:

```
DEPENDENCY TREE TOO LARGE
has pulled more than 104857600 bytes of packages into the build (161198326
bytes so far).
```

So every app author who names the platform by URL passes one flag that carries
no useful meaning to them:

```bash
roc --max-transitive-mb=0 main.roc
```

A local path dependency never trips this. The tally counts URL-fetched nodes
only, which is why `just roc-platform run` passes and the problem stays
invisible until the platform ships.

Two further notes. roc's own diagnostic names a `--max-transitive-bytes` flag
that the CLI does not accept; the accepted spelling is `--max-transitive-mb`.
roc has no test covering a platform against the transitive limit, so the
behaviour is unspecified by its suite rather than deliberate.

**Fix routes, cheapest first.**

1. **Exempt platforms from the transitive limit in roc.** One condition, at
   `package_resolution.zig:918`, symmetric with the per-package exemption that
   already exists. This is the same shape of upstream fix as
   [`roc_interp_fix.md`](roc_interp_fix.md), and it removes the flag for every
   roc platform, not only this one.
2. **Raise roc's default.** Weaker: it moves the number rather than fixing the
   asymmetry, and any platform above the new number hits it again.
3. **Shrink `libhost.a` below 100 MB.** slang dominates the 155 MB, and
   `strip = "debuginfo"` cannot shrink a staticlib. This is real size work with
   its own scope, and it fixes only this platform.

**Done means.** An app author names the platform by URL and runs it with no
size flag. `ci/bundle_test.sh` prints `MEASURED: an app needs neither
--max-package-mb nor --max-transitive-mb.`, and the "Shipping" section of
`roc-platform/README.md` drops the flag.
