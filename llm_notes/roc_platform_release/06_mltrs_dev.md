# phase 6 — runtime hot-reload flag and `mltrs dev`

> **Re-scoped after implementation, 2026-08-19.** The work landed split:
>
> - §1–§4 (the `VKR_SHADER_HOT_RELOAD` flag) ship as their own PR against
>   `main`. They are a workspace change with no platform dependency.
> - §5 (`mltrs dev`) is **dropped**, not deferred. Run the platform with the
>   flag directly: `VKR_SHADER_HOT_RELOAD=1 roc examples/<name>.roc`.
> - The implementation verified the full stack once: the roc-linked host at
>   first failed at pipeline creation, because the unversioned stub libc made
>   ld.so bind `realpath` to the compat `realpath@GLIBC_2.2.5`
>   ([`../tech_debt.md`](../tech_debt.md) §20, fixed by version pins in
>   `stubs/generate.sh`). With the pins in place, hot reload worked inside
>   the roc-linked host.
> - Two additions this document does not anticipate: `just watch` also sets
>   the flag, and the flag's unconditional `mod shader_watcher` puts `notify`
>   into the release `libhost.a`, adding five libc symbols to the stub set.
>   Whichever branch merges second runs `just roc-platform stubs`.

Sub-plan of [`roc_platform_release.md`](../roc_platform_release.md) §6. The
parent holds the rationale: why the commands shell out to `roc`, and why the
shader watcher stays inside the renderer. This document is the implementation
spec.

Two scope changes against the parent's §6:

- **`mltrs run` is deferred indefinitely.** Its future shape is recorded in
  §6 below: the same release host with a different `EnvConfig`. Nothing in
  this phase blocks it.
- **The hot-reload switch moves from build profile to run time.** Today
  `cfg(debug_assertions)` selects between source recompilation and
  precompiled SPIR-V. After this phase a new `EnvConfig` flag selects, in
  every build profile. `mltrs dev` then runs the release host with the flag
  set.

## 0. Current state

`cfg(debug_assertions)` gates the hot-reload machinery at about 30 sites:

- the watcher module (`crates/renderer/src/lib.rs:6`) and the `SpvBytes`
  trait (`crates/renderer/src/shaders.rs:6-18`);
- three `Renderer` fields, three imports, the `shaders_source_dir` parameter
  of `Renderer::init`, and the watcher construction
  (`crates/renderer/src/renderer.rs:18-24`, `:119-129`, `:268-273`,
  `:431-437`);
- the per-frame poll in `draw_frame` (`renderer.rs:2503-2525`) and the
  recompile functions it calls (`renderer.rs:2788-2960`), plus the deferred
  pipeline destroy in teardown (`renderer.rs:3039`);
- the debug/release pairs of `ShaderPipelineLayout::create_from_atlas`
  (`renderer.rs:5121-5190`) and
  `ComputeShaderPipelineLayout::create_from_atlas` (`renderer.rs:5204-5260`),
  and their five call sites;
- the `get_mut_by_index` accessors and the `raster_state` field
  (`crates/renderer/src/renderer/pipeline.rs:171`, `:197`, `:464`, `:474`);
- the `dev_compile_slang_shaders` / `dev_compile_slang_compute_shaders`
  wrappers (`crates/slang-reflection/src/lib.rs:178`, `:272`);
- the `SHADERS_SOURCE_DIR` argument in `Game::run`
  (`crates/mltrs/src/game/traits.rs:126`).

The gate costs nothing at the dependency level. `shader-slang` is a plain
dependency of `mltrs-slang-reflection`, no `[features]` table exists anywhere
in the workspace, `notify` is a plain dependency of `mltrs-renderer`, and the
generated code embeds SPIR-V through `include_bytes!` in every profile. Both
paths already exist in every binary; the cfg only chooses the caller. Removing
it changes no dependency graph and no binary size.

The gate does force one behavior: a debug build always compiles shaders from
source at pipeline creation, and a release build never does. The sweep runs
debug builds, so it exercises the source-compile path today. §4 keeps that
coverage.

## 1. The flag

Add one field to `EnvConfig` (`crates/renderer/src/env_config.rs`):

```rust
/// `VKR_SHADER_HOT_RELOAD=1` — compile shaders from `shaders/source/` at
/// pipeline creation and recompile them on edit. Unset (or false) uses the
/// precompiled SPIR-V embedded by `mltrs shaders compile`, in every build
/// profile.
pub shader_hot_reload: bool,
```

Parse it with the existing `flag()` helper. Unset means off, in debug builds
too. `just dev` supplies the flag (§4), so the interactive dev loop keeps hot
reload; a bare `cargo run -p <example>` uses precompiled SPIR-V.

## 2. The renderer

Remove every cfg listed in §0 and select at run time. The `enable_egui: bool`
parameter of `Renderer::init` is the shape to copy: a plain runtime value,
no cfg inside the renderer.

- `Renderer::init` takes `shaders_source_dir: &'static str` unconditionally.
  `Game::run` passes `Self::Atlas::SHADERS_SOURCE_DIR` unconditionally
  (`traits.rs:126` loses its cfg).
- The watcher field becomes `shader_watcher: Option<ShaderChanges>`.
  `Renderer::init` sets it from the flag:
  - flag off → `None`. No watcher, no inotify thread.
  - flag on and the directory exists → `Some(shader_watcher::watch(...)?)`.
  - flag on and the directory is missing → return an error that names the
    path and `VKR_SHADER_HOT_RELOAD`. `SHADERS_SOURCE_DIR` is an absolute
    path baked at compile time
    (`crates/cli/templates/shader_atlas.rs.askama:23-24`), so a binary or a
    bundled platform running on another machine hits this case. An
    explicitly requested mode that cannot work fails loudly; it does not
    degrade to precompiled SPIR-V. A runtime source-dir override is phase-5
    work, where runtime shader loading defines the app-side directory
    anyway.
- `shaders_source_dir` and `old_pipelines` become unconditional fields.
  `old_pipelines` stays empty when the flag is off, and the teardown drain
  (`renderer.rs:3039`) runs unconditionally over it.
- The `draw_frame` poll (`renderer.rs:2503`) runs only when `shader_watcher`
  is `Some`. `check_for_shader_recompile`, `try_shader_recompile` and
  `try_compute_shader_recompile` lose their cfgs unchanged.
- Each `create_from_atlas` pair merges into one function whose last
  parameter is `hot_reload_source_dir: Option<&Path>`. `Some(dir)` takes the
  source-compile path (`dev_compile_slang_*`,
  `assert_shader_interface_unchanged`, `spv_bytes`); `None` takes the
  precompiled path (`precompiled_shaders()` / `precompiled_compute_shader()`
  and `pipeline_layout()`). The five call sites in `renderer.rs` pass
  `self.shader_watcher.is_some().then(|| ...)` through one small helper.
- `pipeline.rs`: the `get_mut_by_index` accessors and `raster_state` lose
  their cfgs; `raster_state` drops its `expect(unused)`. The unused
  `get_mut` (`pipeline.rs:463`) keeps its `expect(unused)`.
- The gated imports (`renderer.rs:18-24`) become unconditional, including
  the glob `use log::*;` the recompile functions rely on.

Out of scope, unchanged: `ENABLE_VALIDATION` and every validation cfg, the
`enable_egui = cfg!(debug_assertions)` default, the texture debug-name
fields, and the shader-println device features (`renderer.rs:3760`, `:3785`).

## 3. slang-reflection

Drop the `#[cfg(debug_assertions)]` on `dev_compile_slang_shaders`
(`lib.rs:178`) and `dev_compile_slang_compute_shaders` (`lib.rs:272`). Both
delegate to functions that are already unconditional. No other change.

## 4. justfile and sweep

- `just dev` (both the unix and windows recipes) sets
  `VKR_SHADER_HOT_RELOAD=1` before `cargo run -p {{example}}`.
- `just shader-debug` sets it the same way. It is the other interactive
  loop, and shader printf debugging pairs with shader editing.
- `scripts/headless-sweep.sh` adds `export VKR_SHADER_HOT_RELOAD=1` beside
  `export VKR_SWEEP=1`. The sweep's debug builds compile shaders from source
  today; the export keeps that coverage, including
  `assert_shader_interface_unchanged` against every example's committed
  reflection JSON.
- `docs/testing.md` records the new export where it describes the sweep
  environment.

## 5. `mltrs dev`

Add `Command::Dev(DevArgs)` in `crates/cli/src/main.rs` beside `Shaders`.

```rust
#[derive(Args)]
struct DevArgs {
    /// the roc app to run
    #[arg(long, default_value = "main.roc")]
    app: PathBuf,
    /// the consuming crate's root directory
    #[arg(long, default_value = ".")]
    crate_dir: PathBuf,
}
```

Behavior:

1. Fail with a clear message if `args.app` does not exist.
2. Compile shaders: `build_tasks::write_precompiled_shaders` with
   `generate_rust_source: false` and the same directory defaults `compile()`
   uses (`main.rs:119-134`). This validates the author's slang before the
   game starts and keeps `shaders/compiled/` current for the future
   `mltrs run`.
3. Exec `roc --max-transitive-mb=0 <app>` with `VKR_SHADER_HOT_RELOAD=1`
   set in the child environment. On unix use
   `std::os::unix::process::CommandExt::exec`; elsewhere spawn, wait, and
   exit with the child's status. A `roc` missing from `PATH` produces a
   clear error naming the binary.

Notes:

- `--max-transitive-mb=0` is required for a URL-named platform
  ([`tech_debt.md`](../tech_debt.md) §19) and harmless for a path-named one,
  so `mltrs dev` always passes it.
- The env-var name appears as a string literal in both `mltrs-cli` and the
  renderer. `mltrs-cli` must not depend on the renderer, so the duplication
  is accepted; a test is impossible across the crates, and the done criteria
  cover it end to end.
- The CLI does not watch files. The watcher lives in the renderer inside
  `libhost.a`; the env var enables it.
- "Release build of the renderer" needs no work here: `roc-platform/build.sh`
  already builds `libhost.a` with `cargo build --release`.

Limitation, accepted until phase 5: with a bundled platform on another
machine, `SHADERS_SOURCE_DIR` points at the build machine's tree, so
`mltrs dev` fails at startup with the §2 error. The platform also cannot load
the author's shaders before phase 5, so `mltrs dev` is only useful against a
locally built platform today. Phase 5's runtime shader loading removes both
constraints together.

## 6. `mltrs run`, deferred

`mltrs run` execs `roc <app>` with no compile step and without
`VKR_SHADER_HOT_RELOAD`, so the host uses the SPIR-V in `shaders/compiled/`.
It becomes useful when the platform reads those files at run time, which is
phase-5 work. Record the deferral in the parent plan; add no code.

## Done criteria

- `cargo check --workspace --all-targets`, `just lint`, `cargo fmt` and
  `just test` pass.
- `just sweep` passes. The sweep exports `VKR_SHADER_HOT_RELOAD=1`.
- Debug `cargo run -p basic_triangle` with the variable unset creates
  pipelines from precompiled SPIR-V and starts no watcher.
- `just dev` hot-reloads: edit
  `examples/basic_triangle/shaders/source/basic_triangle.shader.slang`,
  observe `recompiling shaders...` and the visual change.
- `VKR_SHADER_HOT_RELOAD=1 cargo run --release -p basic_triangle`
  hot-reloads the same way. This is the release-plus-flag combination
  `mltrs dev` relies on.
- With the flag set and the source directory absent, startup fails with an
  error naming the path and `VKR_SHADER_HOT_RELOAD`.
- `just roc-platform build && just roc-platform test` passes.
- In `roc-platform/`, `mltrs dev --app examples/basic_triangle.roc`
  compiles the shaders, launches through `roc`, and hot-reloads
  `roc-platform/shaders/source/basic_triangle.shader.slang`.
- `roc examples/basic_triangle.roc` in `roc-platform/` (no variable) still
  runs from precompiled SPIR-V.
