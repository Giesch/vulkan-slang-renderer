import pf.ShaderReflection exposing [
	BlockSchema,
	BufferBinding,
	ComputeShader,
	DescriptorHandle,
	GraphicsIndirectShader,
	GraphicsShader,
	ImmutableBufferBinding,
	PointerAddress,
	ReadBufferBinding,
	SampledTexBinding,
	StorageTexBinding,
	UniformSlot,
]
import BasicTriangle
import Mltrs
import Particles
import RocBlocks
import RocNoBlocks
import VertexPullingFloat3

BlockConsumer := {}

## Little-endian F32 bytes.
one = [0, 0, 128, 63]

two = [0, 0, 0, 64]

three = [0, 0, 64, 64]

four = [0, 0, 128, 64]

five = [0, 0, 160, 64]

six = [0, 0, 192, 64]

seven = [0, 0, 224, 64]

half = [0, 0, 0, 63]

pad = [0, 0, 0, 0]

position = { x: 1.0, y: 2.0, z: 3.0 }

color3 = { x: 4.0, y: 5.0, z: 6.0 }

color4 = { x: 4.0, y: 5.0, z: 6.0, w: 7.0 }

## The GPU block a schema assembles from packed data and one 8-byte value per
## binding, indexed by field key, as the render graph's assembly program does.
assemble : BlockSchema(data, bindings), List(U8), List(U64) -> List(U8)
assemble = |schema, data, binding_values| {
	parts = schema.fields.map(
		|field| {
			bytes =
				match field.kind {
					Data({ src_offset }) =>
						data.sublist({ start: src_offset.to_u64(), len: field.len.to_u64() })
					_ =>
						match binding_values.get(field.key.to_u64()) {
							Ok(value) => ShaderReflection.u64_bytes(value)
							Err(_) => crash "no binding value for a resource field"
						}
				}

			(field.offset.to_u64(), bytes)
		},
	)

	ShaderReflection.pack(schema.gpu_size.to_u64(), parts)
}

## The render graph's layout rules: ascending, non-overlapping fields inside
## `gpu_size`, data inside `data_size`, and one 8-byte field per
## `resource_fields` entry, keyed by its index.
valid_layout : BlockSchema(data, bindings) -> Bool
valid_layout = |schema| {
	ends_in_order = schema.fields.fold(
		Ok(0),
		|end, field|
			match end {
				Ok(previous) if field.offset >= previous => Ok(field.offset + field.len)
				_ => Err({})
			},
	)
	fits = match ends_in_order {
		Ok(end) => end <= schema.gpu_size
		Err(_) => False
	}
	data_fits = schema.fields.all(
		|field|
			match field.kind {
				Data({ src_offset }) => src_offset + field.len <= schema.data_size
				_ => True
			},
	)
	resources_match = schema.resource_fields.len() == schema.fields.count_if(
		|field|
			match field.kind {
				Data(_) => False
				_ => True
			},
	)
	keyed = schema.fields.all(
		|field| {
			key = field.key.to_u64()
			resource = schema.resource_fields.get(key)

			match field.kind {
				Data(_) => key >= schema.resource_fields.len()
				SampledTex => field.len == 8 and resource == Ok(SampledTex)
				StorageTex => field.len == 8 and resource == Ok(StorageTex)
				BufAddr => field.len == 8 and resource == Ok(BufAddr)
			}
		},
	)

	fits and data_fits and resources_match and keyed
}

graphics_slots : GraphicsShader(uniforms, push), uniforms -> List(U32)
graphics_slots = |shader, uniforms| (shader.uniform_slots)(uniforms)

indirect_slots : GraphicsIndirectShader(uniforms, push), uniforms -> List(U32)
indirect_slots = |shader, uniforms| (shader.uniform_slots)(uniforms)

compute_slots : ComputeShader(uniforms, push), uniforms -> List(U32)
compute_slots = |shader, uniforms| (shader.uniform_slots)(uniforms)

# Std430 float3 pointees: a float3 aligns to 16, and a scalar tail-packs at 12.
expect
	RocBlocks.PairVertex.to_bytes({ position, color: color3 })
		== [one, two, three, pad, four, five, six, pad].join()
expect RocBlocks.PairVertex.gpu_size == 32
expect
	RocBlocks.TailVertex.to_bytes({ position, scale: 0.5 })
		== [one, two, three, half].join()
expect RocBlocks.TailVertex.gpu_size == 16
expect
	RocBlocks.WideVertex.to_bytes({ position, color: color4 })
		== [one, two, three, pad, four, five, six, seven].join()
expect RocBlocks.WideVertex.gpu_size == 32
expect VertexPullingFloat3.PairVertex.packer.stride == 32
expect VertexPullingFloat3.TailVertex.packer.stride == 16
expect VertexPullingFloat3.WideVertex.packer.stride == 32
expect
	(BasicTriangle.Vertex.packer.to_bytes)({ position, color: color3 })
		== [one, two, three, pad, four, five, six, pad].join()

# A pure-data block is its own data, packed at its reflected offsets.
tint : RocBlocks.Tint
tint = { color: { x: 1.0, y: 2.0, z: 3.0, w: 4.0 } }

expect RocBlocks.Tint.schema.data_size == RocBlocks.Tint.gpu_size
expect (RocBlocks.Tint.schema.pack_data)(tint) == tint.to_bytes()
expect (RocBlocks.Tint.schema.extract_bindings)({}) == []
expect assemble(RocBlocks.Tint.schema, tint.to_bytes(), []) == tint.to_bytes()
expect valid_layout(RocBlocks.Tint.schema)

# A mixed parameter block: float3 data around a pointer and a handle.
block_params : RocBlocks.BlockParams
block_params = {
	offset: position,
	pairs: PointerAddress.(11),
	scale: color3,
	bias: 0.5,
	tex: DescriptorHandle.(22),
	tails: PointerAddress.(33),
}

tails_target = { buffer: 2, slot: Current, offset: 64 }

block_params_data : RocBlocks.BlockParamsData
block_params_data = { offset: position, scale: color3, bias: 0.5 }

expect RocBlocks.BlockParamsData.gpu_size == 28
expect
	(RocBlocks.BlockParams.schema.pack_data)(block_params_data)
		== [one, two, three, four, five, six, half].join()
expect
	assemble(RocBlocks.BlockParams.schema, block_params_data.to_bytes(), [11, 22, 33])
		== block_params.to_bytes()
expect valid_layout(RocBlocks.BlockParams.schema)
expect
	(RocBlocks.BlockParams.schema.extract_bindings)({
		pairs: ImmutableBufferBinding.new({ buffer: 1, slot: Current, offset: 0 }),
		tex: SampledTexBinding.new(Imported(4)),
		tails: ImmutableBufferBinding.new(tails_target).as_read(),
	})
		== [
			(0, Buffer({ buffer: 1, slot: Current, offset: 0 }, Read)),
			(1, SampledTex(Imported(4))),
			(2, Buffer({ buffer: 2, slot: Current, offset: 64 }, Read)),
		]

# A mixed std430 push block: a pointer and a storage handle, then tail-packed
# data.
block_push : RocBlocks.BlockPush
block_push = {
	wides: PointerAddress.(44),
	target: DescriptorHandle.(55),
	tint: position,
	strength: 0.5,
}

block_push_data : RocBlocks.BlockPushData
block_push_data = { tint: position, strength: 0.5 }

push_schema = RocBlocks.push.schema

expect block_push_data.to_bytes() == [one, two, three, half].join()
expect
	assemble(push_schema, block_push_data.to_bytes(), [44, 55])
		== block_push.to_bytes()
expect valid_layout(push_schema)
expect
	(push_schema.extract_bindings)({
		wides: BufferBinding.new({ buffer: 3, slot: Current, offset: 32 }),
		target: StorageTexBinding.new(Write(6)),
	})
		== [
			(0, Buffer({ buffer: 3, slot: Current, offset: 32 }, Mutate)),
			(1, StorageTex(Write(6))),
		]

# A pointer-only block has no data.
pull_params : VertexPullingFloat3.PullParams
pull_params = {
	pairs: PointerAddress.(1),
	tails: PointerAddress.(2),
	wides: PointerAddress.(3),
}

expect VertexPullingFloat3.PullParams.schema.data_size == 0
expect (VertexPullingFloat3.PullParams.schema.pack_data)({}) == []
expect
	assemble(VertexPullingFloat3.PullParams.schema, [], [1, 2, 3])
		== pull_params.to_bytes()
expect valid_layout(VertexPullingFloat3.PullParams.schema)

# Pointers before scalar data, and a nested shared struct beside a pointer.
sim_params : Particles.SimParams
sim_params = {
	particles_in: PointerAddress.(7),
	particles_out: PointerAddress.(8),
	delta_time: 0.5,
}

previous_particles = { buffer: 0, slot: Previous, offset: 0 }

expect
	assemble(Particles.SimParams.schema, half, [7, 8]) == sim_params.to_bytes()
expect valid_layout(Particles.SimParams.schema)
expect
	(Particles.SimParams.schema.extract_bindings)({
		particles_in: BufferBinding.new(previous_particles).as_read(),
		particles_out: BufferBinding.new({ buffer: 0, slot: Current, offset: 0 }),
	})
		== [
			(0, Buffer({ buffer: 0, slot: Previous, offset: 0 }, Read)),
			(1, Buffer({ buffer: 0, slot: Current, offset: 0 }, Mutate)),
		]

matrices : Mltrs.MvpMatrices
matrices = {
	model: ShaderReflection.Float4x4.identity,
	view: ShaderReflection.Float4x4.identity,
	proj: ShaderReflection.Float4x4.identity,
}

triangle_params : BasicTriangle.TriangleParams
triangle_params = { mvp: matrices, vertices: PointerAddress.(9) }

expect
	assemble(BasicTriangle.TriangleParams.schema, matrices.to_bytes(), [9])
		== triangle_params.to_bytes()
expect valid_layout(BasicTriangle.TriangleParams.schema)

# Widening keeps the buffer target and reads through it.
expect
	BufferBinding.new({ buffer: 5, slot: Current, offset: 0 }).as_read().erase()
		== ReadBufferBinding.new({ buffer: 5, slot: Current, offset: 0 }).erase()

# One uniform slot per parameter block, named after the parameter.
expect
	graphics_slots(
		RocBlocks.shader,
		{
			tint: UniformSlot.new(5, RocBlocks.tint.schema),
			params: UniformSlot.new(9, RocBlocks.params.schema),
		},
	)
		== [5, 9]
expect RocBlocks.tint.index == 0 and RocBlocks.params.index == 1
expect graphics_slots(RocNoBlocks.shader, {}) == []
expect
	indirect_slots(
		VertexPullingFloat3.shader,
		{ params: UniformSlot.new(2, VertexPullingFloat3.params.schema) },
	)
		== [2]
expect
	compute_slots(
		Particles.shader,
		{ sim_params: UniformSlot.new(4, Particles.sim_params.schema) },
	)
		== [4]
expect
	graphics_slots(
		BasicTriangle.shader,
		{ params: UniformSlot.new(0, BasicTriangle.params.schema) },
	)
		== [0]
expect Particles.shader.workgroup_size == { x: 256, y: 1, z: 1 }
expect BasicTriangle.shader.reflection_json.starts_with("{")
