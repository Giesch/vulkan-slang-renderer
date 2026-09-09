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
  `render graph validation failed:` header and one ` -` bullet per error.
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
  `render graph validation failed:` header and one ` -` bullet per error.

## 4. Other unhandled edge and error cases

RESOLVED (2026-09-09). Upload overflow returns an error; execution validates all
captured buffer slots before resolving addresses; the external storage-handle
conversion was removed; picking declares its cursor value so optional picking
lowers successfully; staged bytes use `MaybeUninit<u8>`. Repeat counts and
expansion budgets remain the application's responsibility, as specified by the
parent plan; the aggregate iteration limit was removed.
Texture cursors commit through a callback immediately after successful queue
submission, including when subsequent presentation fails. This cursor-commit
protection was brought forward from phase 3a; the pure `expand` path
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
  uniform bytes. The parent document leaves repeat-related decisions to the
  application (review item 7), so no graph-level expansion budget is imposed.
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

Updated (2026-09-09): the phase-1 inventory now tests the pure core and
lowering side tables. Test coverage justifies keeping migration scaffolding;
it does not make that scaffolding part of production execution.

### Leaving for phase 3

- **Leaving for phase 3 (3a–3c):** `compile.rs` and `expand()` still implement
  a second version of the model the live `PlanCtx` path implements. The pure
  path now has tests, but production execution still uses `PlanCtx`.
  Real lowering supplies `SchemaDesc::layout: None`, so its assembly programs
  remain `Deferred`; `SchemaDesc::size` has no production reader and
  `PushDesc::data` is always `None`. Phase 2 supplies layout metadata; 3a/3b
  wire lowering, assembly, and expansion into execution. Phase 3c must remove
  obsolete scheduling/recording adapters after parity tests pass.
- **Leaving for phase 3 (3a/3b):** `LowerOutput` retains live-resource side
  tables for the new executor. The original no-consumer finding is partly
  obsolete: `RenderGraph::new` now consumes `uniform_slots` and
  `buffer_indices` for runtime buffer validation. `pipeline_indices` and
  `import_handles` still await executor wiring. Preserve legacy picking
  compatibility and either consume or remove the redundant `picking` table
  during cleanup.
- **Leaving for phase 3 (3c):** 3 derivations of the same access partition
  remain: `collect_access` (live), `refs` + `tex_ids` (validate), and `access`
  (compile). `NodeAccess` and `LeafAccess` are the same 4-`Vec` struct at
  different index types. Remove the obsolete live-path derivation when the
  new executor replaces `PlanCtx`; reassess remaining duplication afterward.
- **Leaving for phase 3 (3a–3c):** buffer slot/range metadata is scaffolding
  for compiled address resolution and validation. Wire the metadata needed
  by the executor and remove fields that still have no consumer. Buffer
  access tracking also serves later indirect/table accesses (phase 4) and
  derived synchronization (phase 6); phase 3 does not finish those features.

### Remaining items to address

- `GraphPush::access` is declared, implemented twice, and never called.
  Finding 3a was fixed in validation without this hook. Remove it unless
  executor work establishes a concrete need for it.
- `refs` and `all_leaves` return names every caller discards. Remove the
  unused return values or identify a diagnostic consumer.
- `GroupSource::Value` and `ValueKind::Groups` have test construction sites
  but no production lowering path. Identify the intended typed input and
  its owning phase, or remove this vocabulary until it has a consumer.
- **Done:** `graph_split_def` now renders its structs and trait implementations
  through `graph_split.rs.askama`. Existing output snapshots and all 51 CLI
  tests pass.
- `classify_graph_field` still returns a pre-rendered
  `visit_line: String`, mixing classification with rendering. Executor
  migration does not resolve this codegen maintainability concern.

### Leaving for later phases

- `SizeClass::Window`/`WindowDiv`, `TexUsage::Color`/`Depth`, and
  `RasterTargets::Offscreen` have no production construction sites.
  Their support and removal of the corresponding temporary
  `UnsupportedFeature` diagnostics belong to phase 5, not phase 3.

### Resolved

- `GraphNode::plan` returning `anyhow::Result<()>` now has a real purpose:
  `UploadNode::plan` propagates staging validation errors. The original
  claim that every implementation only returns `Ok(())` is obsolete.
- Keep-or-delete is resolved as keep for the tested compiler, expander,
  and migration side tables. Phase 3 completion still requires production
  integration and removal of obsolete adapters; tests alone do not close
  that work.

## 7. Documentation deviations

Updated (2026-09-09): four original findings are resolved. Plan annotations
and one contradictory synchronization paragraph remain open.

### Remaining items to address

- `01_pure_core.md` still requires no `crates/cli` changes and exempts
  `just test`, although the 07 codegen implementation landed with phase 1.
  It also says `PlanCtx`/`plan()` stays byte-for-byte unchanged until 3a,
  which no longer reflects the §4 fixes. Annotate the implemented scope,
  applicable verification gates, and submission-aware cursor commits brought
  forward from 3a. The 07 plan still names the nonexistent `schedule.rs`;
  update its implementation references and record the combined landing.
- `docs/render_graph.md` under "Barriers and synchronization" still says
  version cursors advance when submission aborts on swapchain recreation.
  This contradicts its updated "Logical textures" section and the current
  submission callback. Replace the stale paragraph: acquisition skips retain
  cursors; successful submission commits them even if presentation fails.

### Resolved

- The texture example now uses `GraphFormat::R32Float`. Both supported
  formats are documented under "Logical textures" and "Limits".
- "Build-time validation" now documents shared-uniform conflicts, nested
  control restrictions, optional gates, upload rules, declaration checks,
  and the temporary unsupported-feature list. External storage restrictions
  are documented under "Logical textures".
- External sampled imports and unsupported mutable external storage bindings
  are now distinguished, matching removal of the storage-handle conversion.
- "Limits" now states that graph shaders require a reflected uniform
  parameter block for `GraphShaderParams` generation.

## Priority

Updated (2026-09-09): §§1–5 are resolved. The phase-1 test inventory is in
place, and §6's keep-or-delete decision is keep for tested migration
scaffolding. This does not imply that the pure compiler and expander are
wired into production execution.

1. Reconcile the remaining documentation discrepancies (§7): correct the
   stale cursor paragraph and annotate 07/phase 1 with the implemented scope
   and verification requirements.
2. Address §6's "Remaining items to address": the unused push-access hook,
   discarded names, runtime dispatch-group vocabulary ownership, and codegen
   classification/rendering separation.
3. During phase 3, integrate the pure compiler, assembly, expansion, and
   required side tables; remove obsolete adapters after parity tests pass
   (§6, "Leaving for phase 3").
4. Keep later feature scaffolding assigned to its owning phases (§6,
   "Leaving for later phases"). Unsupported phase-5 vocabulary is not a
   phase-3 completion blocker.
