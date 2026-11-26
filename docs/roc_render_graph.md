# Roc render graph

The Roc platform builds render graphs in pure Roc, mirroring the Rust
`mltrs-render-graph` API: explicit resource declarations, typed nodes,
lowering into the plain-data `GraphDesc` + `SchemaTable` that
`mltrs_render_graph::plain` executes, and the same logical validation. The
modules live in `roc-platform/platform/`; apps import the exposed ones as
`pf.ResourcePlanner`, `pf.RenderGraph`, `pf.GraphSet`, and `pf.RasterState`.

Nothing renders from Roc yet: `main.roc` still provides only `roc_init`, and
the host does not consume `HostGraph` or `Draw` until the host ABI is wired.

## Building a graph

```roc
(planner1, vertices) = ResourcePlanner.singleton_buffer(
    ResourcePlanner.empty,
    "vertices",
    BasicTriangle.Vertex.packer,
    [{ position: left, color: red }, { position: top, color: blue }, { position: right, color: green }],
)
(planner2, indices) = ResourcePlanner.index_buffer(planner1, "indices", [0, 1, 2], 3)
(planner3, params) = ResourcePlanner.uniform_buffer(planner2, "params", BasicTriangle.params)
(planner4, pipe) = ResourcePlanner.graphics_pipeline(
    planner3,
    { name: "basic_triangle", shader: BasicTriangle.shader, uniforms: { params: params }, raster: RasterState.default },
)

node = RenderGraph.draw_indexed(pipe, indices, params, { vertices: vertices.addr() })

Ok(graph) = RenderGraph.new(planner4, node).map_ok(GraphSet.single)

draw = |frame| graph.draw({ mvp: mvp_with_aspect_ratio(frame.aspect_ratio) })
```

A single-field record must name its field: `{ params: params }`, not
`{ params }`, which Roc parses as a block evaluating to `params`.

`RenderGraph.new` returns `Try(_, [InvalidRenderGraph(Str)])`. The new
compiler's `Try` has `map_ok`, not `and_then`/`try`; `GraphSet.single` cannot
fail, so it composes with `map_ok`.

## Resources (`ResourcePlanner`)

Each constructor appends a declaration and returns the grown planner with a
typed slot: `singleton_buffer`, `immutable_buffer`, `indirect_buffer`
(`DrawIndexedIndirectCommand` records), `cpu_updated_buffer`,
`gpu_updated_buffer`, `index_buffer`, `uniform_buffer` (from a generated
`UniformSpec`), `texture`, `imported_texture`, and the `graphics_pipeline`,
`graphics_indirect_pipeline`, and `compute_pipeline` keys. Slots expose the
Rust binding methods (`addr`, `addr_at`, `read_addr`, `current`, `previous`,
`read`, `read_previous`, `write`, `mutate`) as Roc methods.

A planner only grows, so slots from earlier planners of one chain stay valid.
Every declaration records a `ShaderReflection.Provenance`: its name and a
BLAKE3 digest of everything it declares, including initial contents. Slots,
`UniformSlot`s, and every binding they mint (`SampledTexSource`,
`StorageTexSource`, and `BufferTarget` carry a `DeclaredResource` of id and
provenance) keep it, and lowering rejects any whose provenance differs from
the entry at its id in the planner passed to `RenderGraph.new`
(`ForeignDeclaration`), so unrelated planner chains cannot alias each other's
ids. Identical declarations from two chains share a provenance and are
interchangeable. Pipeline keys compare their whole declaration.

## One parameter block per pipeline

As in Rust's `GraphDesc` (`PipelineDecl::params`) and `PlainRenderGraph`, a
render graph shader declares exactly one `ParameterBlock<T>` global: each
command stages one uniform, so a second block would never be written and its
bindings would escape hazard analysis. Validation rejects a planner pipeline
whose shader declares another count (`PipelineUniformCount`), and one whose
uniform slot's block differs in name or size from the block its shader
reflects (`PipelineUniformMismatch`). The latter also catches a foreign
uniform slot passed to a pipeline constructor, since the shader's generated
`uniform_slots` reports only indices. Codegen still describes shaders with 0
or 2+ blocks; they cannot be render graph pipelines.

## Nodes (`RenderGraph`)

`dispatch`, `draw_vertex_count`, `draw_indexed`, `draw_index_range`, and
`draw_indexed_indirect` take a pipeline key whose shader has `NoPush`. A
shader with a `PushSpec` uses the `*_with_push` variant, which also takes the
complete push input `{ data, bindings }`, fixed at build time as in Rust.
Passing push input to a `NoPush` pipeline or omitting it for a push pipeline
is a type error. A node's frame value is its uniform's `*Data`.

`upload(cpu_slot)` takes `List(elem)` each frame; `optional(inner)` takes
`[None, Some(frame)]` and supplies `Absent` for every inner value on `None`;
`repeat(body)` takes `{ count, body }`; `from_tuple_2`..`from_tuple_12` and
`list` compose graphs.

Nodes sharing a uniform slot must supply the same data each frame, since the
buffer is written once per frame: packing crashes on a mismatch, and sharing a
slot across `repeat` or `optional` scopes is a validation error.

## Lowering and validation

`RenderGraphLower.roc` mirrors `runtime/lower.rs`, except that every block
schema carries its field layout, planner buffers, textures, and imports lower
one-to-one, and uniforms and pipelines intern on use. `RenderGraphValidate.roc`
ports `runtime/validate.rs` error for error, plus the binding checks of the
Rust plain path (block sizes, layouts, buffer address bounds) and declaration
checks Rust makes at resource creation: index buffers are nonempty, have a
nonzero vertex count, and every index is below it (the error names the
buffer, position, index, and vertex count); index ranges and indirect command
ranges fit; initial items fit capacities.

## Host wire form

`ValidatedRenderGraph.HostGraph` carries every buffer (role, element size,
length, packed bytes or indices with their vertex count), uniform buffer size,
pipeline (SPIR-V, reflection JSON, uniform buffer slots, `RasterState`),
import, and the lowered `GraphDesc` and `SchemaTable`, with
`uniform_slots`/`pipeline_indices` mapping description ids to host tables. A
frame is `GraphSet.Draw { graph_id, values : List(HostFrameValue) }`, one
value per `ValueDecl`. `GraphSet.select` crashes when its selector returns a
graph the set did not register, and `GraphSet.draw` selects through it: each `ValidatedGraph` carries its host
graph's digest, and `GraphSet.contains` matches it against the set's graph at
that id. Every type at this boundary, including each tag
payload record, is nominal and non-polymorphic so `roc glue` names each Rust
struct.

## Verification

`just roc-platform roc-test` format-checks and checks every platform module
and runs the platform and `roc-platform/platform-tests/` expects, which build
graphs from hand-written shader descriptors, then checks that
`ForeignSelection.roc` crashes in both `GraphSet.select` and `GraphSet.draw`. `just roc-codegen-test` runs it
too, and additionally builds graphs from freshly generated shader modules
(`crates/cli/fixtures/roc_codegen/RenderGraphConsumer.roc`) and proves API
misuse fails to typecheck (`RenderGraphNegative.roc` and its edits).
