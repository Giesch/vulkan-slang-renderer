# mltrs workspace migration plan

*(update 2026-08, at implementation time: the plan below predates the `gx`
module, the `toon_link` example, `env_config.rs`, and `tev.slang`. The
implementation follows the plan with these drift adjustments:*
- *`env_config.rs` lives in `crates/renderer` (both `renderer.rs` and
  `game/traits.rs` use it), re-exported as `mltrs::env_config`.*
- *`gx/model_manifest.rs` (+ the `gx_enum!` macro) became a fourth library
  crate `crates/gx` (package `gx`), shared by `convert-link` and `toon_link`.
  `gx/tev_pack.rs` could **not** go into a library crate — it imports the
  generated `tev::TevParams` — so it moves into the `toon_link` example crate
  (interim: an `mltrs::gx` facade module).*
- *`convert_link` had moved to `src/gx/bin/`; it still splits out as
  `crates/convert-link`, depending on `gx` rather than on mltrs. Its
  test fixtures reach the repo-root `assets/` via `../../` from the crate.*
- *`shader_branching_snapshots` (postdates the plan) now compiles the
  `fixtures/shaders` corpus in-test instead of reading the monolith's
  committed `shaders/compiled`.*
- *`json.rs`'s roundtrip test gets a committed fixture copy of
  `basic_triangle.json` inside the renderer crate.*
- *§7's corpus gained `ray_march_camera.slang`'s import parent chain via
  `gpu_picking`; the snapshot bodies were diffed old-vs-new at migration time
  and matched exactly for every carried-over file.)*

Refactor the single crate `vulkan-slang-renderer` into a cargo workspace with three library/tool
crates (`mltrs`, `mltrs-renderer`, `mltrs-cli`) plus per-example crates (axum-style). The driving
goal: a consuming Rust project should be able to depend on `mltrs`, run the `mltrs` CLI against its
own slang shaders, and get generated bindings that compile in *its* crate — the examples become the
first consumers of that workflow.

Every phase leaves the repo green: `cargo check --workspace --all-targets`, `just shaders`,
`just test`, `just lint` all pass at each phase boundary.

## Decisions (settled)

- **Virtual workspace root**: root `Cargo.toml` is `[workspace]`-only; members `crates/*` and
  `examples/*`. Dependency versions pinned in `[workspace.dependencies]`; lint policy in
  `[workspace.lints]`; `rustfmt.toml` stays at the root (rustfmt walks up to find it).
- **Package names**: `mltrs` (consumer-facing), `mltrs-renderer` (dir `crates/renderer`),
  `mltrs-cli` (dir `crates/cli`). The CLI's binary is named just **`mltrs`**.
- **Examples become individual crates** under `examples/<name>/`, each with its own `Cargo.toml`,
  `src/main.rs`, `shaders/source/`, `shaders/compiled/`, and `src/generated/`.
- **Assets move into example crates** (viking room model into `examples/viking_room/`, etc.).
  Shared example slang modules (`ray_march.slang`, …) are duplicated where needed.
- **Vendored engine slang modules** (shipped by `mltrs shaders init`): `addr.slang`, `mvp.slang`,
  `projection.slang`, `fullscreen_triangle.slang`, `super_sample.slang`. Example-domain modules
  stay with their examples.

## 0. Target end state

### Directory tree

```
vulkan-slang-renderer-2/
├── Cargo.toml                     # [workspace] only (virtual root)
├── Cargo.lock
├── rustfmt.toml                   # stays at root
├── rust-toolchain.toml
├── justfile
├── .env
├── scripts/                       # unchanged
├── slang/                         # git submodule, unchanged; workspace `exclude`
├── assets/link/                   # convert_link data, stays at repo root
├── crates/
│   ├── renderer/                  # package "mltrs-renderer"
│   │   └── src/
│   │       ├── lib.rs             # pub mod renderer; pub mod shaders; pub mod editor;
│   │       │                      # #[cfg(debug_assertions)] mod shader_watcher;
│   │       ├── renderer.rs  renderer/   # addr, debug, egui, facet_egui, gpu_write, picking,
│   │       │                            # pipeline, platform, storage_buffer, storage_texture,
│   │       │                            # texture, uniform_buffer, vertex_description
│   │       ├── editor.rs
│   │       ├── shaders.rs         # compile entry points (prepare_reflected_* made pub)
│   │       ├── shaders/
│   │       │   ├── atlas.rs
│   │       │   ├── json.rs  json/{parameters.rs,pipeline_builders.rs}
│   │       │   └── reflection.rs  reflection/{parameters.rs,pipeline_layout.rs}
│   │       └── shader_watcher.rs
│   ├── mltrs/                     # package "mltrs" (consumer-facing)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── app.rs
│   │       ├── game.rs  game/traits.rs
│   │       ├── ktx.rs
│   │       ├── model_manifest.rs
│   │       └── util.rs            # manifest_path! macro, load_image(path)
│   ├── cli/                       # package "mltrs-cli", [[bin]] name = "mltrs"
│   │   ├── templates/             # the 4 askama templates (compiled into the binary)
│   │   ├── vendor/                # canonical engine slang modules, embedded via include_str!
│   │   ├── fixtures/
│   │   │   ├── shaders/           # curated snapshot-test corpus (§7)
│   │   │   ├── alignment/         # std140/std430/pointer *.slang fixtures (from shaders/test/)
│   │   │   └── check_crate/       # package "shader-check"; own [workspace]; workspace-excluded
│   │   └── src/
│   │       ├── main.rs            # clap
│   │       ├── build_tasks.rs     # from src/shaders/build_tasks.rs (+ its #[cfg(test)] mod)
│   │       ├── util.rs            # relative_path, local manifest_path helper for tests
│   │       └── snapshots/         # renamed insta snapshots (mltrs_cli__build_tasks__tests__*)
│   └── convert-link/              # package "convert-link", [[bin]] name = "convert_link"
│       └── src/                   # from src/bin/convert_link/
└── examples/
    ├── basic_triangle/
    │   ├── Cargo.toml
    │   ├── shaders/source/        # own *.slang + copied shared modules + vendored engine modules
    │   ├── shaders/compiled/      # committed spirv + json (per-example)
    │   └── src/
    │       ├── main.rs            # was examples/basic_triangle.rs; adds `mod generated;`
    │       └── generated.rs  generated/shader_atlas/…
    ├── … (15 total, see §8)
    └── watercolor/
        ├── src/bin/generate_paper_texture.rs   # second bin; default-run = "watercolor"
        └── textures/…
```

### Crate dependency graph

```
mltrs-renderer  ←  mltrs  ←  examples/* , convert-link
      ↑
   mltrs-cli   (uses mltrs_renderer::shaders::{json, prepare_reflected_*, ...})
```

No cycles. The three edges that currently point the wrong way are resolved by:

- moving `MaxMSAASamples` from `game/traits.rs` into the renderer crate (re-exported from
  `mltrs::game`, so `Game::max_msaa_samples()` signatures in examples are untouched);
- moving `editor.rs` into the renderer crate (needed by `renderer/facet_egui.rs:3`), re-exported
  as `mltrs::editor`;
- moving the `shaders` modules (compile, atlas, json, reflection) *into* the renderer crate, which
  also keeps the ~6 inherent `impl` blocks on `shaders::json::Reflected*` types
  (`renderer.rs:5182-5350`) legal under the orphan rule.

## 1. Root Cargo.toml (end state)

```toml
[workspace]
resolver = "3"
members = ["crates/*", "examples/*"]
exclude = ["crates/cli/fixtures/check_crate", "slang"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
# internal
mltrs = { path = "crates/mltrs" }
mltrs-renderer = { path = "crates/renderer" }

# external
anyhow = "1.0.100"
ash = { version = "0.38.0", features = ["linked"] }
askama = "0.14.0"
clap = { version = "4.5", features = ["derive"] }
egui = "0.33"
egui-ash-renderer = { version = "0.11.0", features = ["dynamic-rendering"] }
facet = { version = "0.42", features = ["reflect"] }
facet-core = { version = "0.42.0", features = ["std"] }
glam = { version = "0.30.3", features = ["serde"] }
heck = "0.5.0"
image = "0.25.6"
insta = { version = "1.43.2", features = ["json", "glob"] }
ktx2 = "0.5.0"
log = "0.4.27"
notify = "8.1.0"
pretty_env_logger = "0.5.0"
rodio = "0.22.2"
rspirv = "0.12.0"
sdl3 = { version = "0.14.29", features = ["ash", "build-from-source-static"] }
serde = { version = "1.0.219", features = ["derive"] }
serde_json = "1.0.141"
shader-slang = { git = "https://github.com/Giesch/slang-rs.git", branch = "main", default-features = false, features = ["static"] }
thiserror = "2.0.17"
tobj = "4.0.3"
uuid = { version = "1.18.1", features = ["v4"] }
vk-mem = "0.5.0"

[workspace.lints.clippy]
type_complexity = "allow"
too_many_arguments = "allow"

# moved from the old package manifest (profiles only apply at the workspace root)
[profile.dev.package."*"]
opt-level = 3
```

- With the clippy allows at workspace level, the `#![allow(...)]` at `renderer.rs:1` can be dropped.
- Every member sets `version.workspace = true`, `edition.workspace = true`, and
  `[lints] workspace = true`, and pulls deps with `{ workspace = true }`.

### Per-crate dependency essentials

- **mltrs-renderer**: anyhow, ash, egui, egui-ash-renderer, facet, facet-core, glam, heck, image,
  log, notify, sdl3, serde, serde_json, shader-slang, thiserror, vk-mem.
- **mltrs**: mltrs-renderer, anyhow, ash, facet, glam, image, ktx2, log, pretty_env_logger, sdl3,
  serde, serde_json (trim by compiling; e.g. drop ash if `ktx.rs` doesn't name `vk::` directly).
- **mltrs-cli**: mltrs-renderer, anyhow, askama, clap, heck, serde_json;
  dev-deps insta, rspirv, uuid. `[[bin]] name = "mltrs"`.
- **convert-link**: mltrs (for `model_manifest`), anyhow, glam, heck, image, serde, serde_json
  (confirm exact set at move time).
- **examples/<name>**: mltrs, anyhow, ash, facet, glam, serde, serde_json, plus per-example extras
  (tobj, rodio, image, log — see §8).

## 2. Module placement table

| Current path | Target |
|---|---|
| `src/app.rs` | `crates/mltrs/src/app.rs` |
| `src/game.rs`, `src/game/traits.rs` | `crates/mltrs/src/game{.rs,/traits.rs}`; `MaxMSAASamples` enum moves to renderer, re-exported here |
| `src/editor.rs` | `crates/renderer/src/editor.rs`; re-exported as `mltrs::editor` |
| `src/renderer.rs` + `src/renderer/*` | `crates/renderer/src/renderer{.rs,/}` |
| `src/shaders.rs` + `src/shaders/{atlas,json*,reflection*}` | `crates/renderer/src/shaders{.rs,/}`; `prepare_reflected_*` made `pub` |
| `src/shader_watcher.rs` | `crates/renderer/src/shader_watcher.rs` |
| `src/shaders/build_tasks.rs` + `src/shaders/snapshots/` | `crates/cli/src/build_tasks.rs` + `crates/cli/src/snapshots/` |
| `src/util.rs` | `crates/mltrs/src/util.rs` (`manifest_path!` macro + `load_image(path)`); `relative_path` copied to `crates/cli/src/util.rs` |
| `src/ktx.rs` | `crates/mltrs/src/ktx.rs` |
| `src/model_manifest.rs` | `crates/mltrs/src/model_manifest.rs` |
| `src/generated*` | deleted at the end; each example generates its own |
| `src/bin/prepare_shaders.rs` | deleted (replaced by `mltrs shaders compile`) |
| `src/bin/generate_paper_texture.rs` | `examples/watercolor/src/bin/` (`default-run = "watercolor"`) |
| `src/bin/convert_link/*` | `crates/convert-link/src/*` |
| `templates/*.askama` | `crates/cli/templates/` (askama resolves relative to the compiling crate) |
| `shaders/source/*.{shader,compute}.slang` | per-example `examples/<name>/shaders/source/` (§8) |
| engine modules (addr/mvp/projection/fullscreen_triangle/super_sample) | canonical copies in `crates/cli/vendor/`; per-example copies via `mltrs shaders init` |
| example-shared modules (ray_march, ray_march_camera, particle, dragon_curve, gpu_picking_common, watercolor_common) | duplicated into each example crate that needs them |
| `shaders/compiled/*` | per-example, regenerated, committed |
| `shaders/test/*.slang` | `crates/cli/fixtures/alignment/` |
| `shaders/test/check_crate/` | `crates/cli/fixtures/check_crate/` (workspace-excluded) |
| `textures/`, `models/`, `audio/` | distributed into example crates (§8) |
| `assets/link/` | stays at repo root |

## 3. The mltrs public API surface

`crates/mltrs/src/lib.rs` (end state):

```rust
pub use mltrs_renderer::{editor, renderer, shaders};

pub mod app;
pub mod game;
pub mod ktx;
pub mod model_manifest;
pub mod util;   // load_image; manifest_path! is #[macro_export]

pub use game::*;
```

What examples / generated code / consumers import:

- `mltrs::game::{Game, Input, Key, MouseButton, WindowDescription, MaxMSAASamples}`
- `mltrs::renderer::*` (Renderer, FrameRenderer, DrawError, handles, PipelineConfig, …),
  `mltrs::renderer::gpu_write::GPUWrite`,
  `mltrs::renderer::vertex_description::{NoVertex, VertexDescription}`
- `mltrs::shaders::atlas::{ShaderAtlasEntry, ComputeShaderAtlasEntry, PrecompiledShader, PrecompiledShaders}`,
  `mltrs::shaders::json::{ReflectionJson, ComputeReflectionJson, ReflectedPipelineLayout, …}`
- `mltrs::editor::{Label, Slider, Checkbox, RadioButton, pascal_to_display}`
- `mltrs::ktx::load_ktx2`, `mltrs::util::load_image`, `mltrs::manifest_path!`, `mltrs::model_manifest`

These paths deliberately mirror the check_crate stub layout (`crate::renderer`,
`crate::shaders::atlas`, `crate::shaders::json`), so a single `import_root` template parameter
covers both `"crate"` (fixture) and `"mltrs"` (real consumers).

## 4. Phase 1 — Parameterize codegen; de-hardcode paths (lands in the monolith)

Goal: all path/import assumptions become parameters while everything still lives in one crate.

### 4.1 `import_root` codegen parameter

- Add `pub import_root: String` to `Config` in `src/shaders/build_tasks.rs` (current value: `"crate"`).
- Thread it into the three template structs and rewrite the hardcoded imports:
  - `templates/shader_atlas_entry.rs.askama` lines 14–19:
    `use crate::renderer::gpu_write::GPUWrite;` → `use {{ import_root }}::renderer::gpu_write::GPUWrite;`
    (same for `vertex_description`, `renderer::*`, `shaders::atlas::…`, `shaders::json::…`)
  - `templates/shader_compute_entry.rs.askama`: same block
  - `templates/shader_shared_module.rs.askama`: the single `GPUWrite` import
- `src/bin/prepare_shaders.rs` passes `import_root: "crate".into()`. Regenerating must produce a
  zero diff in `src/generated/` (before 4.2).

### 4.2 Hot-reload path: bake the shader source dir into generated entries

Current breakage after the split: `shader_watcher::watch()` uses
`manifest_path(["shaders","source"])` (the *library's* manifest dir) and
`dev_compile_slang_shaders` hardcodes cwd-relative `"shaders/source"` (`src/shaders.rs:126,204`).
Both are wrong once examples are separate crates run from the workspace root.

Design (no Game-trait boilerplate; always correct):

- Add to `ShaderAtlasEntry` and `ComputeShaderAtlasEntry` (`src/shaders/atlas.rs`):
  ```rust
  /// dev only: absolute path to the slang source dir this entry was generated from
  fn shaders_source_dir(&self) -> &'static std::path::Path;
  ```
- Templates emit the impl using `env!` — which expands when the *consuming* crate compiles the
  generated file, yielding the example's own dir:
  ```rust
  fn shaders_source_dir(&self) -> &'static std::path::Path {
      std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source"))
  }
  ```
  `shaders/source` stays a fixed convention relative to the consuming crate (matching the CLI
  defaults in §6) — not a codegen parameter.
- `dev_compile_slang_shaders(source_file_name, search_path: &Path)` (and compute variant) gain the
  search-path param; call sites `renderer.rs:5075` / `renderer.rs:5141` pass
  `shader.shaders_source_dir()`.
- `shader_watcher::watch(dir: &Path)`; Renderer makes watching lazy:
  - replace `shader_changes: shader_watcher::ShaderChanges` (`renderer.rs:92`) with a
    `Vec<ShaderChanges>` + `BTreeSet<PathBuf>` of watched dirs (all `#[cfg(debug_assertions)]`)
  - delete the eager `shader_watcher::watch()?` in `Renderer::init` (`renderer.rs:248`)
  - `create_pipeline` (`renderer.rs:989`) and the compute equivalent call
    `self.ensure_watching(shader.shaders_source_dir())`
  - the per-frame event drain iterates all watchers
- Update the hand-written stubs in `shaders/test/check_crate/src/shaders/atlas.rs` — the stubs are
  the codegen-facing API spec and must stay in sync.

### 4.3 `manifest_path` becomes a macro; `load_image` takes a path

`util::manifest_path` bakes `env!("CARGO_MANIFEST_DIR")` at *library* compile time
(`src/util.rs:8`) — wrong for external callers post-split.

- Add:
  ```rust
  #[macro_export]
  macro_rules! manifest_path {
      ($($seg:expr),* $(,)?) => {{
          let p: ::std::path::PathBuf =
              [env!("CARGO_MANIFEST_DIR"), $($seg),*].into_iter().collect();
          p
      }};
  }
  ```
- `load_image(file_name: &str)` → `load_image(path: impl AsRef<Path>)` (drop the internal
  `textures/` prefix).
- Migrate call sites now, while everything is one crate: the asset-using examples
  (e.g. `viking_room.rs:90` → `load_image(manifest_path!["textures", "viking_room.png"])`),
  plus the cwd-relative `"audio/alias_abandon.beats.json"` at `sdf_2d.rs:55`, and
  `generate_paper_texture.rs`.
- Keep the `manifest_path` *fn* only where it is genuinely about the current crate
  (`prepare_shaders.rs`, build_tasks tests) — it moves/dies with them later.

### 4.4 Verify Phase 1

```
cargo check --all
just shaders          # diff should be ONLY the new shaders_source_dir impls
just test             # cargo insta review: snapshots pick up the new trait method
just lint
timeout 3 just dev basic_triangle
just dev watercolor   # edit a .slang while running → hot reload still works
```

## 5. Phase 2 — Workspace scaffolding + crate split

Examples stay as cargo examples of mltrs for this phase. One large structural commit, all
`git mv` to preserve history.

1. Root `Cargo.toml` becomes the virtual workspace (§1) with `members = ["crates/*"]` for now
   (`examples/*` is added in Phase 4 when the first example crate exists — a glob matching flat
   `.rs` files must be avoided).
2. **crates/renderer**: `git mv` renderer.rs, renderer/, editor.rs, shaders.rs,
   shaders/{atlas,json*,reflection*}, shader_watcher.rs. Thin `lib.rs` re-declares the same module
   names so all internal `crate::renderer::…` / `crate::shaders::…` paths keep working unchanged.
   Changes: move `MaxMSAASamples` into renderer.rs (delete the `renderer.rs:15` import;
   `game/traits.rs` re-exports it); make `prepare_reflected_shader` /
   `prepare_reflected_compute_shader` pub.
3. **crates/mltrs**: `git mv` app.rs, game.rs/, ktx.rs, model_manifest.rs, util.rs, generated.rs,
   generated/ (interim), plus `examples/`, `textures/`, `models/`, `audio/`, `shaders/source`,
   `shaders/compiled` into `crates/mltrs/`. lib.rs as §3 plus interim `pub mod generated;`.
   The interim generated code keeps `import_root = "crate"` and compiles because
   `crate::renderer` / `crate::shaders` resolve through the re-exports. Delete
   `src/bin/prepare_shaders.rs`. `generate_paper_texture.rs` temporarily to
   `crates/mltrs/src/bin/`.
4. **crates/cli**: `git mv` build_tasks.rs, snapshots/, templates/; add clap `main.rs` (§6);
   `util.rs` with `relative_path` + a local `manifest_path` fn. Fixtures:
   - `git mv shaders/test/*.slang crates/cli/fixtures/alignment/`
   - `git mv shaders/test/check_crate crates/cli/fixtures/check_crate`; append an empty
     `[workspace]` table to its Cargo.toml **and** list it in root `exclude`; bump its stale
     `glam = "0.29"` pin → `0.30`; ensure its `target/` is gitignored
   - build the curated `fixtures/shaders/` snapshot corpus (§7)
   - test path changes in build_tasks tests: `manifest_path(["fixtures","shaders"])` /
     `["fixtures","alignment"]` / `["fixtures","check_crate"]`; the alignment test's `cargo check`
     with `current_dir(check_crate)` keeps working because check_crate is its own workspace
5. **crates/convert-link** from `src/bin/convert_link/`; `output.rs:13`
   `use vulkan_slang_renderer::model_manifest` → `use mltrs::model_manifest`.
6. Interim justfile edits (full rewrite in Phase 5):
   - `dev`: `cargo run -p mltrs --example {{example}}`
   - `shaders`: `cargo run -p mltrs-cli -- shaders compile --crate-dir crates/mltrs --import-root crate && cargo fmt`
   - `test`: `INSTA_UPDATE=no cargo test --workspace`
   - `lint`: `cargo clippy --workspace --all-targets -- -D warnings` (+ `--release`)
   - `convert-link` / `link-verify-*`: `cargo run -p convert-link --bin convert_link -- …`,
     `cargo test -p convert-link -- --include-ignored`
7. `.env`: `RUST_LOG=mltrs_renderer::renderer::debug=warn,mltrs_renderer::renderer=info`.
8. **Insta snapshot rename**: names embed crate + module path
   (`vulkan_slang_renderer__shaders__build_tasks__tests__*` → `mltrs_cli__build_tasks__tests__*`).
   Delete the moved old snapshots, run `cargo insta test -p mltrs-cli --accept`, then diff the
   accepted content against the old files (alignment content should be identical;
   `generated_files` changes only per the new corpus). Commit.

### Verify Phase 2

```
cargo check --workspace --all-targets
just shaders && git diff --exit-code crates/mltrs/src/generated   # regeneration is a no-op
just test
just lint
timeout 3 just dev basic_triangle ; just dev watercolor   # hot-reload smoke test
```

## 6. Phase 3 — CLI finalization: clap surface + vendoring

(The clap skeleton can land inside Phase 2 step 4; vendoring is the genuinely new piece.)

### Subcommands

```
mltrs shaders compile [--crate-dir <DIR>] [--source-dir <DIR>] [--compiled-dir <DIR>]
                      [--rust-dir <DIR>] [--import-root <PATH>] [--no-rust]
mltrs shaders init [--dir <DIR>] [--force]
```

`shaders` is a clap subcommand group with `init`/`compile` nested subcommands, leaving room for
future non-shader command groups.

- `shaders compile` maps 1:1 onto `build_tasks::Config`:
  - `--crate-dir` default `.`; the other dirs default relative to it: `<crate-dir>/shaders/source`,
    `<crate-dir>/shaders/compiled`, `<crate-dir>/src`
  - `--import-root` default `"mltrs"`; `"crate"` is used by the mltrs interim and permanently by
    the check_crate fixture test (check_crate must not depend on mltrs, which would drag
    sdl3/slang into a `cargo check` fixture)
  - `--no-rust` replaces the `GENERATE_RUST_SOURCE` env var (inverted: rust generation is now the
    default; compile-only is the flag) *(update 2026-07: the env var was since removed from this
    repo; `prepare_shaders` now always generates rust source, matching the default sketched here.
    No `--no-rust` flag exists yet — only the in-process test path uses compile-only)*
  - writes `<compiled>/*.{vert,frag,comp}.spv` + `*.json`, and (unless `--no-rust`)
    `<rust-dir>/generated.rs` + `<rust-dir>/generated/shader_atlas{.rs,/*.rs}`
  - **improvement while here**: delete `<rust-dir>/generated/shader_atlas/` and stale `<compiled>`
    entries before writing, so removed shaders don't leave stale files (the repo currently has
    orphaned `wc_move_pigment_compute.rs` / `wc_transfer_pigment_compute.rs` — evidence this bites)
- `shaders init` writes the 5 vendored engine modules from `crates/cli/vendor/` via `include_str!`
  into `--dir` (default `shaders/source`), refusing to overwrite modified files unless `--force`.
- Askama templates compile into the binary; vendored slang + templates make
  `cargo install mltrs-cli` fully self-contained.

### Consumer story (README snippet)

```
cargo add mltrs            # path/git dep for now
mltrs shaders init         # seeds shaders/source with engine modules
# write shaders/source/my_game.shader.slang
mltrs shaders compile      # emits shaders/compiled + src/generated (imports `mltrs::…`)
# src/main.rs: mod generated; impl Game for MyGame; MyGame::run()
```

Verify: `cargo run -p mltrs-cli -- shaders compile --crate-dir crates/mltrs --import-root crate`
still no-op-diffs; `mltrs shaders init --dir /tmp/x` produces the 5 files; `just test`, `just lint`.

## 7. check_crate + snapshot corpus details

- **check_crate** (`fixtures/check_crate`, package `shader-check`): stays a hand-mirrored stub of
  the codegen-facing API (`src/renderer/{mod,addr,gpu_write,vertex_description}.rs`,
  `src/shaders/{mod,atlas,json}.rs`). The alignment test generates code with
  `import_root: "crate"`, copies it in, runs `cargo check`, cleans up. Keep in sync whenever
  templates/atlas traits change (Phase 1's `shaders_source_dir` addition already touches it).
- **generated_files corpus** (`fixtures/shaders/`): the current test snapshots *all* of
  `shaders/source` (36 files); after examples own their shaders that corpus disappears. Replace
  with a curated set exercising every codegen path:
  - engine modules: addr, mvp, projection, fullscreen_triangle, super_sample (copies of vendor/)
  - a vertex-buffer graphics shader (`basic_triangle.shader.slang`), a vertex-less fullscreen
    shader (`sdf_2d.shader.slang`), a compute + shared-module pair (`particles.compute.slang` +
    `particle_render.shader.slang` + `particle.slang`), and a cross-module-import case
    (`gpu_picking.shader.slang` + `gpu_picking_common.slang` + `ray_march_camera.slang`)
  - (Rejected alternative: keep all 36 sources — double maintenance for no extra coverage.)

## 8. Phase 4 — Example crate migration

Add `"examples/*"` to workspace members when the first crate lands. Order: `basic_triangle` first
(template for the rest), simple ones in bulk, `watercolor` and `gpu_picking` last. Package names
keep underscores so `just dev basic_triangle` → `cargo run -p basic_triangle`.

### Per-example checklist (mechanical)

1. Write `examples/<name>/Cargo.toml`:
   ```toml
   [package]
   name = "<name>"
   version.workspace = true
   edition.workspace = true
   publish = false

   [lints]
   workspace = true

   [dependencies]
   mltrs = { workspace = true }
   anyhow.workspace = true
   ash.workspace = true
   facet.workspace = true
   glam.workspace = true
   serde.workspace = true
   serde_json.workspace = true
   # + extras from the table below
   ```
2. `git mv crates/mltrs/examples/<name>.rs examples/<name>/src/main.rs`; add `mod generated;`;
   rewrite imports `vulkan_slang_renderer::…` → `mltrs::…` and
   `…::generated::shader_atlas::…` → `crate::generated::shader_atlas::…`.
3. `git mv` its shaders from `crates/mltrs/shaders/source/` (own `*.slang`; **copy** shared example
   modules if another example still needs them — `git mv` for the last user); run
   `cargo run -p mltrs-cli -- shaders init --dir examples/<name>/shaders/source` for engine
   modules; commit them.
4. `git mv` assets into the crate; `manifest_path!` call sites need no segment changes.
5. `cargo run -p mltrs-cli -- shaders compile --crate-dir examples/<name>` (import root defaults to
   `mltrs`; the `include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), …))` paths in generated code
   now resolve to the example dir). Commit `src/generated` + `shaders/compiled`.
6. Delete the example's entries from `crates/mltrs/shaders/source` and regenerate the interim
   mltrs atlas so the monolith atlas shrinks as you go.
7. Verify: `cargo check -p <name>`, `timeout 3 cargo run -p <name>`, hot-reload smoke test.

### Example → shaders/assets/deps table (verified from the slang `import` graph)

| example | own shaders | shared example modules (copied) | engine modules (`shaders init`) | assets moved | extra deps |
|---|---|---|---|---|---|
| basic_triangle | basic_triangle.shader.slang | – | mvp | – | – |
| depth_texture | depth_texture.shader.slang | – | mvp | textures/texture.jpg | – |
| viking_room | depth_texture.shader.slang (duplicate — renders via the depth_texture shader) | – | mvp | textures/viking_room.png, models/viking_room.obj | tobj |
| suzanne | suzanne.shader.slang | – | mvp | models/suzanne/* | tobj |
| multi_mesh | multi_mesh.shader.slang | – | mvp | – | – |
| koch_curve | koch_curve.shader.slang | – | fullscreen_triangle | textures/istockphoto-uffizi-*.jpg | – |
| sdf_2d | sdf_2d.shader.slang | – | fullscreen_triangle, super_sample | audio/alias_abandon.{flac,beats.json} | rodio |
| serenity_crt | serenity_crt.shader.slang | – | fullscreen_triangle | textures/serenity_crt/ | – |
| ray_marching | ray_marching.shader.slang | ray_march.slang, ray_march_camera.slang | addr, fullscreen_triangle, projection, super_sample | – | – |
| dragon | dragon.shader.slang | dragon_curve.slang, ray_march.slang, ray_march_camera.slang | addr, fullscreen_triangle, projection | – | – |
| gpu_picking | gpu_picking.shader.slang, gpu_picking_id.shader.slang | gpu_picking_common.slang, ray_march_camera.slang | addr, fullscreen_triangle, projection | – | – |
| particles | particle_render.shader.slang, particles.compute.slang | particle.slang | addr | – | – |
| space_invaders | space_invaders.shader.slang | – | addr, projection | textures/space_invaders/ | – |
| sprite_batch | sprite_batch.shader.slang | – | addr, projection | textures/ravioli_atlas.bmp (verify at move time) | – |
| watercolor | paint_display.shader.slang, paint_brush.compute.slang, wc_*.compute.slang (9) | watercolor_common.slang | addr, fullscreen_triangle | textures/watercolor/paper_height.png; + generate_paper_texture bin, `default-run = "watercolor"` | image |

(Transitive slang imports included: `ray_march.slang` imports `addr`; `ray_march_camera.slang`
imports `projection`; `gpu_picking_common.slang` imports `addr`.)

Each example's `ShaderAtlas` struct is generated locally with only its own entries; imports change
from `vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas` to
`crate::generated::shader_atlas::ShaderAtlas`; field names unchanged.

## 9. Phase 5 — Cleanup

1. Delete from `crates/mltrs`: `examples/`, `shaders/`, `src/generated*`, leftover asset dirs,
   `pub mod generated;`. mltrs no longer runs codegen; drop the interim `--import-root crate`
   invocation from the justfile (the flag itself stays for the check_crate test path). The
   orphaned generated files go away with the dir.
2. Final justfile (unix recipes shown; mirror the pwsh variants):
   ```just
   dev example="basic_triangle":
       cargo run -p {{example}}

   shader-debug example="viking_room":
       RUST_LOG=info VK_LAYER_PRINTF_ONLY_PRESET=1 cargo run -p {{example}}

   # regenerate bindings for one example, or all
   shaders example="all":
       #!/usr/bin/env bash
       set -euo pipefail
       if [ "{{example}}" = "all" ]; then
           for d in examples/*/; do cargo run -p mltrs-cli -- shaders compile --crate-dir "$d"; done
       else
           cargo run -p mltrs-cli -- shaders compile --crate-dir "examples/{{example}}"
       fi
       cargo fmt

   vendor-shaders:
       #!/usr/bin/env bash
       set -euo pipefail
       for d in examples/*/; do cargo run -p mltrs-cli -- shaders init --dir "$d/shaders/source" --force; done

   test:
       INSTA_UPDATE=no cargo test --workspace

   insta:
       cargo insta test -p mltrs-cli --review

   lint:
       cargo clippy --workspace --all-targets -- -D warnings
       cargo clippy --workspace --all-targets --release -- -D warnings

   pre-commit: shaders && lint test
       git add examples/*/shaders/compiled examples/*/src/generated

   paper-texture:
       cargo run -p watercolor --bin generate_paper_texture --release
   ```
   `release:` becomes `cargo run --release -p {{example}}` or is deleted. `sprites`, `beats`,
   `build-slang`, `init-submodules`, `extract-link`: path updates only.
3. Docs: CLAUDE.md command table, README, "run `just shaders` after .slang changes" guidance now
   scoped per example; document the CLI consumer story (§6).
4. `scripts/pre-commit.sh` just calls `just pre-commit` — no change beyond the recipe.
   `.gitignore`: single root `target/`; ensure `crates/cli/fixtures/check_crate/target/` ignored.
5. Final green run: `cargo check --workspace --all-targets`;
   `just shaders && git diff --exit-code 'examples/*/src/generated' 'examples/*/shaders/compiled'`;
   `just test`; `just lint`; boot every example
   (`for d in examples/*/; do timeout 3 cargo run -p $(basename $d); done`); hot-reload smoke test
   on one graphics + one compute example.

## 10. Open risks / gotchas

- **Insta snapshot rename churn** (crate + module path in filenames): handled in Phase 2 step 8;
  the corpus change (§7) also alters the `generated_files` set — review accepted snapshots rather
  than trusting `--accept` blindly.
- **`env!("CARGO_MANIFEST_DIR")` in library fns** bakes the *defining* crate's dir. After Phase 1
  the only intentional uses are cli-local test helpers and template-emitted code. Grep for
  `CARGO_MANIFEST_DIR` at the end of each phase.
- **cwd-relative paths**: `dev_compile_slang_shaders`'s `"shaders/source"` (fixed Phase 1),
  `sdf_2d.rs:55` beats.json (fixed Phase 1), justfile link recipes rely on running `just` from the
  repo root (fine — just runs from the justfile dir).
- **Workspace member glob vs flat examples**: don't add `"examples/*"` to `members` until
  `examples/` contains only crate dirs (Phase 4).
- **check_crate isolation**: needs both its own `[workspace]` table and root `exclude`; bump its
  stale `glam 0.29` pin since generated layout asserts run against real glam.
- **shader-slang linkage in release consumer builds**: the compile fns in `shaders.rs` are
  unconditionally `pub`, so slang statically links even into release example binaries. Optional
  follow-up (not part of this migration): a `slang-compile` feature on mltrs-renderer (default on,
  required by mltrs-cli) gating the compile entry points, `shader_watcher`, and hot-reload paths.
- **Optional follow-up — statically-linked slang sufficiency**: double-check the static
  `shader-slang` build (Giesch/slang-rs fork, `features = ["static"]`) fully supports our compile
  path — `OptimizationLevel::High` + `emit_spirv_directly(true)` (`shaders.rs:58-59`) — without
  dynamically loading SPIR-V utilities (spirv-tools / `libslang-glslang`). Slang can defer SPIR-V
  legalization/optimization to spirv-tools, which some builds load as a shared library at runtime;
  that would silently break the self-contained `cargo install mltrs-cli` story. Verify by running
  `mltrs shaders compile` on a machine/dir without the slang build tree and checking `ldd` /
  runtime dlopen behavior.
- **Duplicated slang module drift**: shared example modules are intentionally duplicated; hot
  reload's `assert_shader_interface_unchanged` catches struct-layout drift at runtime, and
  `just vendor-shaders` re-syncs the engine set. Accepted trade-off.
- **`.env` `SLANG_*` vars use `$PWD`** — still correct since builds run from the workspace root;
  document that direnv/`load-env.ps1` must run from root. `RUST_LOG` crate-name update in Phase 2.
- **`egui` / `egui-ash-renderer` version coupling**: stay pinned together in
  `[workspace.dependencies]`.
- **Windows justfile variants**: every recipe change must be mirrored in the `[windows]` pwsh
  forms; `insta` remains unix-only (known build_tasks windows path issue).
