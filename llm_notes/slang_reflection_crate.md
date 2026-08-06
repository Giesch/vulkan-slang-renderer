# slang reflection crate split — plan

Extract the slang compile/reflect machinery out of `crates/renderer` into a new
library crate, so that **`mltrs-slang-reflection` is the only crate in the
workspace with a direct dependency on `shader-slang`** — and so that no
`shader_slang::` type appears in its public API either, making the boundary a
real seam rather than a bookkeeping one (§4).

The shader watcher (`crates/renderer/src/shader_watcher.rs`) stays exactly where
it is — it's `notify`-based and has no slang dependency; moving it is a separate
question about where hot-reload orchestration belongs.

Every phase leaves the repo green:
`cargo check --workspace --all-targets`, `just test`, `just lint`, `just shaders`.

## Naming

The task named the crate `mltrs-slang-reflecton` (typo). This plan uses
**`mltrs-slang-reflection`**, package name `mltrs-slang-reflection`, directory
`crates/slang-reflection` — matching the existing `crates/renderer` ↔
`mltrs-renderer` convention.

## 0. Current state

`shader-slang` is declared once in `[workspace.dependencies]` and consumed by
exactly one crate: `mltrs-renderer`. Everything slang-facing lives under
`crates/renderer/src/shaders/`:

| file | lines | slang? | other deps |
| --- | --- | --- | --- |
| `shaders.rs` | 361 | **yes** | `ash` (only in `CompiledShader::spv_bytes`) |
| `shaders/reflection.rs` | 44 | **yes** | — |
| `shaders/reflection/parameters.rs` | 836 | **yes** | — |
| `shaders/reflection/pipeline_layout.rs` | 348 | **yes** | — |
| `shaders/json.rs` | 119 | no | `ash`, `crate::renderer::LayoutDescription` |
| `shaders/json/parameters.rs` | 354 | no | `serde` only |
| `shaders/json/pipeline_builders.rs` | 61 | no | `serde` only |
| `shaders/atlas.rs` | 67 | no | `ash`, `crate::renderer::LayoutDescription` |
| `shaders/fixtures/basic_triangle.json` | — | — | roundtrip test fixture |

Consumers:

- `mltrs-renderer`'s hot-reload path (`renderer.rs:4849`, `:4917`) calls
  `shaders::dev_compile_slang_{,compute_}shaders` under `#[cfg(debug_assertions)]`.
- `mltrs-cli` (`build_tasks.rs`) uses `shaders::json::*`,
  `prepare_reflected_{,compute_}shader_with_optimization`,
  `reflect_shared_module_types`, `OptimizationLevel`, and reads
  `CompiledShader::shader_bytecode` directly.
- `mltrs` re-exports `mltrs_renderer::shaders`; the **generated** example code
  and the askama templates import `mltrs::shaders::json::{…}` and
  `mltrs::shaders::atlas::{…}`.

## 1. Target split

Move to `crates/slang-reflection` (package `mltrs-slang-reflection`):

- `shaders.rs`'s compile/reflect entry points → crate root (`lib.rs`)
- `shaders/reflection.rs` + `reflection/{parameters,pipeline_layout}.rs` → `reflection/`
- `shaders/json/{parameters,pipeline_builders}.rs` and the `ReflectionJson` /
  `ComputeReflectionJson` structs → `json/`
- `shaders/fixtures/basic_triangle.json` + the `reflection_value_roundtrip_is_stable`
  test → moves with `json`

Deps: `anyhow`, `serde`, `shader-slang`; dev-dep `serde_json`.
**No `ash`, no `vk-mem`, no `sdl3`.**

Stays in `crates/renderer`:

- `shaders/atlas.rs` — the `ShaderAtlasEntry` / `ShaderAtlasRoot` traits are
  vulkan-facing (`vk::VertexInputBindingDescription`, `LayoutDescription`)
- `layout_bindings_from_pipeline_layout` and the `vk_create` / `to_vk` helpers
- `shader_watcher.rs` — unchanged, per the task
- a `pub mod shaders` façade that re-exports the new crate (see §2)

Resulting graph:

```
mltrs-slang-reflection  ←  mltrs-renderer  ←  mltrs  ←  examples/*
         ↑                                     
      mltrs-cli   (drops its mltrs-renderer dep entirely — see §5)
```

## 2. The path-stability constraint

25 of the 60 committed insta snapshots, all 4 askama templates, and every
example's committed `src/generated/` reference `mltrs::shaders::json::…` and
`mltrs::shaders::atlas::…`. Churning those paths would produce a diff dominated
by mechanical regeneration.

**Decision: keep the public paths byte-identical.** `crates/renderer/src/shaders.rs`
becomes a façade:

```rust
pub mod atlas;                            // stays here (ash + LayoutDescription)
pub use mltrs_slang_reflection::*;        // prepare_reflected_*, CompiledShader, …
pub mod json {
    pub use mltrs_slang_reflection::json::*;
    // renderer-side additions live here too (see §3)
}
```

So `mltrs::shaders::json::ReflectionJson` still resolves, generated code compiles
unchanged, and `just shaders` produces a no-op diff. That is the acceptance test
for phase 3: **`just shaders` must leave the working tree clean.**

## 3. Orphan-rule fallout (the real work)

Moving the json types across a crate boundary breaks three sets of *inherent*
impls that live in `crates/renderer` — inherent impls must be in the defining
crate. Each becomes an extension trait:

1. **`ReflectionJson::layout_bindings` / `ComputeReflectionJson::layout_bindings`**
   (`shaders/json.rs:22,38`). Called from generated code as
   `self.reflection_json.layout_bindings()`.
   → new `pub trait ReflectionLayoutBindings` in the renderer.
   **Where it's exported matters:** every generated atlas entry (graphics *and*
   compute) already has `use mltrs::renderer::*;`, so exporting the trait from
   `crate::renderer` puts it in scope for all of them with **zero template
   changes and zero regeneration**. Also re-export it from `shaders::json` for
   hand-written callers.

2. **`renderer.rs:4964–5127`** — private `vk_create` / `to_vk` on
   `ReflectedDescriptorSetLayout`, `ReflectedDescriptorSetLayoutBinding`,
   `ReflectedBindingType`, `ReflectedPipelineLayout`, `ReflectedPushConstantRange`,
   `ReflectedStageFlags`.
   → one private `trait ToVk` (assoc. type) + a private `trait VkCreate`, both
   crate-internal to the renderer. Call sites (`renderer.rs:4872`, `:4938`,
   `:5010`) are unchanged apart from the trait import.

3. **`CompiledShader::spv_bytes`** (`shaders.rs:315`) — the single reason the
   slang machinery touches `ash` (`ash::util::read_spv`). Only the renderer's
   hot-reload path calls it (`renderer.rs:4862`, `:4867`, `:4935`); the CLI reads
   the raw `shader_bytecode: Vec<u8>` instead.
   → move it to the renderer as `trait SpvBytes` (or a free fn in `shaders`),
   keeping `ash` out of the new crate.

`ReflectedStageFlags::from_slang` (`reflection/pipeline_layout.rs:301`) moves
*with* the reflection code, so it stays an inherent impl in its defining crate —
no change needed.

## 4. slang types in the public API — wrap them

Two slang types currently leak through `mltrs_renderer::shaders`:

- `pub use shader_slang::OptimizationLevel` — re-exported again by
  `mltrs_cli::build_tasks`
- `CompiledShader::stage: slang::Stage` — public field, but no consumer outside
  `shaders.rs` reads it

A re-export would technically satisfy "only direct dependency" (nothing else
declares `shader-slang` in its `Cargo.toml`), but it leaves the boundary
transparent: a slang-rs upgrade that renames a variant still ripples straight
through into `build_tasks.rs`. **Decision: own both types.** After this, no
`shader_slang::` type appears in the new crate's public API at all.

### `OptimizationLevel` — 1:1 mirror

`SlangOptimizationLevel` (`slang-sys/src/bindings.rs:480`) is a stable, tiny
`#[repr(u32)]` enum, so mirror it exactly:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptimizationLevel {
    /// don't optimize at all
    None,
    /// balance code quality against compilation time
    Default,
    /// optimize aggressively
    #[default]
    High,
    /// may take a very long time, or trade space for speed severely
    Maximal,
}
```

with a private `fn to_slang(self) -> slang::OptimizationLevel`. Keeping the name
`OptimizationLevel` means the existing call sites (`OptimizationLevel::High` in
`build_tasks.rs:1804,1870,1915,2133` and `main.rs:131`, `::None` at
`build_tasks.rs:2815`) are untouched — only the import path changes, which phase
3 was already changing. `Default`/`Maximal` are unused today but cost nothing and
keep the mirror honest.

### `ShaderStage` — narrowing, not mirroring

`SlangStage` (`bindings.rs:404`) has 18 variants (Hull, Domain, RayGeneration,
Mesh, …). Mirroring all of them would be pure noise: this engine supports exactly
three, and already rejects the rest — `prepare_reflected_shader` panics unless it
finds both a vertex and a fragment entry point, and
`prepare_reflected_compute_shader` asserts `stage == Compute`. So:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage { Vertex, Fragment, Compute }

impl ShaderStage {
    fn from_slang(stage: slang::Stage) -> Option<Self> { … }
}
```

`CompiledShader::stage` changes from `slang::Stage` to `ShaderStage`. Nothing
outside `shaders.rs` reads that field, so the change is contained.

The upside beyond hiding the type: `from_slang` returning `Option` turns
"shader declares a geometry entry point" into an ordered `anyhow::bail!` at the
point of compilation — naming the stage and the source file — instead of the
current panic-at-a-distance ("failed to load vertex entry point for: …") that
fires later and describes the wrong problem.

### What stays taking `slang::Stage`

`ReflectedStageFlags::from_slang` (`reflection/pipeline_layout.rs:301`) keeps its
`slang::Stage` parameter. It needs the `Stage::None → ReflectedStageFlags::Empty`
mapping, which `ShaderStage` deliberately does not model, and it is entirely
internal to the new crate — the *reflected* type it produces
(`ReflectedStageFlags`) is what crosses the boundary, and that is already ours.

## 5. CLI dependency

After the split, `mltrs-cli`'s only use of `mltrs-renderer` is the `shaders`
module — everything it imports (`json::*`, `prepare_reflected_*`,
`reflect_shared_module_types`, `OptimizationLevel`, `ReflectedShader`) lives in
the new crate. So:

```toml
# crates/cli/Cargo.toml
- mltrs-renderer.workspace = true
+ mltrs-slang-reflection.workspace = true
```

This is the main payoff beyond tidiness: **`cargo build -p mltrs-cli` stops
building `ash`, `vk-mem`, `sdl3`, `egui`, and `egui-ash-renderer`** — none of
which shader codegen needs. `sdl3` is built from source statically on Windows, so
this is a large win for the `just shaders` loop.

The `use mltrs_renderer::shaders::…` imports in `build_tasks.rs:7-13,1565,1768`
become `use mltrs_slang_reflection::…`. This *does* touch `build_tasks.rs`, so
`just test` is mandatory here — but only import lines change, so snapshots should
be unaffected.

## Phases

Each phase is one commit and must end green.

**Phase 1 — create the crate, move the pure-json half.**
New `crates/slang-reflection` with `json/{parameters,pipeline_builders}.rs`, the
`ReflectionJson`/`ComputeReflectionJson` structs, the fixture, and the roundtrip
test. Add `mltrs-slang-reflection` to `[workspace.dependencies]`. Renderer gains
the dep and the `shaders::json` re-export façade. Do §3.1 (`ReflectionLayoutBindings`).
*Verify:* `cargo check --workspace --all-targets`, `just test`, `just shaders`
leaves the tree clean, `just sweep`.

**Phase 2 — move the slang half.**
`shaders.rs`'s compile entry points, `reflection.rs`, `reflection/*` move over.
Do §3.2 (`ToVk`/`VkCreate`) and §3.3 (`SpvBytes`). Drop `shader-slang` from
`crates/renderer/Cargo.toml`. Renderer's `shaders.rs` is now the façade plus
`atlas`. The façade still re-exports slang's `OptimizationLevel` at this point.
*Verify:* the same, plus `grep shader-slang crates/*/Cargo.toml` returns only
`crates/slang-reflection/Cargo.toml`.

**Phase 3 — wrap the slang enums.**
§4: add `OptimizationLevel` and `ShaderStage`, convert at the slang call sites,
switch `CompiledShader::stage`, and replace the entry-point panics with the
`bail!` that `ShaderStage::from_slang` makes possible. Nothing outside the new
crate changes — the name `OptimizationLevel` and its `::High`/`::None` variants
are preserved, so `build_tasks.rs` still compiles against the renderer façade.
*Verify:* `cargo check --workspace --all-targets`, `just test`, plus
`grep -rn "shader_slang\|slang::" crates/slang-reflection/src/lib.rs` shows no
slang type in a `pub` signature.

**Phase 4 — repoint the CLI.**
§5. Drop `mltrs-renderer` from `crates/cli/Cargo.toml`.
*Verify:* `just test` (build_tasks.rs changed), `just shaders`, and confirm
`cargo tree -p mltrs-cli` has no `ash`/`sdl3`.

**Phase 5 — docs.**
Update the workspace-layout section of `CLAUDE.md` with the new crate and the
"only direct slang-rs dependency" invariant.

## Risks

- **Generated-code drift.** The whole plan hinges on `mltrs::shaders::json` and
  `mltrs::shaders::atlas` staying valid paths. If `just shaders` produces a diff
  at any phase boundary, the façade is wrong — fix the façade rather than
  accepting the regenerated files.
- **`ReflectionLayoutBindings` scope.** If some generated file turns out *not* to
  glob `mltrs::renderer::*`, the template import list has to change and all 15
  examples regenerate (plus ~25 snapshots). Checked at planning time: every
  graphics *and* compute atlas entry has the glob. Re-check after any template
  edit.
- **`#[cfg(debug_assertions)]` gating.** `dev_compile_slang_*` and `spv_bytes` are
  debug-only. A release check (`cargo check --workspace --all-targets --release`)
  is worth running once at the end of phase 2 — `--all-targets` alone won't catch
  a broken `#[cfg(not(debug_assertions))]` arm.
