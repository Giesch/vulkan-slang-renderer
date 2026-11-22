# Vertex input types and independent mesh binding

Date: 2026-11-08. Status: proposed; no implementation in this note.

This is the revised option 2 from the VertexBytes discussion: erase upload
storage while retaining the pipeline's vertex interface through construction
of a draw. Independent pipeline/mesh binding is a near-term requirement.

The baseline inspected was `roc-graphs/runtime-shaders` at `f9b4de5`, with
later consumers inspected through `roc-graphs/graph-set-example` at `8ac67e8`.
Recheck branch tips and stack state before implementation. This note describes
a proposed change, not the current API. Track the immediate debt in
[tech_debt.md, entry 20](tech_debt.md#20-remove-the-placeholder-vertexdescription-implementation-on-vertexbytes).

## Problem and existing behavior

- [vertex_description.rs](../crates/renderer/src/renderer/vertex_description.rs#L5)
  requires implementors of `VertexDescription` to implement the renderer's
  `GPUWrite` upload marker as well.
  `VertexBytes` implements both traits, but its layout methods return empty
  vectors. It represents packed data with a runtime layout, not a vertex with
  a statically known layout.
- [pipeline.rs](../crates/renderer/src/renderer/pipeline.rs#L355) carries
  `V: VertexDescription` through the config and builder. `VertexConfig<V>`
  contains both `Vec<V>` and packed-byte variants. Even the byte path therefore
  inherits the bounds needed to compile the typed upload arm.
- Pipeline initialization obtains the actual layout from `ShaderAtlasEntry`.
  [RuntimeShader](../crates/renderer/src/shaders/runtime.rs#L82) derives its
  descriptions from reflection; the empty VertexBytes methods are not the
  source of the current runtime pipeline layout.
- The placeholder also exposes `with_vertices(Vec<VertexBytes>, ...)` and
  typed mesh creation, although VertexBytes is zero-sized. The nonempty-vector
  checks do not make that a meaningful upload.
- Generated builders and `MeshHandle<V>` check typed mesh selection today.
  `PipelineHandle<D, P>` then forgets the vertex type. Vertex/index buffers or
  a shared-mesh index remain attached to the renderer's pipeline storage.

Simply deleting the empty methods leaves the bounds unsatisfied. Removing
every bound also fails: the typed upload arm still needs the upload contract.
Replacing VertexBytes with `u8` or NoVertex would retain the underlying design
problem.

## Design decision

Separate three responsibilities:

1. **Shader input interface:** the layout a pipeline expects, with a nominal
   Rust vertex type when one is known.
2. **Upload storage:** owned typed data or already-packed bytes, used while
   creating GPU resources.
3. **Draw binding:** the association between a pipeline and compatible mesh.

The first change removes VertexBytes and its placeholder implementations.
It preserves interface information in the completed public config and handle.
This supersedes the earlier option 2 proposal to remove the vertex parameter
from the completed config entirely.

Use separate interface states, with names finalized during implementation:
`TypedVertex<V>`, `RuntimeVertex`, and `NoVertexInput`. These states describe
the interface; they implement neither `VertexDescription` nor `GPUWrite`.
Bounds belong on operations that describe or upload actual vertex elements.
Retain VertexDescription for real generated vertex types for this change.

A possible public shape is `PipelineConfig<'a, Interface, Draw, Push>` and a
graphics handle wrapper carrying the same interface around the existing raw
pipeline handle. A typed mesh continues to carry `V`; a runtime mesh carries
layout metadata. A wrapper avoids adding a meaningless vertex parameter to
compute handles. Exact names are illustrative, but preserving the interface
through the returned graphics handle is an acceptance requirement.

Constructors must establish the association between shader and interface.
Do not expose a safe, unconstrained cast from a raw handle to an arbitrary
typed handle. Audit the existing public `build_indexed<V, P>` escape hatch:
generated shader associations or explicit layout validation must support the
claimed guarantee. A phantom type alone does not validate a shader.

## Phase 1: erase upload storage and preserve interface identity

1. Replace `VertexConfig<V>`'s typed storage with a private upload object that
   owns the original `Vec<V>`. Its implementation retains the upload bound on
   `V`; the surrounding config does not need that bound. Keep packed bytes and
   shared meshes as explicit source cases.
2. Separate preallocation validation from upload. Preserve rejection of empty
   vertices/indices and malformed packed byte lengths before GPU allocation.
   Keep layout descriptions sourced from the shader entry.
3. Keep typed builders that accept only `Vec<V>` or `MeshHandle<V>` matching
   their interface. Their completed configs retain `TypedVertex<V>` as a
   witness independently of the erased upload storage.
4. Add a dedicated packed-data builder entry point, provisionally
   `build_indexed_bytes<P>(bytes, indices)`. It produces a runtime-interface
   config and exposes no typed vertex-upload or typed shared-mesh methods.
   Vertex-count construction produces a no-input config. Delete VertexBytes
   and its trait implementations.
5. Return graphics handles that retain the interface witness. Thread the new
   handles through existing draw constructors and pipeline-key adapters.
   Preserve draw-family and push-block checks. During this phase, a draw may
   still use the source fixed at pipeline creation; lower to existing raw keys
   only after the construction API has established the applicable guarantees.
6. Update Rust codegen, its compile fixture, generated bindings, and snapshots
   to describe the revised config and handle types. Audit explicitly typed
   example fields, picking, indirect draws, and shared meshes. Regenerate
   generated files through the CLI workflow; do not edit them by hand.
7. Update current reference documentation, especially
   [docs/render_graph.md](../docs/render_graph.md), for the implemented API.

The upload object is a setup-time allocation and dispatch, not a frame-loop
operation. Preserve typed storage until the existing upload operation copies
it. Do not reinterpret a Vec<V> allocation as Vec<u8> or read uninitialized
Rust padding as ordinary byte values. GPUWrite is not a general byte-container
conversion contract. Follow the graph's existing MaybeUninit staging rules
where staging is involved.

Keep ownership and failure cleanup explicit when moving uploads out of the
generic initializer. Unused configs must drop their CPU data normally, and a
failed upload must not leave new allocations without an owner. This is not
a mandate to fix every pre-existing init cleanup issue in tech debt entry 1.

**Phase 1 exit criteria:** VertexBytes is absent; packed input needs no dummy
layout or upload trait implementation; actual typed inputs retain their
checks; the completed typed config and returned handle preserve vertex
identity. Existing draw behavior and byte-layout rules remain compatible.
Independent mesh selection is not yet claimed by this milestone.

## Phase 2: independent pipelines and meshes

1. Make pipeline creation depend on shader interface, resources, and pipeline
   state, without requiring vertex data. Move vertex/index resource ownership
   into meshes. Typed and runtime mesh constructors establish their layouts.
2. Make an indexed draw require both a pipeline and mesh. Typed constructors
   require matching nominal vertex types. Runtime constructors or preparation
   validate the mesh layout against the pipeline's expected layout and produce
   a private validated binding. Byte-count divisibility alone is insufficient.
3. For the current single-stream layout, compare stride, attribute locations,
   offsets, and formats, with the supported binding and input-rate rules.
   Compare structural GPU layout properties, not reflected type/field names.
   Start with exact supported-layout equality; subset matching, multiple
   streams, and instancing need separately specified compatibility rules.
4. Move the completeness invariant to the draw: a pipeline can exist without
   a mesh, but an indexed draw cannot execute without a compatible mesh.
   A convenience API may create both resources together and return a bound
   draw helper; it must use the same independent resource model.
5. Carry pipeline and mesh identifiers through graph lowering and backend
   interfaces. Resolve vertex/index buffers and whole-mesh counts from the
   selected mesh during recording. Preserve explicit index ranges and indirect
   draw behavior. Audit backend range checks that currently consult pipelines.
6. Retain the resources needed by prepared graphs under the existing ownership
   policy. A runtime compatibility token must identify its validated resource
   pair; it cannot authorize an arbitrary later replacement. Any future mesh
   substitution must pass through typed selection or runtime validation again.
   Keep shader interface-change handling consistent with these guarantees.
7. Migrate examples and the Roc host. The host can initially create a runtime
   mesh for each existing pipeline declaration and bind it in each draw, so
   independent renderer binding does not itself require a Roc wire-format
   change. Sharing meshes independently in the Roc API is an additional step.

Typed construction and runtime validation should converge on the same lowered
draw representation. A prepared graph with fixed bindings need not retain a
Rust vertex type after compatibility is established. Avoid repeated layout
comparisons on every frame.

Keep graph behavior backend-neutral: no Vulkan, SDL, or Slang dependencies in
`crates/render-graph`. Follow its
[AGENTS.md](../crates/render-graph/AGENTS.md) when changing backend interfaces.

**Phase 2 exit criteria:** one pipeline draws two compatible meshes; one mesh
works with two compatible pipelines; typed mismatches fail construction at
compile time; runtime layout mismatches fail before submission; recording and
teardown use the selected mesh correctly.

## Stack placement and downstream changes

Use `gh stack view --json` and actual branch history before selecting the owner
branch. If #37 is still open, put Phase 1's renderer/codegen work there and
adapt consumers in their owning layers before rebasing the rest. If it has
landed, use a new follow-up branch. Keep Phase 2 a separate dependent change
with its own migration and validation. This plan does not authorize rewriting
or publishing the stack merely to record the design.

| Layer inspected | Phase 1 impact |
| --- | --- |
| #37 runtime-shaders | Erased upload storage, interface-bearing configs and handles, graph adapters, Rust codegen/fixtures, generated Rust, and reference docs. |
| #38 roc-codegen | Preserve existing Roc packers and shared layout rules; no required Roc schema change. |
| #39 roc-blueprints | Preserve existing packed mesh declarations and vertex-type checks. |
| #40 platform-host | Replace the VertexBytes builder call/import; adapt stored graphics handle types and conversions into dynamic graph nodes. |
| #42–44 frame-elapsed, ok-destructure, graph-set-example | Inherit the adapted host; verify their examples and lifecycle behavior at the tip. |

The platform-host change is larger than the earlier minimal option 2 estimate:
retaining interface-bearing handles also affects its indexed/vertex-count
collections and DynDrawNode adapters, not just its builder call. Dynamic node
adapters must preserve draw-family checks and the fixed-source guarantee in
Phase 1; Phase 2 adds mesh selection and compatibility validation there.

No ABI/glue regeneration follows merely from the Phase 1 Rust API change.
If Phase 2 later changes Roc declarations to expose shared meshes, that separate
schema change requires corresponding lowering, validation, glue, and examples.

## Verification during implementation

Read [docs/testing.md](../docs/testing.md) before adding checks or accepting
snapshots. This documentation-only plan does not establish test results.

- Start Rust verification with `cargo check --workspace --all-targets`; run
  `cargo fmt` after edits and `just lint` for the completed change.
- Run `just shaders` for regenerated Rust bindings and `just test` whenever
  build_tasks.rs or CLI templates change. Review snapshot differences.
- Extend the renderer API compile harness with positive typed/runtime cases
  and focused negative cases: mismatched typed meshes, unavailable typed
  methods on the runtime builder, and inability to relabel a handle safely.
- Preserve packed-byte stride rejection and reflected-layout coverage. Test
  runtime compatibility with equal-stride but different-offset/format layouts
  once Phase 2 introduces independent binding.
- For graph changes, run `cargo test -p mltrs-render-graph`, including backend
  and dependency-closure coverage. Test both mesh-sharing directions and
  rejection before submission rather than merely comparing helper outputs.
- Run `just sweep` to check Vulkan recording and teardown, including typed,
  shared-mesh, vertex-count, and indirect paths affected by the implementation.
- Check/build the Roc host separately; it is excluded from the root workspace.
  At the stack tip run `just roc-platform test`. Its release host runs establish
  linking and execution, not Vulkan validation coverage. Use the renderer
  sweep for validation, and exercise the packed runtime path in validation
  coverage if existing sweep cases do not reach it.

## Deferred extensions

Shader-owned layout descriptions shared by generated and runtime shader entries
could eventually eliminate VertexDescription entirely. That is independent of
removing its placeholder implementation and is not required by this plan.
Multiple vertex streams, instancing, per-frame mesh substitution, and broader
resource-lifetime refactors remain separate extensions. The retained interface
and binding boundary should accommodate them without inventing fake vertex
element types.
