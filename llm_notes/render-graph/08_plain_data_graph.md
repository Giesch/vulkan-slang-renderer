# Render Graph v2 — Typed Tuple API with an Erased Internal Graph

STATUS: IN PROGRESS — phases 1, 1b, and 2b are implemented, phase 2 is
partially implemented, and phases 3a–7 are not started. The implementation
status section below records where the code lives and what each phase,
decision, and ledger row has reached. Decisions 1–13 record the design
direction. Decisions 12–13 replace the earlier proposal to remove the tuple
API. Unresolved subchoices remain explicit and gate their owning phases.
The Roc target update dated 2026-09-10 takes precedence for validation timing,
schema fingerprints, and prototype portability.
The review update dated 2026-09-07 records later decisions and open questions.
Accepted changes in that update take precedence over earlier wording.
Annotations dated 2026-09-20 mark design text that the implementation
diverged from or has not reached.

## Implementation status — 2026-09-20

Verified against commit `008e173`.

### Code locations

The graph is its own crate. The renderer implements its backend traits.
Paths elsewhere in this document that name
`crates/renderer/src/renderer/render_graph/*.rs` describe the layout before
the extraction.

| Concern | Location |
| --- | --- |
| Typed node API, `RenderGraph`, `PreparedRenderGraph`, `PlanCtx`, `GraphShaderParams`, `BindingResolver`, slot keys, pipeline keys | `crates/render-graph/src/runtime.rs` |
| Plain-data `GraphDesc`, `SchemaTable` (`pub(crate)`) | `crates/render-graph/src/runtime/desc.rs` |
| `GraphError`, `UnsupportedFeature`, `validate()`, `extent_limit_errors()`, `validation_message()` | `crates/render-graph/src/runtime/validate.rs` |
| `LowerCtx`, typed lowering, interners, scope machinery | `crates/render-graph/src/runtime/lower.rs` |
| Pure `compile()` and `build_assembly()` (tests only) | `crates/render-graph/src/runtime/compile.rs` |
| `TexRunState`, pure `expand()` (`expand` tests only) | `crates/render-graph/src/runtime/expand.rs` |
| Backend contract: `GraphFormat`, `PhysicalImage`, `BufferAddressKind`, `BackendTypes`, `PreparationBackend`, `BindingLookup`, `FrameLookup`, `FrameBackend`, `IndexedIndirectArgs` | `crates/render-graph/src/backend.rs` |
| Output vocabulary: `CommandBatch`, `IndirectRequest`, `DrawCallConfig`, `PushConstantBytes` | `crates/render-graph/src/commands.rs` |
| `Addr`, `ReadAddr`, `ImmutableAddr`, `BindlessHandle`, `GPUWrite`, `PushConstantBlock` | `crates/render-graph/src/{addr,bindless,lib}.rs` |
| Test-only desc builder and fake backend | `crates/render-graph/src/runtime/{test_desc,backend_tests}.rs` |
| Dependency-closure test (no ash, vk-mem, SDL, shader-slang, renderer) | `crates/render-graph/tests/dependency_boundary.rs` |
| Crate contracts | `crates/render-graph/AGENTS.md` |
| Renderer backend impls, `validate_uploads`, `validate_indirect_layout` | `crates/renderer/src/renderer/graph_backend.rs` |
| Renderer facade: re-export, `From<&Handle>` for slots and keys, `ToVk for GraphFormat` | `crates/renderer/src/renderer/render_graph.rs` |
| `DrawIndexedIndirectCommand` | `crates/renderer/src/renderer/indirect.rs` |
| Graph split codegen (`classify_graph_field`, `graph_split_def`) | `crates/cli/src/build_tasks.rs`, `crates/cli/templates/graph_split.rs.askama` |
| Stub renderer for generated-code checks | `crates/cli/fixtures/check_crate/src/renderer/render_graph.rs` |
| Public API compile harness | `crates/renderer/tests/render_graph_api_compile.rs`, `crates/renderer/fixtures/api_compile` |
| Reference docs | `docs/render_graph.md`, `docs/testing.md` |

### Lifecycle

- `RenderGraph::new(resources, nodes)` lowers the typed nodes and runs
  `validate`. It needs no renderer. It reports every logical error in one
  `validation_message`.
- `graph.prepare(&mut renderer)` checks texture extents against
  `max_image_dimension_2d`, allocates one `PhysicalImage` per physical image
  through `PreparationBackend::prepare_image`, and requires
  `N: CompatibleWith<B>`. The sealed proof checks the backend's exact
  indirect record type recursively through tuples, arrays, repeat, and optional.
- `PreparedRenderGraph::execute(frame, &N::Frame)` checks every captured
  buffer slot for liveness, builds `PlanCtx`, runs `GraphNode::plan`, and calls
  `FrameBackend::submit(batch, on_submitted)`.
- `FrameRenderer::submit` validates every upload destination and indirect
  range before it queues work. A rejected batch returns `DrawError` before any
  write, submission, or commit. Staged bytes are `MaybeUninit<u8>`.
- `Renderer::draw_frame` calls `on_submitted` immediately after
  `queue_submit2`, before it advances the flight slot and before
  `queue_present_khr`. `on_submitted` installs the proposed texture cursors.

Production execution is the `plan()` path. `compile()` and `expand()` have
no callers outside their own test modules. `runtime.rs` marks `mod compile`,
`mod desc`, `mod expand`, and `mod lower` with
`cfg_attr(not(test), allow(dead_code, reason = ...))`; each reason names
phase 3a. Every production `SchemaDesc` has `layout: None`, so
`build_assembly` returns `AssemblyProgram::Deferred`. Execution assembles GPU
structs through the generated `GraphShaderParams::assemble_input`.

### Phase status

| Phase | Status | Record |
| --- | --- | --- |
| 1 | Implemented | [`08_plain_data_graph/01_pure_core.md`](08_plain_data_graph/01_pure_core.md), [`01_review.md`](08_plain_data_graph/01_review.md) |
| 1b | Implemented | [`01b_simplification.md`](08_plain_data_graph/01b_simplification.md) |
| 2 | Partially implemented: P2.1 harness and part of P2.4 (`.with_push_constant`, `.with_param_bindings`, `GraphShaderParams::Input`). P2.2, P2.3, P2.5, P2.6 and typed list insertion are not started | [`02_schemas_and_compile_checks.md`](08_plain_data_graph/02_schemas_and_compile_checks.md) |
| 2b | Implemented outside the phase plan: backend-neutral crate, backend traits, new/prepare/execute lifecycle, sealed `CompatibleWith`, `IndirectDrawNode`, upload and indirect preflight, `[N; K]` node arrays, 11 everyday examples migrated | [`02b_backend_neutral_crate.md`](08_plain_data_graph/02b_backend_neutral_crate.md) |
| 3a, 3b, 3c | Not started | — |
| 4 | Not started. `toon_link` uses the manual `FrameRenderer` API in both rendering modes | — |
| 5, 6, 7 | Not started | — |

### Ledger status

Every row S1–S8 is open. `UnsupportedFeature` in `runtime/validate.rs`
still has all seven phase-1 rejection variants, and `docs/render_graph.md`
lists them. `BufferKind::Storage` and the upload-to-`Storage` allowance still
exist; watercolor's stroke points use them (S2). Fixed-length `[N; K]` node
arrays exist; setup-length draw lists do not (S4).

### Consumers

- Graph API (13 of 17 examples): `particles`, `watercolor`, `basic_triangle`,
  `depth_texture`, `dragon`, `koch_curve`, `multi_mesh` (`[DrawNode<_>; 18]`),
  `ray_marching`, `recipes`, `sdf_2d`, `serenity_crt`, `suzanne`, `viking_room`.
- Manual `FrameRenderer` API (4): `toon_link` (the phase-4 target; 24 batches,
  24 materials, 5 pipelines, 7 indexed-indirect runs), `gpu_picking` (the
  deferred picking path; `PickingNode` exists and has no consumer),
  `space_invaders`, `sprite_batch`.
- Roc: `crates/cli/src/roc_codegen.rs` emits shader reflection only
  (`docs/roc_shader_codegen.md`). No Roc graph API exists.

### Landed outside the phase plan

- The `mltrs-render-graph` crate and its five backend traits.
- The GPU-free `new` → `prepare` → `execute` lifecycle and
  `PreparedRenderGraph<N, B>` with backend-owned keepalives.
- Sealed `CompatibleWith<B>` and `IndirectDrawNode<S, P, I>` over an opaque
  `DrawIndexedIndirectCommand`.
- Upload and indirect preflight in `FrameRenderer::submit`.
- `[N; K]` as a node with frame `[N::Frame; K]`.
- `ImmutableBufferBinding<T>` into `ReadBufferBinding<T>`.
- Migration of the 11 everyday examples.
- Roc shader reflection codegen, without a graph split or graph API.

### Related records

- `llm_notes/tech_debt.md` items 17 (picking as a second rendering path),
  18 (copied slot keys and per-execute liveness checks), and 19
  (`UploadNode::Frame = Vec<T>`).
- `docs/render_graph.md` is the reference for the public API.
  `docs/testing.md` describes the crate tests, the fake backend, and the
  compile harness.

## Roc target update — 2026-09-10

The product path is a Rust prototype followed by a Roc platform. Rust traits and
lifetimes are implementation tools, not requirements for the portable API.
The detailed phase-2 plan records compiler evidence and implementation gates.

- Keep the typed facade and private plain-data graph. Roc construction, validation,
  and device-independent compilation should evaluate at compile time when all
  inputs are known. Rust runs the corresponding pure work during setup.
- Port the pure algorithms to Roc for constant evaluation. Do not assume the Roc
  evaluator can invoke the existing Rust validator through a hosted function.
  Keep shared semantic fixtures so the implementations can be compared.
- Keep the compiled description free of live handles and retained builder callbacks.
  Resolve logical resource declarations to live resources during platform setup.
  Keep device limits, actual pipeline/uniform identity, generations, allocation,
  and callback-discovered dependencies at the boundary where those inputs exist.
- Do not add mandatory layout fingerprints or per-frame type checks. Typed execution
  supplies complete inputs. Generated layout metadata, interface comparison,
  layout assertions, and assembly tests remain required. Cache fingerprints are
  a possible future optimization with a separately defined artifact contract.
- Check uniform-source ownership with pure validation. Reject missing sources,
  independent duplicate sources, and consumers outside their source scope.
  Roc can run these checks during constant evaluation. Rust runs them at setup.
  Do not require generative lifetimes or borrow checking in the portable API.
  Lack of mutable capture does not by itself prove source ownership: ordinary
  returned values and immutable reuse still need a type/API proof or validation.
- Keep dynamic frame values and their numeric/range checks at runtime. A constant
  graph does not make repeat counts, uploads, dispatch counts, or device state constant.
- Roc scene structure is a build input. Import and parse assets such as JSON in
  Roc to determine draw runs and material counts. This layer does not change at
  runtime. Rust phase 4 keeps its setup-defined lists as the prototype equivalent.
  GPU allocation and resolution of logical references still occur during setup.
- The future Roc platform API will use encoders/decoders for a custom data format.
  Construction of a heterogeneous typed execution tuple remains unexplored future
  design work. Do not design or implement that Roc API in the Rust phase-2 work.
- The Roc host ABI is a separate representation boundary. Do not reinterpret Roc
  records/lists as Rust structs or GPU data. Use explicit platform glue and typed
  packing. Layout metadata describes GPU assembly, not the Roc host ABI.

This update does not implement a Roc platform or settle scalar/array storage,
callback dependency discovery, resize, or graph rebuild. Their existing gates remain.

ANNOTATION (2026-09-20): Roc shader reflection codegen exists
(`crates/cli/src/roc_codegen.rs`, `docs/roc_shader_codegen.md`). It emits
reflection values only. The Roc graph API, the port proof, and
constant-evaluation validation are not implemented. The crate extraction
recorded in `08_plain_data_graph/02b_backend_neutral_crate.md` is the
boundary a scripting platform would sit behind.

## Core restrictions and direction

- The API must prevent omission of required frame data through types.
  Keep `GraphNode::Frame`, tuple composition, and generated complete binding structs.
  Do not expose incremental `FrameValues::set` or a handwritten input mapping.
- Permit dynamic contents inside fixed node types. Toon_link needs setup-length
  lists of indexed indirect draws with shared frame data and per-run pointers.
  Multiple raster nodes must support shadow mapping. Future AAA features must
  preserve the typed input contract, but are not all implementation requirements now.
- Target Roc through a Rust API prototype, followed by a Roc platform port.
  Prove the node/input relationship with Roc compile fixtures before the port.
  Roc constant evaluation runs pure graph validation for compile-time-known graphs.
- Validate Rust graphs during startup/setup, not by evaluating graphs during
  Rust builds. Put every applicable restriction into the public types first.
  Use setup validation for relationships that types cannot enforce.
- Lower typed builders into a private, erased, plain-data `GraphDesc`.
  Pure validation, compilation, and expansion operate on that representation.
  Neither language exposes unchecked graph construction or mutation.

This plan extends `07_graph_api_plan.md` rather than replacing its typed inputs.
The earlier key/value API direction is superseded. Runtime-length lists do not
require erasing the outer graph type when each list has a fixed input contract.
Keep the existing build-once structure, access vocabulary, version cursors, and
staged writes. Add the pure core beneath the typed facade.

Current dynamic scope is Toon_link's setup-defined material runs and the existing
particles/watercolor inputs. Add multiple raster nodes and attachment handling
for shadow mapping and interleaving. GPU-produced draw counts, indirect dispatch,
arbitrary per-element frame contracts, graph rebuild, and complex AAA scheduling
remain future work. No new indirect-count feature enable is required here.

Picking redesign and migration of `examples/gpu_picking` are deferred.
Preserve the existing picking path and any compatibility adapter needed by the
current tuple API. Do not add generic readback or graph-owned picking in this plan.

Conservative barriers land with each supported feature. Barrier optimization
remains a later phase. `06_derived_barriers.md` provides background, but its
last-writer-only analysis is insufficient for this access model.

Validation gaps in the tuple API that this design must close:

- Two nodes that stage different contents into one uniform slot silently
  clobber each other. Current watercolor has moved blur-specific values
  into push constants; its shared blur uniform now contains identical
  data. Toon_link also intentionally shares one frame uniform. The rule:
  one assembly source per uniform slot, any number of consumers (§2).
- A skipped writer or a texture with no version-producing writer can
  leave reads using retained or initially cleared contents. Retained
  contents are treated as intended; initialization assurance is a
  follow-up, per decision 1.
- `GraphTex` is a bare index, not tied to its `GraphResources`; a token
  from another resource set aliases silently when the index is in range.
  Decision 4 closes this with graph-identity tokens.
  ANNOTATION (2026-09-20): open. `GraphTex` is a bare `u32` minted by
  `ResourcePlanner::texture`, and buffer slot keys are bare indices.
- Execute-time expansion (`PlanCtx`) holds `&Renderer`, so repeat/optional
  expansion is not unit-tested.
  ANNOTATION (2026-09-20): superseded. `PlanCtx` reaches the backend through
  `BindingLookup` and `FrameLookup`, and the fake backend in
  `crates/render-graph/src/runtime/backend_tests.rs` prepares and executes
  graphs without Vulkan. The pure `expand()` path remains unwired.

## Design

### 1. `GraphDesc` — the private canonical structure

Pure modules under `crates/renderer/src/renderer/render_graph/`:
`desc.rs`, `validate.rs`, `expand.rs`, `compile.rs`. No `ash` types and no
live renderer handles or indices in `desc.rs`/`validate.rs`/`expand.rs`;
formats become a graph-local enum the renderer maps, as
`mltrs-slang-reflection` does for `ShaderStage`. This keeps the validator
portable to the scripting language's const evaluator.

ANNOTATION (2026-09-20): the pure modules live under
`crates/render-graph/src/runtime/` in the `mltrs-render-graph` crate. All
`desc.rs` items are `pub(crate)`. `tests/dependency_boundary.rs` checks that
the crate's resolved dependency closure contains no renderer, ash, vk-mem,
SDL, or shader-slang package.

The description is a set of declaration tables plus an upload table and an
ordered pass list. Every ID type (`TexId`, `BufferId`, `ImportId`,
`ValueId`, `UniformId`, `PipelineId`) is an index into its table. Rust
builder resource keys carry `(graph_id, resource_id, generation)` as their identity.
The typed key identifies the resource kind and data interface.
The builder checks ownership and generation before lowering each key to a local table ID.
Live setup metadata retains the identity needed to check pipeline bindings.
Both languages construct graphs
through typed builders. `GraphDesc` is an internal plain-data representation,
not a public construction API. Scripts use description-local symbols through
their builder. No live handle appears in the
description. `RenderGraph::new(renderer, resources, nodes)` lowers the typed
nodes and collects live setup inputs internally. Setup verifies each live
pipeline, uniform, and import against its declaration. The tables below are
internal compiler structures, not a public builder API.

ANNOTATION (2026-09-20): the three-part key identity is not implemented.
Slot keys (`UniformSlot`, `StorageSlot`, `GpuOnlySlot`, `ImmutableSlot`,
`SingletonSlot`) and `GraphTex` carry a bare index. `execute` re-checks each
captured slot for liveness every frame instead (`tech_debt.md` item 18).
Construction is two steps: `RenderGraph::new(resources, nodes)` needs no
renderer; `graph.prepare(&mut renderer)` supplies device limits and
allocation and returns the only type with `execute`. No setup check compares
a live pipeline's uniform identity with the graph's source (decision 8).
The `BufferDecl` in the tree has `capacity: Option<u32>` and
`elem_size: Option<u32>`, no `elem_schema`, no `init`, and a fourth kind
`Storage`. Imports are external sampled textures bound through
`BindlessHandle<Sampler2D>` converted into `SampledTexBinding`.

```rust
struct GraphDesc {
    textures:  Vec<TexDecl>,
    buffers:   Vec<BufferDecl>,
    imports:   Vec<ImportDecl>,
    values:    Vec<ValueDecl>,
    uniforms:  Vec<UniformDecl>,
    pipelines: Vec<PipelineDecl>,
    uploads:   Vec<UploadDesc>,
    passes:    Vec<PassDesc>,
}

struct TexDecl { name: String, format: GraphFormat, size: SizeClass, usage: TexUsage }
enum SizeClass { Fixed(u32, u32), Window, WindowDiv(u32) }
enum TexUsage  { Storage, Color, Depth }

struct BufferDecl {
    name: String,
    kind: BufferKind,           // GpuOnlyFlight | Singleton | Immutable
    elem_schema: SchemaId,
    capacity: u32,
    usage: BufUsage,            // Storage | StorageIndirectArgs
    init: BufferInit,           // Zeroed | Provided — bytes supplied at build (decision 1)
}

struct ImportDecl { name: String }  // read-only sampled texture, bound at build (decision 2)

struct ValueDecl { name: String, kind: ValueKind, optional: bool }
enum ValueKind {
    Count,                      // u32: repeat trip counts
    Groups,                     // [u32; 3]: dispatch group counts
    Bytes { schema: SchemaId }, // typed uniform/push data lowered internally
    Array { elem: SchemaId, max_len: u32 },  // upload contents; provisional under decision 3
}

struct UniformDecl { name: String, schema: SchemaId, source: UniformSourceDesc }
struct UniformSourceDesc {
    data: Option<ValueId>,
    bindings: Vec<(FieldKey, ResourceRef)>,
}
struct PipelineDecl { name: String, kind: PipelineKind, params: SchemaId, push: Option<SchemaId> }
enum PipelineKind   { Compute, Graphics }

struct UploadDesc { name: String, buffer: BufferId, value: ValueId }

enum PassDesc {
    Leaf(LeafPass),
    When   { name: String, value: ValueId, body: Vec<LeafPass> },
    Repeat { name: String, count: ValueId, body: Vec<LeafPass> },
}

enum LeafPass {
    Compute(DispatchDesc),
    Raster(RasterDesc),
}

struct DispatchDesc {
    name: String,
    pipeline: PipelineId,
    uniform: UniformId,
    groups: GroupSource,        // Fixed([u32; 3]) | Value(ValueId); indirect dispatch is deferred
    push: Option<PushDesc>,     // per-command push data and bindings
}

struct RasterDesc  { name: String, targets: RasterTargets, draws: Vec<DrawDesc> }
enum RasterTargets { Main, Offscreen { color: Vec<TexId>, depth: Option<TexId> } }

enum ResourceRef {
    Tex(TexId, TexAccess),      // Read | ReadPrevious | Write | Mutate
    Buf(BufferRef, BufAccess),  // Read | Write | Mutate | IndirectArgs
    External(ImportId),
}

struct BufferRef { buffer: BufferId, slot: SlotSel, offset: u32, range: Option<u32> }
enum SlotSel     { Current, Previous }  // flight-slot selection; Singleton/Immutable use Current
```

- Every table entry carries a string name. Names feed validation errors,
  debug labels (RenderDoc navigability), and the script FFI's name
  resolution. Rust call sites use generated binding structs and typed tokens, not names.
- **Values.** Internal value IDs come from typed node lowering. Tuple inputs
  populate all required values automatically. `OptionalNode<B>` takes
  `Option<B::Frame>`: `Some` requires the complete body input, and `None`
  explicitly disables it. Optional presence is not an independently set flag.
- **Uploads** remain an internal table, not GPU passes. Typed upload components
  contribute array inputs to the frame tuple. An optional brush group contains
  both upload data and dispatch data in one `Option<Body::Frame>`.
  Lower its upload to the table with the same presence condition as the dispatch.
  Every upload applies after the flight-slot wait, before GPU execution.
  Tuple position does not create an upload snapshot. An absent group skips its
  upload. A shorter upload does not clear the tail. Overlapping staged bytes
  retain the existing last-staged-write policy.
  Frame uploads target `Immutable` buffers in the current v2 scope.
  The CPU writes only the current flight slot after its wait.
  The GPU cannot write these buffers. `GpuOnlyFlight` and `Singleton`
  buffers reject frame uploads. Setup initialization remains permitted.
- **Control flow is flat.** Nested runtime control remains unsupported.
  Tuple nesting for organization is not runtime control. Typed optional groups
  may contain upload inputs, but lowering never emits uploads inside GPU loops.
  Preserve legacy picking restrictions in its compatibility path. Do not claim
  that an internal `Vec` makes multiple picking operations unrepresentable.

- **Imports** (decision 2) are read-only sampled textures bound at build
  and unchanged while the graph lives. This includes textures reached only
  through GPU tables (toon_link's material textures): they are declared as
  imports without a binding field, so the graph tracks their access and
  keeps them alive. GPU-writable resources are graph-owned buffers or
  textures, never imports.
- **Buffers** keep storage kind, flight-slot selection, offset/range, and
  access as separate concepts in `BufferRef`. Texture version cursors are
  not buffer flight-slot selection. `BufferInit` covers particles' seeded
  particle buffer and toon_link's setup-uploaded args and tables
  (decision 1): creation and initial contents are one declaration.
  Watercolor's brush points migrate from ordinary storage to `Immutable`.
  The current v2 scope does not add an ordinary storage-buffer kind.
- `DrawDesc` carries the existing draw forms and Toon_link's indexed indirect
  runs with per-run push bindings. Each draw names its pipeline, shared uniform
  through `UniformId`, and push data/bindings. `UniformDecl` owns the uniform source.
  Commands do not define duplicate uniform sources.
  `DrawList<Params, Push>` owns an internal
  `Vec<DrawDesc>` whose length is fixed during setup. Its frame contract stays
  fixed when runs are added. List elements cannot add required frame fields.
  Pipelines may differ while retaining compatible typed params/push interfaces.
  Use a tuple of lists for different interfaces. See decision 12 for examples.
  GPU-produced indirect counts and indirect dispatch are deferred.

- **Graph rebuild is unsupported in v2.** A renderer session builds its
  graph once. CPU-driven structural changes (a new scene's draw lists) and
  script reload both need rebuild; rebuild support waits for the scripting
  milestone, its first real consumer. Rebuild requires leak-free graph
  teardown (image removal and descriptor-slot release, which the current
  stores do not support) and a key-stability contract across rebuilds;
  neither is designed here. Window resize is not a rebuild: `Window`
  textures recreate in place (§5).

### 2. Validation — one pure function, all errors

`validate(&GraphDesc, &SchemaTable) -> Result<Analysis, Vec<GraphError>>`.
It collects every error and does not stop at the first: script tooling and
startup diagnostics both want the full list. `GraphError` is an enum with
`Display`, not `anyhow`.

Both languages use types to enforce restrictions that their types can express.
The scripting language is strongly typed and supports build-time evaluation
of pure code. Validation checks relationships that the types cannot enforce.
Examples include resource ownership, overlapping accesses, ranges, and shared uniforms.
Neither language exposes unchecked `GraphDesc` construction. See decision 10.

Validation is a required setup step: the Rust `RenderGraph::new` runs it
before compiling or allocating graph-owned GPU resources in the initial subset.
Validation of dependencies discovered by setup callbacks is deferred to a follow-up task.
That task must define the validation and compilation sequence before phase 4 enables decision-9 callbacks.
S5 remains open until that work and its tests are complete. When scripting
is adopted, script build/const evaluation runs the same logical checks on
the plain description and schema metadata. Device-specific checks still
run during renderer setup. Typed execution prevents missing required inputs. Runtime checks still validate
array lengths, counts, limits, and the internal adapter invariants.
Additional validations for parallel execution are outside this plan.

ANNOTATION (2026-09-20): `RenderGraph::new` runs `validate` with no
renderer. The device check (`extent_limit_errors` against
`maxImageDimension2D`) runs in `prepare`, before allocation. `GraphError`
has 28 variants: the 25 listed in `01_pure_core.md` plus
`PrevReadWithoutWrite`, `TextureExtentZero`, and `TextureExtentTooLarge`.
Upload payload sizes and indirect ranges are checked again at execute time
by `FrameRenderer::submit` against live buffer metadata.

Checks ported from the tuple API:

- undeclared or out-of-range ID, for every table
- conflicting duplicate writes within one command
- mutate+read, mutate+write, write+`read_previous` of one texture within
  one command; ordered draws use decision 7 rather than pass-wide rejection
- repeat-body uniform referencing a texture that rotates in the same
  repeat
- existing picking rules in the unchanged compatibility path

New checks:

- declaration tables: check each `ValueId` against its declared kind.
  `Repeat` uses `Count`. `GroupSource::Value` uses `Groups`.
  Data and uploads use their declared `Bytes` or `Array` schemas.
  Check schema references and compare the required interface metadata during setup.
  Schema IDs are local indices, not runtime type fingerprints. A `When` gate must be optional.
  Check declaration use under the supported subset.
  Allow a uniform source with no consumers, including a source owned by an empty draw list.
  Still check its resource identities, schemas, and complete bindings.
  References from that source count as declaration uses for the resources it retains.
  They do not imply GPU accesses without an executed consumer.
- uploads: check the target capacity and element schema.
  The value kind must be `Array` or `Bytes`.
  The target kind must be `Immutable`. The target slot must be current.
  Reject frame uploads to `GpuOnlyFlight` and `Singleton` before writes.
- one assembly source per uniform slot: each `UniformId` has exactly one
  (data value, bindings) source; any number of dispatches and draws may
  consume it; a second differing source is an error. All active consumers
  must resolve each binding to the same physical resource within one execution.
  Validation checks this rule across texture writes, conditional paths, and
  repeats. Reject sharing if validation cannot establish this property for
  every permitted execution shape. Equal logical references alone are insufficient.
  Setup also rejects different `UniformId`s bound to the same live uniform slot.
  The validator does not attempt to prove two distinct runtime values equal.
  See decision 6 for the reason and alternatives at call sites.
- binding completeness and kind match: every resource field in the
  shader's schema is bound exactly once; a sampled-tex field gets a
  sampled ref; a buffer-pointer field's mutability matches the ref's
  access
- pipeline interface: a pass's pipeline kind matches the pass kind; the
  pipeline's params schema matches the uniform it consumes; push schema
  matches the push desc. Each graphics pipeline has one fixed attachment
  configuration. Check color formats/count, depth/stencil format, and sample
  count against its raster targets. Verify live pipeline metadata at setup.
  Check that the pipeline's uniform identity equals the graph source's uniform identity.
  Compare `(graph_id, resource_id, generation)`. Equal schemas do not replace this check.
  See decision 8.
- indexed indirect draws: validate Toon_link's setup-initialized argument
  buffers, ranges, stride, offset alignment, and draw counts. Track argument
  reads and per-run table reads. GPU-produced argument/count support is deferred.

- device limits: validate known counts and ranges against device limits at
  setup; validate dynamic CPU-supplied values on input.
- raster pass rules: a pass does not sample its own target; targets share
  a size class; depth ops only with a depth target; the final pass targets
  the main output

`Analysis` carries what compile needs: physical image counts (the
`needs_two` rule, generalized), access/hazard templates instantiated for the
executed shape and physical resources, and the value table.

**Initialization scope (decision 1).** Skipping a `When` body or running a
repeat zero times leaves existing resource contents and version cursors
intact. Resource creation and initial contents are one declaration
(`BufferInit`, cleared graph textures); there is no definite-initialization
analysis and no automatic clearing of retained state in this plan. An
earlier producer's presence is not proof that it has run. If an example is
found to consume uninitialized contents during implementation, stop
implementation and raise the affected resource and execution path to the
user before proceeding. See the follow-up at the end of this document.

### 3. Compile and execute

`compile(desc, analysis) -> CompiledGraph`: a flat step program.

- Per uniform source and per command's push block, an **assembly program**: a precomputed list of
  `(dst_offset, src)` where `src` is `FrameBytes { value, src_offset, len }`,
  `ResolveTex(TexId, mode)`, or `ResolveBuf(BufferRef)`.
  Execute-time assembly is memcpy plus handle/address writes — no
  reflection, no string lookups per frame.
  When a source's owning node executes, assemble and stage its uniform even if it has no consumers.
  Apply the normal flight-slot wait and frame-input checks.
  Preserve existing optional and repeat behavior when the owning node does not execute.
  Assembly alone does not execute shader accesses or advance texture version cursors.
  Skipping unused uniform assembly or uploads is a possible later optimization, outside this plan.
- Barrier slots between executed steps and at frame boundaries. A skipped
  `When` or zero-trip repeat must not remove synchronization needed by
  surviving steps. The conservative policy grows with phases 3–5 as
  specified below. Phase 6 optimizes that already-correct policy; it is
  not the first phase that makes new command types safe.
  Ordered draws use the same dependency policy as compute steps. The graph
  inserts required barriers and attachment layout transitions. It does not
  reject a dependency merely because its producer and consumer are draws.
  See decision 7 for rendering boundaries and attachment preservation.
- Expansion (repeat counts, `When` presence, version cursors) is pure:
  `expand(&CompiledGraph, &RunState, &FrameShape) -> ExpandedFrame` in
  `expand.rs`. `ExpandedFrame` contains the step list and proposed next
  state. Reuse `TexRunState`'s cursor arithmetic, not the old executor's
  commit-before-submit behavior.
- Commit proposed state only after successful queue submission. Acquisition
  out-of-date, recording failure, or failure to submit discards it. A
  successful submit commits even if presentation later fails or recreates
  the swapchain. The internal submit path reports submission before presentation.
  The graph commits at that boundary. The final presentation result does not control the commit.
  Resized resource state is reset or
  reinitialized separately; attachment/history policy is decision 5.
  Use option A from the review: commit immediately after successful submission, before presentation or resource replacement.
  The commit installs prepared state without a new fallible operation.
  Apply any replacement reset after that commit.
  Acquisition aborts discard proposed state but still apply any required replacement reset.
- V2 does not impose an expansion budget for repeat counts.
  Applications choose practical counts and test their cost.
  A limit for expanded command counts or memory use is outside this plan.

ANNOTATION (2026-09-20): `compile()` and `expand()` exist and are tested in
`crates/render-graph/src/runtime/{compile,expand}.rs`, and nothing outside
their test modules calls them. Production execution is `GraphNode::plan`
through `PlanCtx`. Every production schema has no layout, so
`build_assembly` returns `Deferred`; execution assembles GPU structs through
the generated `assemble_input`. The commit rule is implemented:
`PreparedRenderGraph::execute` passes the cursor install as `on_submitted`,
and `Renderer::draw_frame` calls it after `queue_submit2` and before
presentation. Acquisition skips and pre-submit errors do not call it.

#### Synchronization policy by feature phase

Use the existing single ordered GPU submission. Account for read-after-write
(RAW), write-after-write (WAW), and write-after-read (WAR) dependencies.
Resolve logical versions and flight slots to physical resource identities;
two aliases of one allocation must share hazard state. Cross-frame state
must cover physical resources still used by earlier submissions.

- **Phase 3:** preserve conservative compute-to-compute and
  compute-to-graphics synchronization, the existing picking/readback and
  presentation transitions, and cross-frame ordering. Include prior reads
  before overwrites, including particles' previous-slot reads and
  watercolor's ping-pong reuse. CPU uploads retain the apply-after-wait
  mechanism and host-write visibility requirements.
- **Phase 4:** track indexed indirect argument reads at
  `DRAW_INDIRECT / INDIRECT_COMMAND_READ` and shader reads of per-run tables.
  Preserve host-upload visibility for setup-initialized arguments. Include these
  reads in hazard analysis for physical reuse. GPU-produced arguments/counts and
  indirect dispatch are not phase-4 requirements.

- **Phase 5:** add graphics-to-compute dependencies and dependencies
  between raster passes. Cover shader accesses, color attachment accesses
  at `COLOR_ATTACHMENT_OUTPUT`, and depth/stencil accesses at
  `EARLY_FRAGMENT_TESTS | LATE_FRAGMENT_TESTS`. Include attachment reads
  from load/blend/depth operations, subsequent sampled reads, and reuse
  after reads. Emit appropriate color/depth image layout transitions at
  rendering boundaries. Extend cross-frame scopes to these accesses, and
  preserve resolve/blit, picking/readback, egui, and presentation ordering.
- **Phase 6:** track last writers and outstanding readers, with stage/access
  scopes, instead of copying 06's last-writer-only algorithm. Instantiate
  hazards for the executed shape and resolved physical resources, including
  loop back-edges and cross-frame reuse. Barrier elision must preserve all
  of the above dependencies. Imports are read-only (decision 2); elision
  never assumes an unknown handle has no dependencies.

The [Vulkan synchronization examples](https://docs.vulkan.org/guide/latest/synchronization_examples.html)
illustrate the relevant compute, indirect, attachment, and WAR cases.

Typed frame inputs:

```rust
// Public relationship retained from the current tuple implementation.
trait GraphNode {
    type Frame;
}
// For node tuples: (A, B)::Frame = (A::Frame, B::Frame).
// OptionalNode<B>::Frame = Option<B::Frame>.
// RepeatNode<B>::Frame = (LoopCount, B::Frame).
// Implemented 2026-09-17: [N; K]::Frame = [N::Frame; K], no length cap.

// Illustrative: complete inputs are required at the execute call.
graph.execute(frame, &(compute_data, raster_data))?;
```

- Stock/generated adapters lower the complete tuple into private input storage.
  Users do not write another field-to-value map. Missing tuple elements and
  incomplete generated binding structs must fail compilation.
- Shared uniform scopes own one typed data input. Their consumers reference
  that source without adding duplicate data inputs. The draw list owns its
  shared uniform source internally. Decision 6 still checks physical selections.
- `Some` requires all optional-body inputs. Array contents remain variable-length
  data, not a variable list of required bindings. Do not add arbitrary per-draw
  frame vectors that require matching a setup list's length.
- Validate remaining dynamic constraints before acquisition or writes.
  Invalid values cause controlled shutdown under decision 11. Internal checks
  detect adapter bugs; they do not replace public type-level completeness.
- Scripts use the same typed input relationship. Any backend byte ABI stays
  internal and checks schemas/lengths. Do not expose string-keyed frame assembly.
- Copy/borrow, padding-safe byte storage, and array representation remain
  decision-3 subchoices. Settle them before their owning phases.

### 4. Codegen changes (`crates/cli`)

- Keep generated `Params`, `*ParamsBindings`, `*ParamsData`, `GraphShaderParams`,
  and `GraphBindingSet`. Do not delete the tuple fixture surface.
- Add schema metadata and internal assembly mappings. Do not require layout hashes.
  Cover resource kinds, nested layouts, and data-to-GPU offsets.
- Use generated complete binding structs at call sites. Internal field IDs
  support lowering; public field-key/value-key mapping is not required.
- Preserve the existing public data type names during migration. Binding-free
  params may use the params struct itself as `Data`; both paths share metadata.
- Extend compile-check fixtures with positive and negative typed API cases.
  Codegen remains additive while the executor changes beneath the facade.

ANNOTATION (2026-09-20): `graph_split.rs.askama` emits, per params or push
type, `*Data` (only when the type has both data and binding fields),
`*Bindings` (only when it has binding fields) with `GraphParamBindingSet` and
`GraphBindingSet` impls, `*Input` (always), the `GraphShaderParams` impl, and
`GraphBindingSet` for `*Input`. Degenerate forms: `Data = Self` with no
bindings, `Data = ()` with no data fields, `Bindings = ()` with no bindings.
Push-constant types are in the params set, so a push block with a resource
field gets a split; types reached only through `ImmutableAddr` do not. No
layout metadata, offsets, sizes, or field tables are emitted, and no
fingerprint code exists. The `check_crate` stub mirrors the crate traits by
hand. The compile harness in `crates/renderer/tests/render_graph_api_compile.rs`
holds 15 positive and 23 negative cases against the real renderer.

### 5. Renderer changes

- Raster passes: `record_command_buffer` gains graph-target rendering —
  begin/end dynamic rendering per `Raster` pass, with attachment transitions in the house sync2 style.
  Color targets use `COLOR_ATTACHMENT_OPTIMAL` for attachment access.
  Later shader access uses `SHADER_READ_ONLY_OPTIMAL` or `GENERAL`, as applicable.
  The existing main depth target uses `DEPTH_STENCIL_ATTACHMENT_OPTIMAL`.
  Preserve that main depth path in phases 1–4.
  Phase 5 adds stored depth, sampled depth, and preservation across internal rendering boundaries.
  The review update records the current depth behavior and the accepted deferral.
  Storage textures keep the
  stay-in-`GENERAL` rule; only `Color`/`Depth` textures get layout edges,
  and those edges live in the compiled barrier slots.
- Compute/draw interleaving: graph submission uses one ordered
  pending-step list instead of the `pending_compute`/`pending_draws`
  split. The manual `FrameRenderer` API keeps its two-phase behavior.
- Reuse indexed indirect commands for Toon_link. Do not enable `drawIndirectCount`
  or add indirect dispatch for this plan. Their GPU-driven smoke cases are deferred.
- Keep renderer-owned picking and readback behavior. General graph readback,
  picking resource ownership, and the picking-example migration are future work.

- Setup initialization (decision 1): one renderer call creates a resource
  and supplies its initial contents (zeroed or provided bytes) across
  every applicable backing slot/version, replacing separate
  allocate-then-write sequences in the examples.
- Shader hot reload: keep `assert_shader_interface_unchanged`'s rejection
  of interface changes; permit code-only reload with the same interface.
  Graph assembly schemas stay fixed until rebuild; per-pipeline attachment
  configuration is preserved in phase 5.
- Resize and teardown: `StorageTextureStorage`/`TextureStorage` gain
  `remove`. Retire each image/view and its sampled/storage descriptor slots
  against the last submission that can use them. Neither overwrite a used
  descriptor nor return its slot to the free list until those users retire.
  Update-after-bind does not replace this lifetime rule.
- `Window`/`WindowDiv` textures recreate in `recreate_swapchain`. Its existing
  device-idle wait is the initial retirement boundary: rewrite descriptors
  in place only after it, preserving handles for the same logical resource.
  Reusing
  a slot for a different logical resource increments a CPU-visible generation;
  stale handles must fail validation (decision 4). Do not reuse storage
  referenced by live imported tables.
  See the [Vulkan descriptor binding rules](https://docs.vulkan.org/refpages/latest/refpages/source/VkDescriptorBindingFlagBits.html).

ANNOTATION (2026-09-20): the renderer reaches the graph through the backend
traits in `crates/renderer/src/renderer/graph_backend.rs`, not through
direct graph access to `Renderer`. `FrameRenderer::submit` validates every
upload destination and indirect range before queueing. Graph-target raster
passes, one ordered step list, the one-call initialization API, and the
resize and teardown items are not implemented. Indexed indirect draws reach
the graph through `IndirectDrawNode` and the sealed `CompatibleWith` proof.

## Phases

Each phase lands green: `cargo check --workspace --all-targets`,
`cargo test -p mltrs-renderer`, `just lint`, `just sweep`, `cargo fmt`.
Phases touching `crates/cli/src/build_tasks.rs` or templates also run
`just test` and `just shaders`.

Early phases implement a supported subset. They reject other paths explicitly.
The design sections describe the final v2 behavior, not the support available
after each intermediate phase. The ledger at the end of this document lists
each temporary rejection and the phase that removes it.

Reject unsupported input before graph allocation or execution. Use a structured
error with the feature, description location, and owning phase. Codegen must
report unsupported v2 schemas without breaking the existing tuple output.
Do not ignore unsupported fields, substitute behavior, or use `unimplemented!`.
Validation and compilation must agree on the supported subset. Test each
temporary rejection. When its owning phase adds support, replace that test
with positive validation, compilation, and execution tests as applicable.
Keep negative tests for invalid uses of the supported feature.

Decision 3's scalar storage and padding rules gate phase 3a. Its array storage
rules gate phase 3b. Phases 1–2 use explicit byte-layout fixtures and emit only
the v2 schemas supported without those decisions. They do not freeze an
unresolved frame-data ABI. Decision 5's resize, extent, attachment, and
final-output rules gate phase 5. Each owning phase settles its required
subchoices before it removes the corresponding rejection.

Status markers dated 2026-09-20 follow each phase title.

1. **Pure core beneath typed nodes** — implemented. Extract `desc`, `validate`, `compile`,
   and `expand`. Keep `RenderGraph<N>` and `N::Frame`. Lower stock/generated
   nodes into internal IDs and access declarations. Port schedule tests and
   test pure expansion without a renderer. Initially support fixed textures,
   direct compute, and one main raster output. Add ownership and uniform-source
   analysis. Test rejection of later-phase paths. No public raw description API.
   Detailed plan: [`08_plain_data_graph/01_pure_core.md`](08_plain_data_graph/01_pure_core.md).
   Phase 1b, a simplification pass, is recorded in
   [`08_plain_data_graph/01b_simplification.md`](08_plain_data_graph/01b_simplification.md).
2. **Additive schemas and compile checks** — partially implemented (P2.1 and
   part of P2.4). Keep generated split types and
   traits. Add layout metadata and automatic input adapters. Test omitted frame
   elements, incomplete resource bindings, and invalid buffer operations as
   compile failures. Positive controls must compile. Prototype typed list insertion
   and uniform ownership contracts before executor work depends on them.
   Detailed plan and accepted API decisions:
   [`08_plain_data_graph/02_schemas_and_compile_checks.md`](08_plain_data_graph/02_schemas_and_compile_checks.md).
   Phase 2b, the backend-neutral crate extraction and the everyday-example
   migration, landed outside this list and is recorded in
   [`08_plain_data_graph/02b_backend_neutral_crate.md`](08_plain_data_graph/02b_backend_neutral_crate.md).
3. **Executor migration without tuple deletion** — not started. Each sub-phase lands green.
   - **3a — Particles and executor core.** Settle scalar storage/padding rules.
     Wire typed lowering and assembly over staged writes. Fix submission-aware
     commits and cross-frame WAR ordering. Migrate initialization and graph identity.
     Test controlled shutdown on invalid dynamic inputs. Add a typed per-frame
     `[u32; 3]` dispatch group-count input, lower it to `GroupSource::Value`
     with `ValueKind::Groups`, and resolve it during execution. The particles
     example uses fixed counts, so test dynamic counts separately. Remove S1.
   - **3b — Watercolor and shared sources.** Settle array storage rules. Implement
     typed uniform scopes, repeat, optional groups, and uploads under the complete
     tuple contract. Preserve retained contents, `read_previous`, push resolution,
     paper imports, and coupled brush inputs. Migrate brush points to `Immutable`.
     Test current-slot uploads and rejection of frame uploads to `GpuOnlyFlight` and `Singleton`.
     Remove S2 and S3.
   - **3c — Internal cleanup.** Remove obsolete scheduling/recording adapters only
     after parity tests pass. Retain the public tuple API, generated split types,
     and legacy picking compatibility. Do not migrate the picking example.
4. **Raster containers and Toon_link lists** — not started. Add `RasterNode<Draws>` and
   `DrawList<Params, Push>` with typed insertion and erased internal runs.
   Support one main target initially. Callback-dependent validation is a separate follow-up task.
   Complete that task before enabling decision-9 initializers for material/draw tables.
   Migrate Toon_link's shared uniform, indexed indirect
   arguments, and per-run pointers. Track indirect/table accesses now. Remove
   S4 and S5. Do not add per-element frame vectors or GPU-driven count commands.
5. **Multiple raster nodes, attachments, and interleaving** — not started. Settle decision 5.
   Add offscreen color/depth, window size classes, ordered recording, retirement,
   and pipeline compatibility. Add all required conservative barriers now.
   Test a shadow pass followed by lighting. Also test raster-to-compute-to-raster
   ordering and an internal rendering split that preserves attachments and MSAA.
   Exercise resize and final-output policy. Remove S6 and S7. Keep picking unchanged.
6. **Derived barriers** — not started. Optimize the correct phase-5 access model using last
   writers and outstanding readers. Test physical aliases, executed control flow,
   and cross-frame reuse. Keep `VKR_CONSERVATIVE_BARRIERS` for comparison.
   Remove S8 after all supported paths work with derivation enabled and disabled.
7. **Documentation and completion audit** — not started. Rewrite `docs/render_graph.md` for
   the typed facade and erased core. Annotate 07 as extended by this plan,
   not as evidence that tuple inputs were removed. Complete the final ledger audit.

Decision 13 records this migration and its testing requirements. Generic readback
and graph-owned picking are not hidden completion gates for these phases.

## Critical files

Paths as of 2026-09-20.

- `crates/render-graph/src/runtime.rs` — typed node API, `RenderGraph`,
  `PreparedRenderGraph`, `PlanCtx`; the executor migration of phase 3 lands
  here.
- `crates/render-graph/src/runtime/{desc,validate,lower,compile,expand}.rs`
  — the pure core.
- `crates/render-graph/src/backend.rs`, `commands.rs` — the backend
  contract and the command vocabulary the renderer consumes.
- `crates/renderer/src/renderer/graph_backend.rs` — backend trait impls,
  upload and indirect preflight.
- `crates/renderer/src/renderer/render_graph.rs` — compatibility facade.
- `crates/renderer/src/renderer.rs` — record path (raster passes, merged
  steps), indexed indirect recording, and rendering-scope splitting.
- `crates/renderer/src/renderer/{storage_texture,texture,descriptor_heap}.rs`
  — remove/recreate paths, in-place descriptor rewrite, slot free list.
- `crates/cli/src/build_tasks.rs`, `crates/cli/templates/graph_split.rs.askama`
  — additive schema emission and typed input adapters.
- `crates/cli/fixtures/check_crate/src/renderer/render_graph.rs` — stub.
- `crates/renderer/tests/render_graph_api_compile.rs`,
  `crates/renderer/fixtures/api_compile` — compile-check harness.
- `examples/{particles,watercolor,toon_link}/src/main.rs` — migrations.
  `toon_link` remains on the manual API.

## Reuse

- `TexRunState` cursor model and its tests (`runtime/expand.rs`).
- The apply-after-wait upload mechanism: `CommandBatch::visit_writes` and
  `validate_uploads` in `graph_backend.rs`.
- `queue_dispatch_raw`, the private `draw_frame`, and the by-index buffer
  accessors behind `BindingLookup` and `FrameLookup`.
- `cmd_barrier2`/`cmd_memory_barrier2` and the house layout-transition
  style.
- The field classification in `build_tasks.rs` (`classify_graph_field`) —
  extended with schema tables alongside generated structs.

## Verification

- Per-phase gates above, including decision-13 compile-fail and positive controls.
- Phase 3: interactive watercolor session (all pigments, debug views, odd
  Jacobi count) and particles; `just sweep` output unchanged. Unit-test no
  state commit on acquire abort/record failure/submit failure, and commit
  after submission even when presentation triggers recreation. Test that a
  forced swapchain recreation preserves version-cursor parity across the
  aborted frame. Test valid identical shared uniforms and rejection of a
  second differing assembly source.
- Phase 4: toon_link interactive parity (all 24 materials, egui debug
  window) and sweep. Test empty, single-run, and multi-run lists with one fixed
  frame type. Check setup-uploaded args, offsets, nested table dependencies, and
  shared uniform assembly. Assert indirect/table access scopes.
  > CORRECTION (2026-09-20): this bullet said 11 materials. The converted
  > manifest (`examples/toon_link/assets/link/converted/link.manifest.json`)
  > lists 24 materials, drawn as 24 batches through 5 pipelines in 7 runs.
  > Decision 13's "11 materials" bullet is corrected the same way.
- Phase 5: shadow/depth and offscreen interleaving examples under sweep; window resize under
  validation; the `VKR_INJECT_VALIDATION_FAULT` self-test still fires. Cover
  attachment writes to compute reads, compute writes to raster reads, depth
  reuse, and retirement of both descriptor slots and image/view resources.
- Phase 6: sweep with derivation on and off; RenderDoc spot-check that
  debug labels name logical resources and versions. Pure hazard tests cover
  RAW/WAW/WAR, skipped `When` bodies, zero/odd/even repeats, aliases, and
  cross-frame physical reuse. Vulkan validation cannot observe arbitrary
  BDA accesses, so sweep alone is not the buffer synchronization oracle.

## Risks

- **Type erasure boundary.** Erasing a draw before checking its input contract
  can hide required data. Typed insertion and compile-fail fixtures must prevent
  that. Startup checks handle graph relationships, not missing public frame fields.
- **Record-path surgery.** Phase 5 rewrites the middle of
  `record_command_buffer`. The offscreen example plus sweep gates it; the
  manual API path stays untouched as a control.
- **Layout metadata drift.** Derive GPU offsets, sizes, and kinds from reflection.
  Verify generated layouts with assertions and assembly tests. Compare full
  interface descriptions where compatibility needs checking. A matching hash
  would not prove that metadata or generated assembly is correct.
- **Assembly-program correctness.** Tables for uniform sources and command push blocks replace
  per-type `assemble` fns. Unit-test the program builder against the
  schema snapshots, and cover a struct with interleaved data/resource
  fields and explicit padding.

## Decision record

Thirteen decisions from the design review. Unresolved subchoices are noted in
place. The superseded discussion prose lives in git history.

### 1. Retained contents and initialization

Resource initialization is part of the renderer API. One setup call,
made from the game's setup method, creates a resource and requires its
initial contents (`BufferInit::Zeroed | Provided`; graph textures clear at
build). Initialization covers every applicable backing slot/version.
Reads of retained contents are legal, including after a skipped `When`
writer or a zero-trip repeat. Full control-flow-aware initialization
analysis is deferred to the follow-up at the end of this document. If an
example reads undefined contents during implementation, stop and report
the resource and execution path to the user.

Evidence: watercolor clears every physical image at build and deliberately
runs the simulation from retained state when the brush is skipped;
particles seeds all GPU-only flight slots at setup; toon_link uploads args
and tables to every applicable slot at setup. `read_previous` means the
version before the latest version-producing write, not always the previous
frame, and `Mutate` does not rotate it. Source inspection found no
concrete missing-initialization case in these paths.

ANNOTATION (2026-09-20): not implemented. No `BufferInit` exists. The
renderer's one-call create-and-initialize APIs are `create_indirect_buffer`
and `create_singleton_buffer`. GPU-only and immutable buffers use two calls
(`create_gpu_only_buffer` + `write_gpu_only_all_frames`,
`create_immutable_buffer` + `write_immutable_all_frames`). Graph textures
clear at `prepare` through `PreparationBackend::prepare_image`.

### 2. External resources and transitive references

External resource support is limited to bindless sampled textures that
are initialized during setup and unchanged while the graph uses them.
Textures reached indirectly through tables are declared as imports and
kept alive for their uses. GPU-writable resources belong to the graph and
receive initial contents through the decision-1 API; particles' externally
allocated buffer migrates to a graph-owned declaration. Mutable external
imports are unsupported. Barrier elision never assumes an unknown handle
has no dependencies.

Evidence: watercolor's paper height image is a setup-uploaded, never
written sampled texture. Toon_link reaches material textures through
`MultiDraw -> IndividualDraw -> Material -> handle` tables that a scan of
top-level parameter fields cannot see.

### 3. Frame-value schemas and byte storage

Decision 12 replaces the earlier `Key<T>` and incremental `set` proposal.
`GraphNode::Frame` and generated binding/data structs define complete inputs.
Stock/generated adapters populate private value storage without a user mapping.
Schema metadata describes internal lowering and assembly. Types enforce input
completeness. Layout assertions and assembly tests check representation correctness.

Unresolved subchoices: copy values or borrow through CPU preparation; array
storage/count representation; initialization of all copied bytes including padding.
Metadata must describe data and GPU layouts, nested types, and resource kinds.
These storage choices must not weaken the typed completeness contract.

ANNOTATION (2026-09-20): still unresolved. `UploadNode::Frame` is `Vec<T>`
(`tech_debt.md` item 19), which is the copy side of the copy/borrow
subchoice. Staged bytes are `MaybeUninit<u8>`, so padding is never read as
initialized. No schema metadata is generated.

### 4. Resource identity and ownership

Each graph gets an incrementing graph ID.
Every graph-owned resource key carries `(graph_id, resource_id, generation)`.
The graph checks all three fields when accepting a key.
The resource ID distinguishes resources within one graph.
The generation distinguishes successive resources that reuse one allocation slot.
Sharing resources between graphs is unsupported.
Graph rebuild is unsupported in v2 (§1), so key stability across rebuilds
is not yet defined.
Allocation-slot reuse increments a CPU-visible generation; stale handles
fail validation — graph identity alone does not detect slot reuse.
Imported tables and their referenced resources stay alive together so raw
GPU handles cannot outlive an allocation and silently observe slot reuse.
The portable description uses plain local indices; provenance checks
happen in the Rust builder before lowering, and in name resolution for
scripts.

Each live pipeline retains the identity of the uniform referenced by its descriptors.
Setup compares that identity with the uniform source that the graph writes.
Example: player and security cameras share one schema but use different uniform resources.
Their keys must differ, and setup must reject a pipeline/source mismatch.
Reason: schema compatibility does not establish resource identity.

ANNOTATION (2026-09-20): not implemented. Keys carry a bare index and no
graph ID or generation. `PreparedRenderGraph::execute` checks every captured
slot for liveness each frame through `FrameLookup` (`tech_debt.md` item 18).
Pipeline keys carry the push interface in their type (`NoPush` or
`PushBlock<B>`) but not a uniform identity.

### 5. Attachments and window resize

Offscreen attachments start clear-and-store, single-sample. Stored depth
attachments may be sampled by later passes so shadow maps work: a shadow
pass clears, renders, and stores its depth attachment; a later lighting
pass samples that produced version while rendering to a separate target.
The graph tracks that dependency and emits the required synchronization
and layout transitions. User-requested attachment `Load` is deferred; load/store with
retained caching or accumulation (GI probe caches) may come later, and if
`Load` lands it must read the latest logical contents in place, not rotate
to an unrelated image. The existing main-output behavior (clear, optional
MSAA resolve, blit, egui, present) is preserved; "backbuffer" denotes that
renderer-managed output path. Pipeline creation and hot reload retain each
offscreen target's format/sample configuration. Decision 7 permits internal
store/load operations to preserve one logical raster pass across a barrier.
These operations do not expose retained attachment loading between logical passes.

Unresolved subchoices: resize reset versus preservation (and rules for
changed dimensions); `WindowDiv` rounding and minimum extent; zero-sized
window suspend versus minimum render extent; whether an unconditional
final output pass is required, or what output means when that pass does
not execute. Fixed-size images (watercolor's 2048x1536 simulation images)
are unaffected by resize.

### 6. Restricted uniform sharing

Consumers may share a uniform slot only when they use one assembly source.
Each binding must select the same physical resource for all active consumers
within one execution. Physical selections may change between executions.
The setup validator must establish this rule for every permitted execution shape.
It must reject sharing when it cannot establish the rule.

Reason: the CPU stages uniform contents before GPU execution. Later staging
to the same slot replaces earlier contents. It does not create a snapshot
for each consumer. A texture write can change which physical image holds
the latest version. Therefore, two `Read(T)` references can require different
image handles within one execution. Equal declarations do not prove equal
assembled contents. A GPU barrier cannot restore an overwritten uniform handle.

Example: a water simulation uses images A and B for logical texture T.
A foam pass reads T from A. An update pass reads A and writes B.
A floating-object pass then reads T from B. The foam and floating-object
passes cannot share a uniform slot that contains their `Read(T)` binding.
Both GPU passes would receive the last handle staged into that slot.

Use separate uniform slots when consumers need different physical selections.
Use push constants when a reference must change within a repeat.
V2 does not allocate a new uniform region automatically for each consumer.
Shared camera data and other bindings with unchanged physical selections remain legal.
The existing repeat-body restriction remains in force.

Tests must cover identical shared data, differing assembly sources, and texture
rotation between consumers. Include conditional writes and zero, odd, and even
repeat counts. Also test rejection of two IDs bound to one live uniform slot.
These checks land with core validation and live setup validation in phase 3a
at the latest. They are permanent rules, not temporary subset rejections.

Update (2026-09-07): enforcement lands in phase 1, not phase 3a. Two nodes
that stage distinct data sources into one uniform slot are an error even when
the staged data is identical. Watercolor's shared blur uniform splits into two
slots (and two pipelines) in phase 1. See
[`08_plain_data_graph/01_pure_core.md`](08_plain_data_graph/01_pure_core.md).

### 7. Ordered steps with implicit synchronization

Graph steps execute in description order by default. The graph inserts the
barriers required by declared accesses. This rule applies to compute steps,
draws, and attachment accesses. Users do not write Vulkan barriers.
The graph may omit a barrier when no dependency requires it.
Description order does not require a full GPU wait between independent commands.

Reason: a consumer must observe the output of its earlier producer.
The same rule must apply when either command is a draw.
Rejecting all dependent draws would force users to manage renderer details.
Track shader reads/writes and attachment reads/writes as distinct access kinds.
Include attachment clear, blend, depth, and store behavior in the analysis.

A logical raster pass need not map to one Vulkan rendering scope.
If a required barrier cannot occur inside rendering, end rendering before
the barrier and begin rendering again after it. Preserve attachment contents
across this internal boundary. Do not clear or rotate attachments again.
Internal store/load operations are permitted for this purpose. This does not
add user-requested attachment `Load` between separate logical raster passes.
Preserve the final resolve behavior when the main output uses MSAA.
One logical raster pass produces one version of each target, including an
empty pass that clears its targets.

Implementation requirements for internal rendering boundaries:

- Preserve the actual multisample attachments, not only their resolved images.
  Their allocation and store policy must support preservation across scopes.
- Clear each attachment once per logical pass. Perform the final resolve
  at the final rendering boundary, after all draws for that pass.
- Include attachment store/load accesses in dependency analysis and synchronization.
- Preserve or restore required draw state when recording continues. Check pipeline,
  descriptors, push constants, vertex/index bindings, and dynamic state as applicable.
- Track all shader resource accesses, including resources reached through tables.
  A top-level parameter scan alone does not establish the complete access set.

Use the existing dynamic-rendering and synchronization2 features for this path.
No additional local-read extension is required. Rendering splits can increase
memory traffic, especially on tile-based GPUs. Later optimization may reduce
splits but must preserve the same dependency and attachment results.

This policy does not make conflicting accesses within one draw safe.
The existing prohibition on sampling a pass's own target remains in force.

An explicit future `parallel` request must pass dependency validation.
It must not disable required barriers or change attachment results.
Keep dependencies before and after the requested group.
The syntax and scheduling of `parallel` remain outside v2.
V2 provides the ordered behavior without requiring this future feature.

Phase 5 implements dependent draws and attachment preservation at internal
rendering boundaries. Extend S6 tests with a shader write followed by a
dependent draw read. Verify earlier color/depth contents survive the barrier.
Also verify one clear/version event and the final main-output resolve.
Phase 6 may remove unnecessary barriers without changing these results.

### 8. One attachment configuration per graphics pipeline

Each graphics `PipelineId` identifies one fixed attachment configuration.
The configuration includes color formats/count, depth/stencil format, and sample count.
Store this configuration in pipeline declaration metadata and verify the live
pipeline against it during setup. Check each raster use against that configuration.
Preserve the configuration during code-only hot reload.

Reason: matching shader parameters do not prove that output attachments match.
The same shaders can serve different outputs through separate pipeline instances.
For example, a main scene and an inventory preview can use different formats
or sample counts. Such configurations receive separate pipeline IDs.
V2 does not create pipeline variants automatically.

Different physical images can use one pipeline when their configurations match.
Image identity is not part of the pipeline configuration.
Phase 5 implements these checks and tests incompatible formats and sample counts.

### 9. Setup initializers discover dependencies through use

Status: future proposal. Callback-dependent validation is outside the initial implementation.
The follow-up task below must define the complete validation and compilation sequence.
Phase 4 cannot close S5 before that task is complete.

A singleton initializer requests resource references from a restricted resolver.
Each request records a dependency from the initialized buffer to that resource.
The caller does not provide a dependency array.

Reason: a separate dependency array duplicates the callback's resource requests.
The two lists can disagree. Recording each request keeps dependency tracking
with the operation that obtains the reference.

The following example shows proposed API syntax, not an implemented API.
`Material` contains a sampled texture handle. `IndividualDraw` contains an
`ImmutableAddr<Material>`. Production types also include their other shader fields.

```rust
let materials = builder.singleton_with(
    "materials",
    1,
    move |init| {
        Ok(vec![Material {
            texture: init.texture(diffuse)?,
        }])
    },
)?;

let draws = builder.singleton_with(
    "draws",
    1,
    move |init| {
        Ok(vec![IndividualDraw {
            material: init.singleton_addr_at(materials, 0)?,
        }])
    },
)?;
```

The following sequence is a proposal for the follow-up task, not a complete validation design.
First validate resource declarations. Then allocate all declared buffers.
Run each initializer once. Record dependencies during resolver calls.
Check discovered dependencies, returned schemas, and element counts before upload.
If initialization fails, fail graph setup and release the new allocations.
The graph retains referenced resources for its lifetime.

The resolver initially returns only singleton addresses and imported texture handles.
It checks graph ownership and element bounds. It cannot create resources,
record commands, or read another buffer's initialized contents.
All addresses exist before initializers run, so callback order does not matter.
Plain initial contents still use `BufferInit::Provided` without a callback.
Callbacks remain live setup inputs outside portable `GraphDesc`.

Every embedded resource reference must come through the resolver.
A captured raw address would bypass discovery. Native callbacks remain trusted
at this boundary until the API prevents such references through construction.
Do not claim that schema checks can validate arbitrary embedded addresses.
Phase 4 implements this path after the follow-up task resolves the validation order.
Tests of Toon_link's tables must pass before S5 closes.

### 10. Typed builders in both languages

Rust and the scripting language both expose typed graph builders.
The scripting language is strongly typed. It supports build-time evaluation
of pure code. Use types to enforce restrictions that the types can express.
Use build-time or startup validation for relationships that types cannot enforce.
Rust runs these pure checks at startup. Scripts can run them during build evaluation.
Device checks and live setup checks run when their inputs become available.

`GraphDesc` remains a plain value inside the implementation.
Neither language exposes its unchecked construction or mutation to callers.
Do not add a public raw-description ingestion path as part of v2.
Both builders lower into the canonical representation for analysis and compilation.

Keep the existing GPU-only, immutable, and singleton buffer semantics.
Keep their BDA types and permitted operations. No current use case requires
another buffer kind or a configurable capability matrix.
The typed APIs prevent invalid buffer operations. Pure validation checks the
remaining graph relationships, including bounds, access conflicts, and uniform sharing.

Reason: an erased execution core does not require removing useful
resource types or typed outer graph structure. Typed construction prevents invalid operations close to their source.
Pure validation handles relationships across the graph without encoding its
entire structure in types.

Decision 12 makes required frame-input presence a type-level contract. Dynamic
counts, array lengths, and internal byte-layout invariants still need runtime checks.

#### Update 2026-09-20: erased nodes for the Roc platform

The Roc platform receives its graph as plain data. Rather than a public raw
description ingestion path, the crate gained `DynDrawNode` (`runtime/erased.rs`):
a draw node built from a pipeline key, a uniform slot, and a GPU size whose
frame value is packed bytes. It lowers through `LowerCtx` and validates under
the same rules as typed nodes, so decision 10 holds: neither language exposes
unchecked construction. The Roc side ports description, lowering, and
validation (`roc-platform/platform/RenderGraph*.roc`) and validates during
constant evaluation; the host re-validates in Rust at setup.

### 11. Invalid frame values cause controlled shutdown

Treat invalid frame values as application programming errors.
Report the value and failed requirement through the application error path.
Stop the frame loop and perform controlled shutdown. Do not retry the frame,
substitute default data, or reuse data from an earlier frame.
The renderer returns the error to the application. It does not call
`process::exit` inside validation.

Check frame inputs before swapchain acquisition, CPU uploads, and command recording.
An invalid frame must not submit work or commit proposed graph state.
Shutdown must account for earlier submissions that can still use resources.

Reason: the application supplies these values under a known graph contract.
A missing required value indicates a broken contract. Continuing with substitute
data can hide that error and change the rendered result.

Decision 12 supersedes the incremental `FrameValues::set` API. Missing required
public inputs now fail type checking. Keep runtime checks for dynamic limits,
array lengths, and internal adapter invariants. The shutdown policy still applies
to invalid values that types cannot exclude. Phase 3a tests that error path
before acquisition, uploads, submission, or state commit.
An expensive repeat count is not an invalid value solely because expansion costs too much.
Limits for expansion cost are outside v2. Applications choose and test their repeat counts.

ANNOTATION (2026-09-20): partially implemented. An oversized upload, a
dropped buffer slot, or an invalid indirect range makes `execute` return
`DrawError` before any write, submission, or cursor commit. The phase-3a
tests for dynamic counts and group values are pending.

### 12. Typed tuple facade with an erased internal representation

Re-adopt `RenderGraph<N>` and `GraphNode::Frame`. Tuple composition derives the
complete frame tuple. Keep generated resource binding structs and data structs.
The scripting type system can express this relationship. Run pure script graph
validation through const evaluation, and Rust graph validation during setup.

Reason: the incremental value API permits omission of a required frame value.
A handwritten mapping introduces another omission point. Stock/generated node
adapters derive the internal mapping from the typed nodes instead.
Runtime-length draw lists do not require a runtime-length outer node tuple.
Toon_link needs setup-defined runs with shared data, not arbitrary new frame fields.

Typed builders lower into a private erased description. Validate and compile
that description, then expand typed inputs without live renderer access.
Keep the resource, initialization, uniform, synchronization, and shutdown decisions.
Do not replace the tuple facade with unchecked erased node construction.

The following Toon_link examples show proposed syntax. The method name
`.with_push_constant(...)` is settled. It accepts a generated combined `*Input` struct. Other exact
signatures remain implementation subchoices. The input restrictions are settled.

```rust
let mut draws = DrawList::<ToonLinkParams, MultiDraw>::new(
    frame_uniform,
    frame_bindings,
);

for run in runs {
    draws.push(
        indexed_indirect(
            &material_pipelines[run.pipeline],
            args.at(run.first),
            run.count,
        )
        .with_push_constant(MultiDrawInput {
            individual_draws: individual_draws.at(run.first),
        }),
    )?;
}

let mut graph = build_graph(renderer, resources, (
    raster(main_targets, draws),
))?;

// The frame type does not change when the number of runs changes.
graph.execute(frame, &(toon_link_frame,))?;
```

`DrawList<Params, Push>` owns one shared uniform source. Typed insertion accepts
only compatible params/push contracts before erasing each draw representation.
Retain those interface types in pipeline/list builder tokens until insertion.
A bare runtime pipeline index cannot prove that contract at compile time.
Each run supplies complete setup push bindings and any required setup push data.
Runs cannot add per-frame fields. Different compatible pipeline instances are legal.
Use a tuple of typed lists for different interfaces. Do not add arbitrary
per-run frame vectors in this plan.

For shared uniforms outside a list, provide a typed source scope:

```rust
with_uniform(uniform, bindings, |shared| {
    (
        dispatch_using(shared, pipeline_a),
        dispatch_using(shared, pipeline_b),
    )
})
// Scope input: (UniformData, BodyFrame).
// Consumers do not request another copy of UniformData.
```

The setup callback constructs consumers, not a manual frame-field mapping.
Decision 6 rejects sharing if physical selections differ between consumers.
Body data unrelated to the shared source remains part of `BodyFrame`.
This scope form is accepted. Its frame input is `(UniformData, BodyFrame)`.
Lower each source into one `UniformDecl` with its data value and bindings.
Lower each consumer to a reference to that declaration.
An empty list keeps its public frame type.
Validate and retain its uniform source and referenced resources, as specified in review item 5.

Multiple raster nodes retain a fixed typed outer structure:

```rust
let mut graph = build_graph(renderer, resources, (
    raster(shadow_targets, shadow_draws),
    raster(main_targets, lighting_draws),
))?;

graph.execute(frame, &(shadow_frame, lighting_frame))?;
```

Lighting binds the shadow texture through its generated resource bindings.
Each raster node retains its fixed pipeline/attachment requirements.
The internal compiler supplies barriers and layout transitions.

Deferred work: graph-owned picking, generic readback, and migration of the picking
example. Preserve existing renderer picking behavior and compatibility only.
Also defer GPU-produced argument/count commands and arbitrary dynamic frame contracts.
Future AAA extensions must preserve completeness or introduce a separately reviewed contract.

ANNOTATION (2026-09-20): the tuple facade is retained. `[N; K]` is a node
with frame `[N::Frame; K]`; `examples/multi_mesh` holds 18 draws that way.
`.with_push_constant(*Input)` exists on every command form.
`DrawList<Params, Push>`, `with_uniform`, `raster(targets, draws)`, and
`build_graph` do not exist. A fixed-length array requires the element count
at compile time; the setup-length list this decision describes remains open.

### 13. Incremental tuple migration and proof of completeness

Use the phases above: pure core, additive codegen, particles/watercolor migration,
Toon_link lists, multiple raster nodes, derived barriers, and documentation.
Do not delete tuple inputs or their generated split types.
Remove old internal execution paths only after the migrated consumers pass their gates.

Reason: the current implementation already enforces the desired input relationship.
Changing scheduling beneath that relationship avoids rebuilding completeness through
an independent mapping. Toon_link and shadow tests exercise dynamic contents and
multiple outputs without requiring a dynamically typed outer graph.
Picking redesign is a separate project, not a dependency of this migration.

Testing strategy:

- Add compile-fail fixtures for omitted frame tuple elements, incomplete generated
  bindings, wrong data types, and incomplete `Some(Body::Frame)` inputs.
- Reject list insertion that requires additional frame data or incompatible
  params/push types before erasure. Test this with compile-fail fixtures.
- Include positive controls for complete inputs, `None`, shared sources, and
  empty/single/multi-run lists with the same frame type. Ensure negative tests
  fail for the intended reason rather than unrelated fixture errors.
  Accept a valid uniform source with zero consumers. Reject invalid keys and bindings even when the list is empty.
  Check normal uniform assembly and staging when the source's owning node executes without consumers.
  Check that the graph retains resources referenced only by that source.
  Check that an empty executed raster pass still clears its targets without shader accesses from absent draws.
- Test pure lowering and expansion against the typed adapters. Verify every
  required internal value receives its source, including optional groups and repeats.
- Test setup ownership, uniform source identity, physical selection conflicts,
  buffer ranges, and callback dependency discovery without relying on Vulkan validation.
  Include two same-schema uniforms with different resource IDs and a stale generation.
  Reject a pipeline whose descriptor uniform differs from the graph's uniform source.
- Preserve particles/watercolor behavior and initialization. Test zero/odd/even
  repeats, optional brush uploads, state commit outcomes, and dynamic-input shutdown.
  Assert that successful submission commits before presentation and any resize reset.
  Presentation failure must not reverse that commit.
- Verify Toon_link's 24 materials (the manifest count; an earlier draft said
  11), shared uniform, nested immutable tables,
  indirect ranges, and per-run pointers through interactive parity and sweep.
- Verify shadow depth sampling, multiple raster nodes, interleaving, rendering
  splits, preserved attachment contents, resize, and final MSAA resolve.
- Keep pure RAW/WAW/WAR, alias, and cross-frame tests. Run sweep with conservative
  and derived barriers. Vulkan validation alone cannot observe all BDA hazards.
- Preserve existing picking tests/sweep behavior without adding generic readback
  or a picking migration gate. Audit every temporary rejection at phase 7.

Phase 2 keeps 12 elements per tuple, nested composition, and `&N::Frame`.
Lists accept complete typed runs. Draws and dispatches use `.with_push_constant(...)`.
The `with_uniform` callback form and scope frame contract are accepted.
Use pure source-ownership checks rather than requiring Rust lifetime branding.
Verify the generated combined push inputs with compile-check fixtures before executor integration.
None may restore incremental missing-value assembly.

### Evidence locations

Search by symbol; line numbers move.

- `examples/particles/src/main.rs`: `create_gpu_only_buffer`,
  `write_gpu_only_all_frames`, `SimParamsBindings`, `RenderParamsBindings`.
- `examples/watercolor/src/main.rs`: `type WcGraph`, `blur_h_pipeline`,
  `blur_v_pipeline`, the `ResourcePlanner` declarations in `setup`, and the
  frame tuple passed to `graph.execute` in `draw`.
- `examples/toon_link/src/main.rs`: `MaterialTable`, `build_materials`,
  `queue_run`, `create_indirect_buffer`, `create_singleton_buffer`, and the
  `ToonLinkParams` write inside `submit_draws`.
- `examples/toon_link/src/generated/shader_atlas/toon_link_modern.rs`:
  `ModernMultiDraw` → `ModernIndividualDraw` → `ModernMaterial` (the
  transitive table fields).
- `crates/renderer/src/renderer/graph_backend.rs`:
  `PreparationBackend::prepare_image` (graph image creation and clear),
  `validate_uploads`, `validate_indirect_layout`.
- `crates/renderer/src/renderer.rs`: `draw_frame` (submission and the
  `on_submitted` call), the main attachment setup, and
  `assert_shader_interface_unchanged`.

## Follow-up after this plan: initialization assurance

Track a separate task to specify and validate initialization of every
resource version and buffer range that can be read: setup imports, first
frame, skipped `When` producers, zero-trip repeats, partial writes, and
resize/rebuild. Distinguish deliberately retained state from undefined
contents, including GPU-generated indirect arguments and count buffers.
Consider explicit initial values/import guarantees and control-flow-aware
validation; do not choose or implement that analysis as part of v2 here.

During this plan, preserve the examples' existing initialization. If an
example encounters this problem, stop implementation and raise the concrete
case to the user rather than silently choosing a clear, fallback, or new
validation rule. Additional validations for parallelism are also future
work, outside this plan.

## Review update — 2026-09-07

The item numbers below match the review discussion.
Items 1, 3, 4, and 5 record accepted design decisions.
Items 2, 6, 7, and 8 record accepted scope limits or deferrals.
All items in this review now have a decision or an explicit scope limit.
Other implementation subchoices elsewhere in this plan remain subject to their existing phase gates.
The `with_uniform` scope form is accepted. Other illustrative helper names do not specify final signatures.

### 1. Typed resource keys carry a three-part identity

Decision: typed resource keys carry `(graph_id, resource_id, generation)`.
The graph checks ownership, resource identity, and generation when accepting a key.
Graph identity alone cannot distinguish two uniforms in the same graph.
The pipeline must retain the identity of the uniform used by its descriptors.
Setup must compare that identity with the source that the graph writes.
Adding identity without this comparison does not detect a mismatch.

Example: player and security cameras use separate uniforms with the same schema.
The graph writes the security-camera uniform, but the pipeline references the player uniform.
The schema check passes. A check of the resource identities detects the mismatch.

Reason: the type describes permitted data. The resource identity identifies the buffer that contains those data.
The setup check compares resource identities before execution.
The type still constrains the resource interface. The identity distinguishes individual resources with that interface.

Status (2026-09-20): not implemented. See the decision-4 annotation.

### 2. Defer validation of dependencies discovered by setup callbacks

Decision: this work is outside the initial implementation.
Record the problem, example, and required work in the follow-up task below.
The current main-only migration can proceed without decision-9 callbacks.
Phase 4's table support still depends on this follow-up. S5 remains open.

Reason: Toon_link's nested references require a complete dependency chain.
The initial implementation does not need to solve callback-dependent validation before supporting particles and watercolor.
This decision does not permit unchecked tables or disable resource checks.

### 3. Commit submitted state before presentation

Decision: use option A. Commit immediately after successful submission, before presentation or resource replacement.
A successful submission must commit even if presentation later fails.
Resource replacement must not inherit obsolete cursor or image-layout state.

Example: a reflection pass writes image B of a window-sized texture.
Presentation then causes resize. Resize replaces both backing images and resets their state.
A later commit of the old state can overwrite that reset.

The renderer reports successful submission before it presents the frame.
The graph commits the submitted state. The renderer then presents and performs any required resource replacement.
The graph applies replacement resets after the commit.
An acquisition abort discards the proposed state and still applies any replacement resets.

Reason: control flow establishes the order directly.
The renderer must expose a submission boundary before presentation or invoke a commit operation at that boundary.
The commit must not introduce a fallible operation after successful submission.

This decision does not choose whether resize preserves or resets texture history.
Decision 5 still owns that choice. Preserve fixed-size resource state across resize.
Test successful submission followed by presentation failure and by resource replacement.
Test acquisition, recording, and submission failures without a graph-state commit.

Status (2026-09-20): implemented. `FrameBackend::submit` takes an
`on_submitted` callback; `Renderer::draw_frame` calls it after
`queue_submit2` and before `queue_present_khr`. The fake backend tests cover
submission-time commits. Resize replacement resets are phase 5.

### 4. Upload brush points through an Immutable buffer

Decision: migrate watercolor's brush points to `Immutable` in phase 3b.
Frame uploads can write an `Immutable` buffer's current flight slot after its wait.
The GPU cannot write that buffer. Previous-slot access remains unavailable for this buffer kind.
Reject frame uploads to `GpuOnlyFlight` and `Singleton` in the typed API.
Check those restrictions again when validating internal upload declarations.

Setup can initialize all applicable slots of a `GpuOnlyFlight` buffer.
After setup, only the GPU can write that buffer.
Setup creates and initializes a `Singleton` buffer once.
Neither the CPU nor the GPU can modify a singleton after setup.

Example: the game uploads brush points each frame. The brush shader reads the points.
Particles instead use a `GpuOnlyFlight` buffer because the simulation writes particles and reads previous-slot output.
A material table can use `Singleton` when its contents remain fixed after setup.

Reason: these three uses have different write permissions and lifetimes.
The brush does not require a new buffer kind. The name `Immutable` describes the restriction on GPU writes.
Test valid brush uploads and rejection of frame uploads to the other two kinds.

Status (2026-09-20): not done. Watercolor's stroke points use
`create_storage_buffer`, `StorageSlot`, and `upload(...)`.
`BufferKind::Storage` remains in the description (ledger S2).

### 5. A typed scope owns one uniform source

Decision: adopt `with_uniform(uniform, bindings, |shared| body)` with frame input `(UniformData, BodyFrame)`.
The scope owns one typed data input and one complete binding set.
`UniformDecl` owns the source data reference and bindings after lowering.
Commands reference its `UniformId`. Commands do not define duplicate sources.
Consumers add only their unrelated inputs to `BodyFrame`.

Example syntax for a terrain draw and a tree draw follows.
Both pipelines use the camera uniform. Neither draw needs additional frame data in this example.

```rust
let scene = with_uniform(camera_uniform, CameraParamsBindings {}, |camera| {
    raster(main_targets, (
        draw_using(camera, terrain_pipeline),
        draw_using(camera, trees_pipeline),
    ))
});

let mut graph = build_graph(renderer, resources, (scene,))?;

// One scope supplies camera data. Each draw has unit frame input.
graph.execute(frame, &((camera_data, ((), ())),))?;
```

Reason: the graph should know the source independently of its consumers.
A setup-defined tree list can be empty in a desert scene.
An empty list must retain the same public frame type as a nonempty list.
The unused-declaration rule therefore needs a defined treatment for sources with no consumers.
The source ownership and frame contract are settled.

Decisions for empty lists:

- Allow a uniform source with no consumers. Check its resource identities, schemas, and complete bindings during setup.
  Example: a desert scene has no tree draws, but a stale key in its tree source still causes a setup error.
- Assemble and stage the uniform normally when its owning node executes, even if no draw consumes it.
  Example: the empty tree list still receives its required camera data and stages its uniform.
  Skipping this work is a possible optimization, outside the current plan.
- Retain the source and all resources referenced by its bindings for the graph's lifetime.
  Example: the empty tree list retains its leaf texture even though no tree shader samples it.
  Do not remove those resources from the allocation plan solely because the source has no consumers.

Reason: predictable API behavior takes priority over avoiding unused work.
The application supplies the same frame type and receives the same checks for empty and nonempty lists.
Resource lifetimes do not depend on the number of consumers.
An absent optional body still skips its work under the existing rules.

An unused source's bindings retain resources but do not create shader accesses.
Uniform assembly alone does not advance texture version cursors.
Decision 7 already requires an executed raster pass to clear its targets even when its draw list is empty.
An empty tree list contributes no draws. An empty shadow pass still produces a cleared shadow image.

Potential follow-up: optimize unused work or report it through development-time warnings.
For example, a warning could identify a uniform that receives data but has no consumers.
The developer could then remove that source explicitly if the application does not need it.
Neither automatic removal nor these warnings are implementation requirements for this plan.

Status (2026-09-20): `with_uniform` is not implemented. Each command names
its uniform buffer as a constructor argument. `validate` accepts a
`UniformDecl` with no consumers and checks its bindings (phase 1).

### 6. Preserve existing optional behavior

Decision: close this review item. Additional optional-step features are outside scope.
Existing optional behavior remains unchanged.

The review does not establish a missing public feature in the brush path.
`Some(Body::Frame)` already supplies all body inputs. `None` skips the body.
Downstream work can use retained contents, as decision 1 permits.

Example: watercolor passes `None` when the player does not paint.
The graph skips the brush upload and brush dispatch.
The water simulation continues from retained textures. This behavior requires no new public API.

The adapter must preserve the relationship between inputs in the typed optional body.
A new presence-ID mechanism and automatic skipping of downstream consumers are outside this review's scope.
They are not additional implementation requirements.

If an application separates the upload and dispatch conditions, it must keep those conditions consistent.
For example, an absent upload and an active brush dispatch can make the shader read retained points.
Those points can come from the current slot's earlier use.
That example does not apply to the existing coupled `Option<Body::Frame>` unless the adapter is incorrect.
Test the adapter's coupled behavior without adding a new public condition.

### 7. Do not add repeat-expansion budgets

Decision: limits for expanded command counts or memory use are outside v2.
Applications choose practical repeat counts and test their cost.
Do not add a new rejection or completion requirement for expensive repeat counts.
Existing checks for actual device limits and invalid inputs remain required.

Example: a water solver uses a small iteration count that the game tests for quality and frame time.
An erroneous count of one billion could require an excessive execution list.
This plan does not promise to detect that cost or return a controlled error for it.

Reason: selecting and testing practical counts remains the application's responsibility.
The initial implementation does not need a separate policy for expansion budgets.

### 8. Defer depth changes until their owning feature phase

Decision: defer the depth extension. Preserve the current main depth path through phase 4.
Phase 5 remains responsible for stored depth, sampled depth, and preservation across internal rendering boundaries.

The renderer creates its main depth image with `DEPTH_STENCIL_ATTACHMENT` usage.
It uses `DEPTH_STENCIL_ATTACHMENT_OPTIMAL` for depth attachment access.
Its transition covers depth and includes stencil when the format has stencil.
Main rendering clears depth to 1.0 and uses `DONT_CARE` for the depth store operation.
The current main depth path does not sample the depth image.

Example: depth testing hides a wall behind a nearer character during the main pass.
No later pass needs those depth values, so this path can discard them.
A shadow pass has a different requirement. Lighting must sample the depth values that the shadow pass produced.

Add the required storage, sampling usage, and transitions when phase 5 introduces shadow depth sampling.
Phase 5 must also preserve depth across internal rendering boundaries.
The current discard policy cannot satisfy either new use.
The review's color-layout wording concern does not identify a defect in the existing depth-layout selection.

## Follow-up task: validate dependencies discovered by setup callbacks

Status: deferred from the initial implementation by review item 2.
This task must finish before phase 4 enables decision-9 table initializers and closes S5.

Problem: the proposed validator checks that every declaration is used before resource allocation.
Setup callbacks discover some resource references only after allocation.
Before those callbacks run, the validator cannot see the complete dependency chain.
An early unused-declaration check can reject a resource that a table actually references.
Compilation before dependency discovery can also omit accesses and resources that the graph must retain.

Example: a character draw reads a draw table. The draw table references a material table.
The material table references the character's shirt texture.
The setup callbacks reveal the last two references. An earlier check can incorrectly report the shirt texture as unused.

Required follow-up work:

- Define which checks can run before allocation.
- Define how callbacks record resource references as plain local IDs.
- Define when the validator checks complete resource use and dependencies.
- Define how compilation receives the complete dependency information.
- Define cleanup when callbacks or later checks fail.
- Test the character-table example without Vulkan validation.

Reason: the compiler needs complete dependencies for access analysis and resource lifetimes.
Additional work is required. The existing phrase "additional check" does not specify a sufficient contract.
This task is separate from the later initialization-assurance task.

## Final verification: remove all temporary subset rejections

This ledger tracks temporary implementation limits. All rows start open.
Each owning phase records the tests and implementation that close its row.
If implementation needs another temporary rejection, add a row and assign
an owning phase before merging that rejection.

| ID | Temporary rejection | Phase that removes it | Required evidence | Status (2026-09-20) |
| --- | --- | --- | --- | --- |
| S1 | V2 frame-data schemas or ingestion that require unresolved scalar padding or mixed data/resource layout rules | 3a | Generated data/GPU layout metadata, safe byte assembly, mixed-field/padding tests, and particles migration | Open. `GroupSourceValue` rejected; no layout metadata emitted |
| S2 | Frame uploads and runtime array values | 3b | Array schema/count checks, current-slot Immutable brush uploads, rejection of GpuOnlyFlight/Singleton frame uploads, shorter-upload behavior, and watercolor's upload coupled to `When` | Open. `BufferKind::Storage` and upload-to-`Storage` remain; watercolor stroke points use them |
| S3 | Migrated core paths beyond the 3a subset | 3b | Typed shared sources, repeat, optional groups, and existing draw paths. Preserve legacy picking compatibility without redesign | Open. Production runs `plan()`; `compile`/`expand` unwired |
| S4 | Typed setup-length draw lists and per-run push bindings | 4 | Toon_link migration, fixed frame type across list lengths, typed insertion, and indexed indirect range/access checks | Open. `[N; K]` arrays and `IndirectDrawNode` exist; no setup-length list; toon_link on the manual API |
| S5 | Setup tables that contain references to graph buffers or imported textures | 4, after the callback-validation follow-up | Complete the deferred validation design. Toon_link's material/draw tables initialize correctly. Verify dependency closure, reference ownership, lifetimes, and applicable backing allocations | Open |
| S6 | Offscreen color/depth targets, dependent draw barriers, and pass orders beyond compute followed by one unconditional main raster pass | 5 | Offscreen example, attachment/version and pipeline checks, interleaving barriers, attachment preservation across internal rendering boundaries, and tests for the final output policy | Open. `ColorAttachmentUsage`, `DepthAttachmentUsage`, `OffscreenTargets`, `MultipleRasterPasses`, `RasterInWhen` rejected |
| S7 | `Window` and `WindowDiv` graph textures | 5 | Resize, zero-extent, rounding, history policy, and descriptor/resource lifetime tests | Open. `WindowSizeClass` rejected; `ResourcePlanner::texture` only mints `Fixed` |
| S8 | Derived synchronization mode | 6 | All supported command types pass hazard tests and sweep with derivation enabled and disabled | Open. Barrier templates exist only in the unwired `compile.rs` |

The rejection variants live in `UnsupportedFeature` in
`crates/render-graph/src/runtime/validate.rs`, and `docs/render_graph.md`
lists them under "Build-time validation".

Before completing phase 7:

- Verify that every ledger row has implementation and test evidence.
- Search the validator, compiler, executor, and codegen for temporary feature
  errors, phase guards, TODOs, and unsupported-path branches.
- Verify that no supported v2 input reaches a temporary rejection.
- Verify that removal of a rejection includes validation, assembly, expansion,
  recording, synchronization, and resource lifetime support where applicable.
- Keep rejection tests for invalid inputs and permanent v2 exclusions.
- Keep the documented exclusions for nested control, mutable imports, graph
  rebuild, user-requested attachment `Load`, and offscreen MSAA. Picking redesign,
  generic readback, GPU-driven counts/indirect dispatch, and arbitrary per-run
  frame contracts are deferred work, not open ledger items.
- Resolve each open row before declaring v2 complete. If scope changes,
  obtain an explicit design decision and update the design and this ledger.

The conservative barrier mode remains available after S8 closes. It is a
supported diagnostic option, not an unfinished implementation path.
