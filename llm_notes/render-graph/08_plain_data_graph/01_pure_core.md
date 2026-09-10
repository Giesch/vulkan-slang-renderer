# Phase 1 — Pure Core Beneath Typed Nodes

STATUS: IMPLEMENTED; review reconciled (2026-09-09) — implementation record for phase 1 of
[`../08_plain_data_graph.md`](../08_plain_data_graph.md). That document owns
the v2 design and decisions 1–13. This document specifies the phase-1 work at
implementation depth. Where the two disagree, the decision record below wins
for phase-1 scope.

Historical line references below describe the planned extraction, not the
current working tree. The 07 typed API and this phase landed together; neither
`schedule.rs` nor `BuildCtx` existed in this branch history. Instructions to
port, move, or delete them describe design ancestry, not remaining work.
Tests cover the specified access shapes but cannot establish parity against
an independent `BuildCtx` implementation in this repository.

The implementation updates here supersede the original extraction-only scope.
The review record is `01_review.md`. Tested migration scaffolding remains until
its owning phase wires it into production; phase-1 completion does not imply
executor migration or closure of later ledger rows.

## Scope

Phase 1 puts a pure, plain-data core beneath the typed tuple facade:

- Typed nodes lower into a private `GraphDesc`.
- One pure `validate()` produces every error and an `Analysis`.
- `RenderGraph::new` is the single validation authority: lower → validate →
  allocate. No separate `schedule.rs` or `BuildCtx` remains.
- Pure `compile()` and `expand()` exist and are unit-tested. They are **not
  wired into `execute()`**. Production execution retains `PlanCtx`/`plan()`,
  with review fixes for staging errors, buffer validation, padding-safe byte
  staging, optional picking, and submission-aware cursor commits.

Non-goals for phase 1: executor migration (3a), schema codegen (phase 2),
frame uploads to `Immutable` (3b), draw lists (4), offscreen targets (5),
derived barriers (6). The combined 07/phase-1 landing includes split-type
codegen, templates, snapshots, and the `check_crate` fixture. Phase-2 schema
metadata remains separate.

Graph API consumers in the tree: `examples/watercolor` (stress: optional,
upload, repeat, push blocks, prev-reads, one draw) and `examples/particles`
(minimal: one dispatch, one draw, GPU-only buffer). `gpu_picking` and
`toon_link` use the manual `FrameRenderer` API and are untouched.
`PickingNode`, `draw_indexed`, `draw_index_range`, and `draw_indexed_indirect`
have zero example consumers but stay public API and must lower correctly.

## Phase-1 decisions (2026-09-07)

1. **Single authority.** `RenderGraph::new` lowers every constructible graph
   (including repeat, optional, upload, picking) into `GraphDesc`. `validate()`
   subsumes all 15 `BuildCtx` checks. Physical image counts come from
   `Analysis`. `BuildCtx` is deleted in this phase, not kept as a shadow path.
2. **Decision 6 enforced in phase 1.** Two nodes that stage distinct data
   sources into one uniform slot are an error, even when the staged data is
   identical. The validator never proves two runtime values equal. This
   supersedes the parent document's "phase 3a at the latest" and its
   implication that watercolor's identical shared blur uniform stays valid.
   Watercolor's two blur nodes split (see "Watercolor changes").
3. **`GraphFormat` public in phase 1.** `GraphResources::texture` takes a
   graph-local `GraphFormat` enum instead of `vk::Format`. The facade maps it
   with the house `ToVk` pattern.
4. **Full pure compile.** `compile.rs` lands with the flat step program,
   conservative barrier templates, and the assembly-program builder. Assembly
   programs are tested against hand-authored `SchemaTable` fixtures only;
   graphs lowered from typed nodes get layout-less schemas and `Deferred`
   assembly until phase-2 codegen emits layouts.

## Module layout

All under `crates/renderer/src/renderer/render_graph/`. The facade file
`crates/renderer/src/renderer/render_graph.rs` declares the modules.

| File | Purity | Contents |
| --- | --- | --- |
| `desc.rs` | pure: no `ash`, no live indices | `GraphFormat` (pub), ID newtypes, `GraphDesc` tables, `SchemaTable` |
| `validate.rs` | pure | `GraphError`, `Analysis`, `validate()` |
| `compile.rs` | pure | `CompiledGraph`, `compile()`, `build_assembly()` |
| `expand.rs` | pure | `TexRunState` (moved here), `RunState`, `FrameShape`, `ExpandedFrame`, `expand()` |
| `lower.rs` | facade-side: no `ash`; live indices only as plain `usize`/`u64` | `LowerCtx`, `NodeAccess` (moved here), `LowerOutput` |
| `render_graph.rs` | facade: keeps `ash` | binding types, node types, `GraphNode`, `PlanCtx`, `BindingResolver`, `RenderGraph`, `GraphFormat → vk::Format` mapping |
| `schedule.rs` | never created in the combined landing | — |

Visibility rules:

- All modules are private (`mod desc; mod validate; ...`).
- The only new `pub` item is `GraphFormat`, re-exported with
  `pub use desc::GraphFormat;` (flows out through `renderer.rs:57`).
- `LowerCtx` and `NodeAccess` stay pub-in-a-private-module.
  `GraphNode` stays sealed. No public
  raw description API exists.
- The new files are child modules of `render_graph`, so their tests can
  construct private facade types (`RawBufferBinding`, `GraphTex(0)`) directly.
  This is load-bearing for pure tests without a renderer.

## `desc.rs`

No `ash` import. No live renderer handle or index in any type. Everything is
`pub(crate)` with `pub(crate)` fields, except `GraphFormat` (`pub`).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphFormat {
    R32Float,     // maps to vk::Format::R32_SFLOAT
    Rgba32Float,  // maps to vk::Format::R32G32B32A32_SFLOAT
}
```

ID newtypes, each `#[derive(Debug, Clone, Copy, PartialEq, Eq)]` over a
`pub(crate) u32` (`FieldKey`: `u16`): `TexId`, `BufferId`, `ImportId`,
`ValueId`, `UniformId`, `PipelineId`, `SchemaId`, `FieldKey`.

```rust
pub(crate) struct GraphDesc {
    pub(crate) textures:  Vec<TexDecl>,
    pub(crate) buffers:   Vec<BufferDecl>,
    pub(crate) imports:   Vec<ImportDecl>,
    pub(crate) values:    Vec<ValueDecl>,
    pub(crate) uniforms:  Vec<UniformDecl>,
    pub(crate) pipelines: Vec<PipelineDecl>,
    pub(crate) uploads:   Vec<UploadDesc>,
    pub(crate) passes:    Vec<PassDesc>,
}

pub(crate) struct TexDecl { pub name: String, pub format: GraphFormat, pub size: SizeClass, pub usage: TexUsage }
pub(crate) enum SizeClass { Fixed(u32, u32), Window, WindowDiv(u32) }  // Window/WindowDiv reject (S7)
pub(crate) enum TexUsage  { Storage, Color, Depth }                    // Color/Depth reject (S6)

pub(crate) struct BufferDecl { pub name: String, pub kind: BufferKind, pub capacity: Option<u32>, pub elem_size: Option<u32> }
pub(crate) enum BufferKind { GpuOnlyFlight, Singleton, Immutable, Storage }

pub(crate) struct ImportDecl { pub name: String }

pub(crate) struct ValueDecl { pub name: String, pub kind: ValueKind, pub optional: bool }
pub(crate) enum ValueKind {
    Count,
    Groups,                                  // no phase-1 producer; GroupSource::Value rejects (S1)
    Bytes  { schema: SchemaId },
    Array  { elem: SchemaId, max_len: u32 },
}

pub(crate) struct UniformDecl { pub name: String, pub schema: SchemaId, pub source: UniformSourceDesc }
pub(crate) struct UniformSourceDesc {
    pub data: Option<ValueId>,               // always Some for phase-1 lowered uniforms
    pub bindings: Vec<(FieldKey, ResourceRef)>,
}

pub(crate) struct PipelineDecl { pub name: String, pub kind: PipelineKind, pub params: SchemaId, pub push: Option<SchemaId> }
pub(crate) enum PipelineKind { Compute, Graphics }

pub(crate) struct UploadDesc { pub name: String, pub buffer: BufferId, pub value: ValueId }

pub(crate) enum PassDesc {
    Leaf(LeafPass),
    When   { name: String, value: ValueId, body: Vec<LeafPass> },
    Repeat { name: String, count: ValueId, body: Vec<LeafPass> },
}
pub(crate) enum LeafPass { Compute(DispatchDesc), Raster(RasterDesc) }

pub(crate) struct DispatchDesc {
    pub name: String, pub pipeline: PipelineId, pub uniform: UniformId,
    pub groups: GroupSource, pub push: Option<PushDesc>,
}
pub(crate) enum GroupSource { Fixed([u32; 3]), Value(ValueId) }

pub(crate) struct PushDesc { pub data: Option<ValueId>, pub schema: SchemaId, pub bindings: Vec<(FieldKey, ResourceRef)> }

pub(crate) struct RasterDesc { pub name: String, pub targets: RasterTargets, pub draws: Vec<DrawDesc> }
pub(crate) enum RasterTargets { Main, Offscreen { color: Vec<TexId>, depth: Option<TexId> } }

pub(crate) struct DrawDesc { pub name: String, pub pipeline: PipelineId, pub uniform: UniformId, pub call: DrawCall, pub push: Option<PushDesc> }
pub(crate) enum DrawCall {
    VertexCount(u32),
    WholeIndexed,                            // count resolves at execute; nothing to validate purely
    IndexRange { first_index: u32, index_count: u32 },
    IndexedIndirect { args: BufferRef, draw_count: u32 },
}

pub(crate) enum ResourceRef { Tex(TexId, TexAccess), Buf(BufferRef, BufAccess), External(ImportId) }
pub(crate) enum TexAccess { Read, ReadPrevious, Write, Mutate }
pub(crate) enum BufAccess { Read, Write, Mutate, IndirectArgs }
pub(crate) struct BufferRef { pub buffer: BufferId, pub slot: SlotSel, pub offset: u32, pub range: Option<u32> }
pub(crate) enum SlotSel { Current, Previous }
```

Phase-1 deviations from the parent document's §1, all temporary:

- `BufferDecl` has no `elem_schema` and no `init`. Buffers are interned
  references to live handles; the decision-1 creation/init API is phase 3a.
  `capacity`/`elem_size` are `Some` only when the typed slot carries a length.
- `BufferKind::Storage` is a compatibility kind for watercolor's stroke-points
  buffer. Phase 3b migrates that buffer to `Immutable` and removes the variant
  (ledger row S2).
- `PushDesc.data` is `None` for every lowered push: the push data half is a
  build-time constant on the node (`ComputeNodeWithPush.push_data`), and byte
  storage rules are decision-3 gated (3a). Fixtures may set `Some`.

Schema table, also in `desc.rs`:

```rust
pub(crate) struct SchemaTable { pub schemas: Vec<SchemaDesc> }
impl SchemaTable {
    pub(crate) fn push(&mut self, s: SchemaDesc) -> SchemaId;
    pub(crate) fn get(&self, id: SchemaId) -> Option<&SchemaDesc>;
}

pub(crate) struct SchemaDesc {
    pub name: String,
    pub size: u32,
    pub resource_fields: Vec<ResourceFieldKind>,  // GraphBindingSet::visit order; FieldKey = position
    pub layout: Option<SchemaLayout>,             // None for all phase-1 lowered schemas
}
pub(crate) enum ResourceFieldKind { SampledTex, StorageTex, BufAddr }

pub(crate) struct SchemaLayout { pub fields: Vec<SchemaField> }  // ascending dst offset
pub(crate) struct SchemaField { pub key: FieldKey, pub offset: u32, pub len: u32, pub kind: SchemaFieldKind }
pub(crate) enum SchemaFieldKind {
    Data { src_offset: u32 },  // copy from frame value bytes; explicit padding is a Data field
    SampledTex, StorageTex, BufAddr,
}
```

## `lower.rs`

`NodeAccess` moves here verbatim from `schedule.rs:12-31` for the legacy
`plan()` path. Cleanup removes the unused `GraphPush::access` hook:
`GraphPush::lower_input` preserves push bindings in `PushDesc`, from which
validation and compilation derive access information. Later phases use that
lowered information; they do not require a separate push-access hook.

`LowerCtx` accumulates errors; no method returns `Result`. The facade merges
lowering errors with validation errors and reports all of them at once.

```rust
pub struct LowerCtx { /* all fields private */ }

pub(crate) struct LowerOutput {
    pub desc: GraphDesc,
    pub schemas: SchemaTable,
    pub errors: Vec<GraphError>,
    pub picking: Option<usize>,        // legacy facade marker: graphics pipeline index
    // Transient side tables. Phase 1 produces them for tests and drops them;
    // executor-only tables await phase 3a; buffer validation retains its tables.
    pub uniform_slots: Vec<usize>,     // UniformId  -> live uniform buffer index
    pub buffer_indices: Vec<usize>,    // BufferId   -> live storage/singleton buffer index
    pub pipeline_indices: Vec<usize>,  // PipelineId -> live pipeline index
    pub import_handles: Vec<u64>,      // ImportId   -> BindlessHandle::to_raw()
}

impl LowerCtx {
    pub(crate) fn new(textures: Vec<TexDecl>) -> Self;
    pub(crate) fn dispatch(&mut self, pipeline_index: usize, groups: [u32; 3],
                           uniform: UniformInput, push: Option<PushInput>);
    pub(crate) fn draw(&mut self, pipeline_index: usize, call: LowerDrawCall,
                       uniform: UniformInput, push: Option<PushInput>);
    pub(crate) fn upload(&mut self, buffer_index: usize, elem_size: u32, capacity: u32);
    pub(crate) fn begin_repeat(&mut self);
    pub(crate) fn end_repeat(&mut self);
    pub(crate) fn begin_optional(&mut self);
    pub(crate) fn end_optional(&mut self);
    pub(crate) fn picking(&mut self, pipeline_index: usize);
    pub(crate) fn finish(self) -> LowerOutput;
}

pub(crate) struct UniformInput { pub slot: usize, pub gpu_size: u32, pub data_size: u32, pub bindings: Vec<GraphBinding> }
pub(crate) struct PushInput { pub size: u32, pub bindings: Vec<GraphBinding> }
pub(crate) enum LowerDrawCall {
    VertexCount(u32), WholeIndexed,
    IndexRange { first_index: u32, index_count: u32 },
    IndexedIndirect { args_index: usize, byte_offset: u64, draw_count: u32 },
}
```

### Interners

- `intern_buffer(kind, index, capacity, elem_size) -> (BufferId, SlotSel)`.
  Key is `(BufferKind, index)` after kind-class merging:
  `BufferBindingKind::Storage → (Storage, Current)`,
  `GpuOnlyCurrent → (GpuOnlyFlight, Current)`,
  `GpuOnlyPrevious → (GpuOnlyFlight, Previous)` — Current and Previous of one
  buffer intern to **one** `BufferId`,
  `Immutable → (Immutable, Current)`, `Singleton → (Singleton, Current)`.
  A later sighting may fill a `None` capacity or elem_size.
- `intern_import(handle) -> ImportId` — keyed by `BindlessHandle::to_raw()`
  (`bindless.rs:29-31`).
- `intern_pipeline(kind, index, params_schema, push_schema) -> PipelineId` —
  keyed by `(kind, index)`.
- `intern_uniform(slot, data: ValueId, schema, bindings) -> UniformId` — the
  decision-6 check. First sighting of a live slot creates the `UniformDecl`.
  A later sighting with identical `(data, bindings)` returns the existing
  `UniformId` (merge). Anything else records
  `GraphError::UniformSourceConflict { slot }` and returns the existing ID so
  lowering continues and reports more errors. Every typed node declares a
  fresh `ValueId` for its frame data, so two nodes sharing one live slot
  always conflict in phase 1. The merge path serves the future `with_uniform`
  scope (3b) and direct unit tests.
- `binding_to_ref(b: GraphBinding) -> Option<ResourceRef>`:
  - `SampledRef::Graph(t)` → `Tex(TexId(t.0), Read)`;
    `GraphPrevious(t)` → `Tex(.., ReadPrevious)`;
    `SampledRef::External(h)` → `External(intern_import(h))`.
  - `StorageRef::Graph(t, Write|Mutate)` → `Tex(.., Write|Mutate)`;
    `StorageRef::External(_)` → record `GraphError::MutableExternalImport`
    (permanent, decision 2; zero consumers today) and return `None`.
  - `Buffer(raw)` → `Buf(BufferRef { buffer, slot, offset: raw.byte_offset as u32, range: None }, access)`.
    `GraphBinding::Buffer` erases mutability, so phase 1 derives a
    conservative `BufAccess` from the kind: `Storage → Mutate`,
    `GpuOnlyFlight` Current → `Mutate`, Previous → `Read`,
    `Immutable → Read`, `Singleton → Read`. Precise access arrives with
    phase-2 field metadata. If `raw.byte_offset > u32::MAX`, record
    `GraphError::BufferOffsetOverflow` and clamp.

### Scopes, passes, values

- Scope state: `Top | Repeat { body, count } | Optional { body, first_value }`.
  `begin_repeat` or `begin_optional` inside any non-Top scope records
  `GraphError::NestedControlFlow` and ignores the begin (with a matching pop
  on end). This check lives in lowering because `PassDesc` bodies are
  `Vec<LeafPass>`: nesting is unrepresentable in the desc.
- `begin_repeat` declares `ValueDecl { kind: Count, optional: false }`.
- Every value declared inside an `Optional` scope gets `optional: true`.
  `end_optional` sets `When.value` to the **first** `ValueId` declared inside
  the scope — one presence per `Option<Body::Frame>`, the coupled-presence
  rule. A scope that declares no values records
  `GraphError::EmptyOptionalScope`. A scope with no leaf passes (upload-only
  body) emits no `When` pass; the upload value's optionality carries the
  presence.
- Draw coalescing: top-level draws append to an open trailing
  `Raster(Main)` pass. A later non-draw pass closes it; a draw after that
  opens a second raster pass, which validation reports
  (`PassAfterMainRaster` / `MultipleRasterPasses`). Draws inside a
  `Repeat`/`Optional` scope become single-draw `Raster` leaves in that body;
  validation rejects them (`RasterInRepeat` permanent; `RasterInWhen` → S6).
- `picking(pipeline_index)` writes only the facade marker; no desc entry.
  A second call records `GraphError::MultiplePickingNodes`. `finish()` records
  `GraphError::PickingWithoutDraw` when picking was seen and no draw lowered.
  This is the unchanged legacy compatibility path.
- Schemas: one per uniform slot first-sighting
  (`size: gpu_size`, `resource_fields` from bindings in visit order,
  `layout: None`); one per data value (`size: data_size`); one per upload
  element; one per push block.
- Names are deterministic and index-based: `"tex{i}"`, `"buf.storage.{index}"`,
  `"uniform{slot}"`, `"dispatch{n}"`, `"draw{n}"`, `"upload{n}"`,
  `"repeat{n}"`, `"when{n}"`, `"pipeline.compute.{index}"`. They feed
  `GraphError` context and later debug labels.

### Per-node lowering

`GraphNode::declare` becomes `fn lower(&self, cx: &mut LowerCtx)` (no
`Result`). `plan` retains the legacy executor with the review fixes listed
in Scope. Existing accessors supply raw indices:
`PipelineIndex::raw()` (`pipeline.rs:20-21`) and `BindlessHandle::to_raw()`.
Sizes come from `size_of::<S>()`, `size_of::<S::Data>()`, `size_of::<B>()`.
A new helper beside `collect_access` collects bindings:
`fn collect_bindings<B: GraphBindingSet>(b: &B) -> Vec<GraphBinding>`.
`collect_access` stays: `plan()` still uses it for `apply_writes`.

| Node | Lowering | Lower-time errors |
| --- | --- | --- |
| `ComputeNode<S>` | `cx.dispatch(pipeline.raw(), groups, UniformInput { slot, gpu_size, data_size, bindings: collect_bindings(&self.bindings) }, None)`; declares one `Bytes` value | `UniformSourceConflict`, `MutableExternalImport`, `BufferOffsetOverflow` |
| `ComputeNodeWithPush<S,B>` | same, plus `push: Some(PushInput { size_of::<B>(), collect_bindings(&self.push_bindings) })`; `push_data` is not lowered (`PushDesc.data = None`) | same |
| `DrawNode<S,P>` (4 call kinds) | `cx.draw(pipeline.raw(), call, UniformInput {...}, push)`. `IndexedIndirect` interns `(Immutable, args_index)` and builds `BufferRef { slot: Current, offset, range: None }` with `BufAccess::IndirectArgs`. `push` comes from a new `GraphPush::lower_input(&self) -> Option<PushInput>` (`()` → `None`, `PushValues<B>` → `Some`) | same |
| `PickingNode<C>` | `cx.picking(pipeline.raw())`; no desc entry | `MultiplePickingNodes`; `PickingWithoutDraw` at `finish()` |
| `UploadNode<T>` | `cx.upload(slot.index, size_of::<T>(), slot.len)`; interns `(Storage, index)`; declares one `Array` value and an element schema | none at lower time |
| `RepeatNode<B>` | `begin_repeat(); body.lower(cx); end_repeat();` | `NestedControlFlow` |
| `OptionalNode<B>` | `begin_optional(); inner.lower(cx); end_optional();` | `NestedControlFlow`, `EmptyOptionalScope` |
| tuples 1..=12 | `self.$f.lower(cx);` in order | — |

## `validate.rs`

```rust
pub(crate) struct Analysis { pub tex_phys: Vec<u32> }

pub(crate) fn validate(desc: &GraphDesc, schemas: &SchemaTable) -> Result<Analysis, Vec<GraphError>>;
```

`validate` collects every error; it never stops at the first. Checks, in
order, each over the full desc:

1. **ID range** — every ID reachable anywhere in the desc is bounds-checked
   against its table (`IdOutOfRange`). Later checks skip out-of-range IDs
   (`let Some(..) = .. else continue`); invalid descs must not panic.
2. **Unsupported subset** — the `UnsupportedInPhase1` matrix below.
3. **Value kinds** — `Repeat.count` is `Count`; `When.value` has
   `optional == true`; `UniformSourceDesc.data` is `Bytes`; `PushDesc.data`
   (when `Some`) is `Bytes`; `UploadDesc.value` is `Array`;
   `GroupSource::Value` is unsupported, and when present its value must be
   `Groups`.
4. **Uploads** — target kind is `Storage` or `Immutable` (`Storage` is the
   temporary S2 allowance); `GpuOnlyFlight` and `Singleton` targets are
   `UploadTargetKind` errors. When `max_len` and target capacity are both
   known, `max_len <= capacity` (`UploadTooLarge`).
5. **Uniform sources and binding shape** — for every `UniformDecl`, including
   sources with zero consumers (review item 5):
   `bindings.len() == schema.resource_fields.len()`; keys are exactly `0..n`
   in order; each ref kind matches positionally — `Tex(_, Read|ReadPrevious)`
   and `External` match `SampledTex`; `Tex(_, Write|Mutate)` matches
   `StorageTex`; `Buf` matches `BufAddr`
   (`BindingCountMismatch`/`BindingKindMismatch`). Same for every `PushDesc`.
6. **Per-command texture conflicts** — a command is one `DispatchDesc` or one
   `DrawDesc`; its access set is the union of its uniform source's bindings
   and its own push bindings. Within one command: duplicate `Write`
   (`DuplicateWrite`); `Mutate`+`Read` (`MutateAndRead`); `Mutate`+`Write`
   (`MutateAndWrite`); `Write`+`ReadPrevious` (`WriteAndPrevRead`).
   `Mutate`+`ReadPrevious` is legal.
7. **Pass structure** — at most one `Raster` pass (zero is legal: a
   compute-only graph draws nothing); it is a top-level `Leaf` and last
   (`PassAfterMainRaster` for any pass after it); `Raster` in a `Repeat` body
   is `RasterInRepeat` (permanent); `Raster` in a `When` body is
   `UnsupportedInPhase1 { RasterInWhen }`; a second `Raster` is
   `UnsupportedInPhase1 { MultipleRasterPasses }`.
8. **Draw and pipeline rules** — a draw command's access set contains no
   `Tex(_, Write|Mutate)` (`DrawWritesTexture`); `DispatchDesc.pipeline` is
   `Compute` and `DrawDesc.pipeline` is `Graphics` (`PipelineKindMismatch`);
   `IndexedIndirect.args` buffer kind is `Immutable`
   (`IndirectArgsNotImmutable`; the typed constructor guarantees it, the
   check covers hand-built descs).
9. **Repeat rotation rule** — per `Repeat` pass: a texture rotates when any
   body command's uniform-source or push bindings `Write` it. A body dispatch
   whose **uniform source** bindings touch a rotating texture is
   `RepeatUniformRotatesTexture`. Push bindings are exempt: they re-resolve
   per iteration. `rotates` resets per pass, so textures rotated in an
   earlier repeat are stable in the next.
10. **Analysis derivation** — start every texture at 1 physical image; any
    command with `ReadPrevious(t)` → `tex_phys[t] = 2`; any command with both
    `Read(t)` and `Write(t)` → `tex_phys[t] = 2`; `Mutate` never raises the
    count. This reproduces `BuildCtx`'s `needs_two` exactly; parity tests 42
    and 43 pin it.

### `GraphError`

`pub(crate) enum GraphError` in `validate.rs`,
`#[derive(Debug, Clone, PartialEq)]`, manual `Display` prefixed
`render graph: `. Keep the key phrases of the old messages ("previous
version", "mutates and reads", "more than once", "undeclared", "push block",
"repeat", "precede", "only read", "at most one", "at least one draw") so
diagnostics do not regress. Variants carry plain indices and strings.

ANNOTATION (2026-09-08, review §3d–§3g): the implementation differs from
this section in five ways.

- The texture-hazard variants (`DuplicateWrite`, `MutateAndRead`,
  `MutateAndWrite`, `WriteAndPrevRead`, `RepeatUniformRotatesTexture`,
  `DrawWritesTexture`) carry `tex: String` — the decl name, which embeds the
  `res.texture(...)` call site — not `tex: u32`.
- `validate()` also emits `TextureExtentZero { texture, width, height }` and
  `PrevReadWithoutWrite { command, tex }`.
- `RenderGraph::new` emits
  `TextureExtentTooLarge { texture, width, height, max }` through
  `extent_limit_errors`, checked against the device's `maxImageDimension2D`.
- `Display` has no `render graph: ` prefix. The `validation_message` header
  identifies the source; a prefix on every bullet would repeat it.
- `TableKind` implements `Display` with lowercase table names.

Emitted by `validate()` (ported `BuildCtx` check in parentheses; the 15
checks are the complete list in `schedule.rs:50-207`):

| Variant | Fields | Check |
| --- | --- | --- |
| `IdOutOfRange` | `{ table: TableKind, id: u32 }` | undeclared texture, generalized to every table |
| `DuplicateWrite` | `{ command: String, tex: u32 }` | double write |
| `MutateAndRead` | `{ command: String, tex: u32 }` | mutate+read |
| `MutateAndWrite` | `{ command: String, tex: u32 }` | mutate+write |
| `WriteAndPrevRead` | `{ command: String, tex: u32 }` | write+read_previous |
| `RepeatUniformRotatesTexture` | `{ repeat: String, uniform: String, tex: u32 }` | loop rotation rule; keep "move the reference into the node's push block" |
| `RasterInRepeat` | `{ repeat: String }` | no draw inside repeat (permanent) |
| `PassAfterMainRaster` | `{ pass: String }` | merges "compute must precede every draw" and "repeat must precede every draw" |
| `DrawWritesTexture` | `{ draw: String, tex: u32 }` | draws only read; keep "can only read graph textures" |
| `PipelineKindMismatch` | `{ command: String, expected: PipelineKind }` | new |
| `IndirectArgsNotImmutable` | `{ draw: String }` | new |
| `WhenGateNotOptional` | `{ when: String }` | new |
| `ValueKindMismatch` | `{ value: String, expected: &'static str, found: &'static str }` | new |
| `UploadTargetKind` | `{ upload: String, kind: &'static str }` | new (permanent, decision 4 of the review update) |
| `UploadTooLarge` | `{ upload: String, max_len: u32, capacity: u32 }` | new |
| `BindingCountMismatch` | `{ uniform: String, expected: usize, found: usize }` | new |
| `BindingKindMismatch` | `{ uniform: String, field: u16 }` | new |
| `UnsupportedInPhase1` | `{ feature: UnsupportedFeature, at: String }` | new |

Emitted by `LowerCtx`:

| Variant | Fields | Check |
| --- | --- | --- |
| `NestedControlFlow` | `{ outer: &'static str, inner: &'static str }` | nested repeat, extended to optional; keep "nested repeat is not supported" for repeat/repeat |
| `UniformSourceConflict` | `{ slot: usize }` | decision 6 |
| `MultiplePickingNodes` | `{}` | at most one picking node (legacy path) |
| `PickingWithoutDraw` | `{}` | picking needs a draw (legacy path) |
| `EmptyOptionalScope` | `{}` | new |
| `MutableExternalImport` | `{}` | permanent (decision 2) |
| `BufferOffsetOverflow` | `{ offset: u64 }` | new |

`UnsupportedFeature` prints a description and its owning phase through
`Display` — the structured feature/location/phase error the parent document
requires:

| Feature | Rejected input | Owning phase / ledger |
| --- | --- | --- |
| `WindowSizeClass` | `SizeClass::Window`, `SizeClass::WindowDiv(_)` | 5 / S7 |
| `ColorAttachmentUsage` | `TexUsage::Color` | 5 / S6 |
| `DepthAttachmentUsage` | `TexUsage::Depth` | 5 / S6 |
| `OffscreenTargets` | `RasterTargets::Offscreen { .. }` | 5 / S6 |
| `MultipleRasterPasses` | second `Raster` pass | 5 / S6 |
| `RasterInWhen` | `Raster` leaf in a `When` body | 5 / S6 |
| `GroupSourceValue` | `GroupSource::Value(_)` | 3a / S1 |

## `compile.rs`

Pure. Consumes `GraphDesc` + `Analysis` + `SchemaTable`. The module doc
comment states: not called by `RenderGraph::new` or `execute()` in phase 1;
phase 3a wires it. Unit tests are its only callers.

```rust
pub(crate) struct CompiledGraph {
    pub passes: Vec<CompiledPass>,
    pub assemblies: Vec<AssemblyProgram>,  // indexed by AsmId
    pub tex_phys: Vec<u32>,
    pub value_count: u32,                  // from desc.values.len(); sizes FrameShape
}
pub(crate) struct AsmId(pub u32);

pub(crate) enum CompiledPass {
    Leaf(CompiledLeaf),
    Repeat { count: ValueId, body: Vec<CompiledLeaf> },
    When   { gate: ValueId, body: Vec<CompiledLeaf> },
}
pub(crate) struct CompiledLeaf {
    pub kind: CompiledLeafKind,
    pub barrier_before: BarrierKind,  // template; expand suppresses it for the first executed step
    pub access: LeafAccess,           // texture access union for cursor math
}
pub(crate) enum CompiledLeafKind {
    Dispatch { pipeline: PipelineId, groups: GroupSource, uniform: UniformId, asm: AsmId, push_asm: Option<AsmId> },
    Raster   { draws: Vec<CompiledDraw> },
}
pub(crate) struct CompiledDraw { pub pipeline: PipelineId, pub uniform: UniformId, pub asm: AsmId, pub push_asm: Option<AsmId>, pub call: DrawCall }
pub(crate) struct LeafAccess { pub reads: Vec<TexId>, pub prev_reads: Vec<TexId>, pub writes: Vec<TexId>, pub mutates: Vec<TexId> }
pub(crate) enum BarrierKind { ComputeSync, ComputeToGraphics }

pub(crate) enum AssemblyProgram { Steps(Vec<AssemblyStep>), Deferred }  // Deferred: schema has no layout
pub(crate) struct AssemblyStep { pub dst_offset: u32, pub src: AssemblySrc }
pub(crate) enum AssemblySrc {
    FrameBytes { value: ValueId, src_offset: u32, len: u32 },
    ResolveTex { tex: TexId, access: TexAccess },
    ResolveBuf(BufferRef),
    External(ImportId),
}

pub(crate) fn compile(desc: &GraphDesc, analysis: &Analysis, schemas: &SchemaTable) -> CompiledGraph;
pub(crate) fn build_assembly(schema: &SchemaDesc, data: Option<ValueId>, bindings: &[(FieldKey, ResourceRef)]) -> AssemblyProgram;
```

Rules:

- One `AssemblyProgram` per `UniformDecl` in table order (`AsmId(u) == u`),
  then one per distinct `PushDesc` in pass order.
- `build_assembly`: `layout == None` → `Deferred`. Otherwise walk
  `layout.fields` in order, one step per field. `Data { src_offset }` →
  `FrameBytes` (validate guarantees `data` is `Some` when a data field
  exists; on fixture misuse, `debug_assert!` and emit nothing). Resource
  kinds → the binding with the matching `FieldKey` → `ResolveTex` /
  `ResolveBuf` / `External`. No coalescing of adjacent data fields. Bytes no
  field covers are never copied; explicit padding must be a `Data` field to
  be copied (test 56 pins this).
- Barrier template, conservative, mirrors the positional rules in
  `queue_dispatch_raw` (`renderer.rs:5878-5906`) and `record_command_buffer`:
  every dispatch leaf gets `ComputeSync`; every raster leaf gets
  `ComputeToGraphics`. Suppression of the first executed step's barrier
  happens in `expand`, so a skipped `When` or a zero-trip repeat cannot
  remove synchronization between surviving steps.
- `LeafAccess` is the per-command texture access union (uniform source
  bindings + push bindings).

## `expand.rs`

`TexRunState` moves here verbatim from `schedule.rs:210-244` — fields,
methods, doc comments — with one signature change:
`pub(crate) fn new(phys_count: u32) -> Self` (takes 1 or 2, not a bool). Its
four cursor tests move with it. The facade keeps using it
(`use expand::TexRunState;`) because `PlanCtx`/`BindingResolver` consume it at
execute time.

```rust
pub(crate) struct RunState { pub tex: Vec<TexRunState> }
impl RunState { pub(crate) fn new(tex_phys: &[u32]) -> Self; }

/// Indexed by ValueId. counts entries for non-Count values and present
/// entries for non-optional values are ignored.
pub(crate) struct FrameShape { pub counts: Vec<u32>, pub present: Vec<bool> }
impl FrameShape { pub(crate) fn neutral(value_count: u32) -> Self; }  // counts=0, present=true

pub(crate) struct ExpandedFrame { pub steps: Vec<ExecStep>, pub next: RunState }
pub(crate) struct ExecStep {
    pub barrier_before: Option<BarrierKind>,  // None only for the first executed step
    pub leaf: LeafRef,
    pub tex: Vec<ResolvedTex>,
}
pub(crate) struct LeafRef { pub pass: u32, pub body_index: u32, pub iteration: u32 }
pub(crate) struct ResolvedTex { pub tex: TexId, pub phys: u32, pub access: TexAccess }

pub(crate) fn expand(graph: &CompiledGraph, state: &RunState, shape: &FrameShape) -> ExpandedFrame;
```

Semantics: clone `state` into a working copy; walk passes in order. `When`
emits its body iff `shape.present[gate]`. `Repeat` emits its body
`shape.counts[count]` times. Per executed leaf, resolve `reads → read_phys()`,
`prev_reads → prev_phys()`, `mutates → read_phys()`, `writes → write_phys()`;
then `commit_write()` per write — the same order as `PlanCtx::apply_writes`
(`render_graph.rs:1332-1336`). The input `state` is untouched; `next` is a
proposal (commit-on-submit is 3a). `expand` is infallible; shape validity is
the caller's contract (3a wires the decision-11 checks before it). Module doc
comment states: not wired into `execute()` in phase 1.

## Facade changes (`render_graph.rs`)

- Module header: replace `mod schedule;` with
  `mod desc; mod validate; mod compile; mod expand; mod lower;`, plus
  `pub use desc::GraphFormat;` and the needed `use` lines.
- `GraphResources::texture(&mut self, width: u32, height: u32, format: GraphFormat) -> GraphTex`
  (`render_graph.rs:96`) — the only public signature change in the crate.
  The private facade `TexDecl` stores `GraphFormat`.
- `impl ToVk for GraphFormat { type Vk = vk::Format; ... }` in the facade,
  following the `ReflectedStageFlags` pattern (`renderer.rs:5580-5591`);
  the private `ToVk` trait (`renderer.rs:5377`) is visible to this child
  module.
- `GraphNode::declare(&self, b: &mut BuildCtx) -> anyhow::Result<()>` becomes
  `fn lower(&self, cx: &mut LowerCtx);`. All 8 node impls and the tuple macro
  update mechanically (drop the `?`/`Ok(())`).
- `GraphPush` gains `fn lower_input(&self) -> Option<PushInput>`
  (`()` → `None`; `PushValues<B>` → `Some`).
- `RenderGraph::new` (`render_graph.rs:1376-1413`) becomes:
  1. Build `Vec<desc::TexDecl>` from `resources.decls`
     (`SizeClass::Fixed(w, h)`, `TexUsage::Storage`).
  2. `let mut cx = LowerCtx::new(tex_decls); nodes.lower(&mut cx); let out = cx.finish();`
  3. `let mut errors = out.errors;` then `validate(&out.desc, &out.schemas)`:
     on `Err(v)` extend `errors`; on `Ok(a)` keep the analysis. When `errors`
     is non-empty, `bail!` one `anyhow` error: line 1 is
     `render graph validation failed:`, then one `  - {error}` line per
     `GraphError`. All errors report at once.
  4. Allocate physical images as today (`render_graph.rs:1389-1402`), with
     the count from `analysis.tex_phys[i]` and the format from
     `out.desc.textures[i].format` via `.to_vk()`;
     `tex.push(TexRunState::new(analysis.tex_phys[i]))`.
  5. Do not call `compile()` or store `CompiledGraph` yet. Retain the
     `uniform_slots` and `buffer_indices` needed for runtime buffer validation;
     other tested side tables await phase-3 executor wiring.
- `execute()` keeps the legacy planner. Review fixes validate captured buffer
  slots, propagate staging errors, and commit texture cursors immediately after
  successful queue submission. Acquisition skips preserve cursors; presentation
  failures after submission keep the committed versions. This part of option A
  was brought forward from phase 3a.

## Watercolor changes (`examples/watercolor/src/main.rs`)

`GraphFormat` migration:

- Add `GraphFormat` to the `mltrs::renderer` import.
- Line 333: `vk::Format::R32_SFLOAT` → `GraphFormat::R32Float`.
- Line 334: `vk::Format::R32G32B32A32_SFLOAT` → `GraphFormat::Rgba32Float`.
- The 14 `res.texture(...)` calls (lines 335-348) use the two locals and do
  not change. `paper_height` (line 352) is a renderer storage texture, not a
  graph texture; it keeps `vk::Format::R32_SFLOAT` and the `ash::vk` import
  stays.

Blur uniform split (decision 6):

Nodes 6 and 7 (lines 536-548 and 550-562) both reference
`&blur_params_buffer` with distinct frame-data values (two
`wc_gaussian_blur_compute::Params` literals in the frame tuple near line
814). Under the phase-1 rule this is `UniformSourceConflict`. The fix needs a
**second pipeline, not only a second buffer**: `blur_pipeline` binds
`&blur_params_buffer` into its descriptors at creation (lines 422-427), and
no phase-1 check catches a pipeline/source mismatch (that check is
decision 8, phase 5). A node that stages into a new buffer while dispatching
the old pipeline silently reads the old buffer.

1. After line 375:
   `let blur_v_params_buffer = renderer.create_uniform_buffer::<wc_gaussian_blur_compute::Params>()?;`
2. After the existing `blur_pipeline` (line 427):
   ```rust
   let blur_v_pipeline =
       renderer.create_compute_pipeline(shaders.wc_gaussian_blur_compute.pipeline_config(
           wc_gaussian_blur_compute::Resources { params_buffer: &blur_v_params_buffer },
       ))?;
   ```
3. Node 7 (lines 550-551): `&blur_pipeline, &blur_params_buffer` →
   `&blur_v_pipeline, &blur_v_params_buffer`.
4. Struct `Watercolor`: add
   `_blur_v_params_buffer: UniformBufferHandle<wc_gaussian_blur_compute::Params>`
   next to `_blur_params_buffer` (line 106) and initialize it near line 643.
5. The frame tuple already supplies two identical `Params` values; it does
   not change. Rendered output is identical; `just sweep` output must not
   change.

The format-only extraction does not change particles (`GraphResources::new()`,
zero textures); its typed graph migration belongs to the combined 07 landing.

## Test inventory

72 tests. This inventory is the contract; do not trim it. All tests are
in-module `#[cfg(test)]`, no GPU, no mock renderer. Ported tests switch from
error-substring asserts to variant matches
(`assert!(errs.iter().any(|e| matches!(e, GraphError::MutateAndRead { .. })))`).
Validate-side tests use a test-local desc builder
(`fn desc_with(...)` + `fn dispatch_pass(...)`) that fabricates
value/uniform/pipeline/schema entries around hand-written `ResourceRef` sets —
the replacement for `schedule.rs`'s `access()`/`prev_access()` helpers.

Ported, group A — per-command and Analysis (`validate.rs`; from
`schedule.rs:269-374`):

1. `read_write_same_node_needs_two_images` → `tex_phys == [2, 1]`
2. `read_and_write_across_nodes_stays_single_image` → `[1]`
3. `push_block_read_write_also_needs_two`
4. `prev_read_forces_two_images`
5. `prev_read_and_write_same_node_is_an_error` → `WriteAndPrevRead`
6. `prev_read_and_mutate_same_node_is_allowed` (and `tex_phys == [2]`)
7. `mutate_alone_stays_single_image`
8. `mutate_and_read_same_node_is_an_error` → `MutateAndRead`
9. `mutate_and_write_same_node_is_an_error` → `MutateAndWrite`
10. `double_write_same_node_is_an_error` → `DuplicateWrite`
11. `undeclared_texture_is_an_error` → `IdOutOfRange { table: Texture, id: 1 }`

Ported, group B — structure rules (`validate.rs`; from
`schedule.rs:391-465`):

12. `loop_body_uniform_may_not_reference_rotating_texture` (jacobi shape) → `RepeatUniformRotatesTexture`
13. `later_body_write_also_makes_an_earlier_uniform_stale`
14. `loop_body_uniform_may_reference_loop_stable_texture` (positive)
15. `textures_rotated_in_an_earlier_loop_are_stable_in_the_next` (two `Repeat` passes, positive)
16. `draw_inside_repeat_is_an_error` → `RasterInRepeat`
17. `compute_after_draw_is_an_error` → `PassAfterMainRaster`
18. `draw_node_may_not_write` → `DrawWritesTexture`

Ported, group C — lowering-time (`lower.rs`, driving `LowerCtx` directly;
from `schedule.rs:376-389, 467-489`):

19. `nested_repeat_is_an_error` → `NestedControlFlow`
20. `repeat_after_exit_is_allowed`
21. `a_second_picking_node_is_an_error` → `MultiplePickingNodes`
22. `picking_without_draws_is_an_error` (via `finish()`) → `PickingWithoutDraw`
23. `picking_with_a_draw_is_allowed`

Ported, group D — cursor tests (`expand.rs`; from
`schedule.rs:323, 491-520`; only `TexRunState::new(1)`/`new(2)` changes):

24. `prev_phys_is_the_other_image`
25. `single_image_cursor_never_moves`
26. `two_image_cursor_alternates_per_write`
27. `odd_write_counts_carry_across_frames`

Decision-6 tests:

28. `two_nodes_sharing_a_uniform_slot_reject` (`lower.rs`) — two `dispatch()`
    calls, one slot → `UniformSourceConflict`; the desc still holds one
    `UniformDecl` (error recovery)
29. `identical_uniform_source_merges` (`lower.rs`) — `intern_uniform` twice
    with equal `(data, bindings)` → same `UniformId`, no error
30. `same_slot_differing_bindings_reject` (`lower.rs`)
31. `two_consumers_of_one_uniform_id_is_legal` (`validate.rs`) — hand desc,
    two dispatches referencing one `UniformId` → Ok
32. `unconsumed_uniform_source_is_still_checked` (`validate.rs`) — a
    `UniformDecl` no pass references, with a kind mismatch →
    `BindingKindMismatch` (review item 5)

New lowering tests (`lower.rs`):

33. `optional_inside_repeat_rejects` → `NestedControlFlow`
34. `repeat_inside_optional_rejects` → `NestedControlFlow`
35. `nested_optional_rejects` → `NestedControlFlow`
36. `empty_optional_scope_is_an_error` → `EmptyOptionalScope`
37. `optional_gate_is_first_scope_value_and_all_scope_values_optional` —
    upload + dispatch in one optional scope: `When.value` is the upload's
    `Array` value; both values have `optional: true`
38. `gpu_only_current_and_previous_intern_to_one_buffer` — one `BufferId`,
    `SlotSel::Current` and `Previous`
39. `external_sampled_handle_interns_once`
40. `mutable_external_storage_binding_rejects` → `MutableExternalImport`
41. `top_level_draws_coalesce_into_one_raster_pass` — two draws → one
    `Raster` pass with two `DrawDesc`s
42. `watercolor_shaped_lowering_parity` — drive `LowerCtx` through the full
    watercolor shape (optional(upload + dispatch), 2 dispatches,
    repeat(dispatch with push), 4 dispatches, draw) with distinct slots;
    assert table sizes, pass shapes, `validate` Ok, and `tex_phys` equal to
    the `BuildCtx` result for the same access sets (derive the expected
    vector from the binding sets in `examples/watercolor/src/main.rs:460-628`)
43. `particles_shaped_lowering_parity` — one dispatch (GPU-only previous +
    current) + one draw; no textures; validate Ok

Later-phase and permanent rejections (`validate.rs`, hand-built descs — the
typed API cannot construct these):

44. `window_size_class_rejected_until_phase5` → `UnsupportedInPhase1 { WindowSizeClass }`
45. `window_div_rejected_until_phase5`
46. `color_usage_rejected_until_phase5`
47. `depth_usage_rejected_until_phase5`
48. `offscreen_targets_rejected_until_phase5`
49. `second_raster_pass_rejected_until_phase5` → `MultipleRasterPasses`
50. `raster_in_when_rejected_until_phase5` → `RasterInWhen`
51. `group_source_value_rejected_until_phase3a` → `GroupSourceValue`
52. `upload_to_gpu_only_rejects` → `UploadTargetKind` (permanent)
53. `upload_to_singleton_rejects` → `UploadTargetKind` (permanent)
54. `when_gate_must_be_optional` → `WhenGateNotOptional`
55. `repeat_count_must_be_count_kind` → `ValueKindMismatch`

Compile tests (`compile.rs`):

56. `assembly_interleaved_data_resource_and_padding` — hand `SchemaLayout`:
    `Data(vec2)@0 len 8`, `Data(pad)@8 len 8`, `SampledTex@16 len 4`,
    `BufAddr@24 len 8`, `Data(f32)@32 len 4`; assert the exact
    `(dst_offset, src)` list, the explicit padding `FrameBytes` step, and
    that bytes 20..24 are covered by no step
57. `assembly_data_only_schema`
58. `assembly_resource_only_schema` — `ResolveTex`/`ResolveBuf`/`External` in
    field order
59. `layoutless_schema_defers_assembly` → `AssemblyProgram::Deferred` (the
    lowered-graph path)
60. `dispatches_get_compute_sync_barrier_template` — three dispatches, each
    leaf `barrier_before == ComputeSync`
61. `raster_gets_compute_to_graphics_barrier_template`
62. `repeat_and_when_structure_survives_compile` — pass shapes preserved;
    `LeafAccess` unions correct (uniform + push bindings)

Expansion tests (`expand.rs`; each builds a small desc and runs
`validate → compile → expand`):

63. `zero_trip_repeat_emits_nothing_and_keeps_cursors`
64. `odd_repeat_flips_cursor_parity` — jacobi shape, count 3 → final cursor
    1, three steps
65. `even_repeat_restores_parity` — count 4 → final cursor 0
66. `cursor_parity_matches_tex_run_state_replay` — replay the same access
    sequence through a raw `TexRunState` by hand; every `ResolvedTex.phys`
    matches
67. `skipped_when_emits_nothing_and_keeps_cursors` — body has a write;
    `present[gate] = false`; cursors unchanged
68. `skipped_when_keeps_barriers_between_survivors` — dispatch A,
    When(B, skipped), dispatch C → steps `[A (barrier None), C (barrier Some(ComputeSync))]`
69. `first_executed_step_has_no_barrier` — also covers a graph whose first
    pass sits inside an executed `When`
70. `raster_only_graph_has_no_leading_barrier`
71. `input_run_state_is_not_mutated` — expand twice from one `RunState`,
    identical results
72. `mutate_does_not_advance_cursor_write_does`

## Task breakdown

Each task ends with the repo compiling and its listed verification green.
The table retains the original extraction sequence for historical context.
In the combined landing, tests replace blanket module-level dead-code allows
with `cfg_attr(not(test), allow(dead_code, reason = ...))` on tested migration
modules. Task 10 removes unused APIs but preserves scaffolding assigned to a
later phase.

| # | Task | Files | Verification |
| --- | --- | --- | --- |
| 1 | Create `desc.rs` with only `GraphFormat`; `mod desc; pub use desc::GraphFormat;`; change `GraphResources::texture` to take `GraphFormat` and store it; `impl ToVk for GraphFormat`; `.to_vk()` at the `create_storage_texture` call in `RenderGraph::new`; migrate watercolor lines 333-334 and its import | `desc.rs`, `render_graph.rs`, `examples/watercolor/src/main.rs` | `cargo check --workspace --all-targets && cargo test -p mltrs-renderer && cargo fmt` |
| 2 | Watercolor blur split (buffer + pipeline + node 7 + struct field). Must land before task 9 | `examples/watercolor/src/main.rs` | task-1 commands + `just sweep` (output unchanged) |
| 3 | Fill `desc.rs`: ID newtypes, tables, `SchemaTable` | `desc.rs` | `cargo check --workspace --all-targets && cargo fmt` |
| 4 | `validate.rs` part 1: `GraphError` + `Display`, `Analysis`, `validate()` with ID-range, per-command conflicts, Analysis derivation; tests 1–11 + the desc builder | `validate.rs`, `render_graph.rs` | `cargo test -p mltrs-renderer && cargo fmt` |
| 5 | `validate.rs` part 2: value kinds, binding shape, uploads, pass structure, draw/pipeline rules, rotation rule, `UnsupportedInPhase1` matrix; tests 12–18, 31–32, 44–55 | `validate.rs` | same |
| 6 | `compile.rs`: `CompiledGraph`, `compile()`, `build_assembly()`; tests 56–62 | `compile.rs`, `render_graph.rs` | same |
| 7 | `expand.rs`: move `TexRunState` out of `schedule.rs` (facade constructs it with `new(if needs_two { 2 } else { 1 })` until task 9; `schedule.rs` keeps `BuildCtx`); `RunState`/`FrameShape`/`ExpandedFrame`/`expand()`; tests 24–27 (moved), 63–72 | `expand.rs`, `schedule.rs`, `render_graph.rs` | same |
| 8 | `lower.rs`: `LowerCtx`, interners, scope machinery, `LowerOutput`; move `NodeAccess` here (`schedule.rs` switches to `use super::lower::NodeAccess;`); tests 19–23, 28–30, 33–43 | `lower.rs`, `schedule.rs` | same |
| 9 | The flip: `declare` → `lower` on `GraphNode`, all 8 node impls, tuple macro, `GraphPush::lower_input`, `collect_bindings`; rewrite `RenderGraph::new` per the facade section; verify every `schedule.rs` test appears in this inventory, then **delete `schedule.rs`** and `mod schedule;` | `render_graph.rs`, delete `schedule.rs` | `cargo check --workspace --all-targets && cargo test -p mltrs-renderer && just lint && just sweep && cargo fmt` |
| 10 | Cleanup: remove unused APIs; retain tested migration scaffolding with documented phase ownership and non-test dead-code allowances; review CLI fixture, template, and snapshot changes | new modules | full gates, as task 9 |

Task 9 is the only large task, and it is mechanical: every rule already
landed tested in tasks 4–8.

## Invariants

- Production execution retains `PlanCtx`, `BindingResolver`, staged writes,
  and the existing recording path, with the review fixes described in Scope.
  Cursors commit after successful submission. `compile()` and `expand()` have
  no callers outside tests.
- Generated-code-facing contracts retained: `GraphBinding`, `GraphBindingSet`,
  `GraphShaderParams`, the `BindingResolver` method set,
  `SampledTexBinding`/`StorageTexBinding`/`RawBufferBinding`/`BufferBinding*`.
  The combined landing implements and tests these contracts in `crates/cli`,
  including fixture, template, and snapshot changes.
- The pure-core extraction adds two public format API items: the
  `GraphResources::texture` format parameter and the `GraphFormat` export. `GraphNode`/`GraphPush` change but
  are sealed (their methods name pub-in-private types). Nothing from
  `desc`/`validate`/`compile`/`expand`/`lower` leaks through
  `pub use render_graph::*` except `GraphFormat`.
- Access-shape coverage: tests 42–43 check `analysis.tex_phys` against the
  specified watercolor and particles expectations. No independent `BuildCtx`
  implementation exists in this history for a parity comparison. Error
  messages are variant-listed and retain the specified key phrases.
- `just sweep` remains the visual regression gate. The blur split should
  preserve output; the separate blur-H `read_previous()` → `read()` review fix
  intentionally restores current-version sampling. Particles migrates as part
  of the combined 07 landing.
- New strictness with zero consumers, documented rather than hidden:
  `optional(repeat(..))`, nested `optional`, draws inside `optional`, and
  external storage-image bindings were constructible and are rejected after
  phase 1 (`NestedControlFlow`, `RasterInWhen`/S6, `MutableExternalImport`
  permanent). No example uses any of them.

## Ledger mapping

Phase-1 rejections map onto existing ledger rows in the parent document; no
new rows are required:

- S1 (3a): `GroupSourceValue`, and the un-lowered push data
  (`PushDesc.data = None`).
- S2 (3b): `BufferKind::Storage` compatibility kind and the
  upload-to-`Storage` allowance; both leave with the `Immutable` migration.
- S6 (5): `ColorAttachmentUsage`, `DepthAttachmentUsage`, `OffscreenTargets`,
  `MultipleRasterPasses`, `RasterInWhen`.
- S7 (5): `WindowSizeClass`.

Permanent rules, not ledger rows: `RasterInRepeat`, `UploadTargetKind`,
`MutableExternalImport`, `UniformSourceConflict`, the picking compatibility
checks.

## Resolved subchoices

Recorded so the implementer does not re-decide them:

1. Decision-6 timing: enforcement lands in phase 1 (see the phase-1 decision
   list). The parent document's decision 6 carries an annotation.
2. The blur fix requires a second pipeline, not only a second buffer: the
   pipeline's descriptors bind the uniform buffer at creation, and the
   pipeline/source identity check is decision 8 (phase 5).
3. `BufferDecl` deviates from the parent §1 (no `elem_schema`/`init`,
   optional capacity, temporary `Storage` kind); aligned in 3a/3b.
4. The nested-control-flow check lives in lowering: `PassDesc` bodies are
   `Vec<LeafPass>`, so `validate` cannot see nesting.
5. `When` gate representation: the first `ValueId` declared inside the
   optional scope, with every scope value `optional: true`; no new
   `ValueKind` variant.
6. `GraphBinding::Buffer` erases mutability; phase 1 derives conservative
   `BufAccess` from the buffer kind. Precise access arrives with phase-2
   field metadata.
7. Push data is not lowered (`PushDesc.data = None`); push assembly programs
   are fixture-only, like uniform ones.
8. `GroupSource::Value` rejection sits under S1; `RasterInWhen` sits under
   S6's "pass orders beyond compute followed by one unconditional main raster
   pass".
9. `PassAfterMainRaster` merges the "compute must precede every draw" and
   "repeat must precede every draw" checks; both ported tests match it.
10. No new accessors are needed for lowering: `PipelineIndex::raw()`
    (`crates/renderer/src/renderer/pipeline.rs:20-21`) and
    `BindlessHandle::to_raw()`
    (`crates/renderer/src/renderer/bindless.rs:29-31`) already exist.

## Gates

The combined landing requires these gates (status below):

```
cargo check --workspace --all-targets
cargo test -p mltrs-renderer
just shaders
just test
just lint
just sweep
cargo fmt --all --check
```

`just test` is required for the combined landing because CLI sources,
templates, fixtures, and snapshots changed. The shader, check, test, lint, and
formatting gates passed for cleanup commit `ff5b9e2`; that verification did not
rerun `just sweep`. Record visual sweep results separately from automated tests.
