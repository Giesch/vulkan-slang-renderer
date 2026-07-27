# Offscreen testing: validation errors and golden images

Status: **design, 2026-07-25. Partly implemented.** Code references verified
against `link-phase-07-plan` @ 6b1868c. Measurements (driver capabilities,
session type) taken on Dan's laptop on the same date.

**What now exists.** The validation half — a sweep that runs every example
under lavapipe and fails on validation output — ships as
`scripts/headless-sweep.sh`, wired to `just headless-all`, and found a real
renderer bug on its first full run. It is a *shell* harness: no
`HeadlessConfig`, no counters, no frame limit, no virtual clock, no capture.
It also does not use Xvfb — `SDL_VIDEODRIVER=offscreen` needs no X server at
all — and it greps the log rather than reading an exit code, for the reason
given in §2 below. See [`build_reproducibility.md`](build_reproducibility.md)
§7 for what it does, what it deliberately owns about its environment, and how
it was adversarially tested. Everything else in this note — §§1, 3-8, and all
golden-image work — is still design.

**Correction (2026-07-27) to point 2 below:** `timeout` sends **SIGTERM**, not
SIGKILL, and SDL converts it into an `SDL_QUIT` event, so the event loop exits
normally and `Drop` *does* run — measured three ways in
[`build_reproducibility.md`](build_reproducibility.md) §7.4. Teardown leaks are
therefore automatable today, provided `SDL_NO_SIGNAL_HANDLERS` stays unset.

Companion documents: [`tech_debt.md`](tech_debt.md) — its §1 catalogues the
teardown-time object leaks that produce validation errors at
`vkDestroyDevice`, which is exactly the class this harness would catch
automatically instead of by eye. This note supersedes the scattered `todo.org`
entries on the subject (lines 5-7, 156-160, 224-227, 249-253, and 494) as the
detailed design; those entries can point here.

---

## Why

Today the only way to check an example for Vulkan validation errors is
`timeout 3 just dev EXAMPLE`, documented in `CLAUDE.md:17`. That has three
problems:

1. It opens a window on the dev machine.
2. ~~`timeout` SIGKILLs the process, so `drain_gpu()` (`src/app.rs:62`) and
   `Drop for Renderer` (`src/renderer.rs:2782`) never run~~ — **wrong, see the
   correction below**; teardown is where the `tech_debt.md` §1 leaks report
   themselves, and it does run.
3. **There is no signal.** `vulkan_debug_utils_callback`
   (`src/renderer/debug.rs:14-49`) only calls `log::error!` and returns
   `vk::FALSE`. A run with 500 validation errors exits 0. Catching them means
   a human reading stderr.

This note also settles a question that comes up whenever "run it in CI" is
proposed: **which cloud service is needed.** The answer is none, for either
half of the problem. See [Why not a cloud GPU](#why-not-a-cloud-gpu).

**Outcome.** `just headless-all` sweeps every example under a software driver
and exits nonzero on any validation error. `just golden-all` captures frames
and diffs them against images blessed on the RTX 3070 Ti.

---

## Why not a cloud GPU

**Validation is a layer, not a driver feature.** The Vulkan loader inserts
`VK_LAYER_KHRONOS_validation` between the application and the ICD:

```
examples/*.rs  →  ash  →  libvulkan.so.1 (loader)
                             ↓
                    VK_LAYER_KHRONOS_validation   ← all checking happens here
                             ↓                      (CPU, its own shadow state)
                    ICD, selected by VK_DRIVER_FILES:
                      nvidia_icd.json  → RTX 3070 Ti
                      lvp_icd.json     → lavapipe: SPIR-V → LLVM JIT → CPU
```

The layer keeps a shadow copy of every object, handle lifetime, image layout,
descriptor binding, and sync scope, and checks calls against the spec before
they reach the driver. That bookkeeping is entirely CPU-side and ICD-agnostic.

Lavapipe — Mesa's Vulkan frontend over llvmpipe, already installed at
`/usr/share/vulkan/icd.d/lvp_icd.json` — reports **Vulkan 1.4.318** and
satisfies every feature `choose_physical_device` requires (`src/renderer.rs:3127`):
`samplerAnisotropy`, `shaderDrawParameters`, `timelineSemaphore`,
`bufferDeviceAddress`, `dynamicRendering`, `synchronization2`. It also exposes
`VK_KHR_swapchain`, `VK_KHR_xlib_surface`, and `VK_EXT_headless_surface`.
`choose_physical_device` sorts `PHYSICAL_DEVICE_TYPE_CPU` last but does accept
it (`:3163`), so lavapipe is selected when it's the only ICD offered.

So validation testing runs on any x86 CPU — this laptop, a free GitHub Actions
runner, anything.

**What lavapipe does *not* cover.** Worth stating so the harness isn't
oversold:

- **Limit- and format-dependent checks.** Validation compares usage against the
  *actual* device's `VkPhysicalDeviceLimits` and per-format
  `VK_FORMAT_FEATURE_*` bits. Lavapipe's differ from NVIDIA's, so a bug that
  only manifests at a limit NVIDIA has and lavapipe doesn't is structurally
  invisible. See the MSAA measurement below for a concrete instance.
- **Real-driver misbehavior.** Validation-clean code can still render garbage
  or hang on NVIDIA — uninitialized reads, races that a software rasterizer's
  serialized execution never exposes.
- **Speed.** llvmpipe is a JIT'd CPU rasterizer; cost scales with
  pixels × fragment-shader complexity. `basic_triangle` is trivial;
  `ray_marching`, `sdf_2d`, and `watercolor` will be slow.

**Golden images are a different problem that lavapipe does not solve.**
Floating-point precision (fma contraction, transcendental ULP tolerances the
spec permits), texture filtering and mip selection, and MSAA all diverge
legitimately between implementations. Goldens are per-driver, full stop. But
this machine has an **RTX 3070 Ti**, so goldens run on real hardware locally —
the same harness pointed at a different `VK_DRIVER_FILES`. No cloud GPU for
either half.

**On Modal specifically** (asked about directly, so recorded here): Modal's GPU
containers run under gVisor, whose `nvproxy` forwards NVIDIA ioctls selectively
and implements a CUDA-centric surface. Graphics support is
[an open request, not a shipped capability](https://github.com/google/gvisor/issues/10856).
Modal's GPU documentation never mentions Vulkan, OpenGL, or EGL. Even setting
that aside, Vulkan in an NVIDIA container needs `NVIDIA_DRIVER_CAPABILITIES` to
include `graphics` so the toolkit mounts `nvidia_icd.json`, which Modal doesn't
document exposing. It is the wrong tool. The same reasoning rules out most
serverless-GPU vendors; if a cloud runner is ever wanted, a plain Docker
container on a non-gVisor host (self-hosted runner, GitHub GPU runner) is the
shape that works.

---

## Approach

One shared substrate — headless mode, frame limit, deterministic clock —
feeding two consumers:

```
just headless EXAMPLE   → Xvfb + lavapipe    → validation counter → exit 1 on error
just golden   EXAMPLE   → XWayland + NVIDIA  → resolve_image → PNG → tolerance diff
```

The codebase is most of the way there already. Game rendering targets an
offscreen `resolve_image` (`src/renderer.rs:138`) that is already
`TRANSFER_SRC | COLOR_ATTACHMENT` (`:216`), MSAA-resolved (`:1761`), and
egui-free — egui draws to the *swapchain* image afterward (`:2036`). The
swapchain is touched in exactly two places: the upscale blit (`:1991`) and that
egui pass. `src/renderer/picking.rs` is a working template for
`cmd_copy_image_to_buffer` → `BufferMemory::Readback` → mapped read.

**No surfaceless refactor.** Threading `Option<Surface>`/`Option<Swapchain>`
through `init`, `check_required_extensions`, `QueueFamilyIndices::find`,
`choose_physical_device`, `create_swapchain`, `recreate_swapchain`,
`record_command_buffer`, and `draw_frame` was considered and rejected: it costs
~10 call sites in `renderer.rs`, and skipping the present path means the
harness can't catch present-related validation errors or exercise `Drop`
honestly. Run the real swapchain under a virtual X server instead.

---

## 1. Configuration — env vars read inside `Game::run()`

Every example's `main()` is just `Foo::run()`, so env vars read inside `run()`
(`src/game/traits.rs:80`) are the only non-invasive knob. **Nothing in
`examples/` changes for validation testing.**

New module `src/headless.rs` (`pub mod headless;` in `src/lib.rs`):

```rust
pub struct HeadlessConfig {
    pub frames: usize,
    pub capture_frame: Option<usize>,
    pub capture_path: Option<PathBuf>,
    pub render_scale: f32,
    pub max_msaa: Option<MaxMSAASamples>,
    pub show_window: bool,
    pub fail_on_warnings: bool,
    pub frame_delay: Option<Duration>,
    pub require_device: Option<String>,
}

impl HeadlessConfig {
    /// Returns None unless VKR_HEADLESS is set to a truthy value.
    pub fn from_env() -> anyhow::Result<Option<Self>>;
    /// Called after run_loop returns, once Renderer has dropped.
    pub fn check_validation(&self) -> anyhow::Result<()>;
}
```

| var | default | meaning |
|---|---|---|
| `VKR_HEADLESS` | unset | master switch |
| `VKR_HEADLESS_FRAMES` | `60` | frames to submit, then quit |
| `VKR_HEADLESS_CAPTURE` | unset | output PNG path; enables capture |
| `VKR_HEADLESS_CAPTURE_FRAME` | `= FRAMES` | 1-based frame to capture |
| `VKR_HEADLESS_RENDER_SCALE` | `1.0` | pins scale when `Game::render_scale()` is `None` |
| `VKR_HEADLESS_MSAA` | unset | `1｜2｜4｜8` override |
| `VKR_HEADLESS_FRAME_DELAY_MS` | unset | virtual clock step override |
| `VKR_HEADLESS_HIDE_WINDOW` | `0` | skip `startup_window.show()` |
| `VKR_HEADLESS_ALLOW_WARNINGS` | `0` | don't fail on validation WARNINGs |
| `VKR_REQUIRE_DEVICE` | unset | substring assert on `deviceName` |
| `VKR_VALIDATION` | `cfg!(debug_assertions)` | force validation on/off (see §6) |

**Deliberately no window-size override.** `examples/sprite_batch.rs:130` and
`examples/space_invaders.rs` derive their projection matrix from
`Self::initial_window_size()`; overriding the actual window size would desync
projection from viewport. Goldens use each example's natural size.

---

## 2. Validation counters — `src/renderer/debug.rs`

Two `static AtomicUsize` counters incremented in the existing severity match
arms of `vulkan_debug_utils_callback` (`:33-46`):

```rust
static VALIDATION_ERRORS: AtomicUsize = AtomicUsize::new(0);
static VALIDATION_WARNINGS: AtomicUsize = AtomicUsize::new(0);

pub fn validation_error_count() -> usize;
pub fn validation_warning_count() -> usize;
```

The return value stays `vk::FALSE` and the `DebugPrintf` INFO special case
(`:38-43`) is untouched. The `match` is on the exact severity flag, which is
safe — debug-utils delivers exactly one severity bit per callback, and the
existing code already relies on that.

**Count unconditionally; gate only the check.** Two relaxed atomic increments
on a path that already does `format!` + `CStr::from_ptr` + a log call cost
nothing, and unconditional counting makes the counters usable from anywhere
later (e.g. an assertion inside an example) without plumbing a flag.

**Where the check runs.** `HeadlessConfig::check_validation()` is called from
`Game::run()` *after* `app.run_loop(...)` returns. `run_loop` takes `self` by
value, so `App` — and therefore `Renderer` — has already dropped, and
`Renderer::drop` (`:2782`) does its `device_wait_idle` plus every destroy call.
Teardown errors are counted, and the whole log has already printed, before the
check fires. It returns `Err`, which gives exit code 1 from `main()` with
`Error: …`. No `std::process::exit`, nothing skipped.

**WARNING is a failure by default.** Only core validation is enabled
(`get_required_layers`, `:2942`) — not best-practices — so warnings are rare
and generally real. `VKR_HEADLESS_ALLOW_WARNINGS=1` is the escape hatch,
needed because lavapipe may emit warnings NVIDIA doesn't.

**Known trap, since fixed:** `get_max_usable_sample_count` falls through to
`TYPE_1` when the requested count is unsupported, and `record_command_buffer`
used to resolve unconditionally from it — a validation error, which the sweep
duly found (`build_reproducibility.md` §7.5). The single-sample path now skips
the resolve attachment entirely, so pinning `VKR_HEADLESS_MSAA=2` under
lavapipe, which lacks 2×, is merely a silent downgrade to 1× rather than an
error — still not what you want for a comparison. See §9.

---

## 3. Frame limit and loop — `src/game/traits.rs:80` + `src/app.rs:29`

`run_loop` gains `max_frames: Option<usize>` and returns
`anyhow::Result<Option<image::RgbaImage>>`, so the capture is read out before
`self` (and thus `Renderer`) drops at end of function while all file IO stays
in `traits.rs`. `image` is a normal dependency, so `app.rs` can name the type.

- **Limit on `renderer.total_frames()`, not a local counter.** `draw_frame`
  increments `total_frames` *after* a successful `acquire_next_image`
  (`:2206`); the `ERROR_OUT_OF_DATE_KHR` early-return at `:2198` does not. A
  local iteration count would let the loop limit and
  `VKR_HEADLESS_CAPTURE_FRAME` disagree. Using `total_frames` makes the capture
  frame number mean the same thing inside `record_command_buffer`.
- **`iteration_cap` (`frames * 4`)** that `bail!`s on a swapchain-recreation
  spin rather than looping forever.
- **Skip the `SDL_DelayPrecise` pacing** (`:52-57`) in headless mode.
- **Keep event pumping.** Under Xvfb SDL has a real X connection; `poll_iter`
  just yields nothing, it costs nothing to keep, and it preserves the
  Quit/Escape path for debugging a mapped headless run.
- **The clock tick lives inside `if !self.minimized`**, paired 1:1 with a draw,
  so elapsed time and frame index stay locked together.

In `run()`:

- `enable_egui = cfg!(debug_assertions) && headless.is_none()`
- `render_scale` falls back to `cfg.render_scale` instead of
  `compute_render_scale_for_display` (`src/game/traits.rs:121`), which returns
  0.5/0.75/1.0 depending on **display** resolution and would otherwise make
  `render_extent` machine-dependent. A `Game::render_scale()` override still
  wins — the example's own intent is part of what's being tested.
- `max_msaa` honors the env override.

**`Renderer::init` keeps its 4-arg signature.** The headless bits arrive via
three post-init setters (`use_virtual_clock`, `enable_frame_capture`,
`assert_device_name_contains`) rather than threading an `Option<&HeadlessConfig>`
into a 250-line constructor.

---

## 4. Virtual clock

Golden images require frame N to be identical every run. Today 11 examples
animate off their own `start_time: Instant`.

New `Renderer` fields beside `total_frames` (`:90`):

```rust
clock_elapsed: Duration,   // wall time windowed; frame_count * frame_delay headless
clock_delta:   Duration,
virtual_clock: bool,       // set by Renderer::use_virtual_clock()

pub(crate) fn tick_clock(&mut self, wall_delta: Duration, fixed_delta: Duration) {
    self.clock_delta = if self.virtual_clock { fixed_delta } else { wall_delta };
    self.clock_elapsed += self.clock_delta;
}
```

`tick_clock` is `pub(crate)` — `app.rs` is in-crate, examples can't desync it.
Accumulating deltas rather than `start.elapsed()` means both modes share one
code path and it stays monotonic.

`FrameRenderer` accessors beside `render_scale()` (`:5517`): `elapsed()`,
`elapsed_secs() -> f32`, `frame_delta()`, `frame_delta_secs() -> f32`,
`frame_index() -> usize`.

Migration shape — drop the local field, read from the renderer:

```rust
// examples/dragon.rs
- let time = (Instant::now() - self.start_time).as_secs_f32();
+ let time = renderer.elapsed_secs();
```

`elapsed_secs()` takes `&self` and `renderer` isn't moved until the following
`draw_vertex_count(...)`, so this has the same shape as the existing
`window_resolution()` call and compiles as-is.

| file | change |
|---|---|
| `dragon.rs`, `sdf_2d.rs`, `koch_curve.rs`, `serenity_crt.rs`, `multi_mesh.rs`, `ray_marching.rs` | `renderer.elapsed_secs()` |
| `viking_room.rs`, `suzanne.rs`, `depth_texture.rs` | `renderer.elapsed()`, `Duration` signatures unchanged |
| `toon_link.rs` | same; migrated so it keeps compiling even though its assets are gitignored |
| `particles.rs` | `renderer.frame_delta_secs()` for the compute `SimParams.delta_time`; drop `last_frame` |
| `sprite_batch.rs`, `watercolor.rs` | **leave alone** — their `Instant` is in `update()` (no `FrameRenderer` in scope) and only feeds an egui FPS label |
| `basic_triangle.rs`, `gpu_picking.rs`, `space_invaders.rs` | no change needed |

Two intentional behavior changes, worth calling out in the commit message:

- Time starts at 0 on the **first frame**, not at `setup()`. Examples with slow
  asset loads (`viking_room`, `suzanne`, `multi_mesh`, `toon_link`) no longer
  jump-start mid-animation. An improvement.
- Time **freezes while minimized** instead of continuing, so there's no
  animation jump on restore. Also an improvement.

---

## 5. Capture — new `src/renderer/capture.rs`

Modeled on `picking.rs` but **single-buffered**, not
`[_; MAX_FRAMES_IN_FLIGHT]` — one capture per process, so there's no slot reuse
to guard.

```rust
pub(super) struct CaptureResources {
    pub target_frame: usize,   // the total_frames value to copy
    pub buffer: vk::Buffer,
    pub memory: vk_mem::Allocation,
    pub mapped: *mut u8,
    pub byte_len: usize,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
    pub recorded: bool,        // set once the copy is actually recorded
}
```

Built with `create_memory_buffer(allocator, byte_len, TRANSFER_DST,
BufferMemory::Readback)` (`:3857`), then
`allocator.get_allocation_info(&memory).mapped_data`. `BufferMemory::Readback`
(`:3823`) is `AutoPreferHost + HOST_ACCESS_RANDOM | MAPPED` with
`HOST_COHERENT` forced, so no `invalidate` is ever needed.

### Three required changes to existing code

**1. `format_block_info` (`:4118`) must accept BGRA.** It currently matches
only `R8G8B8A8_{SRGB,UNORM}` and returns `None` otherwise — but
`PREFERRED_SURFACE_FORMAT` is `B8G8R8A8_SRGB` (`:3197`), so capture silently
fails without this. It's a size/alignment table, not a decode table, so adding
`B8G8R8A8_{SRGB,UNORM}` is safe for the existing mip-upload caller. Widen its
doc comment to mention frame-capture readback.

**2. Widen two barrier stage masks.** `:1732` sets `src_stage_mask(BLIT)` and
`:1919` sets `dst_stage_mask(BLIT)`. `BLIT` and `COPY` are distinct
`PipelineStageFlags2` bits, so a `cmd_copy_image_to_buffer` under those
barriers is *itself* a sync-validation error — the harness would fail on its
own capture path. Both become `ALL_TRANSFER` (which covers
`COPY | BLIT | RESOLVE | CLEAR`; cost is nil). **Land this and verify a clean
`just headless` run before adding the copy.**

**3. `recreate_swapchain` (`:2463`) drops `self.capture`** with a
`log::warn!`. It recreates resolve images at a possibly-new `render_extent`, so
a spurious resize would otherwise yield a wrong-sized image; this turns it into
the clean "capture frame never reached" error instead.

### Where the copy goes

**At `:1921`, immediately after the `resolve_to_blit_src` barrier and *before*
the blit.** Three reasons:

- The resolve image is already in `TRANSFER_SRC_OPTIMAL` from the barrier one
  line above — zero extra transitions.
- It captures pre-upscale, pre-egui pixels at exactly `render_extent`, which is
  the deterministic quantity. The blit at `:1991` is a `Filter::LINEAR` upscale
  to `image_extent`, which would bake in driver-dependent filtering.
- `total_frames` is already incremented at this point (`:2206`, before
  `record_command_buffer` is called at `:2276`/`:2347`), so
  `target_frame == self.total_frames` compares against the true frame number.

The `is_capture_frame` / `as_ref` / `as_mut` dance is needed because
`record_command_buffer` takes `&mut self` and `self.device` is used in the same
expression.

### Readback synchronization: `drain_gpu()`, not the timeline wait

Picking reads through `frame_timeline` (`:2221`) and tolerates 2-frame
staleness by design. Capture must not. `run_loop` already calls
`self.renderer.drain_gpu()?` (`device_wait_idle`) after the loop, which is
spec-equivalent to waiting on fences for all submissions and establishes the
host-domain memory dependency; `take_captured_frame()` runs right after. With
`HOST_COHERENT` memory that's the whole story — no timeline reasoning, no
staleness window, no extra submit.

A blocking `begin/end_single_time_commands` copy after the loop was considered
and rejected: the resolve image's layout isn't tracked across frames (each
frame re-transitions from `UNDEFINED`), `flight_slot` has already advanced past
the captured frame, and `end_single_time_commands` (`:4538`) does a
`device_wait_idle` anyway. The in-frame copy is simpler and captures the frame
the app actually presented.

BGRA→RGBA swizzle (`px.swap(0, 2)`) before `image::RgbaImage::from_raw`.
Destroy in `Drop` beside the existing `picking.take()` block (`:2827`), which
already establishes the required ordering before
`ManuallyDrop::drop(&mut self.allocator)` at `:2868`.

---

## 6. `ENABLE_VALIDATION` becomes env-driven

`ENABLE_VALIDATION: bool = cfg!(debug_assertions)` (`:61`) forces an unwanted
coupling. Debug builds recompile `.slang` at pipeline-creation time via
`dev_compile_slang_shaders` (`ShaderPipelineLayout::create_from_atlas`,
`:5071`), and `assert_shader_interface_unchanged` (`:5041`) guards only the
reflection *interface*, not the bytecode — so a local Slang version bump can
change codegen and shift pixels without tripping any assert. Goldens need
reproducible SPIR-V, i.e. a release build, which today has no validation at all.

```rust
fn enable_validation() -> bool {
    static CACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| match std::env::var("VKR_VALIDATION").as_deref() {
        Ok("1") | Ok("true")  => true,
        Ok("0") | Ok("false") => false,
        _ => cfg!(debug_assertions),
    })
}
```

**The `OnceLock` is essential** — `Drop` (`:2879`) must observe the same value
as init, or you destroy a null messenger (or leak one). Four call sites:
`get_required_layers` (`:2942`), `Renderer::init` (`:2814`),
`maybe_create_debug_messager_extension` (`src/renderer/debug.rs:59`), and
`Drop` (`:2879`).

This unlocks the right combination for goldens: **release build (committed,
reproducible SPIR-V; no `shader_watcher`; no `old_pipelines`) with
`VKR_VALIDATION=1`**. Validation sweeps still run in debug so the debug-only
paths get exercised too. The cost is two build caches — worth it.

Update the "Key Constants" section of `CLAUDE.md`, which documents
`ENABLE_VALIDATION` as a constant.

---

## 7. Comparison harness

```
goldens/rtx3070ti/*.png      # committed; device slug in the path
goldens/manifest.json        # device, profile, render_scale, msaa, frames,
                             # tolerance, per-example sha256 + extent
src/bin/image_diff.rs        # new bin
target/golden/               # gitignored: .actual.png, .diff.png
```

The device slug is in the path because cross-driver goldens are not achievable
(§9) — this makes it structurally impossible to compare an NVIDIA golden
against a lavapipe run, and leaves room for a `goldens/lavapipe/` set later.

**The manifest refuses to compare** if the live config (device name, extent,
frames, MSAA) doesn't match what was blessed. That's the single
highest-value safeguard against silently blessing garbage. It mirrors the
existing `scripts/link_converted.sha256` convention.

```
image_diff <actual.png> <expected.png>
    [--channel-tolerance N]   default 4
    [--max-diff-pixels N]     default 200
    [--hard-fail-delta N]     default 64   (any single pixel over this fails)
    [--diff-out <path.png>]
```

Per-pixel `delta = max(|dr|, |dg|, |db|, |da|)`; a pixel differs when
`delta > channel_tolerance`. Fail if the differing count exceeds the budget, or
if any single pixel exceeds the hard limit. On failure write a diff PNG —
expected image at 25% luminance, differing pixels painted red at intensity
`delta` — and print `differing=N/total max_delta=M mean_delta=…`.

A Rust bin rather than a script: `image = "0.25.6"` is already a normal
dependency used for PNG writing in `src/bin/generate_paper_texture.rs:90`,
`src/bin/*.rs` is an established pattern here, and it keeps decode consistent
with encode.

**Blessing.** `just bless EXAMPLE` captures straight into
`goldens/rtx3070ti/<example>.png` guarded by `VKR_REQUIRE_DEVICE=NVIDIA`, then
regenerates the manifest entry. Review with `git diff --stat goldens/` and by
eyeballing — the same workflow as insta snapshots.

**`just test` is untouched and stays GPU-free.** `INSTA_UPDATE=no cargo test`
currently runs on any machine with no GPU, no X server, and no Vulkan loader,
and `pre-commit` depends on it (`pre-commit: shaders && lint test`). Making it
GPU-dependent would break the hook everywhere. All GPU work lives in new
recipes. If `cargo test` ergonomics are wanted later, the shape is
`tests/golden.rs` with `#[ignore]`d tests shelling out to the recipes, invoked
as `cargo test --test golden -- --include-ignored` — mirroring the existing
`link-verify-p1` gate. Defer it; plain recipes are less machinery.

---

## 8. justfile recipes

Prereq (one-time, needs sudo): `sudo apt install xvfb` — not currently
installed.

```just
# toon_link needs gitignored Wind Waker assets
HEADLESS_EXAMPLES := "basic_triangle depth_texture dragon gpu_picking koch_curve \
multi_mesh particles ray_marching sdf_2d serenity_crt space_invaders sprite_batch \
suzanne viking_room watercolor"

# deterministic subset, safe to compare against goldens
GOLDEN_EXAMPLES := "basic_triangle depth_texture dragon gpu_picking koch_curve \
multi_mesh ray_marching sdf_2d serenity_crt space_invaders suzanne viking_room"
```

- `headless EXAMPLE FRAMES` — Xvfb + lavapipe, debug build; fails on any
  validation error
- `headless-nvidia EXAMPLE FRAMES` — real XWayland display + NVIDIA ICD, window
  hidden
- `headless-all FRAMES` — shell loop over `HEADLESS_EXAMPLES`, each under
  `timeout 300`, collecting failures
- `golden EXAMPLE` / `golden-all` — release + `VKR_VALIDATION=1` + NVIDIA,
  capture then `image_diff`
- `bless EXAMPLE` / `bless-all` — capture into `goldens/rtx3070ti/`, guarded by
  `VKR_REQUIRE_DEVICE=NVIDIA`

Load-bearing environment details:

- **`SDL_VIDEODRIVER=x11` is mandatory.** This is a Wayland session
  (`XDG_SESSION_TYPE=wayland`, `WAYLAND_DISPLAY=wayland-1`, `DISPLAY=:1`
  XWayland). Without it SDL3 sees `WAYLAND_DISPLAY`, connects to the real
  compositor, and silently bypasses Xvfb — opening a real window. Worse, an SDL
  Wayland surface that is never committed never receives a frame callback, so
  FIFO present blocks forever.
- **`VK_DRIVER_FILES`**, not the deprecated `VK_ICD_FILENAMES` — the installed
  loader is 1.4.328 and supports it.
- Recipes set `RUST_LOG` explicitly. `.env` sets a good default but direnv may
  not be loaded under a bare `just`, and env_logger defaults to `error`. The
  *counters* are filter-independent, so a wrong `RUST_LOG` still gives a
  correct exit code with an empty log — annoying, not wrong.
- Every run wrapped in `timeout 300`. `acquire_next_image` uses `u64::MAX`
  (`:2191`), so any WSI stall is an unkillable hang.
- Debug recipes need the Slang build env from `.env` (`SLANG_LIB_DIR` etc.)
  because debug builds recompile `.slang` at pipeline-creation time. Release
  recipes still link Slang but never invoke it.
- `choose_swap_present_mode` (`:3210`) prefers `MAILBOX`. Under Xvfb + lavapipe
  that should be offered, so runs aren't vsync-throttled. Falling back to FIFO
  costs ~1 s at 60 frames — fine.

---

## 9. Risks

### Window mapping

Under Xvfb the screen is already invisible, so mapping the window costs
nothing — while presenting to an *unmapped* X11 window is where WSI behavior
gets driver-specific. Mesa's x11 software path generally handles
`xcb_put_image` to a non-viewable drawable, but NVIDIA's X11 WSI is a black
box, and a `PresentCompleteNotify` that never arrives means
`acquire_next_image(u64::MAX, …)` hangs forever.

**So: map the window by default in headless mode; `VKR_HEADLESS_HIDE_WINDOW=1`
is opt-in.** Hiding is genuinely useful for the NVIDIA path, which runs on the
real XWayland display where a mapped window would pop up. If that hangs, drop
the flag and accept a flashing window, or run NVIDIA under Xvfb too.

**Corollary: never run headless mode under the SDL Wayland backend.**
`SDL_VIDEODRIVER=x11` is load-bearing, per §8.

### NVIDIA may not be selectable under Xvfb

`QueueFamilyIndices::find` (`:3015`) requires
`get_physical_device_surface_support`, and `choose_physical_device` silently
`continue`s a device with no presentation queue (`:3099`). If NVIDIA's WSI
declines to present to an Xvfb screen it doesn't drive, the NVIDIA device is
skipped and you either fall through to llvmpipe or hit the `bail!` at `:3170` —
either way you could bless goldens on the wrong device without noticing.
`VKR_REQUIRE_DEVICE` + `assert_device_name_contains` exists specifically to
turn that into a hard error. NVIDIA has supported PRIME-offload presentation to
non-NVIDIA X screens since 435.x so this probably works, but **verify before
building recipes around it.** Fallback: the real XWayland `:1` display with
`VKR_HEADLESS_HIDE_WINDOW=1`.

Note this machine also has an Intel Iris Xe iGPU. `choose_physical_device`
sorts DISCRETE first (`:3158-3167`), so NVIDIA wins when it's eligible — but
"eligible" is exactly what's in doubt here.

### Cross-driver goldens are impossible — measured, not theoretical

```
lavapipe   framebufferColorSampleCounts = {1×, 4×}
NVIDIA     framebufferColorSampleCounts = {1×, 2×, 4×, 8×}
Intel Xe   framebufferColorSampleCounts = {1×, 2×, 4×, 8×, 16×}
```

With the default `MaxMSAASamples`, `get_max_usable_sample_count` (`:4974`)
picks different counts per driver — different geometry coverage, different
resolve. Pinning `VKR_HEADLESS_MSAA=4` equalizes the *count*, but not
rasterization tie-breaks, shader float precision (llvmpipe vs NVIDIA
transcendentals in the SDF and ray-marching examples differ by far more than
4/255), texture filtering, or anisotropy.

**Accept the split: lavapipe does validation only; goldens are NVIDIA-only, in
`goldens/rtx3070ti/`.** Pin `VKR_HEADLESS_MSAA=4` for goldens anyway so a
future GPU swap doesn't silently change the sample count.

### Examples that can't participate

- **`toon_link`** — requires `assets/link/`, which `.gitignore` excludes
  wholesale (`/assets/`). Excluded from both lists. Its `Instant` usage is
  still migrated so it keeps compiling.
- **`sprite_batch`** — `randomize_sprite` (`:153`) calls `SDL_rand`/`SDL_randf`
  every frame from SDL's global time-seeded state. Validation-only.
- **`particles`** — integrates GPU state across frames. The virtual clock makes
  `delta_time` deterministic, but verify the initial particle state isn't
  randomized before promoting it to `GOLDEN_EXAMPLES`.
- **`watercolor`** — needs mouse input to produce content; the no-input frame
  should be a constant blank canvas, but it's a compute ping-pong, so confirm
  two runs match before promoting.
- **`gpu_picking`** — deterministic, but its readback depends on mouse
  position, which is (0,0) headless. Fine; the golden just shows the un-picked
  state.

### Golden churn

At 800×600 RGBA a fractal PNG is ~150-400 KB; 12 goldens ≈ 2-4 MB per bless.
`.git` is already 2.1 GB so absolute size is a non-issue, but **churn** isn't:
PNGs don't delta-compress, so every re-bless adds a full blob per image.
Mitigations: bless rarely and deliberately; start with 4-6 curated examples
(`dragon`, `sdf_2d`, `viking_room`, `suzanne`, `koch_curve`, `basic_triangle`)
and grow only when a golden has actually caught something; optionally
`VKR_HEADLESS_RENDER_SCALE=0.5` to quarter the pixel count, at the cost of
hiding fine-detail regressions. Do **not** reach for git-lfs — there's no
`.gitattributes` today and it would complicate every clone.

### Smaller items

- **`pretty_env_logger::init()`** (`src/game/traits.rs:84`) panics on a second
  call. Irrelevant for one-example-per-process, but it rules out an in-process
  multi-example runner — hence the shell-loop sweep.
- **`just lint` runs clippy `--all-targets` in both debug and release.** The new
  `enable_validation()` fn and the raw-pointer `mapped: *mut u8` in
  `CaptureResources` must be clean in both. `picking.rs` already carries a
  `*mut u32` field, so the pattern is precedented (and `CaptureResources`
  becomes `!Send` the same way — irrelevant, `Renderer` is single-threaded).
- **Xvfb screen size feeds `compute_render_scale_for_display`** via
  `display.get_bounds()`. At `-screen 0 1920x1080x24` that yields 1.0 anyway,
  but headless must not depend on it — `VKR_HEADLESS_RENDER_SCALE` (default
  1.0) short-circuits it entirely. Note `watercolor` already pins
  `render_scale() == Some(1.0)`.

---

## Implementation order

Each step is independently verifiable.

1. **Validation counters** (`src/renderer/debug.rs`) — self-contained, no
   behavior change
2. **`HeadlessConfig` + `run()`/`run_loop()` frame limit** — the validation
   harness now works on the real display
3. **Xvfb + lavapipe recipes** — `just headless`, `just headless-all`
4. **Virtual clock** + the example migrations
5. **`enable_validation()`** env switch
6. **Barrier widening** (verify a clean run *before* proceeding), then
   `src/renderer/capture.rs` + `format_block_info` BGRA
7. **`src/bin/image_diff.rs`**, `goldens/`, `just golden` / `just bless`

## Verification

Prereq:

```bash
sudo apt install xvfb
```

Run `cargo check --all-targets` after every Rust change; `just lint` and
`cargo fmt` at the end of each step.

**Validation harness (steps 2-3):**

```bash
just headless basic_triangle 30 ; echo "exit=$?"            # expect 0
just headless dragon 5 2>&1 | grep -i "llvmpipe\|device"    # confirm lavapipe chosen
just headless-all 30
```

Negative control: temporarily break a barrier, confirm `exit=1` and
`"vulkan validation failed: N error(s)"`.

**Determinism (steps 4 + 6) — the key test.** Two identical runs must be
byte-identical:

```bash
for i in 1 2; do
  SDL_VIDEODRIVER=x11 VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json \
    VKR_HEADLESS=1 VKR_HEADLESS_FRAMES=30 VKR_HEADLESS_MSAA=4 \
    VKR_HEADLESS_CAPTURE=/tmp/dragon_$i.png \
    xvfb-run -a -s "-screen 0 1920x1080x24" cargo run --example dragon
done
sha256sum /tmp/dragon_1.png /tmp/dragon_2.png   # MUST match
```

Negative controls for the virtual clock: `FRAMES=31` must produce a *different*
image (time advanced one step), and injecting an artificial 200 ms sleep into a
`FRAMES=30` run must still match. Also `file /tmp/dragon_1.png` should report
`PNG image data, 800 x 600, 8-bit/color RGBA` — and eyeball it, since swapped
red and blue means the BGRA swizzle is inverted.

**Golden harness (step 7):**

```bash
just bless dragon
just golden dragon ; echo "exit=$?"                          # expect 0
cargo run --release --bin image_diff -- \
  /tmp/dragon_1.png goldens/rtx3070ti/suzanne.png            # expect 1
just golden-all
```

**Regression guard — the existing workflow must be untouched:**

```bash
env -u DISPLAY -u WAYLAND_DISPLAY just test   # must still pass with no GPU/X at all
just lint
just dev basic_triangle                        # windowed path, animation speed unchanged
```

---

## Critical files

- `src/renderer.rs` — `capture` field (~`:189`), `enable_frame_capture` /
  `take_captured_frame` / clock accessors, copy at `:1921`, barrier widening at
  `:1732` and `:1919`, `format_block_info` BGRA at `:4118`,
  `enable_validation()` replacing the const at `:61`, `Drop` at `:2827`
- `src/game/traits.rs` — `run()` at `:80-114`
- `src/app.rs` — `run_loop()` at `:29-70`
- `src/renderer/debug.rs` — counters in `vulkan_debug_utils_callback` at
  `:14-49`
- `src/renderer/picking.rs` — the readback template for the new
  `src/renderer/capture.rs`
- `src/headless.rs`, `src/bin/image_diff.rs` — new
- `justfile`, `CLAUDE.md` — new recipes; `ENABLE_VALIDATION` doc update
