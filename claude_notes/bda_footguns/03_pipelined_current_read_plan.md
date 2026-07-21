# Domain-marked address types: fix BDA footgun #3 (pipelined current-read race)

Plan, 2026-07-21. Addresses [../bda_footguns.md](../bda_footguns.md) §3 (and §9 as a
side effect). Code references verified against main @ fb67b29.

## Context

`bda_footguns.md` §3: under `Renderer::enable_pipelined_compute`, frame N's graphics
waits only on compute N−1, so compute N runs concurrently with graphics N. A graphics
shader reading a gpu-only buffer's *current* ring slot (this frame's compute output) is
a GPU data race, guarded today only by a doc comment. The hole: codegen picks
`Addr`/`ReadAddr` purely from Slang pointer mutability (never consulting which shader
the parameter block belongs to), and `From<Addr<T>> for ReadAddr<T>` launders "writable
current slot" into the same `ReadAddr<T>` graphics parameter blocks declare —
`examples/particles.rs:121` (`gpu.current_addr(...).into()`) is exactly that pattern.

**Conceptual model:** the true hazard axis is the *execution domain*, not shader stage.
Frame N has two mutually-unordered domains — the **pipelined compute submission** and
the **frame submission** (graphics command buffer, plus anything recorded into it). A
buffer's *current* slot is safe only for consumers in its producer's domain (ordered
after it via barriers); *previous* slots cross domains freely (both domains wait on last
frame's compute timeline). Compute-vs-graphics coincides with this split only because
today's architecture puts all compute on one side of a global flag; a future mixed mode
(some compute dispatches recorded in the graphics command buffer) would break the
stage-based framing but not the domain-based one.

**Decisions:**
1. **Forbid entirely** — frame-domain (graphics) parameter blocks may never hold a
   gpu-only buffer's current-slot address, even in non-pipelined mode where it happens
   to be safe (safety must not depend on a runtime flag). No escape hatch. particles.rs
   accepts one frame of latency.
2. **Ban pointer fields in buffer-element position** in codegen (closes footgun #9;
   reflection already rejects it at src/shaders/reflection/parameters.rs:330-336 and
   :276-278, so this is defense-in-depth — no in-tree usage).
3. **Spelling: generic `Addr<T, S>` with domain-named markers** — `FrameDomain` /
   `PipelinedDomain`, sealed `Domain` trait. Chosen over concrete `ComputeAddr` types
   because cross-domain conversion becomes *unrepresentable* (2 `From` impls total, both
   domain-preserving) and it's the only spelling that can later express
   domain-polymorphic generated structs if dispatch-site-determined domains ever land.

## Steps (land as one change — there is no intermediate compiling state)

### 1. `src/renderer/addr.rs` — domain markers

```rust
mod sealed { pub trait Sealed {} }
/// Execution domain a pointer may be embedded for. Sealed.
pub trait Domain: sealed::Sealed + 'static {}
/// The frame submission: graphics, and anything recorded into its command buffer.
pub enum FrameDomain {}      // uninstantiable markers
/// The pipelined compute submission (compute shaders' parameter blocks —
/// conservatively, even when pipelining is off and compute shares the frame submit).
pub enum PipelinedDomain {}
// + Sealed/Domain impls for both

#[repr(transparent)]
pub struct Addr<T, S: Domain> {
    address: u64,
    _marker: PhantomData<fn() -> (T, S)>,  // fn() -> keeps Send/Sync/Copy unconditional
}
```

- Same parameterization for `ReadAddr<T, S>`. `ImmutableAddr<T>` untouched (immutable is
  safe in any domain).
- Thread `S: Domain` through the manual `Clone`/`Copy`/`Debug`/`Serialize` impls,
  `from_raw` (stays `pub(super)`) and `to_raw`. Serialize still emits raw u64.
- Conversions — the ONLY two `From` impls, both domain-preserving:
  `From<Addr<T, S>> for ReadAddr<T, S>` and `From<ImmutableAddr<T>> for ReadAddr<T, S>`.
  Cross-domain conversion is unrepresentable.
- Update size/align static asserts with a concrete domain. **No default domain param** —
  inference from struct-literal fields covers all real call sites.

### 2. `src/renderer.rs` — `Gpu` minting (:5077-5115) and docs

```rust
pub fn addr<T, S: Domain>(&self, sb: &StorageBufferHandle<T>) -> Addr<T, S>            // generic: CPU-written pre-wait, safe in any domain
pub fn current_addr<T>(&self, b: &GpuOnlyBufferHandle<T>) -> Addr<T, PipelinedDomain>  // concrete — the fix
pub fn previous_addr<T, S: Domain>(&self, b: &GpuOnlyBufferHandle<T>) -> ReadAddr<T, S> // generic: previous slots cross domains freely
// current_immutable_addr unchanged
```

Bodies unchanged. Doc updates: `current_addr` explains the domain model (current slot is
producer-domain-only); `enable_pipelined_compute` (:845-856) drops the "must use
previous_addr" warning + TODO — now enforced by types (also fixes the stale
`current_gpu_only_addr` method name in that comment). `Domain`/markers reach generated
code via the existing `pub use addr::*;` (renderer.rs:44-45).

### 3. `src/shaders/build_tasks.rs` — domain threading + bans

- New `enum ShaderDomain { Frame, Pipelined }` with
  `fn marker(self) -> &'static str` → `"FrameDomain"` / `"PipelinedDomain"`.
- Add a `domain: ShaderDomain` param to exactly three functions: `gather_struct_defs`,
  `generate_std140_struct_fields`, `generate_std430_struct_fields`. Seed `Frame` at :299
  and :344 (graphics `.shader.slang` path), `Pipelined` at :573 (compute
  `.compute.slang` path); pass through at :729, :793, :799, :876, :929, :936, :952.
- Pointer branch (:869-905): first the element ban — `panic!` when `alignment` is
  `Some(Alignment::Std430 { .. })` ("BDA pointer fields are only supported in parameter
  blocks, not buffer-element structs; a stored address encodes one fixed ring slot and
  goes stale as the containing buffer rotates"), matching the existing matrix-rejection
  panic style at :988. Then domain-suffixed emission:
  `Addr<{pointee}, {domain.marker()}>` / `ReadAddr<…>`; `ImmutableAddr<{pointee}>`
  unchanged.
- Shared-module guard in `tag_source_modules` (:1320): panic if a def being tagged has
  any field whose type starts with `Addr<`/`ReadAddr<`/`ImmutableAddr<` — pointer-bearing
  structs must live in the shader entry file (domain-typed pointers can't be shared
  across a graphics and a compute shader, and the shared-module template doesn't import
  addr types anyway). This preempts a misleading `struct_defs_compatible`
  "incompatible layout" panic.
- Fix the two direct test callers of `gather_struct_defs` (:1596, :1648 →
  `ShaderDomain::Frame`); add a `#[should_panic]` test for the element ban modeled on
  `small_matrix_fields_are_rejected`.
- Verified no changes needed: askama templates (print `field.type_name` verbatim),
  layout-assert generation (built from the same strings, :1063-1085),
  `field_alignment`/`rust_type_alignment` (prefix/substring matching still works with
  the domain arg appended).

### 4. `shaders/test/check_crate/src/renderer/addr.rs` — vendored stub

Simplified stub, not a copy: mirror the shape (`Domain` trait + markers + `S` params +
updated asserts) so the `alignment_tests` compile check (build_tasks.rs:1495-1560)
passes. Must land with step 1.

### 5. Regenerate: `just shaders`

Expected diffs in `src/generated/shader_atlas/` (7 files): `FrameDomain` in
ray_marching, gpu_picking, gpu_picking_id, space_invaders, particle_render;
`PipelinedDomain` in particles_compute, paint_brush_compute. sprite_batch
(ImmutableAddr) and all shared modules unchanged.

### 6. `examples/particles.rs`

- Line 121: `particles: gpu.previous_addr(&self.particle_buffer)` (was
  `current_addr(...).into()`).
- Delete the `memory_barrier` at :97-102 and the now-unused `use ash::vk;`. Verified
  safe: the non-pipelined submit already waits on `compute_timeline` value N−1 at
  `VERTEX|FRAGMENT|COMPUTE` stages whenever compute pipelines exist
  (src/renderer.rs:2230-2244) — a full execution+memory dependency ordering
  compute N−1's slot-(N−1) writes before graphics N's reads. Replace with a comment
  explaining this.
- Other examples need no edits — `gpu.addr(...).into()` sites (ray_marching:163,
  gpu_picking:113/122, space_invaders:383, watercolor:1063) infer their domain from the
  destination field; the domain-preserving `From` is the unique applicable impl for an
  `Addr` source.

### 7. Docs — `claude_notes/bda_footguns.md`

Mark #3 done (domain-typed addrs, no escape hatch; explain the domain model briefly) and
#9 done (reflection already rejected it; codegen now bans it too); update the coverage
table. Add residual note: `Gpu::addr` mints *writable* addresses for both domains of the
same storage slot, so compute-writes + graphics-reads of a CPU-written storage buffer
still races under pipelined compute (kin to #11 — out of scope here). Refresh line refs
in the sections touched.

## Verification

1. `just shaders` (regenerates + fmt)
2. `cargo check --all-targets` (plain `--all` misses examples)
3. `just test` → expected snapshot churn: `generated_files@` for the 7 files above;
   `alignment_tests@` pointer_dual_context / pointer_params_compute /
   pointer_pointee_layout. Review then `cargo insta accept`.
4. `just lint`
5. `timeout 3 just dev particles` — no validation errors; particles animate (one extra
   frame of latency is accepted).

## Risks

- **check_crate lockstep**: forgetting step 4 fails `alignment_tests` with an opaque
  compile error inside the check crate — do it with step 1.
- **`.into()` inference** relies on `From<Addr<T,S>> for ReadAddr<T,S>` staying the
  unique `From` into `ReadAddr` for an `Addr` source; a future extra impl would force
  turbofish at those sites.
- **Naming**: compute blocks carry `PipelinedDomain` even when the app never enables
  pipelining (conservative by decision #1) — doc comments on the markers must make this
  explicit so the name doesn't confuse non-pipelined users.
