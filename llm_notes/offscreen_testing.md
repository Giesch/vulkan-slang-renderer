# Offscreen testing: validation errors and golden images

Status: **design, 2026-07-27.** Supersedes the 2026-07-25 version, which was
built around Xvfb and treated validation and goldens as one deliverable. Code
references verified against `main` @ `aae5b71`. Driver and environment
measurements taken on Dan's laptop the same day.

**Outcome (2026-08-28).** Phase 1's goal shipped as `scripts/headless-sweep.sh`
+ `just sweep` / `just sweep-self-test` (see `docs/testing.md`), by a different
mechanism: `SDL_VIDEODRIVER=offscreen` plus a pinned lavapipe ICD instead of
`VK_EXT_headless_surface`. The `WindowTarget` refactor, `src/headless.rs` /
`VKR_HEADLESS*`, `run_loop(…, iteration_cap)` and the lavapipe WSI probe were
not built and are moot. Still unbuilt from Phase 1: the §4 `vk stack:`
fingerprint line. Phase 2 (goldens) stays deferred.

**Split into two phases:**

- **Phase 1 — windowless validation gate.** Designed to completion below. Runs
  an example with no window and no display server and exits nonzero on any
  Vulkan validation error. This is what gets built next.
- **Phase 2 — golden images.** Kept as a sketch (§9-§12). Its central problem —
  which driver goldens are blessed on — is now *documented* and **deliberately
  left open**; see §12.

Companion documents: [`tech_debt.md`](tech_debt.md) — its §1 catalogues the
teardown-time object leaks that produce validation errors at `vkDestroyDevice`,
which is exactly the class phase 1 catches automatically instead of by eye.
[`link_rendering/phase_07.md`](link_rendering/phase_07.md) is the worked example
of why this note exists at all. This note supersedes the scattered `todo.org`
entries on the subject (lines 5-7, 156-160, 224-227, 249-253, and 494); those
entries can point here.

---

## Why

Today the only way to check an example for Vulkan validation errors is
`timeout 3 just dev EXAMPLE`, documented in `CLAUDE.md`. That has three
problems:

1. It opens a window on the dev machine.
2. ~~`timeout` SIGKILLs the process, so `drain_gpu()` (`src/app.rs:62`) and
   `Drop for Renderer` never run.~~ **False; corrected 2026-07-29.** `timeout`
   sends SIGTERM, SDL turns it into `SDL_QUIT`, and the loop exits normally, so
   both *do* run and `vkDestroyDevice` reports leaked objects. Teardown coverage
   is therefore free rather than blocked — see `build_reproducibility.md` §7.4
   for the measurements. What remains true is the narrower point: the teardown
   window is where the `tech_debt.md` §1 leaks report themselves, and a harness
   must not break it (SIGKILL and `SDL_NO_SIGNAL_HANDLERS=1` both do).
3. **There is no signal.** `vulkan_debug_utils_callback`
   (`src/renderer/debug.rs:14-49`) only calls `log::error!` and returns
   `vk::FALSE`. A run with 500 validation errors exits 0. Catching them means a
   human reading stderr.

**P7 turned that from a nuisance into a blocked phase.** `toon_link`'s albedo
work was implemented in a Claude Code web container with no video device — SDL
failed at init for *every* example, not just `toon_link` — so the phase landed
with all static gates green and **every runtime gate marked NOT RUN**, including
the validation sweep and the numeric gamma check that the plan called its
primary gate. See phase_07.md's Recorded facts. A phase can be written in a
container; it currently cannot be *verified* in one.

**Outcome of phase 1.** `just headless EXAMPLE` and `just headless-all` run with
no window and no display server, on the laptop and in the container, and exit
nonzero on any validation error.

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

Lavapipe — Mesa's Vulkan frontend over llvmpipe, installed at
`/usr/share/vulkan/icd.d/lvp_icd.json` — reports **Vulkan 1.4.318**,
`driverInfo = Mesa 25.2.8 (LLVM 20.1.2)`, `deviceType = CPU`, and satisfies
every feature `choose_physical_device` requires (`src/renderer.rs:3071`):
`samplerAnisotropy`, `shaderDrawParameters`, `timelineSemaphore`,
`bufferDeviceAddress`, `dynamicRendering`, `synchronization2`.
`choose_physical_device` sorts `PHYSICAL_DEVICE_TYPE_CPU` last but does accept
it, so lavapipe is selected when it's the only ICD offered.

Critically for phase 1, **lavapipe advertises `VK_EXT_headless_surface` on its
own** — confirmed by running `vulkaninfo` with
`VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json`, which still lists the
extension when lavapipe is the only ICD in play. That is what makes a run with
no display server at all possible; see §Approach.

So validation testing runs on any x86 CPU — this laptop, a Claude Code web
container, a GitHub Actions runner.

**What the container needs installed.** Recorded from the P7 run (phase_07.md
deviation 3) plus what this design adds:

```
libasound2-dev libvulkan-dev mesa-vulkan-drivers vulkan-validationlayers
+ git submodule update --init --recursive   (slang)
+ a from-source slang build configured with -DSLANG_ENABLE_SLANG_RHI=OFF
    (slang-rhi's CMake tries to fetch OptiX and fails behind the proxy)
```

The slang build is the expensive step and is required regardless of profile —
`sdl3` is built from source statically and the renderer links slang either way.
Debug builds additionally *invoke* slang at pipeline-creation time
(`dev_compile_slang_shaders`), so `.env`'s `SLANG_LIB_DIR` must be present.

**What lavapipe does *not* cover.** Worth stating so the harness isn't oversold:

- **Limit- and format-dependent checks.** Validation compares usage against the
  *actual* device's `VkPhysicalDeviceLimits` and per-format
  `VK_FORMAT_FEATURE_*` bits. Lavapipe's differ from NVIDIA's, so a bug that
  only manifests at a limit NVIDIA has and lavapipe doesn't is structurally
  invisible. See the MSAA measurement in §12.
- **Real-driver misbehavior.** Validation-clean code can still render garbage or
  hang on NVIDIA — uninitialized reads, races that a software rasterizer's
  serialized execution never exposes.
- **Anything visual.** Phase 1 does not replace `just dev` or the P7-style
  eyeball gates. It answers one question: is the API usage legal?
- **Speed.** llvmpipe is a JIT'd CPU rasterizer; cost scales with
  pixels × fragment-shader complexity. `basic_triangle` is trivial;
  `ray_marching`, `sdf_2d`, and `watercolor` will be slow.

**On Modal specifically** (asked about directly, so recorded here): Modal's GPU
containers run under gVisor, whose `nvproxy` forwards NVIDIA ioctls selectively
and implements a CUDA-centric surface. Graphics support is
[an open request, not a shipped capability](https://github.com/google/gvisor/issues/10856).
Modal's GPU documentation never mentions Vulkan, OpenGL, or EGL. Even setting
that aside, Vulkan in an NVIDIA container needs `NVIDIA_DRIVER_CAPABILITIES` to
include `graphics` so the toolkit mounts `nvidia_icd.json`, which Modal doesn't
document exposing. It is the wrong tool. The same reasoning rules out most
serverless-GPU vendors; if a cloud runner is ever wanted, a plain Docker
container on a non-gVisor host is the shape that works.

---

## The reproducibility contract

The question this design has to answer is: *does a green run on the laptop mean
anything about the container, and vice versa?*

**The gate is "zero validation errors", not "identical validation output."**
Phase 1 does not pin the Vulkan stack and does not compare error text across
machines. Consequences, stated plainly so nobody re-litigates them later:

- A newer validation layer that finds **more** errors is a feature, not drift to
  suppress. The fix is the error, not the layer version.
- A run that fails in one place and passes in the other is a **real bug** —
  either in our code or in an assumption about limits — and gets investigated,
  not pinned around.
- Therefore: **no pinned container, no version assert, no blessed-stack file.**
  That machinery is only justified once pixels are being compared, which is
  phase 2's problem (§12).

What the harness *does* owe you is that any result is **explicable**. The dev
machine has ambient Vulkan configuration that can silently change which layer
runs, none of which exists in a fresh container:

- `.zshrc` sets `VK_ADD_LAYER_PATH` to a LunarG SDK 1.4.328 install, while apt
  also ships `vulkan-validationlayers` 1.3.275. Two manifests provide the same
  layer name; which one the loader picks is search-order dependent.
- Implicit layers load without the app asking: `nvidia_layers.json` and
  `VkLayer_MESA_device_select.json` are both present, and the latter exists
  specifically to reorder physical devices.
- `vkconfig` is installed and writes `~/.local/share/vulkan/settings.d/`, which
  can enable or disable validation features process-wide. Currently empty — but
  one GUI click away from not being.

So the recipes (§5) pin the *environment*, not the versions:

- `VK_DRIVER_FILES` set explicitly to the lavapipe ICD — never inherit ICD
  choice from whatever the machine has installed.
- `VK_LOADER_LAYERS_DISABLE=~implicit~` so `nvidia_layers` and
  `MESA_device_select` cannot participate. The loader is 1.4.328, which supports
  both this and `VK_DRIVER_FILES`.
- `VK_ADD_LAYER_PATH` cleared for the run, so the validation layer in use is the
  one the recipe's environment names rather than whichever the interactive shell
  happened to add.

…and the harness prints a **stack fingerprint** (§4) identifying exactly which
loader, layer, and ICD produced the result. It never fails a run. Its whole job
is to turn "it fails in the container but not on my laptop" from an
investigation into a five-second diff.

**If that ever proves insufficient**, the escalation is a pinned container image
used in both places — *not* a fingerprint assert, which would only convert an
interesting failure into a confusing one.

---

## Approach

```
just headless EXAMPLE  →  no SDL video subsystem
                       →  VK_EXT_headless_surface + lavapipe
                       →  N frames  →  validation counters  →  exit 1 on error
```

The codebase is most of the way there already. Game rendering targets an
offscreen `resolve_image` (`src/renderer.rs:138`) that is already
`TRANSFER_SRC | COLOR_ATTACHMENT` (`:216`), MSAA-resolved, and egui-free — egui
draws to the *swapchain* image afterward (`:1999`). `src/renderer/picking.rs` is
a working template for readback, which phase 2 will need.

### Why not Xvfb (reversal from the 2026-07-25 design)

The previous version ran the real SDL/X11 window under a virtual X server. That
was a reasonable answer when the only requirement was "no window on Dan's
screen." It is the wrong answer now:

- The cloud target is a Claude Code web container: no X server, no GPU, no
  display of any kind, and `xvfb` is one more thing to install and keep working
  in an environment where package installs already caused friction (phase_07.md
  deviation 3).
- Xvfb reintroduces display-derived state — screen geometry feeding
  `compute_render_scale_for_display` (`src/game/traits.rs:121`), window mapping
  semantics, WSI paths that differ per ICD — every bit of which is variance
  between the two machines for no benefit.
- `Xvfb` is not installed here and, under this design, never needs to be.

The old design's two Xvfb-specific risk sections ("Window mapping", "NVIDIA may
not be selectable under Xvfb") are deleted with it, as is the
`SDL_VIDEODRIVER=x11` requirement — with no SDL video subsystem initialized,
there is no SDL backend to force. That variable was load-bearing before and is
now moot; noted here so its disappearance doesn't read as an oversight.

### The narrow refactor: `WindowTarget`

The old note rejected threading `Option<Surface>` / `Option<Swapchain>` through
~10 call sites, and **that rejection still stands.** The swapchain and present
path stay fully alive — present-path validation and an honest `Drop` are half
the value of the harness. What changes is only *where the surface comes from*:

```rust
// src/renderer.rs, replacing the `window: Window` field at :102
enum WindowTarget {
    Sdl(sdl3::video::Window),
    Headless { width: u32, height: u32 },
}

impl WindowTarget {
    fn size(&self) -> (u32, u32);
    fn size_in_pixels(&self) -> (u32, u32);
    fn instance_extensions(&self) -> anyhow::Result<Vec<CString>>;
    fn create_surface(&self, entry: &ash::Entry, instance: &ash::Instance)
        -> anyhow::Result<vk::SurfaceKHR>;
    fn as_sdl(&self) -> Option<&sdl3::video::Window>;
}
```

Nothing becomes optional; one enum absorbs the difference. Call sites:

| site | today | change |
|---|---|---|
| `renderer.rs:102` | `window: Window` field | `window: WindowTarget` |
| `:241` | `init(window: Window, …)` | takes `WindowTarget` |
| `:250` | `window.size()` | through the enum |
| `:266-267` | `window.vulkan_instance_extensions()` | headless → `[VK_KHR_surface, VK_EXT_headless_surface]` |
| `:303` | `window.vulkan_create_surface(instance.handle())` | headless → `ash::ext::headless_surface::Instance::create_headless_surface` (present in ash 0.38) |
| `:411` | `window: window.clone()` | `Sdl` arm clones; `Headless` is `Copy`-cheap |
| `:2078` | `self.window.raw()` for SDL text input | `as_sdl()`; unreachable headless (egui off) but must compile |
| `:2493` | `create_swapchain(&self.window, …)` in `recreate_swapchain` (`:2462`) | takes `&WindowTarget` |
| `:2971` | `check_required_extensions` hardcodes `platform::OS_SURFACE_EXT` | takes the target's required set instead |
| `:3239` | `choose_swap_extent(window: &Window, …)` | takes `&WindowTarget` |
| `:3266` | `create_swapchain(window: &Window, …)` | takes `&WindowTarget` |

`src/renderer/platform.rs` keeps `OS_SURFACE_EXT` for the SDL path; the headless
path bypasses it entirely, so the `#[cfg(target_os)]` table is untouched.

### No SDL video subsystem at all

In `Game::run()` (`src/game/traits.rs:80`), headless mode skips `sdl.video()`
and window creation **entirely** — no display server is contacted. This is safe
by construction: `sdl3::init()` calls `SDL_Init(0)` with **no** subsystem flags
(`sdl3-0.14.42/src/sdl3/sdl.rs:92`), and `Sdl::video()` (`:147`) is what
actually brings up the video subsystem. Nothing else in `run()` needs it.

- **Extent** comes from `Self::initial_window_size()` (`traits.rs:42`), which is
  deterministic and is already what `sprite_batch` and `space_invaders` build
  their projection matrices from. No display geometry anywhere.
- **Render scale**: `compute_render_scale_for_display` (`traits.rs:121`) queries
  the display and must not run. `VKR_HEADLESS_RENDER_SCALE` (default 1.0)
  replaces it; a `Game::render_scale()` override still wins, because the
  example's own intent is part of what's being tested.
- **egui** stays off: `enable_egui = cfg!(debug_assertions) && headless.is_none()`
  (`traits.rs:98`).
- **Events**: there is no `EventPump`, so `App::run_loop` takes
  `Option<EventPump>` (§3).

`Renderer::init` keeps its 4-arg signature; the headless bits arrive through the
`WindowTarget` it is handed plus the config read in `run()`.

### Probe lavapipe's headless WSI before writing recipes

This is the one genuinely unknown piece — Mesa's headless WSI is far less
travelled than its X11 path. Treat it as an explicit step, not an assumption,
and record the answers in this file:

- Does it offer `B8G8R8A8_SRGB` (`PREFERRED_SURFACE_FORMAT`, `:3196`)? If not,
  `choose_swap_surface_format` (`:3201`) silently takes `fallback_format` —
  fine for phase 1, but it changes what phase 2 would capture.
- Which present modes? `choose_swap_present_mode` (`:3209`) prefers MAILBOX and
  falls back to FIFO. FIFO against a headless surface should not throttle, but
  confirm — a 60-frame run that takes 60 vsyncs would be a clue that something
  is emulating a display.
- Is `capabilities.current_extent` the `u32::MAX` sentinel or a fixed value?
  `choose_swap_extent` (`:3239`) handles both; record which path runs, because
  it decides whether `initial_window_size()` is honored or overridden.
- Does `get_physical_device_surface_support` (`:3037`, inside
  `QueueFamilyIndices::find` at `:3014`) return true? If not,
  `choose_physical_device` silently `continue`s past lavapipe (`:3099`) and the
  run dies at the no-suitable-device `bail!` — a confusing failure worth
  recognizing on sight.

---

## 1. Configuration — env vars read inside `Game::run()`

Every example's `main()` is just `Foo::run()`, so env vars read inside `run()`
are the only non-invasive knob. **Nothing in `examples/` changes.**

New module `src/headless.rs` (`pub mod headless;` in `src/lib.rs`):

```rust
pub struct HeadlessConfig {
    pub frames: usize,
    pub render_scale: f32,
    pub strict: bool,
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
| `VKR_HEADLESS` | unset | master switch: no SDL video, headless surface, frame limit |
| `VKR_HEADLESS_FRAMES` | `60` | frames to submit, then quit |
| `VKR_HEADLESS_RENDER_SCALE` | `1.0` | replaces the display query |
| `VKR_HEADLESS_STRICT` | `0` | also fail on validation WARNINGs |

**Deferred to phase 2, with reasons:** `VKR_HEADLESS_CAPTURE` /
`_CAPTURE_FRAME` (need `capture.rs`, §9); `VKR_HEADLESS_MSAA` (only matters when
comparing pixels, and pinning it risks the `TYPE_1` trap in §6);
`VKR_HEADLESS_FRAME_DELAY_MS` (the virtual clock, §10); `VKR_VALIDATION` (§11);
`VKR_REQUIRE_DEVICE` (guards *blessing* on the wrong device — a goldens
concern). `VKR_HEADLESS_HIDE_WINDOW` is gone as a concept: there is no window.

**Deliberately no window-size override.** `examples/sprite_batch.rs` and
`examples/space_invaders.rs` derive their projection matrix from
`Self::initial_window_size()`; overriding the extent independently would desync
projection from viewport.

---

## 2. Validation counters — `src/renderer/debug.rs`

Two `static AtomicUsize` counters incremented in the existing severity match arms
of `vulkan_debug_utils_callback` (`:14-49`, arms at `:34-46`):

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

**Count unconditionally; gate only the check.** Two relaxed atomic increments on
a path that already does `format!` + `CStr::from_ptr` + a log call cost nothing,
and unconditional counting makes the counters usable from anywhere later (e.g.
an assertion inside an example) without plumbing a flag.

**Where the check runs.** `HeadlessConfig::check_validation()` is called from
`Game::run()` *after* `app.run_loop(...)` returns. `run_loop` takes `self` by
value, so `App` — and therefore `Renderer` — has already dropped, and
`Renderer::drop` (`:2782`) has done its `device_wait_idle` plus every destroy
call. Teardown errors are counted, and the whole log has already printed, before
the check fires. It returns `Err`, which gives exit code 1 from `main()` with
`Error: …`. No `std::process::exit`, nothing skipped.

**Errors always fail; warnings only under `--strict`.** This inverts the
2026-07-25 design, which failed on WARNING by default with an
`ALLOW_WARNINGS=1` escape hatch. The reasoning changed with the reproducibility
contract: errors are the category we've committed to treating as failures
regardless of layer version, while warnings are exactly the version-sensitive,
advisory category that would otherwise make a layer upgrade look like a
regression. Only core validation is enabled (`get_required_layers`, `:2940`) —
not best-practices — so warnings stay rare and are worth printing in the summary
either way.

Summary line on failure, so the exit code is never the only signal:

```
vulkan validation failed: 3 error(s), 1 warning(s) over 60 frames
```

---

## 3. Frame limit and loop — `src/game/traits.rs:80` + `src/app.rs:29`

`run_loop` becomes
`run_loop(self, event_pump: Option<EventPump>, max_frames: Option<usize>)`.

- **Limit on `renderer.total_frames()`, not a local counter.** `draw_frame`
  increments `total_frames` (`:2205`) *after* a successful
  `acquire_next_image`; the `ERROR_OUT_OF_DATE_KHR` early-return does not. A
  local iteration count would drift from the frame number the renderer reports —
  and phase 2's capture-frame selection depends on those being the same thing.
- **`iteration_cap` (`frames * 4`)** that `bail!`s on a swapchain-recreation
  spin rather than looping forever.
- **Skip the `SDL_DelayPrecise` pacing** (`src/app.rs:52-57`) in headless mode.
- **Skip `handle_events`** when there's no pump (`app.rs:68`). With no video
  subsystem there is no window to close and no Quit event to receive; the frame
  limit is the only exit condition, backed by `timeout` in the recipe.
- The `minimized` path is unreachable headless and stays as-is.

---

## 4. Stack fingerprint

One line, logged at `info` during `Renderer::init`, in both windowed and
headless mode:

```
vk stack: loader 1.4.328 | layer VK_LAYER_KHRONOS_validation spec 1.4.328 impl 1
        | icd llvmpipe "Mesa 25.2.8 (LLVM 20.1.2)" api 1.4.318
        | device "llvmpipe (LLVM 20.1.2, 256 bits)"
```

Sources: `entry.try_enumerate_instance_version()`;
`entry.enumerate_instance_layer_properties()` filtered to the validation layer
(the loader reports the manifest it would actually load, which is what makes
this worth printing given the dual-install hazard in §Reproducibility);
`VkPhysicalDeviceDriverProperties` (`driverName` / `driverInfo`) chained onto
the existing `physical_device_properties` query; and `deviceName`.

**Diagnostic only — it never fails a run.** Recipes echo it into the sweep
output so a per-example failure carries its environment with it.

---

## 5. justfile recipes

No `xvfb-run`, no `sudo apt install xvfb`, no `SDL_VIDEODRIVER`.

```just
LVP_ICD := "/usr/share/vulkan/icd.d/lvp_icd.json"

# toon_link needs gitignored Wind Waker assets
HEADLESS_EXAMPLES := "basic_triangle depth_texture dragon gpu_picking koch_curve \
multi_mesh particles ray_marching sdf_2d serenity_crt space_invaders sprite_batch \
suzanne viking_room watercolor"

# run one example windowless under lavapipe; nonzero exit on validation errors
[unix]
headless example frames="60":
    env -u VK_ADD_LAYER_PATH \
      VKR_HEADLESS=1 VKR_HEADLESS_FRAMES={{frames}} \
      VK_DRIVER_FILES={{LVP_ICD}} \
      VK_LOADER_LAYERS_DISABLE=~implicit~ \
      RUST_LOG=vulkan_slang_renderer=info \
      timeout 300 cargo run --example {{example}}
```

`headless-all FRAMES` is a shell loop over `HEADLESS_EXAMPLES` collecting
failures and printing a final tally — not an in-process runner, because
`pretty_env_logger::init()` (`traits.rs:84`) panics on a second call.

Load-bearing details:

- **Recipes set `RUST_LOG` explicitly.** `.env` sets a good default but direnv
  may not be loaded under a bare `just`, and env_logger defaults to `error`. The
  *counters* are filter-independent, so a wrong filter still yields a correct
  exit code with an unhelpfully empty log — annoying, not wrong.
- **Every run wrapped in `timeout 300`.** `acquire_next_image` uses `u64::MAX`
  (`:2191`), so any WSI stall is an unkillable hang. This matters more with an
  unfamiliar WSI backend, not less.
- **Debug builds need the slang env from `.env`** (`SLANG_LIB_DIR` etc.), since
  they recompile `.slang` at pipeline-creation time.
- `toon_link` is excluded from `HEADLESS_EXAMPLES` because `/assets/` is
  gitignored, but `just headless toon_link` works wherever
  `just extract-link && just convert-link` has run — which makes it the natural
  home for P7's outstanding sweep line.

---

## 6. Phase 1 risks

### lavapipe's headless WSI is the new unknown

The probe list at the end of §Approach is the mitigation: run it first, record
the answers here, and only then write recipes around it. The plausible failure
modes are a missing preferred surface format (harmless, `fallback_format`
covers it), a surprising `current_extent`, or no presentation-queue support at
all (loud and immediate).

Fallback if headless surfaces turn out to be a dead end: SDL's X11 backend under
a virtual X server, i.e. the old design, resurrected from git history. Do not
build both up front.

### MSAA

`get_max_usable_sample_count` (`:4973`) falls through to `TYPE_1` (`:5002`) with
a comment noting that triggers a validation error. Lavapipe offers `{1×, 4×}`
(measured, §12), so the default `Max8` path lands on 4× and never reaches the
fallthrough. **Don't pin MSAA in phase 1** — pinning a count the ICD lacks is
the one way to walk into that trap deliberately.

### No real-driver coverage

Validation-clean under llvmpipe is not the same as render-correct on NVIDIA.
`just dev` remains the eyeball path, and phase 1 does not discharge any of the
P7-style manual gates. What it does discharge is the "was there a validation
error?" line item, which is currently the most tedious and least reliable one.

### Examples that can't participate cleanly

- **`toon_link`** — gitignored assets; excluded from the sweep list, runnable
  locally.
- **`gpu_picking`** — its readback depends on mouse position, which is absent
  headless. Fine for validation; the picking path still records and reads back.
- **`watercolor`** — needs mouse input to produce content, so it renders a blank
  canvas. Still exercises the compute ping-pong, which is the interesting part
  for validation.
- **`sprite_batch`**, **`particles`** — nondeterministic content
  (`SDL_rand`-seeded, GPU state integrated across frames). Irrelevant to
  validation; both matter in phase 2 (§12).

### Smaller items

- `pretty_env_logger::init()` (`traits.rs:84`) panics on a second call — rules
  out an in-process multi-example runner, hence the shell-loop sweep.
- `just lint` runs clippy `--all-targets` in both debug and release; the new
  `WindowTarget` enum and `headless.rs` must be clean in both.
- **`just test` stays GPU-free and untouched.** `INSTA_UPDATE=no cargo test`
  currently runs with no GPU, no X server, and no Vulkan loader, and
  `pre-commit` depends on it. All GPU work lives in new recipes.

---

## 7. Phase 1 implementation order

Each step independently verifiable; `cargo check --all-targets` after every
Rust change, `just lint` + `cargo fmt` at the end of each.

1. **Validation counters** (`src/renderer/debug.rs`) — self-contained, no
   behavior change.
2. **`WindowTarget` enum, SDL arm only** — pure refactor; every example still
   opens a window and behaves identically.
3. **Headless surface creation** + run the §Approach probe list; record results
   in this file.
4. **`HeadlessConfig`, frame limit, `Option<EventPump>`** — the harness works.
5. **Stack fingerprint** line.
6. **`just headless` / `just headless-all`.**

## 8. Phase 1 verification

```bash
just headless basic_triangle 30 ; echo "exit=$?"   # expect 0, and no window appears
just headless dragon 5 2>&1 | grep "vk stack:"     # confirm lavapipe, not NVIDIA
just headless-all 30
```

- **Negative control — required, not optional.** Temporarily break a barrier
  (e.g. revert one of the `:1731` / `:1918` stage masks phase 2 wants widened,
  or drop a layout transition), confirm `exit=1` and the
  `vulkan validation failed: N error(s)` summary, then revert. A gate that has
  never failed is not a gate; this is the same discipline P5 used for raster
  state ("every artifact perturbed and reverted; none proved nothing").
- **Teardown coverage**: confirm the counter catches a leak reported at
  `vkDestroyDevice` — `tech_debt.md` §1 lists the known ones, so if the sweep
  comes back green everywhere, check that those are actually fixed rather than
  assuming the harness works. Already done once, by skipping a
  `destroy_image_view` and watching `VUID-vkDestroyDevice-device-05137` appear
  (`build_reproducibility.md` §7.2); redo it if the harness changes shape.
- **Cross-machine**: run `just headless-all 30` on the laptop, then in a
  container with no video device. That is the entire point of the exercise.
- **Regression guard — the existing workflow must be untouched:**

```bash
env -u DISPLAY -u WAYLAND_DISPLAY just test   # still passes with no GPU/X at all
just lint
just dev basic_triangle                       # still opens a window, same animation speed
```

---

# Phase 2 — golden images (sketch, not designed to completion)

Everything below is deferred. It is kept because the analysis is done and
re-deriving it would be waste, not because it is ready to build. §12's driver
question is **deliberately open**.

## 9. Capture — new `src/renderer/capture.rs`

Modeled on `picking.rs` but **single-buffered**, not
`[_; MAX_FRAMES_IN_FLIGHT]` — one capture per process, so there's no slot reuse
to guard. Built with `create_memory_buffer(allocator, byte_len, TRANSFER_DST,
BufferMemory::Readback)` (`:3856`), then
`allocator.get_allocation_info(&memory).mapped_data`. `BufferMemory::Readback`
(`:3819`, match arm `:3841`) is `AutoPreferHost + HOST_ACCESS_RANDOM | MAPPED`
with `HOST_COHERENT` forced, so no `invalidate` is ever needed.

**Three required changes to existing code:**

1. **`format_block_info` (`:4117`) must accept BGRA.** It currently matches only
   `R8G8B8A8_{SRGB,UNORM}` — but `PREFERRED_SURFACE_FORMAT` is `B8G8R8A8_SRGB`
   (`:3196`), so capture silently fails without this. It's a size/alignment
   table, not a decode table, so adding `B8G8R8A8_{SRGB,UNORM}` is safe for the
   existing mip-upload caller.
2. **Widen two barrier stage masks.** `:1731` sets `src_stage_mask(BLIT)` and
   `:1918` sets `dst_stage_mask(BLIT)`. `BLIT` and `COPY` are distinct
   `PipelineStageFlags2` bits, so a `cmd_copy_image_to_buffer` under those
   barriers is *itself* a sync-validation error — the harness would fail on its
   own capture path. Both become `ALL_TRANSFER`. **Land this and confirm a clean
   `just headless` run before adding the copy** — phase 1 makes that check
   trivial, which is a good argument for this ordering.
3. **`recreate_swapchain` (`:2462`) drops `self.capture`** with a `log::warn!`,
   turning a wrong-sized image into a clean "capture frame never reached" error.

**Where the copy goes:** at `:1921`, immediately after the `resolve_to_blit_src`
barrier (`:1909`, submitted `:1920`) and *before* the blit (`:1960`). The
resolve image is already in `TRANSFER_SRC_OPTIMAL`; it captures pre-upscale,
pre-egui pixels at exactly `render_extent`; and `total_frames` is already
incremented at that point, so the capture-frame number means the same thing
inside `record_command_buffer`.

**Readback synchronization: `drain_gpu()`, not the timeline wait.** Picking
tolerates 2-frame staleness by design; capture must not. `run_loop` already
calls `drain_gpu()` (`device_wait_idle`) after the loop, which establishes the
host-domain dependency; read the mapping right after. BGRA→RGBA swizzle
(`px.swap(0, 2)`) before `image::RgbaImage::from_raw`. Destroy in `Drop` beside
the existing `picking.take()` block (`:2827`), which already establishes the
ordering before `ManuallyDrop::drop(&mut self.allocator)` (`:2869`).

## 10. Virtual clock

Golden images require frame N to be identical every run; 11 examples animate off
their own `start_time: Instant`. New `Renderer` fields beside `total_frames`,
ticked once per drawn frame with a fixed delta in headless mode, exposed as
`FrameRenderer::elapsed()` / `elapsed_secs()` / `frame_delta_secs()` /
`frame_index()`. `tick_clock` stays `pub(crate)` so examples can't desync it,
and accumulating deltas rather than `start.elapsed()` keeps both modes on one
monotonic path.

Migration: `dragon`, `sdf_2d`, `koch_curve`, `serenity_crt`, `multi_mesh`,
`ray_marching` → `elapsed_secs()`; `viking_room`, `suzanne`, `depth_texture` →
`elapsed()`; `particles` → `frame_delta_secs()` for `SimParams.delta_time`;
`toon_link` likewise, so it keeps compiling. Leave `sprite_batch` and
`watercolor` alone — their `Instant` lives in `update()` with no `FrameRenderer`
in scope and only feeds an egui FPS label.

Two intentional behavior changes worth a commit-message callout: time starts at
0 on the first frame rather than at `setup()` (so slow-loading examples no
longer jump-start mid-animation), and time freezes while minimized (so there's
no jump on restore). Both are improvements.

## 11. `ENABLE_VALIDATION` becomes env-driven

`ENABLE_VALIDATION: bool = cfg!(debug_assertions)` (`:61`) couples validation to
profile. Goldens want reproducible SPIR-V, i.e. a release build using the
committed `shaders/compiled/` bytecode — which today has no validation at all.
Debug builds recompile `.slang` at pipeline creation, and
`assert_shader_interface_unchanged` (`:5041`) guards only the reflection
*interface*, not the bytecode, so a local Slang bump can shift pixels without
tripping anything.

Replace with an `enable_validation()` function backed by a `OnceLock` reading
`VKR_VALIDATION` (`1`/`0`, defaulting to `cfg!(debug_assertions)`). **The
`OnceLock` is essential** — `Drop` must observe the same value as init, or you
destroy a null messenger or leak one. Four call sites: `get_required_layers`
(`:2940`), `Renderer::init`, `maybe_create_debug_messager_extension`
(`debug.rs:51`), and `Drop`. (The 2026-07-25 version said to update `CLAUDE.md`'s
"Key Constants" section — no such section exists, and `CLAUDE.md` never mentions
`ENABLE_VALIDATION`. Nothing to update there.)

Phase 1 does not need this: it runs debug, where validation is already on.

## 12. The goldens determinism problem — **decision deferred**

Two independent obstacles, both measured rather than assumed.

**Cross-driver goldens are impossible.**

```
lavapipe   framebufferColorSampleCounts = {1×, 4×}
NVIDIA     framebufferColorSampleCounts = {1×, 2×, 4×, 8×}
Intel Xe   framebufferColorSampleCounts = {1×, 2×, 4×, 8×, 16×}
```

With the default `MaxMSAASamples`, `get_max_usable_sample_count` (`:4973`) picks
a different count per driver — different coverage, different resolve. Pinning
equalizes the *count* but not rasterization tie-breaks, float precision (llvmpipe
vs NVIDIA transcendentals in the SDF and ray-marching examples differ by far more
than 4/255), texture filtering, or anisotropy.

**Cross-*machine* lavapipe goldens are also not free** — this is the new
finding, and the reason the driver question isn't being answered yet. Lavapipe
reports itself as `llvmpipe (LLVM 20.1.2, **256 bits**)`. That vector width is
derived from the host CPU's ISA: this i7-12700H has no AVX-512, so llvmpipe JITs
256-bit vectors, while a cloud CPU that does (Sapphire Rapids, Zen 4/5) would
JIT 512-bit — different codegen, potentially different pixels in exactly the
float-heavy shaders goldens are most valuable for. `LP_NATIVE_VECTOR_WIDTH=256`
pins it. Mesa and LLVM versions also move with the distro and would need pinning
too — i.e. the container that phase 1 deliberately avoids.

The three options, with costs:

| option | gates in CI? | real-driver coverage? | cost |
|---|---|---|---|
| lavapipe only (`goldens/lavapipe/`) | yes | none | pin `LP_NATIVE_VECTOR_WIDTH` + Mesa/LLVM versions, i.e. a container |
| NVIDIA only (`goldens/rtx3070ti/`) | no | yes | goldens can only ever be checked on Dan's laptop |
| both sets | yes | yes | double the blessing work and double the PNG churn |

**Deliberately open**, pending phase 1 experience — in particular, whether the
container turns out to be a place we actually want pixel comparison to happen,
or just a place we want API-legality checked.

Whatever is chosen, the device slug belongs in the path
(`goldens/<device>/*.png`) so it is structurally impossible to compare a golden
against a run from a different driver, and a `goldens/manifest.json` should
**refuse to compare** when the live config (device name, extent, frames, MSAA)
doesn't match what was blessed — mirroring the existing
`scripts/link_converted.sha256` convention.

## 13. Comparison harness

`src/bin/image_diff.rs`, a new bin (`image = "0.25.6"` is already a dependency
and `src/bin/*.rs` is an established pattern, which keeps decode consistent with
encode):

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

`just bless EXAMPLE` captures into `goldens/<device>/<example>.png` and
regenerates the manifest entry; review with `git diff --stat goldens/` and by
eyeballing, the same workflow as insta snapshots.

**Golden churn** is the one sizing concern: PNGs don't delta-compress, so every
re-bless adds a full blob. Bless rarely, start with 4-6 curated examples
(`dragon`, `sdf_2d`, `viking_room`, `suzanne`, `koch_curve`, `basic_triangle`),
and grow only when a golden has actually caught something. Do **not** reach for
git-lfs — there's no `.gitattributes` today and it would complicate every clone.

---

## Critical files

Phase 1:

- `src/renderer.rs` — `WindowTarget` replacing `window: Window` (`:102`),
  `init` (`:241`), instance extensions (`:266`), surface creation (`:303`),
  stored window (`:411`), SDL text input (`:2078`), `recreate_swapchain`
  (`:2462`), `check_required_extensions` (`:2971`), `choose_swap_extent`
  (`:3239`), `create_swapchain` (`:3266`), fingerprint in `init`
- `src/renderer/debug.rs` — counters in `vulkan_debug_utils_callback` (`:14-49`)
- `src/game/traits.rs` — `run()` (`:80`), `enable_egui` (`:98`),
  `compute_render_scale_for_display` (`:121`)
- `src/app.rs` — `run_loop` (`:29`), pacing (`:52-57`), `handle_events` (`:68`)
- `src/renderer/platform.rs` — `OS_SURFACE_EXT` stays, SDL path only
- `src/headless.rs` — new
- `justfile`, `CLAUDE.md` — new recipes; document the headless flag

Phase 2 additionally: `src/renderer/capture.rs` (new, templated on
`src/renderer/picking.rs`), `src/bin/image_diff.rs` (new), `goldens/`,
`format_block_info` (`:4117`), the barrier masks (`:1731`, `:1918`), and
`ENABLE_VALIDATION` (`:61`).
