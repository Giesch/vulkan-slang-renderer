# Phase 2 — Additive Schemas and Compile Checks

STATUS: IMPLEMENTATION PLAN — revised for the Roc target on 2026-09-10.
Rust implementation is authorized by a separate implementation request. This document
records the plan, not completed implementation or a tested Roc API.
This document expands phase 2 of [the parent plan](../08_plain_data_graph.md).
The accepted decisions below constrain the implementation plan.

## Accepted decisions — 2026-09-09

### 1. Keep the current tuple limit

Keep the limit of 12 elements in each node tuple for now.
Use nested tuples when a graph needs more than 12 nodes.
Preserve the corresponding nested frame tuple.
Phase 2 must test the 12-element boundary and composition through nested tuples.
Do not add an unbounded composition API in this phase.

### 2. Preserve the current frame contract

Keep `GraphNode::Frame` as an associated type without a lifetime parameter.
Keep the execution input as `&N::Frame`.
Phase 2 must not require a lifetime-based replacement for this contract.
Borrowed array inputs and array storage remain decisions for phase 3b.
This decision does not select a byte representation or permit reads of uninitialized padding.
Scalar storage and padding remain subject to the phase 3a gate in the parent plan.

### 3. Insert complete typed runs and use a push-constant builder — 2026-09-10

Adopt option A: construct a complete typed run, then insert it into the draw list.
The proposed insertion contract is `push(run) -> Result<(), GraphError>`.
Preserve params and push types until insertion checks their compatibility.
Each run supplies its complete setup inputs and cannot add required frame inputs.

Use the builder pattern for push bindings on both draws and dispatches.
The method name is `.with_push_constant(...)`.
Do not require separate draw or dispatch constructors for commands with push constants.
The method accepts the generated combined `*Input` type. It must preserve complete
push bindings and the distinction between setup data and permitted per-frame data.
Compile checks must reject a command that requires push constants but omits them.
They must also reject incompatible push types and incomplete push bindings.
Existing constructor compatibility and migration steps belong in the detailed plan.

## Target evaluation and revised direction — 2026-09-10

The Rust prototype must establish portable contracts and tests. It must not require
Roc to reproduce Rust associated types, lifetimes, or mutable builders literally.
The parent plan's typed facade and plain-data core fit this target well.
The main corrections are validation timing, removal of mandatory fingerprints,
and separation of logical graph construction from live resource binding.

### 4. Keep layout metadata without mandatory fingerprints

Remove the fingerprint requirement. Do not check frame input types again through
hashes. Preserve generated complete data/binding types and their static guarantees.
Use full interface metadata for setup compatibility and existing shader reload checks.
Keep schema IDs as local table indices. They are not a public type identity protocol.

GPU field offsets, sizes, scalar kinds, nested layouts, resource semantics, and
assembly mappings remain necessary. Rust object bytes, Roc host bytes, and GPU
bytes are distinct representations. Matching metadata cannot prove itself correct.
Test generation and assembly against independently specified expected layouts.
Do not add a cache format, hash algorithm, or serialized artifact version in phase 2.

### 5. Validate source ownership as a pure graph relationship

Keep `with_uniform(uniform, bindings, |shared| body)` and its
`(UniformData, BodyFrame)` frame contract. Do not require branded Rust lifetimes.
The prototype may use copyable source tokens with private constructors.
Validation must reject a consumer outside its source's enclosing scope, an unknown
source, and independent sources that target one uniform. Compare logical source
identities, not equal schemas or equal frame values.

Roc constant evaluation can run these checks when the graph is constant.
Do not use the earlier mutable-capture example as a Roc requirement. Conversely,
immutability alone does not establish ownership. The API must also control returned
tokens, immutable reuse, and caller construction of internal descriptions.
If Roc types make an invalid case impossible, demonstrate that with a compile fixture.
Keep pure validation for relationships not proved by those fixtures.
Live pipeline/uniform identity and stale generations still need setup checks.

## Evidence from the Roc compiler

Inspected local checkout `/home/danknutson/Projects/roc`, HEAD `55f5b10260`.
This was a source inspection. No Roc compiler build or graph API experiment ran.
The design file is forward-looking. Existing tests support specific capabilities,
not a claim that the entire proposed graph API already works.

- [design.md:295](/home/danknutson/Projects/roc/design.md#L295): checking owns
  effect analysis and constant-root selection. Eligibility depends on runtime data,
  control dependency, and effects. A hosted renderer call is not the route to pure validation.
- [design.md:680](/home/danknutson/Projects/roc/design.md#L680): eligible expressions
  evaluate during checking, including diagnostic effects from `crash` and `expect`.
  Returning an error value alone is not a compile error. A Roc construction wrapper
  must explicitly report validation failure during constant evaluation.
- [design.md:699](/home/danknutson/Projects/roc/design.md#L699): records and lists can
  have shared static storage. Callable-containing constants have different restoration
  rules. Lower builder callbacks away before producing the portable description.
- [eval_comptime_finalization_tests.zig](/home/danknutson/Projects/roc/src/eval/test/eval_comptime_finalization_tests.zig):
  fixtures cover constants, cross-module evaluation, records, lists, taken/untaken
  crash branches, and failed expectations. Reuse these semantic expectations in a future spike.
- [static-data-host/platform/main.roc](/home/danknutson/Projects/roc/test/static-data-host/platform/main.roc):
  platform requirements expose concrete records, lists, tuples, and callable values.
  This is evidence for a platform boundary, not proof of a generic `Graph(Frame)` API.
- [design.md:13856](/home/danknutson/Projects/roc/design.md#L13856): host signatures
  follow checked platform declarations and the C ABI. Generated glue must preserve
  ownership and representation. Private Rust layouts are not this ABI.

## Validation responsibilities

| Restriction | Rust prototype | Eventual Roc platform |
| --- | --- | --- |
| Complete frame tuple, bindings, push contract, buffer operation types | Rust compile checks | Roc type checks |
| Source ownership, declaration use, static ranges and access conflicts | Pure setup validation | Pure constant evaluation when inputs are constant |
| Device-independent scheduling and assembly plan | Pure setup compilation | Constant evaluation when inputs are constant |
| Live resource identity, generation, descriptor bindings, device limits | Setup | Platform setup |
| Runtime-created list elements or discovered table dependencies | Setup when known | Setup when known, unless moved to build inputs |
| Upload lengths, dynamic groups/counts and resolved runtime ranges | Frame preparation before side effects | Frame preparation before side effects |
| Input adapter bugs and GPU layout correctness | Assertions and unit tests | Glue/packing tests and internal invariant checks |

A checked graph does not remove dynamic validation. Do not repeat full static graph
validation each frame. Do not expose raw byte input or incremental missing-value assembly.
The pure core stays free of Vulkan, platform calls, and live resource identities.

## Starting point in this Rust tree

Read these files before edits. Search by symbol because line numbers move.

| File | Current role and phase-2 change |
| --- | --- |
| `crates/renderer/src/renderer/render_graph.rs` | `GraphShaderParams`, `GraphBindingSet`, node constructors, tuple inputs, legacy `plan`. Add metadata bridge and prototype typed builders without replacing execution. |
| `crates/renderer/src/renderer/render_graph/desc.rs` | `SchemaDesc`, optional `SchemaLayout`, field offsets. Extend the metadata model without exposing `GraphDesc`. |
| `crates/renderer/src/renderer/render_graph/lower.rs` | Typed lowering currently constructs layout-less schemas. Pass generated supported layouts and capture private input mappings. |
| `crates/renderer/src/renderer/render_graph/compile.rs` | `build_assembly` produces steps or `Deferred`. Exercise real generated layouts through this existing compiler. |
| `crates/renderer/src/renderer/render_graph/expand.rs` | Pure `FrameShape` and expansion. Test adapter output without renderer access. |
| `crates/cli/src/build_tasks.rs` | `graph_split_def`, resource classification, reflection and alignment fixtures. Generate metadata from retained semantic information. |
| `crates/cli/templates/graph_split.rs.askama` | Keep params/data/bindings and existing assembly. Emit additive metadata/adapters. |
| `crates/cli/fixtures/check_crate` | Generated syntax/layout fixture with stub renderer types. It cannot prove real renderer API restrictions. |

Phase 1 is implemented. Its reconciled records are `01_pure_core.md` and `01_review.md`.
Production execution still uses `plan`. Pure compilation/expansion is tested scaffolding.
Do not describe phase 2 as executor migration or as closure of S1–S5.

## Implementation sequence

Complete each step and its evidence before the next dependent step.
Keep prototype-only APIs separate from production execution until their owning phase.

### P2.1 — Establish the real API compile harness

1. Add positive and negative fixtures that compile against `mltrs-renderer`.
2. Type-check functions with renderer handles as parameters. Do not allocate a GPU
   merely to test whether an API call compiles.
3. Reuse generated fixture modules where practical. Keep the CLI stub fixture for
   generated-code coverage, but do not accept its stubs as proof of API safety.
4. Run each negative case independently. Require failure at the intended operation
   and inspect the relevant diagnostic. A missing import or broken dependency is not success.
5. Pair each negative fixture with a minimally changed positive case. Use isolated
   temporary fixture output so concurrent tests cannot overwrite generated modules.

Evidence: omitted tuple element, wrong binding
kind and buffer element type fail. Their positive controls compile. Twelve-element
and nested tuples compile. Repeat uses `(LoopCount, BodyFrame)` and optional uses
`Option<BodyFrame>` without weakening completeness.

Implementation record — P2.1 is implemented.

- Harness: `crates/renderer/tests/render_graph_api_compile.rs`. Fixture crate:
  `crates/renderer/fixtures/api_compile`, one bin per case, outside the
  workspace (root `Cargo.toml` `exclude`) because its negative bins never
  compile and its `src/generated` exists only while the test runs.
- The harness generates the fixture's bindings, checks every case, then deletes
  the generated output, the same shape `alignment_tests` uses for the CLI stub
  fixture. Nothing generated is committed and no justfile recipe maintains it.
  It writes the vendored `mltrs.slang` beside the five committed slang sources,
  generates with `--import-root mltrs_renderer` instead of the default `mltrs`,
  and removes `shaders/compiled/`, `src/generated/` and the vendored module
  afterwards. `mltrs-cli` is a dev-dependency of `mltrs-renderer` for this; it
  does not depend on the renderer, so there is no cycle. Running the renderer's
  tests now needs the slang compiler, and the test is `#[cfg(not(windows))]`
  like the other codegen tests.
- The case types come from the generated modules, so they cannot drift from what
  `crates/cli/templates/` emits. Five slang sources supply them: `particle`
  (`Particle`, `OtherElement`), `sim` (`SimParams`), `tex` (`TexParams`),
  `other` (`OtherParams`), `render` (`RenderParams`).
- The fixture's `Cargo.lock` is a pruned copy of the root lock, and
  `CARGO_TARGET_DIR` points at the workspace target, so the checks reuse the
  workspace build. The pinning is load-bearing: a fresh resolve picks different
  `sdl3` versions and rebuilds SDL from source. The harness passes `--locked`,
  which `check_crate` does not, so drift fails loudly rather than silently
  duplicating heavy dependency builds. `docs/testing.md` records the recovery
  step.
- The missing-binding negative case and its positive control were removed as
  redundant with the generated required fields. The harness now has 12 cases.
- Case set (positive control listed beside each negative):
  - omitted tuple element: `negative_omitted_tuple_element` fails E0308
    (`RenderParamsData` appears only in the expected frame type); control
    `positive_complete_tuple`.
  - wrong binding kind: `negative_wrong_binding_kind` fails E0308
    (`SampledTexBinding` vs `StorageTexBinding`); control
    `positive_right_binding_kind`.
  - wrong buffer element type: `negative_wrong_buffer_element_type` fails E0308
    (`BufferBinding<Particle>` vs `BufferBinding<OtherElement>`); control
    `positive_right_buffer_element_type`.
  - repeat: `positive_repeat_frame` pins `Frame = (LoopCount, SimParamsData)`;
    `negative_repeat_frame_without_loop_count` fails E0308 on the bare count.
  - optional: `positive_optional_frame` pins `Frame = Option<Vec<Particle>>`;
    `negative_optional_frame_without_option` fails E0308 on the bare body.
  - `positive_tuple_twelve_elements` and `positive_nested_tuples` compile;
    the nested case composes upload, repeat, and optional inside the tuples.
- The CLI stub fixture (`crates/cli/fixtures/check_crate`) is unchanged. It
  compiles generated code against stub renderer types, which needs no graphics
  stack. This harness compiles generated code against the real renderer.
- Each negative case is one `cargo check --bin` invocation. The harness rejects
  a failure caused by an unresolved import or dependency (E0432/E0433/E0463),
  and requires the expected error code, the expected type names, and the case
  file name in the diagnostic. The file name pins the failure to the case
  source rather than to a generated module or the renderer. The harness collects
  every case result, cleans up, then asserts, so a failure reports all cases and
  leaves no generated files behind.
- No case constructs a `Renderer`. Every case type-checks functions with
  renderer handles as parameters. A case that declares graph textures takes
  `&mut GraphResources`, because `RenderGraph::new` consumes the collection.
- The harness itself runs `cargo`, so it waits when another build holds the
  build-directory lock. Under `cargo test` it does not deadlock: the outer
  build releases the lock before the test binaries run.
- Original gates run before removing the missing-binding pair: the harness
  (generation plus 14 cases, 2.5 s once the workspace is
  built), `cargo check --workspace --all-targets`,
  `cargo test -p mltrs-renderer`, `just lint`, `just test`, `just sweep`,
  `cargo fmt`. A vacuity check confirmed the harness fails when a negative case
  is changed to compile.

### P2.2 — Add metadata without choosing a new frame ABI

1. Describe GPU size/alignment, field offsets/widths, nested types, and resource kinds.
   Retain read/write/immutable pointer meaning and pointee information where relevant.
2. Keep CPU data layout and GPU layout separate. Do not use `size_of::<Data>()`
   as proof of a packed input representation. Do not derive a byte slice from `Copy`
   or the existing `GPUWrite` marker.
3. Add an explicitly supported/deferred metadata result. Resource-only schemas and
   explicit byte-layout fixtures establish the first supported assembly path.
   GPU metadata may describe additional layouts without claiming safe input encoding.
4. Preserve all existing split types and traits. A separate additive metadata trait
   is preferable to forcing every legacy implementation to adopt a new byte ABI.
   Export only the metadata types needed by generated downstream code.
5. Preserve existing rejections for nested resource-bearing fields. Additive metadata
   must not silently enable their assembly or table dependency handling.
6. Preserve S1 and S2. Report deferred schema support explicitly on the v2 path.
   Legacy tuple generation must remain usable. Do not encode unsupported fields as empty layouts.

Evidence: resource-only, binding-free, nested data, interleaved data/resources,
synthetic padding, and user fields with padding-like names have metadata snapshots
or explicit deferred cases. Supported generated metadata reaches `build_assembly`.
Expected offsets and operations come from fixture specifications, not copied generator output.

### P2.3 — Prototype automatic typed input adaptation

1. Capture private value destinations during lowering. Do not derive a second mapping
   by independently traversing nodes and assuming both traversals allocate identical IDs.
2. Adapt complete tuples recursively. Adapt `Some` with the complete body and `None`
   with absence of all body work. Repeat reuses its one complete body input.
3. Populate `FrameShape` and explicit supported fixture data without renderer access.
   Keep scalar serialization behind the P2.2 support boundary until phase 3a.
4. Test every expected destination exactly once, including identical node types at
   different tuple positions. Missing destinations are internal errors, not public defaults.
5. Keep actual uploads, acquisition, staging and submission on the existing path.
   Phase 3 integrates the adapter after storage decisions and byte-safety tests.

Evidence: zero/odd/even repeats, nested tuple ordering, optional coupled upload/dispatch
presence, and consecutive frames that alternate `Some` and `None`. Test the coupling
through shape fixtures without claiming phase-3b upload execution is implemented.

### P2.4 — Prototype complete draw and dispatch builders

1. Retain params/push interfaces in pipeline tokens until insertion or node completion.
   A bare pipeline index is insufficient. Do not add an unchecked public retyping operation.
2. Use one constructor family per command form. Add `.with_push_constant(...)` to
   attach the complete push contract. Accept the generated flat `*Input` intermediate
   so data and typed resource bindings remain together.
3. An unfinished command that requires push constants cannot become a graph node or
   enter a list. A no-push command can finish directly. Wrong push types must fail.
4. Implement typed list insertion as `push(complete_run) -> Result<(), GraphError>`
   in the prototype. Check static type compatibility before erasing the stored run.
5. Setup checks cover resource identities and ranges. An empty list and a populated
   list must have the same frame type. Runs contain setup push data/bindings only.
6. Keep legacy push constructors as compatibility wrappers where possible until
   their consumers migrate. Do not remove them merely to demonstrate the new syntax.

Evidence: different compatible pipeline instances compile. Missing push attachment,
incomplete push bindings, wrong params/push interface, and extra per-run frame input
fail. Zero, one, and several runs use one frame contract. Full raster execution remains phase 4.

### P2.5 — Prototype uniform source ownership

1. Represent source identity and enclosing ownership explicitly during lowering.
   Each scope creates one source. Consumers reference it and add no duplicate uniform data.
2. Validate ownership from explicit scope information, not names, schema equality,
   physical-slot coincidence, or runtime value equality.
3. Test an unknown source, a token reused outside its owner, an optional source with
   an unconditional external consumer, and independent sources targeting one uniform.
4. Test a valid shared source with several consumers and with an empty list. Retain
   and validate an unused source and its bindings. Do not infer GPU accesses from assembly.
5. Keep scope frame input `(UniformData, BodyFrame)`. Test unrelated body inputs and
   a missing outer uniform value. Keep source staging/retention execution work in phase 3b.

Evidence: pure tests reject invalid relationships and accept valid shared ownership.
Compile tests prove complete frame inputs. Do not add lifetime branding as a phase gate.

### P2.6 — Preserve buffer restrictions and record the handoff

Test the real current API's permitted pointer conversions and rejection of writes
through read-only bindings. Test wrong element types and previous-slot access on
unsupported buffer kinds. Do not implement Immutable uploads here to satisfy a test.
The v2 prototype must reject frame uploads to GpuOnlyFlight and Singleton and admit
an Immutable upload contract. Production upload migration remains phase 3b.
Label prototype tests separately from tests of the existing Storage upload path.

Record the chosen metadata bridge, supported/deferred schema cases, builder argument
shape, fixture commands, and remaining gates. Link each claim to implementation/tests.
Keep all ledger rows open until their owning phase supplies execution evidence.

## Roc port proof required before platform implementation

This proof is future port work, not a claim established by Rust tests or a request
to modify the Roc compiler during phase 2. Keep its fixtures in this repository when implemented.

- Express a typed graph/input pair and generated complete bindings using actual Roc
  syntax. Test missing inputs and incompatible pipelines. Establish how abstract
  graph construction prevents unchecked values. Do not assume Rust trait syntax translates.
- Build a small constant graph of records/lists. Run pure validation and compilation
  without hosted calls. Prove a valid graph passes `roc check` and an invalid graph
  produces a compile diagnostic. Returning `Err` without consuming it is insufficient.
- Test source ownership through ordinary immutable value reuse. Use a compile failure
  when the API prevents construction, otherwise a constant-evaluation validation failure.
- Pass a compiled constant graph through a minimal platform. Check concrete generated
  glue, host ownership, and frame packing with sentinels. No Vulkan is needed for this proof.
- Use shared expected semantic cases for Rust and Roc: source IDs, schema fields,
  access errors, and command order. Compare structured results, not language-specific diagnostics.
- Record which graph parts actually become static data. Keep runtime frame values
  and live resource binding separate from that evidence.

## Future Roc decisions — 2026-09-10

Scene structure is known at build time through assets imported into Roc. Import
and parse JSON or another asset format to determine material counts and draw runs.
This layer does not change at runtime. Rust setup-defined lists remain the prototype
for that fixed structure. Live GPU allocation and resource binding still run at setup.
This decision does not move callback-discovered GPU addresses into constant evaluation.

The future platform API will use encoders/decoders for a custom data format.
A mechanism for constructing a heterogeneous typed tuple for `execute`, comparable
to Rust tuple composition, remains unexplored future design work. Do not implement
or specify the Roc API now. The port proof above is deferred until that design work.
The custom format's schema, encoding, ownership and compatibility rules are also
future work. Do not assume Rust or Roc memory layouts define the encoded format.

## Combined generated input — 2026-09-10

Generate a flat `*Input` intermediate type with both ordinary data fields and typed
resource binding fields. Preserve existing `*Data` and `*Bindings` types and the
frame tuple contract. `.with_push_constant(...)` accepts the pipeline's generated
input type directly. The GPU struct remains separate from the unresolved input.

`GraphShaderParams::Input` identifies the generated intermediate. `input` combines
split data/bindings, and `assemble_input` resolves that intermediate to the GPU struct.
The existing `assemble` entry point delegates through both operations. Existing
push constructors remain compatible through `PushValues<B>` and `push_values`.
A pipeline that requires a push block creates an unfinished command until the caller
supplies its matching input. Unfinished commands do not implement `GraphNode`.

Example: `.with_push_constant(BlurDispatchInput { input_tex, output_tex, direction })`.
Push data remains fixed at setup, and resource references resolve per command execution.
This change implements part of P2.4. It does not complete metadata, list insertion,
uniform scopes, or the remaining phase-2 work.

Scalar packing (3a), arrays (3b), callback dependencies (before S5), and resize (5)
retain their existing decision gates. This plan does not resolve them incidentally.

## Completion checks

For implementation, run the new compile harness and relevant pure unit tests first.
Then run the parent phase gates: `cargo check --workspace --all-targets`,
`cargo test -p mltrs-renderer`, `just lint`, `just sweep`, and `cargo fmt`.
Codegen changes also require `just test` and `just shaders`. Inspect regenerated
snapshots and example changes. Report unavailable hardware/tool prerequisites explicitly.

Phase 2 completes when supported generated schemas feed pure assembly, automatic
adapters pass their pure tests, complete-run/source prototypes satisfy their fixtures,
and existing examples retain behavior. This does not claim executor integration,
a finished Roc API, or a closed later-phase ledger row.
