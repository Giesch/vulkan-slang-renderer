import pf.ShaderReflection exposing [BlockSchema, ComputeShader, NoPush, UniformSlot]
import pf.ResourcePlanner
import pf.RenderGraph
import pf.GraphSet
import pf.Game

## Negative controls `just roc-platform roc-test` expects to crash:
## `GraphSet.select` and `GraphSet.draw` reject a graph their set did not
## register, and a `Game.new` game rejects a submission from a set other
## than the one it registered.
ForeignSelection := {}

schema : BlockSchema({ value : U32 }, {})
schema = {
	name: "Params",
	gpu_size: 4,
	data_size: 4,
	resource_fields: [],
	fields: [{ key: 0, offset: 0, len: 4, kind: Data({ src_offset: 0 }) }],
	pack_data: |data| ShaderReflection.u32_bytes(data.value),
	extract_bindings: |_| [],
}

shader : ComputeShader({ params : UniformSlot({ value : U32 }, {}) }, NoPush)
shader = ComputeShader.{
	name: "count",
	compute_spv: [1],
	reflection_json: "{}",
	reflection: {
		source_file_name: "count.compute.slang",
		global_parameters: [
			ParameterBlock({ parameter_name: "params", element_type: ShaderReflection.StructType.{ type_name: "Params", fields: [] } }),
		],
		compute_entry_point: { entry_point_name: "main", stage: Compute, parameters: [] },
		workgroup_size: { x: 1, y: 1, z: 1 },
		pipeline_layout: {
			descriptor_set_layouts: [
				{
					binding_ranges: [
						{ binding: 0, descriptor_type: ConstantBuffer, descriptor_count: 1, stage_flags: All, size: 4 },
					],
				},
			],
			push_constant_ranges: [],
			bindless_heap_set: None,
		},
	},
	workgroup_size: { x: 1, y: 1, z: 1 },
	uniform_slots: |slots| [slots.params.index()],
	push: NoPush.{},
}

(planner1, params) = ResourcePlanner.uniform_buffer(ResourcePlanner.empty, "params", { parameter_name: "params", index: 0, schema })

(planner, pipe) = ResourcePlanner.compute_pipeline(planner1, { name: "count", shader, uniforms: { params: params } })

Ok(graphs) =
	RenderGraph.new(planner, RenderGraph.dispatch(pipe, params, { x: 1, y: 1, z: 1 }, {}))
		.map_ok(GraphSet.single)

twice = GraphSet.map2(graphs, graphs, |_, right| right)

Ok(other) =
	RenderGraph.new(planner, RenderGraph.dispatch(pipe, params, { x: 2, y: 1, z: 1 }, {}))
		.map_ok(GraphSet.single)

## A set whose build returns another set's graph instead of its own.
foreign = GraphSet.map2(graphs, graphs, |_, _| other.register())

expect graphs.select(|_| twice.register()).draw({ value: 1 }).to_host().graph_id == 1

expect foreign.draw({ value: 1 }).to_host().graph_id == 0

## `graphs` and `other` share a type but not a definition: this game
## registers `graphs` and submits `other`'s graph at the same id.
mismatched : Game
mismatched = Game.new({
	init!: |_| Game.Init.titled("foreign submission"),
	graphs,
	draw: |_frame| other.draw({ value: 1 }),
})

expect Game.draw(mismatched, { aspect_ratio: 1.0, elapsed: 0.0 }).graph_id == 0

## The positive control: drawing from the registered set is accepted.
matched : Game
matched = Game.new({
	init!: |_| Game.Init.titled("own submission"),
	graphs,
	draw: |_frame| graphs.draw({ value: 1 }),
})

expect Game.draw(matched, { aspect_ratio: 1.0, elapsed: 0.0 }).graph_id == 0
