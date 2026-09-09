# Phase 1 — Implementation Review

STATUS: REVIEW RECORD (2026-09-08) — audit of branch
`render-graph-plain-data-core` (`50c8037..f927629`, 6 commits) against
[`../08_plain_data_graph.md`](../08_plain_data_graph.md),
[`phase_1.md`](01_pure_core.md) (the file is named `01_pure_core.md`), and
[`../07_graph_api_plan.md`](../07_graph_api_plan.md).
Line references point at the branch tip.

## Scope note

The merge base `50c8037` contains no render graph. The branch lands the 07
typed-tuple API and 08 phase 1 as one change. `schedule.rs` and `BuildCtx`
never existed in this history. Every phase-1 instruction phrased as "port",
"move verbatim", or "delete `schedule.rs`" was executed as "write from
scratch". The parity that phase_1.md pins with ported tests therefore has no
independent reference implementation in this repository.

Most of the implementation matches the plans. Verified faithful line-by-line:
module layout and visibility, the `GraphDesc` tables, the interners, the scope
machinery, the repeat rotation rule, the `tex_phys` derivation, the 25
`GraphError` variants and their key phrases, `build_assembly` semantics,
barrier templates, `expand()` semantics, the untouched execute path, the
watercolor blur split, the `GraphFormat` migration, and the particles
migration. The findings below are the deviations.

## 1. Behavior regression: blur H reads one version too far back

RESOLVED (2026-09-08). `examples/watercolor/src/main.rs:551` binds
`wet_mask.read()`. `07_graph_api_plan.md` carries the correction. The
finding as written:

`examples/watercolor/src/main.rs:551` — node 6 (Gaussian blur H) binds
`wet_mask.read_previous()`. The correct access is `wet_mask.read()`.

Derivation against `50c8037:examples/watercolor/src/main.rs`. Let `P` be
`sim_parity` at the top of `draw()`. `PingPong::write_storage(p)` writes
`side[!p]`:

- Old blur H (line 749, queued pre-flip): `read_sampled(!P)` = `side[!P]`.
- Old brush (line 868, closure, post-flip): mutates `side[!P]`.
- Old capillary (line 1002, closure): `write_storage(!P)` = `side[P]`.
- Old display (line 807, post-flip): `read_sampled(!P)` = `side[!P]`.

Old blur H reads the live, brush-mutated image that holds the last frame's
capillary output. In graph terms at node 6, no write to `wet_mask` has run
this frame: the brush only mutates, and capillary writes at node 10. That
image is the current version, so the access is `read()`. `read_previous()`
resolves to the other physical image, which holds capillary output from 2
frames back. The blur → `flow_outward` edge-darkening path therefore misses
the current brush strokes and the previous capillary spread.

Display at `main.rs:633` runs after the capillary write, so its
`read_previous()` is correct.

Cause: `07_graph_api_plan.md` states "blur H and the display sample the
pre-capillary wet mask" and prescribes `read_previous` for both. The prose is
wrong for blur H. Position relative to the version-producing write decides the
access mode. No validation rule catches the mistake, and `tex_phys` is 2
either way.

Fix: change line 551 to `wet_mask.read()`. Annotate the 07 deviation list.

## 2. The 72-test inventory is missing

RESOLVED (2026-09-08). All 72 inventory tests are in-module `#[cfg(test)]`
modules, plus 7 tests that pin the fixes and the vocabulary below. A shared
builder lives in `crates/renderer/src/renderer/render_graph/test_desc.rs`. The
4 `#[expect(dead_code)]` attributes are `#[cfg_attr(not(test), allow(...))]`:
tests reach every item, execution wiring lands in 3a. `BufAccess::Write` and
`BufAccess::IndirectArgs` were deleted; nothing constructs them. The finding as
written:

`phase_1.md` §"Test inventory": "72 tests. This inventory is the contract; do
not trim it." The branch contains 0 of them. No `#[cfg(test)]` module exists
under `crates/renderer/src/renderer/render_graph/`.

Consequences:

- `compile.rs` (303 lines) and `expand.rs` minus `TexRunState` have no
  callers. Unit tests were their only planned phase-1 callers.
- The 4 module-level `#[expect(dead_code)]` attributes at
  `render_graph.rs:14-31` keep them compiling. Task 10 of phase_1.md requires
  removing every allow and deleting items that stay dead.
- Every rule in `validate.rs` (754 lines) is unverified. Findings 3a, 3b, and
  3c below are the failures that inventory tests 18, 29, 31, and 41 target.
- Parity tests 42–43 (`tex_phys` equals the `BuildCtx` `needs_two` result) are
  asserted nowhere.

## 3. Validation defects

### 3a. `DrawWritesTexture` does not fire for a mutate-only draw

RESOLVED (2026-09-08). `validate` reports one error per distinct texture a draw
writes or mutates. The finding as written:

`validate.rs:684-697`. The condition counts `Write` and `Mutate` refs, but the
error is gated on `writes.first()`. A draw whose only texture access is
`Mutate` produces no error. `DrawNode::plan` never applies writes and
`GraphPush::access` has no callers, so the executor does not track the write
either. A fragment shader can then read-modify-write a physical image the
compute passes use for the same frame's version, with only the pass-level
compute→graphics barrier.

### 3b. Per-command checks run per raster pass, not per draw

RESOLVED (2026-09-08). A `commands()` helper yields one entry per
`DispatchDesc` and one per `DrawDesc`. The id-range checks, the write and mutate
hazard checks, and the `tex_phys` updates iterate commands. For a raster pass
the `command` field of an error now carries the draw name. The finding as
written:

`validate.rs:544-618`. Lowering coalesces all top-level draws into one
`RasterDesc`, and `refs()` (`validate.rs:224`) flattens every draw in the pass
into one access set. `DuplicateWrite`, `MutateAndRead`, `MutateAndWrite`,
`WriteAndPrevRead`, and the read∩write `tex_phys` rule then evaluate the
merged set. phase_1.md check 6 defines a command as one `DispatchDesc` or one
`DrawDesc`. Two independent draws (one mutates a texture, one reads it)
produce a false `MutateAndRead` that blames the pass. The check at
`validate.rs:684` also sits inside `for draw in &raster.draws` but is
loop-invariant, so one write flags every draw.

### 3c. The uniform-source merge path is unreachable

RESOLVED (2026-09-08). `uniform()` interns before it mints. On a slot hit it
compares the interned bindings and the recorded data size; equal sources return
the existing `UniformId` with no error, and no schema or value row is created.
The finding as written:

`lower.rs:263-301`. `uniform()` mints a fresh `SchemaId` and `ValueId`
(lines 264-274) before the intern lookup, so
`old.data == Some(data)` at line 279 is always false. Effects:

- Every second sighting of a slot reports `UniformSourceConflict`. Phase-1
  behavior for typed nodes is correct by accident; the merge branch that
  serves the 3b `with_uniform` scope and inventory test 29 cannot execute.
- Each repeat sighting leaves 1 orphan `ValueDecl` and 1 orphan `SchemaDesc`
  in the tables. `compile()` counts the orphans into `value_count`
  (`compile.rs:301`), which sizes `FrameShape`.

### 3d. Texture extents are not validated

RESOLVED (2026-09-08). `validate` rejects a zero `Fixed` extent with
`TextureExtentZero`. `RenderGraph::new` checks `Fixed` extents against
`maxImageDimension2D` through `extent_limit_errors` and reports
`TextureExtentTooLarge`. Both errors name the texture. The finding as written:

`render_graph.rs:111` accepts any `u32` pair. `validate.rs` checks the size
class, not the `Fixed(w, h)` values. `res.texture(0, 0, ...)` reaches
`vkCreateImage` and fails as an opaque allocator error instead of a
`GraphError` naming the texture. Device limits
(`maxImageDimension2D`) are also unchecked.

### 3e. `read_previous` on a never-written texture reads a cleared image

RESOLVED (2026-09-08). `validate` rejects `ReadPrevious` on a texture no
command writes with `PrevReadWithoutWrite`, blamed on the first reader. A
`Mutate` does not satisfy the rule, because only a `Write` advances the
cursor. A `Write` inside a zero-consumer uniform source does not satisfy it
either. A `Write` later in pass order does: frame N's write is frame N+1's
previous version. The finding as written:

`ReadPrevious` sets `tex_phys` to 2, but the cursor only advances on
`commit_write`. If every producer uses `Mutate`, `prev_phys()` selects the
image that nothing ever writes. The consumer samples black on every frame with
no error. The validator holds the full access set and can reject
`ReadPrevious` on a texture with no `Write` anywhere.

### 3f. Unconsumed uniform sources skip ID-range checks

RESOLVED (2026-09-08). Resource ids are bounds-checked on the declaration
that carries them: uniform sources (consumed or not), push blocks, and
`Offscreen` target lists. A source shared by several commands reports once.
The `OffscreenTargets` rejection runs in the leaf walk, so raster leaves
inside `When`/`Repeat` bodies are covered. The finding as written:

`validate.rs:386-410` shape-checks every `UniformDecl`, but the
`TexId`/`BufferId`/`ImportId` bounds checks run only in the leaf walk. An
out-of-range ID inside a zero-consumer source is not reported. Review item 5
of the parent document requires resource-identity checks for zero-consumer
sources. `RasterTargets::Offscreen` IDs are also never bounds-checked, and the
`OffscreenTargets` rejection covers only top-level leaves, not raster leaves
inside a `When` or `Repeat` body.

### 3g. Duplicate and colliding diagnostics

RESOLVED (2026-09-08). One diagnostic per defect:

- A second raster pass reports only `MultipleRasterPasses`;
  `PassAfterMainRaster` covers non-raster passes after the main raster.
- Dispatch names come from a dedicated counter, so they advance inside
  `Repeat`/`Optional` bodies.
- `GraphResources::texture` is `#[track_caller]` and records the call site
  into the texture name (`tex0 (src/main.rs:42)`). The hazard errors carry
  the name instead of the index.
- A binding that fails to lower drops from the schema fields and the
  bindings together, so `MutableExternalImport` does not cascade into shape
  errors.
- `UnsupportedFeature` and `TableKind` implement `Display`.
  `UnsupportedFeature` prints a description and the owning phase/ledger row.
- `RenderGraph::new` reports through `validation_message`: a
  `render graph validation failed:` header and one `  - ` bullet per error.
  The per-error `render graph: ` prefix is gone.

The finding as written:

- A second raster pass reports both `PassAfterMainRaster`
  (`validate.rs:464`) and `UnsupportedInPhase1 { MultipleRasterPasses }`
  (`validate.rs:528`).
- `lower.rs:379` names dispatches `dispatch{passes.len()}`. The count does not
  advance inside a `Repeat`/`Optional` body, so every dispatch in one body
  shares one name.
- Textures are named `tex{n}`. Errors print the index with no path back to the
  `res.texture(...)` call site.
- `lower.rs:241-250` filters failed bindings while `fields()` keeps them, so 1
  `MutableExternalImport` cascades into 1 `BindingCountMismatch` plus 1
  `BindingKindMismatch` per shifted field.
- `UnsupportedFeature` prints the Debug variant name and "phase 1". phase_1.md
  requires a description and the owning phase/ledger row.
- `RenderGraph::new` joins errors with newlines. phase_1.md specifies a
  `render graph validation failed:` header and one `  - ` bullet per error.

## 4. Other unhandled edge and error cases

RESOLVED (2026-09-09). Upload overflow returns an error; execution validates all
captured buffer slots before resolving addresses; the external storage-handle
conversion was removed; picking declares its cursor value so optional picking
lowers successfully; staged bytes use `MaybeUninit<u8>`. Repeat expansion now
has a configurable aggregate limit (65,536 iterations per frame by default).
Texture cursors commit through a callback immediately after successful queue
submission, including when subsequent presentation fails. The last two changes
bring the runtime protections forward from phase 3a; the pure `expand` path
remains unwired. The findings as written:

- `render_graph.rs:1456-1462`: `stage_storage` uses `assert!` on upload
  length. An oversized frame `Vec<T>` aborts the process mid-frame.
  `GraphNode::plan` already returns `anyhow::Result` and can carry the error.
- Slot keys (`UniformSlot`, `StorageSlot`, `GpuOnlySlot`, ...) capture bare
  indices. Dropping the buffer and then executing the graph panics on the
  `unwrap()` inside `mapped_mem_by_index`
  (`uniform_buffer.rs:67`, `storage_buffer.rs:311`) with no node or buffer
  named.
- `render_graph.rs:174` exposes
  `From<BindlessHandle<RwTexture2D>> for StorageTexBinding`, but
  `lower.rs:192` rejects every `StorageRef::External` with
  `MutableExternalImport`. The API type-checks and always fails at build.
  Delete the impl or implement the path.
- `optional(picking(..))` fails with `EmptyOptionalScope` (`lower.rs:541`)
  because `PickingNode` declares no values, but
  `OptionalNode<PickingNode<C>>::Frame = Option<C>` is well-formed.
- `render_graph.rs:1200-1206`: `LoopCount` is unbounded per-frame input. Each
  iteration allocates ~148 bytes of pending-dispatch state plus the staged
  uniform bytes. The parent document defers expansion budgets (review item 7),
  so this is accepted scope, recorded here for phase 3a.
- `render_graph.rs:1358-1367`: `StagedWrites::stage` copies `size_of::<S>()`
  bytes through a `*const u8`, which reads interior padding as initialized
  `u8`. Prefer `MaybeUninit<u8>` or a bytemuck-style bound.
- Version cursors advance on a submit aborted by swapchain recreation
  (`render_graph.rs:1569-1572`). This matches the manual API and the plan
  defers the option-A commit to phase 3a. For a 2-image texture the following
  frame swaps `read`/`read_previous` content.

## 5. Codegen edge cases (`crates/cli/src/build_tasks.rs`)

RESOLVED (2026-09-09). Parameter selection and resource analysis use shader-local
reflection; shared types select their declaring module within a shader context.
Resource-bearing types are a transitive closure, including arrays. Generated
names are checked against visible structs/enums and earlier generated names.
Padding carries an explicit synthetic flag. Unknown handle types and nested
resources return source-qualified errors rather than panics. Regression tests
cover these cases; six alignment snapshots drop accidental cross-shader trait
implementations. The findings as written:

- `params_types` (`:162`) and `resource_bearing` (`:171`,
  `resource_bearing_types` at `:1621`) are global bare-name sets. Shader B's
  unrelated local `struct Params` with a resource-bearing field trips the
  `assert!` at `:1509-1515` and panics the whole atlas compile. 8 watercolor
  shaders name their block `Params`. Key both sets per shader or by
  `(module, type)`.
- `resource_bearing_types` inspects direct fields only. A handle 2 struct
  levels deep classifies as `Data`, lands in the per-frame struct, and escapes
  version tracking — the case the assert exists to prevent. Compute the set as
  a transitive closure.
- The synthesized `{X}Data`/`{X}Bindings` names have no collision check
  against user structs. A shader struct named `ParamsData` produces E0428 in
  generated code with no pointer to the `.slang` source.
- `classify_graph_field` (`:1435`) classifies padding by the `_padding_` name
  prefix (`:1437`). A user field named `_padding_hint` is dropped from the
  data struct and assembled as `Default::default()` with no diagnostic. Thread
  a synthetic-padding flag instead.
- The handle match covers exactly `BindlessHandle<Sampler2D>` and
  `BindlessHandle<RwTexture2D>`. A future marker type silently classifies as
  `Data`. Add a catch-all panic arm for `BindlessHandle<`.
- The `assert!` at `:1509` is a user-reachable diagnostic delivered as a
  panic without the source file name. Return `anyhow::Error` like
  `assert_push_constant_size`.
- The generated files themselves check out: assemble order, binding kinds,
  visit order, `FieldKey` derivation, and the check_crate fixture surface all
  match.

## 6. Unneeded complexity

- `compile.rs` and `expand()` implement a second version of the model the
  live `PlanCtx` path implements, with no test that the two agree (~460
  lines). Inside them: `SchemaDesc::layout` is always `None`
  (`lower.rs:121-128` is the only constructor), so the assembly IR
  (`SchemaLayout`, `AssemblyStep`, `AssemblySrc`) can only produce
  `Deferred`; `SchemaDesc::size` is written everywhere and read nowhere;
  `PushDesc::data` is always `None`.
- `LowerOutput` side tables (`picking`, `uniform_slots`, `buffer_indices`,
  `pipeline_indices`, `import_handles`) have no consumer.
  `RenderGraph::new` reads only `desc`, `schemas`, and `errors`. The interner
  bookkeeping that maintains them serves nothing until 3a.
- 3 derivations of the same access partition: `collect_access` (live),
  `refs` + `tex_ids` (validate), `access` (compile). `NodeAccess` and
  `LeafAccess` are the same 4-`Vec` struct at different index types.
- `GraphNode::plan` returns `anyhow::Result<()>`; every impl returns
  `Ok(())`. The 2 conditions that should use it are an `assert!` (§4) and a
  `debug_assert!`. `GraphPush::access` is declared, implemented twice, and
  never called; it is the hook that finding 3a needs.
- Desc vocabulary with no construction site: `SizeClass::Window`/`WindowDiv`,
  `TexUsage::Color`/`Depth`, `RasterTargets::Offscreen`,
  `GroupSource::Value`, `ValueKind::Groups`. 5 of 7 `UnsupportedFeature`
  arms are dead diagnostics until phase 5.
- `BufAccess`, `SlotSel`, and `BufferRef::range` are computed and never
  consumed. `refs` and `all_leaves` return names every caller discards.
- `graph_split_def` builds ~125 lines of output through
  `lines.push(format!(...))` while every other generated shape uses an
  askama template. `classify_graph_field` returns a pre-rendered
  `visit_line: String`, mixing classification with rendering.

Keep-or-delete is one decision: either land the phase-1 test inventory that
makes `compile`/`expand`/side-tables live, or delete them and reintroduce
them in 3a.

## 7. Documentation deviations

- `docs/render_graph.md:42` shows `res.texture(W, H, vk::Format::R32_SFLOAT)`.
  The API takes `GraphFormat` (`render_graph.rs:111`), which has 2 variants.
  The snippet does not compile. The format restriction is absent from
  "Limits".
- The doc's validation-error list contains only the 10 legacy rules.
  `UniformSourceConflict` (the rule that forced the blur split),
  `NestedControlFlow` for `optional`, `MutableExternalImport`,
  `EmptyOptionalScope`, the upload rules, and the `UnsupportedInPhase1`
  matrix are undocumented. phase_1.md requires the new strictness
  "documented rather than hidden".
- The doc says external textures bind via `handle.bindless_handle().into()`
  without distinguishing sampled (works) from storage (always rejected, §4).
- phase_1.md's invariant "no `crates/cli` changes" and its gate exemption for
  `just test` do not hold for this branch, because the 07 codegen phase landed
  in the same change. phase_1.md carries no annotation recording the merge.
- A shader whose parameter block has no uniform fields gets no
  `GraphShaderParams` impl and cannot be a graph node. The limit is real and
  absent from `docs/render_graph.md` "Limits".

## Priority

1. Fix the mutate-only draw hole and per-draw check granularity (§3a, §3b).
2. Land the test inventory (§2); it pins 1 and decides §6's keep-or-delete.
3. Fix or delete the unreachable merge path and its orphan rows (§3c).
4. Reconcile `docs/render_graph.md` and annotate 07/phase_1 with what landed
   (§7).

§1, §2, and §3 are fixed. §6's keep-or-delete resolved as keep: the
inventory reaches every item of `compile.rs`, `expand.rs`, and lowering's side
tables, so each is verified rather than merely compiled. Items 4, 5, and 7
stand.
