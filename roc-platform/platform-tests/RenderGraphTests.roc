import pf.ShaderReflection exposing [
	BlockSchema,
	BufferBinding,
	ComputeReflection,
	ComputeShader,
	ElementPacker,
	GraphicsIndirectShader,
	GraphicsReflection,
	GraphicsShader,
	ImmutableBufferBinding,
	NoPush,
	PushSpec,
	SampledTexBinding,
	StorageTexBinding,
	UniformSlot,
	UniformSpec,
]
import pf.ResourcePlanner
import pf.RenderGraph
import pf.GraphSet
import pf.RasterState

## Builds graphs through the public API from hand-written shader
## descriptors, the same shapes `mltrs shaders compile` generates, and checks
## the lowered host graph, the packed frame values, and validation errors.
RenderGraphTests := {}

# --- Hand-written shaders ----------------------------------------------------

empty_entry_point = |stage| { entry_point_name: "main", stage, parameters: [] }

empty_layout = { descriptor_set_layouts: [], push_constant_ranges: [], bindless_heap_set: None }

## A reflected `ParameterBlock` global per `(type_name, size)`, each with
## its constant buffer, as a render graph pipeline checks them.
blocks_layout : List((Str, U64)) -> ShaderReflection.PipelineLayout
blocks_layout = |blocks| {
	ranges = blocks.map_with_index(
		|(_, size), binding| {
			binding: binding.to_u32_wrap(),
			descriptor_type: ConstantBuffer,
			descriptor_count: 1,
			stage_flags: All,
			size,
		},
	)

	{ ..empty_layout, descriptor_set_layouts: [{ binding_ranges: ranges }] }
}

block_globals : List((Str, U64)) -> List(ShaderReflection.GlobalParameter)
block_globals = |blocks|
	blocks.map(
		|(type_name, _)|
			ParameterBlock({
				parameter_name: "params",
				element_type: ShaderReflection.StructType.{ type_name, fields: [] },
			}),
	)

graphics_reflection : List((Str, U64)) -> GraphicsReflection
graphics_reflection = |blocks| {
	source_file_name: "test.shader.slang",
	global_parameters: block_globals(blocks),
	vertex_entry_point: empty_entry_point(Vertex),
	fragment_entry_point: empty_entry_point(Fragment),
	pipeline_layout: blocks_layout(blocks),
}

compute_reflection : List((Str, U64)) -> ComputeReflection
compute_reflection = |blocks| {
	source_file_name: "test.compute.slang",
	global_parameters: block_globals(blocks),
	compute_entry_point: empty_entry_point(Compute),
	workgroup_size: { x: 8, y: 8, z: 1 },
	pipeline_layout: blocks_layout(blocks),
}

Vertex : { id : U32 }

vertex_packer : ElementPacker(Vertex)
vertex_packer = { stride: 16, to_bytes: |vertex| ShaderReflection.pack(16, [(0, ShaderReflection.u32_bytes(vertex.id))]) }

u32_packer : ElementPacker(U32)
u32_packer = { stride: 4, to_bytes: ShaderReflection.u32_bytes }

## One `U32` of data at offset 0 and an address at 8.
TintData : { tint : U32 }

VertexBindings : { vertices : ImmutableBufferBinding(Vertex) }

triangle_schema : BlockSchema(TintData, VertexBindings)
triangle_schema = {
	name: "TriangleParams",
	gpu_size: 16,
	data_size: 4,
	resource_fields: [BufAddr],
	fields: [
		{ key: 1, offset: 0, len: 4, kind: Data({ src_offset: 0 }) },
		{ key: 0, offset: 8, len: 8, kind: BufAddr },
	],
	pack_data: |data| ShaderReflection.u32_bytes(data.tint),
	extract_bindings: |bindings| [(0, bindings.vertices.erase())],
}

triangle_params : UniformSpec(TintData, VertexBindings)
triangle_params = { parameter_name: "params", index: 0, schema: triangle_schema }

TriangleUniforms : { params : UniformSlot(TintData, VertexBindings) }

triangle_shader : GraphicsShader(TriangleUniforms, NoPush)
triangle_shader = GraphicsShader.{
	name: "triangle",
	vertex_spv: [1, 2, 3, 4],
	fragment_spv: [5, 6, 7, 8],
	reflection_json: "{\"source_file_name\":\"triangle\"}",
	reflection: graphics_reflection([(triangle_schema.name, triangle_schema.gpu_size.to_u64())]),
	uniform_slots: |slots| [slots.params.index()],
	push: NoPush.{},
}

indirect_shader : GraphicsIndirectShader(TriangleUniforms, NoPush)
indirect_shader = GraphicsIndirectShader.{
	name: "indirect",
	vertex_spv: [9],
	fragment_spv: [10],
	reflection_json: "{}",
	reflection: graphics_reflection([(triangle_schema.name, triangle_schema.gpu_size.to_u64())]),
	uniform_slots: |slots| [slots.params.index()],
	push: NoPush.{},
}

## A graphics shader that writes a storage texture, which draws may not.
StorageBindings : { target : StorageTexBinding }

storage_schema : BlockSchema({}, StorageBindings)
storage_schema = {
	name: "StorageParams",
	gpu_size: 8,
	data_size: 0,
	resource_fields: [StorageTex],
	fields: [{ key: 0, offset: 0, len: 8, kind: StorageTex }],
	pack_data: |_| [],
	extract_bindings: |bindings| [(0, bindings.target.erase())],
}

storage_params : UniformSpec({}, StorageBindings)
storage_params = { parameter_name: "params", index: 0, schema: storage_schema }

storage_draw_shader : GraphicsShader({ params : UniformSlot({}, StorageBindings) }, NoPush)
storage_draw_shader = GraphicsShader.{
	name: "storage_draw",
	vertex_spv: [1],
	fragment_spv: [2],
	reflection_json: "{}",
	reflection: graphics_reflection([(storage_schema.name, storage_schema.gpu_size.to_u64())]),
	uniform_slots: |slots| [slots.params.index()],
	push: NoPush.{},
}

## A compute shader reading one texture through its uniform and writing
## one through its push block, like a Jacobi step.
StepData : { weight : U32 }

SampleBindings : { source : SampledTexBinding }

step_schema : BlockSchema(StepData, SampleBindings)
step_schema = {
	name: "StepParams",
	gpu_size: 16,
	data_size: 4,
	resource_fields: [SampledTex],
	fields: [
		{ key: 0, offset: 0, len: 8, kind: SampledTex },
		{ key: 1, offset: 8, len: 4, kind: Data({ src_offset: 0 }) },
	],
	pack_data: |data| ShaderReflection.u32_bytes(data.weight),
	extract_bindings: |bindings| [(0, bindings.source.erase())],
}

step_params : UniformSpec(StepData, SampleBindings)
step_params = { parameter_name: "params", index: 0, schema: step_schema }

PushData : { iteration : U32 }

PushBindings : { input : SampledTexBinding, output : StorageTexBinding }

step_push_schema : BlockSchema(PushData, PushBindings)
step_push_schema = {
	name: "StepPush",
	gpu_size: 24,
	data_size: 4,
	resource_fields: [SampledTex, StorageTex],
	fields: [
		{ key: 0, offset: 0, len: 8, kind: SampledTex },
		{ key: 1, offset: 8, len: 8, kind: StorageTex },
		{ key: 2, offset: 16, len: 4, kind: Data({ src_offset: 0 }) },
	],
	pack_data: |data| ShaderReflection.u32_bytes(data.iteration),
	extract_bindings: |bindings| [(0, bindings.input.erase()), (1, bindings.output.erase())],
}

StepUniforms : { params : UniformSlot(StepData, SampleBindings) }

step_shader : ComputeShader(StepUniforms, PushSpec(PushData, PushBindings))
step_shader = ComputeShader.{
	name: "step",
	compute_spv: [11, 12],
	reflection_json: "{}",
	reflection: compute_reflection([(step_schema.name, step_schema.gpu_size.to_u64())]),
	workgroup_size: { x: 8, y: 8, z: 1 },
	uniform_slots: |slots| [slots.params.index()],
	push: { parameter_name: "push", schema: step_push_schema },
}

## A compute shader writing a CPU-updated buffer.
CountData : { count : U32 }

ParticleBindings : { particles : BufferBinding(U32) }

spawn_schema : BlockSchema(CountData, ParticleBindings)
spawn_schema = {
	name: "SpawnParams",
	gpu_size: 16,
	data_size: 4,
	resource_fields: [BufAddr],
	fields: [
		{ key: 0, offset: 0, len: 8, kind: BufAddr },
		{ key: 1, offset: 8, len: 4, kind: Data({ src_offset: 0 }) },
	],
	pack_data: |data| ShaderReflection.u32_bytes(data.count),
	extract_bindings: |bindings| [(0, bindings.particles.erase())],
}

spawn_params : UniformSpec(CountData, ParticleBindings)
spawn_params = { parameter_name: "params", index: 0, schema: spawn_schema }

spawn_shader : ComputeShader({ params : UniformSlot(CountData, ParticleBindings) }, NoPush)
spawn_shader = ComputeShader.{
	name: "spawn",
	compute_spv: [13],
	reflection_json: "{}",
	reflection: compute_reflection([(spawn_schema.name, spawn_schema.gpu_size.to_u64())]),
	workgroup_size: { x: 64, y: 1, z: 1 },
	uniform_slots: |slots| [slots.params.index()],
	push: NoPush.{},
}

## A compute shader whose block has neither data nor bindings.
empty_schema : BlockSchema({}, {})
empty_schema = {
	name: "EmptyParams",
	gpu_size: 16,
	data_size: 0,
	resource_fields: [],
	fields: [],
	pack_data: |_| [],
	extract_bindings: |_| [],
}

empty_params : UniformSpec({}, {})
empty_params = { parameter_name: "params", index: 0, schema: empty_schema }

empty_shader : ComputeShader({ params : UniformSlot({}, {}) }, NoPush)
empty_shader = ComputeShader.{
	name: "empty",
	compute_spv: [14],
	reflection_json: "{}",
	reflection: compute_reflection([(empty_schema.name, empty_schema.gpu_size.to_u64())]),
	workgroup_size: { x: 1, y: 1, z: 1 },
	uniform_slots: |slots| [slots.params.index()],
	push: NoPush.{},
}

## A compute shader writing two storage textures through its uniform.
PairBindings : { first : StorageTexBinding, second : StorageTexBinding }

pair_schema : BlockSchema({}, PairBindings)
pair_schema = {
	name: "PairParams",
	gpu_size: 16,
	data_size: 0,
	resource_fields: [StorageTex, StorageTex],
	fields: [
		{ key: 0, offset: 0, len: 8, kind: StorageTex },
		{ key: 1, offset: 8, len: 8, kind: StorageTex },
	],
	pack_data: |_| [],
	extract_bindings: |bindings| [(0, bindings.first.erase()), (1, bindings.second.erase())],
}

pair_params : UniformSpec({}, PairBindings)
pair_params = { parameter_name: "params", index: 0, schema: pair_schema }

pair_shader : ComputeShader({ params : UniformSlot({}, PairBindings) }, NoPush)
pair_shader = ComputeShader.{
	name: "pair",
	compute_spv: [15],
	reflection_json: "{}",
	reflection: compute_reflection([(pair_schema.name, pair_schema.gpu_size.to_u64())]),
	workgroup_size: { x: 1, y: 1, z: 1 },
	uniform_slots: |slots| [slots.params.index()],
	push: NoPush.{},
}

groups = { x: 1, y: 1, z: 1 }

## The validation message `RenderGraph.new` rejects `graph` with, or "".
message_for : RenderGraph(frame), ResourcePlanner -> Str
message_for = |graph, planner| error_message(RenderGraph.new(planner, graph))

error_message : Try(a, [InvalidRenderGraph(Str)]) -> Str
error_message = |result|
	match result {
		Ok(_) => ""
		Err(InvalidRenderGraph(message)) => message
	}

# --- The BasicTriangle shape: vertex pulling with a hardware index buffer --

(triangle_planner1, vertices) = ResourcePlanner.singleton_buffer(
	ResourcePlanner.empty,
	"vertices",
	vertex_packer,
	[{ id: 1 }, { id: 2 }, { id: 3 }],
)

(triangle_planner2, indices) = ResourcePlanner.index_buffer(triangle_planner1, "indices", [0, 1, 2], 3)

(triangle_planner3, params) = ResourcePlanner.uniform_buffer(triangle_planner2, "params", triangle_params)

(triangle_planner, triangle_pipe) = ResourcePlanner.graphics_pipeline(
	triangle_planner3,
	{
		name: "basic_triangle",
		shader: triangle_shader,
		uniforms: { params: params },
		raster: RasterState.default,
	},
)

triangle_node = RenderGraph.draw_indexed(
	triangle_pipe,
	indices,
	params,
	{ vertices: vertices.addr() },
)

Ok(triangle) = RenderGraph.new(triangle_planner, triangle_node).map_ok(GraphSet.single)

triangle_host = GraphSet.definitions(triangle)

expect triangle.draw({ tint: 7 }).to_host() == { graph_id: 0, values: [Bytes([7, 0, 0, 0])] }

expect
	match triangle_host {
		[host] =>
			host.buffers
				== [
					{
						name: "vertices",
						role: Singleton,
						element_size: 16,
						len: 3,
						contents: Bytes(
							[
								[1, 0, 0, 0],
								List.repeat(0, 12),
								[2, 0, 0, 0],
								List.repeat(0, 12),
								[3, 0, 0, 0],
								List.repeat(0, 12),
							].join(),
						),
					},
					{
						name: "indices",
						role: Index,
						element_size: 4,
						len: 3,
						contents: Indices({ indices: [0, 1, 2], vertex_count: 3 }),
					},
				]

		_ => Bool.False
	}

expect
	match triangle_host {
		[host] =>
			host.uniform_buffers == [{ name: "params", size: 16 }]
				and host.uniform_slots == [0]
				and host.pipeline_indices == [0]
				and host.import_decls == []

		_ => Bool.False
	}

expect
	match triangle_host {
		[host] =>
			host.pipelines
				== [
					{
						name: "basic_triangle",
						kind: Graphics,
						stages: Graphics({ vertex_spv: [1, 2, 3, 4], fragment_spv: [5, 6, 7, 8], raster: RasterState.default }),
						reflection_json: "{\"source_file_name\":\"triangle\"}",
						uniform_buffers: [0],
					},
				]

		_ => Bool.False
	}

## The lowered description: one data value, the uniform's data and block
## schemas, and one indexed draw of the index buffer in the main pass.
expect
	match triangle_host {
		[host] =>
			host.desc.values
				== [{ name: "params.data", kind: Bytes({ schema: 0 }), optional: Bool.False, scope: Top }]
				and host.desc.uniforms
					== [
						{
							name: "params",
							schema: 1,
							source: {
								data: Some(0),
								bindings: [{ key: 0, resource: Buf({ buffer: 0, slot: Current, offset: 0, range: None }, Read) }],
							},
						},
					]
				and host.desc.pipelines == [{ name: "basic_triangle", kind: Graphics, params: 1, push: None }]

		_ => Bool.False
	}

expect
	match triangle_host {
		[host] =>
			host.desc.passes
				== [
					Leaf(
						Raster({
							name: "main",
							raster_targets: Main,
							draws: [{ name: "draw0", pipeline: 0, uniform: 0, call: WholeIndexed({ indices: 1 }), push: None }],
						}),
					),
				]

		_ => Bool.False
	}

expect
	match triangle_host {
		[host] =>
			host.schemas.schemas
				== [
					{ name: "params.data", size: 4, resource_fields: [], layout: None },
					{
						name: "TriangleParams",
						size: 16,
						resource_fields: [BufAddr],
						layout: Some({
							fields: [
								{ key: 1, offset: 0, len: 4, kind: Data({ src_offset: 0 }) },
								{ key: 0, offset: 8, len: 8, kind: BufAddr },
							],
						}),
					},
				]

		_ => Bool.False
	}

# Both `SingletonSlot.addr(slot)` and `slot.addr()` work.
expect ResourcePlanner.SingletonSlot.addr(vertices).erase() == vertices.addr().erase()

expect {
	vertices_provenance = vertices.buffer_ref().decl.provenance
	declared = { id: 0, provenance: vertices_provenance }

	vertices.addr_at(2).erase() == Buffer({ buffer: declared, slot: Current, offset: 32 }, Read)
		and vertices_provenance.name == "vertices"
		and vertices_provenance.digest.len() == 32
}

expect indices.index_count() == 3

# --- Compute, repeat, optional, uploads, and every draw kind ---------------

(full_planner0, field) = ResourcePlanner.texture(ResourcePlanner.empty, "field", 8, 8, R32Float)

(full_planner1, seed) = ResourcePlanner.texture(full_planner0, "seed", 4, 4, Rgba32Float)

(full_planner2, step_uniform) = ResourcePlanner.uniform_buffer(full_planner1, "step", step_params)

(full_planner3, step_pipe) = ResourcePlanner.compute_pipeline(full_planner2, { name: "step", shader: step_shader, uniforms: { params: step_uniform } })

(full_planner4, particles) = ResourcePlanner.cpu_updated_buffer(full_planner3, "particles", u32_packer, 8, [9])

(full_planner5, spawn_uniform) = ResourcePlanner.uniform_buffer(full_planner4, "spawn", spawn_params)

(full_planner6, spawn_pipe) = ResourcePlanner.compute_pipeline(full_planner5, { name: "spawn", shader: spawn_shader, uniforms: { params: spawn_uniform } })

(full_planner7, mesh) = ResourcePlanner.immutable_buffer(full_planner6, "mesh", vertex_packer, [{ id: 4 }, { id: 5 }, { id: 6 }, { id: 7 }])

(full_planner8, quad_indices) = ResourcePlanner.index_buffer(full_planner7, "quad_indices", [0, 1, 2, 2, 3, 0], 4)

(full_planner9, draw_uniform) = ResourcePlanner.uniform_buffer(full_planner8, "draw", triangle_params)

(full_planner10, draw_pipe) = ResourcePlanner.graphics_pipeline(full_planner9, { name: "draw", shader: triangle_shader, uniforms: { params: draw_uniform }, raster: RasterState.no_depth })

(full_planner11, commands) = ResourcePlanner.indirect_buffer(
	full_planner10,
	"commands",
	[
		{ index_count: 3, instance_count: 1, first_index: 0, vertex_offset: 0, first_instance: 0 },
		{ index_count: 3, instance_count: 1, first_index: 3, vertex_offset: -1, first_instance: 0 },
	],
)

(full_planner12, indirect_uniform) = ResourcePlanner.uniform_buffer(full_planner11, "indirect", triangle_params)

(full_planner, indirect_pipe) = ResourcePlanner.graphics_indirect_pipeline(full_planner12, { name: "indirect", shader: indirect_shader, uniforms: { params: indirect_uniform }, raster: RasterState.default })

jacobi =
	RenderGraph.repeat(
		RenderGraph.dispatch_with_push(
			step_pipe,
			step_uniform,
			groups,
			{ source: seed.read() },
			{ data: { iteration: 5 }, bindings: { input: field.read(), output: field.write() } },
		),
	)

spawn =
	RenderGraph.optional(
		RenderGraph.from_tuple_2((
			RenderGraph.upload(particles),
			RenderGraph.dispatch(spawn_pipe, spawn_uniform, groups, { particles: particles.addr() }),
		)),
	)

draws =
	RenderGraph.list([
		RenderGraph.draw_vertex_count(draw_pipe, draw_uniform, 6, { vertices: mesh.addr() }),
		RenderGraph.draw_index_range(draw_pipe, quad_indices, draw_uniform, 3, 3, { vertices: mesh.addr() }),
	])

indirect = RenderGraph.draw_indexed_indirect(indirect_pipe, quad_indices, indirect_uniform, commands, 0, 2, { vertices: mesh.addr_at(1) })

full_graph = RenderGraph.from_tuple_4((jacobi, spawn, draws, indirect))

Ok(full) = RenderGraph.new(full_planner, full_graph).map_ok(GraphSet.single)

full_host = GraphSet.definitions(full)

full_frame = |spawned| (
	{ count: 3, body: { weight: 2 } },
	spawned,
	[{ tint: 1 }, { tint: 1 }],
	{ tint: 4 },
)

## The repeat count, the step uniform, and the push block's build-time data;
## the optional scope's upload and uniform; one value for the shared draw
## uniform; the indirect draw's uniform.
expect
	full.draw(full_frame(None)).to_host().values
		== [
			Count(3),
			Bytes([2, 0, 0, 0]),
			Bytes([5, 0, 0, 0]),
			Absent,
			Absent,
			Bytes([1, 0, 0, 0]),
			Bytes([4, 0, 0, 0]),
		]

expect
	full.draw(full_frame(Some(([1, 2, 3], { count: 3 })))).to_host().values
		== [
			Count(3),
			Bytes([2, 0, 0, 0]),
			Bytes([5, 0, 0, 0]),
			Array({ count: 3, bytes: [1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0] }),
			Bytes([3, 0, 0, 0]),
			Bytes([1, 0, 0, 0]),
			Bytes([4, 0, 0, 0]),
		]

expect
	match full_host {
		[host] =>
			host.desc.values.map(|value| (value.name, value.optional, value.scope))
				== [
					("repeat0.count", Bool.False, Top),
					("step.data", Bool.False, Repeat({ count: 0 })),
					("dispatch0.push.data", Bool.False, Repeat({ count: 0 })),
					("upload0.value", Bool.True, When({ gate: 3 })),
					("spawn.data", Bool.True, When({ gate: 3 })),
					("draw.data", Bool.False, Top),
					("indirect.data", Bool.False, Top),
				]

		_ => Bool.False
	}

expect
	match full_host {
		[host] =>
			match host.desc.passes {
				[Repeat(repeat_pass), When(when_pass), Leaf(Raster(raster))] =>
					(repeat_pass.name, repeat_pass.count, when_pass.name, when_pass.value) == ("repeat0", 0, "when1", 3)
						and repeat_pass.body.map(leaf_name) == ["dispatch0"]
						and when_pass.body.map(leaf_name) == ["dispatch1"]
						and repeat_pass.body.map(leaf_groups) == [Fixed({ x: 1, y: 1, z: 1 })]
						and raster.draws.map(|draw| draw.call)
							== [
								VertexCount(6),
								IndexRange({ indices: 2, first_index: 3, index_count: 3 }),
								IndexedIndirect({ indices: 2, args: { buffer: 3, slot: Current, offset: 0, range: None }, draw_count: 2 }),
							]

				_ => Bool.False
			}

		_ => Bool.False
	}

leaf_name = |leaf|
	match leaf {
		Compute(dispatch) => dispatch.name
		Raster(raster) => raster.name
	}

leaf_groups = |leaf|
	match leaf {
		Compute(dispatch) => dispatch.groups
		Raster(_) => Value(0)
	}

expect
	match full_host {
		[host] => host.desc.uploads == [{ name: "upload0", buffer: 0, value: 3 }]
		_ => Bool.False
	}

expect
	match full_host {
		[host] =>
			host.buffers.map(|buffer| (buffer.name, buffer.role, buffer.element_size, buffer.len))
				== [
					("particles", CpuUpdated, 4, 8),
					("mesh", Immutable, 16, 4),
					("quad_indices", Index, 4, 6),
					("commands", IndirectArgs, 20, 2),
				]
				and host.desc.buffers.map(|buffer| buffer.kind) == [Storage, Immutable, Index, Immutable]
				and host.desc.textures
					== [
						{ name: "field", format: R32Float, size: Fixed(8, 8), usage: Storage },
						{ name: "seed", format: Rgba32Float, size: Fixed(4, 4), usage: Storage },
					]

		_ => Bool.False
	}

expect
	match full_host {
		[host] =>
			host.pipelines.map(|pipeline| (pipeline.name, pipeline.kind))
				== [("step", Compute), ("spawn", Compute), ("draw", Graphics), ("indirect", GraphicsIndirect)]
				and host.pipeline_indices == [0, 1, 2, 3]
				and host.uniform_slots == [0, 1, 2, 3]

		_ => Bool.False
	}

expect
	match full_host {
		[host] =>
			match host.buffers.get(3) {
				Ok({ contents: Bytes(bytes), .. }) =>
					bytes
						== [
							[3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
							[3, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0],
						].join()

				_ => Bool.False
			}

		_ => Bool.False
	}

expect ResourcePlanner.DrawIndexedIndirectCommand.gpu_size == 20

# --- GraphSet ----------------------------------------------------------------

both = GraphSet.map2(triangle, full, |left, right| { triangle: left, full: right })

draw_full = both.select(|graphs| graphs.full)

expect GraphSet.definitions(both).len() == 2

expect draw_full.draw(full_frame(None)).to_host().graph_id == 1

expect both.select(|graphs| graphs.triangle).draw({ tint: 2 }).to_host() == { graph_id: 0, values: [Bytes([2, 0, 0, 0])] }

## A graph is only its own set's: another set's graph, or this set's id
## past its end, is not.
twice = GraphSet.map2(triangle, triangle, |_, right| right)

expect triangle.contains(triangle.register())

expect !triangle.contains(twice.register())

expect !full.contains(triangle.register())

expect both.contains(both.register().full) and both.contains(both.register().triangle)

expect twice.contains(twice.register())

# --- Declaration checks ------------------------------------------------------

## A graph drawing `index_list` against `vertex_count` vertices.
index_graph_message : List(U32), U32 -> Str
index_graph_message = |index_list, vertex_count| {
	(planner1, uniform) = ResourcePlanner.uniform_buffer(ResourcePlanner.empty, "params", triangle_params)
	(planner2, pipe) = ResourcePlanner.graphics_pipeline(planner1, { name: "pipe", shader: triangle_shader, uniforms: { params: uniform }, raster: RasterState.default })
	(planner3, buffer) = ResourcePlanner.immutable_buffer(planner2, "vertices", vertex_packer, [{ id: 0 }])
	(planner, index_slot) = ResourcePlanner.index_buffer(planner3, "bad_indices", index_list, vertex_count)

	RenderGraph.draw_indexed(pipe, index_slot, uniform, { vertices: buffer.addr() })
		|> message_for(planner)
}

expect index_graph_message([], 3).contains("index buffer bad_indices has no indices")

expect index_graph_message([0], 0).contains("index buffer bad_indices has a vertex count of 0")

expect index_graph_message([0, 1, 5, 7], 3).contains("index buffer bad_indices index 5 at position 2 is not below its vertex count 3")

expect index_graph_message([0, 1, 2], 3) == ""

range_message : U32, U32 -> Str
range_message = |first_index, index_count|
	RenderGraph.draw_index_range(draw_pipe, quad_indices, draw_uniform, first_index, index_count, { vertices: mesh.addr() })
		|> message_for(full_planner)

expect range_message(4, 3).contains("draw draw0's index range [4, 4 + 3) is out of bounds for an index buffer of 6 indices")

expect range_message(0, 6) == ""

indirect_message : U32, U32 -> Str
indirect_message = |first_command, draw_count|
	RenderGraph.draw_indexed_indirect(indirect_pipe, quad_indices, indirect_uniform, commands, first_command, draw_count, { vertices: mesh.addr() })
		|> message_for(full_planner)

expect indirect_message(1, 2).contains("draw draw0's indirect commands [1, 1 + 2) are not a nonempty range inside an argument buffer of 2 commands")

expect indirect_message(0, 0).contains("are not a nonempty range")

expect indirect_message(1, 1) == ""

## An immutable buffer of records is not an argument buffer.
expect {
	(planner, plain_args) = ResourcePlanner.immutable_buffer(full_planner, "plain_args", ResourcePlanner.DrawIndexedIndirectCommand.packer, [{ index_count: 3, instance_count: 1, first_index: 0, vertex_offset: 0, first_instance: 0 }])

	RenderGraph.draw_indexed_indirect(indirect_pipe, quad_indices, indirect_uniform, plain_args, 0, 1, { vertices: mesh.addr() })
		|> message_for(planner)
		|> Str.contains("argument buffer plain_args was not declared with ResourcePlanner.indirect_buffer")
}

expect {
	(planner, too_full) = ResourcePlanner.cpu_updated_buffer(full_planner, "too_full", u32_packer, 2, [1, 2, 3])

	RenderGraph.upload(too_full)
		|> message_for(planner)
		|> Str.contains("buffer too_full starts with 3 elements but holds at most 2")
}

expect {
	(planner, empty_buffer) = ResourcePlanner.singleton_buffer(full_planner, "nothing", vertex_packer, [])

	RenderGraph.draw_vertex_count(draw_pipe, draw_uniform, 3, { vertices: empty_buffer.addr() })
		|> message_for(planner)
		|> Str.contains("buffer nothing has no elements")
}

## An address past the end of its buffer.
expect
	RenderGraph.draw_vertex_count(draw_pipe, draw_uniform, 3, { vertices: mesh.addr_at(4) })
		|> message_for(full_planner)
		|> Str.contains("draw binding 0 offset 64 is past the end of buffer mesh (64 bytes)")

# --- Texture hazards ---------------------------------------------------------

(hazard_planner1, scratch) = ResourcePlanner.texture(full_planner, "scratch", 4, 4, R32Float)

(hazard_planner2, pair_uniform) = ResourcePlanner.uniform_buffer(hazard_planner1, "pair", pair_params)

(hazard_planner, pair_pipe) = ResourcePlanner.compute_pipeline(hazard_planner2, { name: "pair", shader: pair_shader, uniforms: { params: pair_uniform } })

step_with = |source, input, output|
	RenderGraph.dispatch_with_push(
		step_pipe,
		step_uniform,
		groups,
		{ source: source },
		{ data: { iteration: 0 }, bindings: { input, output } },
	)

pair_with = |first, second| RenderGraph.dispatch(pair_pipe, pair_uniform, groups, { first, second })

hazard_message = |graph| error_message(RenderGraph.new(hazard_planner, graph))

expect hazard_message(step_with(seed.read(), field.read(), field.write())) == ""

expect hazard_message(step_with(seed.read(), field.read(), field.mutate())).contains("dispatch0 mutates and reads texture field")

expect hazard_message(pair_with(field.write(), field.write())).contains("dispatch0 writes texture field more than once")

expect hazard_message(pair_with(field.write(), field.mutate())).contains("dispatch0 mutates and writes texture field")

expect
	hazard_message(step_with(seed.read(), field.read_previous(), scratch.mutate()))
		.contains("dispatch0 reads texture field's previous version, but no command writes field")

expect
	hazard_message(step_with(seed.read(), field.read_previous(), field.write()))
		.contains("dispatch0 writes texture field and reads its previous version")

expect
	hazard_message(RenderGraph.repeat(step_with(field.read(), seed.read(), field.write())))
		.contains("repeat0: step's uniform block references rotating texture field; move the reference into the node's push block")

## The push block rotates per iteration, so a repeated push-bound texture
## is fine.
expect hazard_message(RenderGraph.repeat(step_with(seed.read(), field.read(), field.write()))) == ""

expect
	hazard_message(
		RenderGraph.from_tuple_3((
			step_with(field.read(), seed.read(), scratch.write()),
			step_with(field.read(), seed.read(), field.write()),
			step_with(field.read(), seed.read(), scratch.write()),
		)),
	)
		.contains("step's uniform block is shared by several commands, but dispatch1 writes texture field, which the block references, between them")

# --- Ordering and control flow -----------------------------------------------

triangle_draw = RenderGraph.draw_vertex_count(draw_pipe, draw_uniform, 3, { vertices: mesh.addr() })

empty_compute = |planner_uniform| RenderGraph.dispatch(empty_pipe, planner_uniform, groups, {})

(control_planner1, empty_uniform) = ResourcePlanner.uniform_buffer(full_planner, "empty", empty_params)

(control_planner2, empty_pipe) = ResourcePlanner.compute_pipeline(control_planner1, { name: "empty", shader: empty_shader, uniforms: { params: empty_uniform } })

(control_planner3, storage_draw_uniform) = ResourcePlanner.uniform_buffer(control_planner2, "storage_draw", storage_params)

(control_planner, storage_draw_pipe) = ResourcePlanner.graphics_pipeline(control_planner3, { name: "storage_draw", shader: storage_draw_shader, uniforms: { params: storage_draw_uniform }, raster: RasterState.default })

control_message = |graph| error_message(RenderGraph.new(control_planner, graph))

expect
	control_message(RenderGraph.from_tuple_2((triangle_draw, empty_compute(empty_uniform))))
		.contains("pass dispatch0 must precede the main raster pass")

expect control_message(RenderGraph.repeat(triangle_draw)).contains("a draw node cannot appear inside repeat repeat0")

expect
	control_message(RenderGraph.optional(triangle_draw))
		.contains("when0: a raster pass inside an optional scope (phase 5, ledger S6) is unsupported in phase 1")

expect control_message(RenderGraph.repeat(RenderGraph.optional(RenderGraph.upload(particles)))).contains("nested optional inside repeat is not supported")

expect control_message(RenderGraph.optional(empty_compute(empty_uniform))).contains("optional scope has no frame value")

expect
	control_message(RenderGraph.draw_vertex_count(storage_draw_pipe, storage_draw_uniform, 3, { target: field.write() }))
		.contains("draw draw0 can only read graph textures (texture field)")

## Two draws through one uniform slot with different bindings.
expect
	control_message(
		RenderGraph.from_tuple_2((
			RenderGraph.draw_vertex_count(draw_pipe, draw_uniform, 3, { vertices: mesh.addr() }),
			RenderGraph.draw_vertex_count(draw_pipe, draw_uniform, 3, { vertices: mesh.addr_at(1) }),
		)),
	)
		.contains("uniform slot 2 has more than one data source")

expect control_message(RenderGraph.from_tuple_2((RenderGraph.optional(RenderGraph.upload(particles)), triangle_draw))) == ""

expect
	control_message(
		RenderGraph.from_tuple_2((
			RenderGraph.repeat(RenderGraph.dispatch(spawn_pipe, spawn_uniform, groups, { particles: particles.addr() })),
			RenderGraph.dispatch(spawn_pipe, spawn_uniform, groups, { particles: particles.addr() }),
		)),
	)
		.contains("uniform spawn is written by commands in different repeat or optional scopes")

## The draw's uniform is not one its pipeline reads.
expect
	control_message(RenderGraph.draw_vertex_count(draw_pipe, indirect_uniform, 3, { vertices: mesh.addr() }))
		.contains("draw0 writes uniform indirect, which pipeline draw does not read")

# --- Planner lineage ---------------------------------------------------------

(impostor_planner1, impostor_params) = ResourcePlanner.uniform_buffer(ResourcePlanner.empty, "params", triangle_params)

(_impostor_planner, impostor_pipe) = ResourcePlanner.graphics_pipeline(
	impostor_planner1,
	{
		name: "impostor",
		shader: triangle_shader,
		uniforms: { params: impostor_params },
		raster: RasterState.no_depth,
	},
)

## A key minted by an unrelated planner chain aliases id 0 of this one.
expect
	RenderGraph.draw_indexed(impostor_pipe, indices, params, { vertices: vertices.addr() })
		|> message_for(triangle_planner)
		|> Str.contains("pipeline impostor (id 0) was declared by a different resource planner than the one passed to RenderGraph.new")

## A slot from a later planner is undeclared in an earlier one.
expect
	RenderGraph.new(triangle_planner2, triangle_node)
		|> error_message
		|> Str.contains("pipeline basic_triangle (id 0) was declared by a different resource planner")

## Slots from earlier planners of the same chain stay valid.
expect error_message(RenderGraph.new(triangle_planner, triangle_node)) == ""

## Bindings minted by another planner chain name ids this planner declares
## differently: a same-named texture of another size, a same-named buffer of
## another capacity.
(foreign_planner1, foreign_field) = ResourcePlanner.texture(ResourcePlanner.empty, "field", 16, 16, R32Float)

(_foreign_planner, foreign_particles) = ResourcePlanner.cpu_updated_buffer(
	foreign_planner1,
	"particles",
	u32_packer,
	16,
	[],
)

expect
	step_with(foreign_field.read(), seed.read(), scratch.write())
		|> message_for(hazard_planner)
		|> Str.contains("texture field (id 0) was declared by a different resource planner")

expect
	RenderGraph.dispatch(spawn_pipe, spawn_uniform, groups, { particles: foreign_particles.addr() })
		|> message_for(full_planner)
		|> Str.contains("buffer particles (id 0) was declared by a different resource planner")

## An identical declaration from another chain is interchangeable.
expect {
	(_, same_field) = ResourcePlanner.texture(ResourcePlanner.empty, "field", 8, 8, R32Float)

	step_with(same_field.read(), seed.read(), scratch.write()) |> message_for(hazard_planner) == ""
}

## A foreign uniform slot passed to a pipeline names this planner's slot 0,
## whose block is not the one the shader reflects.
other_schema : BlockSchema(TintData, VertexBindings)
other_schema = { ..triangle_schema, name: "OtherParams" }

other_shader : GraphicsShader(TriangleUniforms, NoPush)
other_shader = GraphicsShader.{
	name: "other",
	vertex_spv: [1],
	fragment_spv: [2],
	reflection_json: "{}",
	reflection: graphics_reflection([("OtherParams", 16)]),
	uniform_slots: |slots| [slots.params.index()],
	push: NoPush.{},
}

(_other_planner, other_uniform) = ResourcePlanner.uniform_buffer(
	ResourcePlanner.empty,
	"other",
	{ ..triangle_params, schema: other_schema },
)

expect {
	(planner, pipe) = ResourcePlanner.graphics_pipeline(
		triangle_planner,
		{ name: "other", shader: other_shader, uniforms: { params: other_uniform }, raster: RasterState.default },
	)
	message =
		RenderGraph.draw_vertex_count(pipe, other_uniform, 3, { vertices: vertices.addr() })
			|> message_for(planner)

	message.contains("pipeline other reads uniform params (TriangleParams, 16 bytes) for its shader's ParameterBlock (OtherParams, 16 bytes)")
		and message.contains("uniform other (id 0) was declared by a different resource planner")
}

# --- Parameter block counts --------------------------------------------------

no_block_shader : GraphicsShader({}, NoPush)
no_block_shader = GraphicsShader.{
	name: "no_blocks",
	vertex_spv: [1],
	fragment_spv: [2],
	reflection_json: "{}",
	reflection: graphics_reflection([]),
	uniform_slots: |_| [],
	push: NoPush.{},
}

TwoUniforms : {
	tint : UniformSlot(TintData, VertexBindings),
	params : UniformSlot(TintData, VertexBindings),
}

two_block_shader : GraphicsShader(TwoUniforms, NoPush)
two_block_shader = GraphicsShader.{
	name: "two_blocks",
	vertex_spv: [1],
	fragment_spv: [2],
	reflection_json: "{}",
	reflection: graphics_reflection([("TriangleParams", 16), ("TriangleParams", 16)]),
	uniform_slots: |slots| [slots.tint.index(), slots.params.index()],
	push: NoPush.{},
}

## A draw through a pipeline with no parameter block, and one through a
## pipeline whose second block it never writes.
expect {
	(planner, pipe) = ResourcePlanner.graphics_pipeline(
		triangle_planner,
		{ name: "no_blocks", shader: no_block_shader, uniforms: {}, raster: RasterState.default },
	)

	RenderGraph.draw_vertex_count(pipe, params, 3, { vertices: vertices.addr() })
		|> message_for(planner)
		|> Str.contains("pipeline no_blocks's shader declares 0 ParameterBlock globals; a render graph shader needs exactly one ParameterBlock<T>")
}

expect {
	(planner1, tint) = ResourcePlanner.uniform_buffer(triangle_planner, "tint", triangle_params)
	(planner, pipe) = ResourcePlanner.graphics_pipeline(
		planner1,
		{ name: "two_blocks", shader: two_block_shader, uniforms: { tint: tint, params: params }, raster: RasterState.default },
	)

	RenderGraph.draw_vertex_count(pipe, tint, 3, { vertices: vertices.addr() })
		|> message_for(planner)
		|> Str.contains("pipeline two_blocks's shader declares 2 ParameterBlock globals")
}

# --- Imported textures -------------------------------------------------------

(import_planner1, environment) = ResourcePlanner.imported_texture(hazard_planner, "environment")

Ok(imported) =
	RenderGraph.new(import_planner1, step_with(environment.read(), seed.read(), scratch.write()))
		.map_ok(GraphSet.single)

expect
	match GraphSet.definitions(imported) {
		[host] =>
			host.import_decls == [{ name: "environment" }]
				and host.desc.import_decls == [{ name: "environment" }]
				and host.desc.uniforms.map(|uniform| uniform.source.bindings) == [[{ key: 0, resource: External(0) }]]

		_ => Bool.False
	}

## A same-named import from another chain is the same external texture.
expect {
	(_, other_environment) = ResourcePlanner.imported_texture(ResourcePlanner.empty, "environment")

	step_with(other_environment.read(), seed.read(), scratch.write()) |> message_for(import_planner1) == ""
}

expect {
	(_, other_import) = ResourcePlanner.imported_texture(ResourcePlanner.empty, "sky")

	step_with(other_import.read(), seed.read(), scratch.write())
		|> message_for(import_planner1)
		|> Str.contains("import sky (id 0) was declared by a different resource planner")
}

# --- Block layouts -----------------------------------------------------------

overlap_schema : BlockSchema(TintData, VertexBindings)
overlap_schema = {
	..triangle_schema,
	fields: [
		{ key: 1, offset: 4, len: 8, kind: Data({ src_offset: 0 }) },
		{ key: 0, offset: 8, len: 8, kind: BufAddr },
	],
}

expect {
	(planner1, uniform) = ResourcePlanner.uniform_buffer(full_planner, "overlap", { ..triangle_params, schema: overlap_schema })
	(planner, pipe) = ResourcePlanner.graphics_pipeline(planner1, { name: "overlap", shader: triangle_shader, uniforms: { params: uniform }, raster: RasterState.default })
	message = error_message(RenderGraph.new(planner, RenderGraph.draw_vertex_count(pipe, uniform, 3, { vertices: mesh.addr() })))

	message.contains("schema TriangleParams fields 1 and 0 overlap")
		and message.contains("overlap data field 1 reads [0, 0 + 8) past its 4-byte data value")
}
