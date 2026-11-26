## Shader reflection schema, logical shader value types, and GPU byte packing.
##
## Generated shader modules import this module as `pf.ShaderReflection`.
## Every size or count that originates as a Rust `usize` is a `U64`.
##
## The `to_bytes` packers produce the bytes the GPU reads. Handles and
## addresses carry no GPU validity, ownership, lookup, or dereference
## guarantee.
ShaderReflection := {}.{
	Optional(a) : [None, Some(a)]

	ScalarType : [Float32, Int32, Uint32, Uint64]

	VectorElementType : [Scalar({ scalar_type : ScalarType })]

	Stage : [Vertex, Fragment, Compute]

	StageFlags : [Vertex, Fragment, Compute, All, Empty]

	Binding : [
		Uniform({ offset : U64, size : U64 }),
		PushConstant({ offset : U64, size : U64 }),
		DescriptorTableSlot({ index : U64, count : U64 }),
		VaryingInput({ index : U64, count : U64 }),
		ConstantBuffer({ index : U64, count : U64 }),
	]

	StructType := {
		type_name : Str,
		fields : List(Field),
	}

	EnumType : {
		type_name : Str,
		tag_type : [Uint32, Int32],
		cases : List({ name : Str, value : I64 }),
	}

	ResourceResult : [
		Scalar({ scalar_type : ScalarType }),
		Vector({ element_count : U64, element_type : VectorElementType }),
		Struct(StructType),
	]

	Field : [
		Scalar(
			{
				field_name : Str,
				binding : Binding,
				scalar_type : ScalarType,
			},
		),
		Vector(
			[
				Bound(
					{
						field_name : Str,
						binding : Binding,
						element_count : U64,
						element_type : VectorElementType,
					},
				),
				Semantic(
					{
						field_name : Str,
						semantic_name : Str,
						element_count : U64,
						element_type : VectorElementType,
					},
				),
			],
		),
		Struct(
			{
				field_name : Str,
				binding : Binding,
				struct_type : StructType,
			},
		),
		Matrix(
			{
				field_name : Str,
				binding : Binding,
				row_count : U32,
				column_count : U32,
				element_type : VectorElementType,
			},
		),
		Resource(
			{
				field_name : Str,
				binding : Binding,
				resource_shape : [Texture2D, RWTexture2D],
				result_type : ResourceResult,
			},
		),
		Pointer(
			{
				field_name : Str,
				binding : Binding,
				pointee_type : StructType,
				pointee_size : U64,
				# immutable here means immutable to the gpu
				access : [ReadWrite, Read, Immutable],
			},
		),
		Array(
			{
				field_name : Str,
				binding : Binding,
				element_scalar_type : ScalarType,
				element_count : U64,
				element_stride : U64,
			},
		),
		Enum(
			{
				field_name : Str,
				binding : Binding,
				enum_type : EnumType,
			},
		),
		DescriptorHandle(
			{
				field_name : Str,
				binding : Binding,
				shape : [Sampler2D, RwTexture2D],
			},
		),
	]

	GlobalParameter : [
		ParameterBlock(
			{
				parameter_name : Str,
				element_type : StructType,
			},
		),
		PushConstant(
			{
				parameter_name : Str,
				element_type : StructType,
				element_size : U64,
			},
		),
	]

	EntryParameter : [
		Struct(
			{
				parameter_name : Str,
				binding : Optional(Binding),
				type_name : Str,
				fields : List(Field),
			},
		),
		Scalar(
			[
				Bound(
					{
						parameter_name : Str,
						binding : Binding,
						scalar_type : ScalarType,
					},
				),
				Semantic(
					{
						parameter_name : Str,
						semantic_name : Str,
						scalar_type : ScalarType,
					},
				),
			],
		),
	]

	EntryPoint : {
		entry_point_name : Str,
		stage : Stage,
		parameters : List(EntryParameter),
	}

	DescriptorBinding : {
		binding : U32,
		descriptor_type : [
			Sampler,
			Texture,
			ConstantBuffer,
			CombinedTextureSampler,
			StorageImage,
		],
		descriptor_count : U32,
		stage_flags : StageFlags,
		size : U64,
	}

	PushConstantRange : {
		stage_flags : StageFlags,
		offset : U32,
		size : U32,
	}

	PipelineLayout : {
		descriptor_set_layouts : List({ binding_ranges : List(DescriptorBinding) }),
		push_constant_ranges : List(PushConstantRange),
		bindless_heap_set : Optional(U32),
	}

	GraphicsReflection : {
		source_file_name : Str,
		global_parameters : List(GlobalParameter),
		vertex_entry_point : EntryPoint,
		fragment_entry_point : EntryPoint,
		pipeline_layout : PipelineLayout,
	}

	ComputeReflection : {
		source_file_name : Str,
		global_parameters : List(GlobalParameter),
		compute_entry_point : EntryPoint,
		workgroup_size : { x : U32, y : U32, z : U32 },
		pipeline_layout : PipelineLayout,
	}

	## Byte packing for one buffer element: `stride` bytes per element.
	## Every generated struct with a GPU layout exposes one as `packer`.
	ElementPacker(elem) : { stride : U32, to_bytes : elem -> List(U8) }

	## The texture a sampled binding reads: a logical graph texture's latest
	## version, the version before its latest write, or an imported texture.
	SampledTexSource : [Read(U32), ReadPrevious(U32), Imported(U32)]

	## The logical graph texture a storage binding writes, and how: `Write`
	## produces the next version, `Mutate` edits the current one in place.
	StorageTexSource : [Write(U32), Mutate(U32)]

	## The buffer slot a buffer binding addresses. Only GPU-updated buffers
	## keep a `Previous` slot.
	BufferSlot : [Current, Previous]

	## A buffer binding's declared buffer, slot, and the byte offset of the
	## bound element.
	BufferTarget : { buffer : U32, slot : BufferSlot, offset : U32 }

	## Whether a shader may write through a buffer binding.
	BufferAccess : [Read, Mutate]

	## One type-erased binding, as a block schema's `extract_bindings`
	## reports it next to its field key.
	GraphBinding : [
		SampledTex(SampledTexSource),
		StorageTex(StorageTexSource),
		Buffer(BufferTarget, BufferAccess),
	]

	## A sampled texture for a `Sampler2D.Handle` field.
	SampledTexBinding :: { source : SampledTexSource }.{
		new : SampledTexSource -> SampledTexBinding
		new = |source| SampledTexBinding.{ source }

		erase : SampledTexBinding -> GraphBinding
		erase = |binding| SampledTex(binding.source)
	}

	## A storage texture for a `RWTexture2D.Handle` field.
	StorageTexBinding :: { source : StorageTexSource }.{
		new : StorageTexSource -> StorageTexBinding
		new = |source| StorageTexBinding.{ source }

		erase : StorageTexBinding -> GraphBinding
		erase = |binding| StorageTex(binding.source)
	}

	## A read-write buffer of `elem` for a `mltrs::Addr<T>` field.
	BufferBinding(elem) :: { target : BufferTarget }.{
		new : BufferTarget -> BufferBinding(elem)
		new = |target| BufferBinding.{ target }

		erase : BufferBinding(elem) -> GraphBinding
		erase = |binding| Buffer(binding.target, Mutate)

		## Bind the same buffer to a `mltrs::ReadAddr<T>` field.
		as_read : BufferBinding(elem) -> ReadBufferBinding(elem)
		as_read = |binding| ReadBufferBinding.new(binding.target)
	}

	## A read-only buffer of `elem` for a `mltrs::ReadAddr<T>` field.
	ReadBufferBinding(elem) :: { target : BufferTarget }.{
		new : BufferTarget -> ReadBufferBinding(elem)
		new = |target| ReadBufferBinding.{ target }

		erase : ReadBufferBinding(elem) -> GraphBinding
		erase = |binding| Buffer(binding.target, Read)
	}

	## A buffer of `elem` the GPU never writes, for a `mltrs::ImmutableAddr<T>`
	## field.
	ImmutableBufferBinding(elem) :: { target : BufferTarget }.{
		new : BufferTarget -> ImmutableBufferBinding(elem)
		new = |target| ImmutableBufferBinding.{ target }

		erase : ImmutableBufferBinding(elem) -> GraphBinding
		erase = |binding| Buffer(binding.target, Read)

		## Bind the same buffer to a `mltrs::ReadAddr<T>` field.
		as_read : ImmutableBufferBinding(elem) -> ReadBufferBinding(elem)
		as_read = |binding| ReadBufferBinding.new(binding.target)
	}

	## The resource kind of each binding field, indexed by field key.
	ResourceFieldKind : [SampledTex, StorageTex, BufAddr]

	## Where a schema field's bytes come from: `src_offset` into the packed
	## per-frame data, or a resolved 8-byte binding.
	SchemaFieldKind : [Data({ src_offset : U32 }), SampledTex, StorageTex, BufAddr]

	## One field of a GPU block: `len` bytes at `offset`. A binding field's
	## `key` indexes `resource_fields`; data fields take the keys after them.
	SchemaField : { key : U16, offset : U32, len : U32, kind : SchemaFieldKind }

	## How a parameter block or push block splits into per-frame `data` and
	## build-time `bindings`, and how the two assemble into `gpu_size` bytes.
	##
	## `pack_data` produces exactly `data_size` bytes. A block without data
	## fields has `data_size` 0 and no `Data` fields, so it lowers with no
	## data value.
	BlockSchema(data, bindings) : {
		name : Str,
		gpu_size : U32,
		data_size : U32,
		resource_fields : List(ResourceFieldKind),
		fields : List(SchemaField),
		pack_data : data -> List(U8),
		extract_bindings : bindings -> List((U16, GraphBinding)),
	}

	## A shader's `ParameterBlock` global: its position among the shader's
	## parameter blocks, in descriptor-set-layout order, and its schema.
	UniformSpec(data, bindings) : {
		parameter_name : Str,
		index : U32,
		schema : BlockSchema(data, bindings),
	}

	## A declared uniform buffer for one parameter block's schema.
	UniformSlot(data, bindings) :: {
		index : U32,
		schema : BlockSchema(data, bindings),
	}.{
		new : U32, BlockSchema(data, bindings) -> UniformSlot(data, bindings)
		new = |slot_index, block_schema|
			UniformSlot.{ index: slot_index, schema: block_schema }

		index : UniformSlot(data, bindings) -> U32
		index = |slot| slot.index

		schema : UniformSlot(data, bindings) -> BlockSchema(data, bindings)
		schema = |slot| slot.schema
	}

	## A shader's push-constant block.
	PushSpec(data, bindings) := {
		parameter_name : Str,
		schema : BlockSchema(data, bindings),
	}

	## The push type of a shader without a push-constant block.
	NoPush := {}

	## A graphics shader drawn with direct vertex-count or indexed draws.
	## `uniform_slots` maps one `UniformSlot` per parameter block, in a
	## record named after the parameters, to slot indices in
	## descriptor-set-layout order.
	GraphicsShader(uniforms, push) := {
		name : Str,
		vertex_spv : List(U8),
		fragment_spv : List(U8),
		reflection_json : Str,
		reflection : GraphicsReflection,
		uniform_slots : uniforms -> List(U32),
		push : push,
	}

	## A graphics shader whose vertex entry point reads `SV_DrawIndex`, drawn
	## with indexed-indirect draws.
	GraphicsIndirectShader(uniforms, push) := {
		name : Str,
		vertex_spv : List(U8),
		fragment_spv : List(U8),
		reflection_json : Str,
		reflection : GraphicsReflection,
		uniform_slots : uniforms -> List(U32),
		push : push,
	}

	## A compute shader; `workgroup_size` is its threads per workgroup, from
	## `[numthreads]`.
	ComputeShader(uniforms, push) := {
		name : Str,
		compute_spv : List(U8),
		reflection_json : Str,
		reflection : ComputeReflection,
		workgroup_size : { x : U32, y : U32, z : U32 },
		uniform_slots : uniforms -> List(U32),
		push : push,
	}

	## A physical buffer address.
	PointerAddress := U64.{
		is_eq : _

		to_bytes : PointerAddress -> List(U8)
		to_bytes = |PointerAddress.(raw)| u64_bytes(raw)
	}

	## A bindless descriptor index.
	DescriptorHandle := U64.{
		is_eq : _

		to_bytes : DescriptorHandle -> List(U8)
		to_bytes = |DescriptorHandle.(raw)| u64_bytes(raw)
	}

	Float1 := { x : F32 }.{
		is_eq : _

		to_bytes : Float1 -> List(U8)
		to_bytes = |vector| f32_bytes(vector.x)
	}

	Float2 := { x : F32, y : F32 }.{
		is_eq : _

		to_bytes : Float2 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y].join_map(f32_bytes)
	}

	Float3 := { x : F32, y : F32, z : F32 }.{
		is_eq : _

		to_bytes : Float3 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y, vector.z].join_map(f32_bytes)
	}

	Float4 := { x : F32, y : F32, z : F32, w : F32 }.{
		is_eq : _

		to_bytes : Float4 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y, vector.z, vector.w].join_map(f32_bytes)
	}

	Float4x4 := {
		row_0 : Float4,
		row_1 : Float4,
		row_2 : Float4,
		row_3 : Float4,
	}.{
		is_eq : _

		identity : Float4x4
		identity = {
			row_0: { x: 1.0, y: 0.0, z: 0.0, w: 0.0 },
			row_1: { x: 0.0, y: 1.0, z: 0.0, w: 0.0 },
			row_2: { x: 0.0, y: 0.0, z: 1.0, w: 0.0 },
			row_3: { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
		}

		# row-major
		to_bytes : Float4x4 -> List(U8)
		to_bytes = |matrix| [matrix.row_0, matrix.row_1, matrix.row_2, matrix.row_3]
			.join_map(|row| row.to_bytes())
	}

	Int1 := { x : I32 }.{
		is_eq : _

		to_bytes : Int1 -> List(U8)
		to_bytes = |vector| i32_bytes(vector.x)
	}

	Int2 := { x : I32, y : I32 }.{
		is_eq : _

		to_bytes : Int2 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y].join_map(i32_bytes)
	}

	Int3 := { x : I32, y : I32, z : I32 }.{
		is_eq : _

		to_bytes : Int3 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y, vector.z].join_map(i32_bytes)
	}

	Int4 := { x : I32, y : I32, z : I32, w : I32 }.{
		is_eq : _

		to_bytes : Int4 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y, vector.z, vector.w].join_map(i32_bytes)
	}

	Int4x4 := {
		row_0 : Int4,
		row_1 : Int4,
		row_2 : Int4,
		row_3 : Int4,
	}.{
		is_eq : _

		identity : Int4x4
		identity = {
			row_0: { x: 1, y: 0, z: 0, w: 0 },
			row_1: { x: 0, y: 1, z: 0, w: 0 },
			row_2: { x: 0, y: 0, z: 1, w: 0 },
			row_3: { x: 0, y: 0, z: 0, w: 1 },
		}

		# row-major
		to_bytes : Int4x4 -> List(U8)
		to_bytes = |matrix| [matrix.row_0, matrix.row_1, matrix.row_2, matrix.row_3]
			.join_map(|row| row.to_bytes())
	}

	Uint1 := { x : U32 }.{
		is_eq : _

		to_bytes : Uint1 -> List(U8)
		to_bytes = |vector| u32_bytes(vector.x)
	}

	Uint2 := { x : U32, y : U32 }.{
		is_eq : _

		to_bytes : Uint2 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y].join_map(u32_bytes)
	}

	Uint3 := { x : U32, y : U32, z : U32 }.{
		is_eq : _

		to_bytes : Uint3 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y, vector.z].join_map(u32_bytes)
	}

	Uint4 := { x : U32, y : U32, z : U32, w : U32 }.{
		is_eq : _

		to_bytes : Uint4 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y, vector.z, vector.w].join_map(u32_bytes)
	}

	Uint4x4 := {
		row_0 : Uint4,
		row_1 : Uint4,
		row_2 : Uint4,
		row_3 : Uint4,
	}.{
		is_eq : _

		identity : Uint4x4
		identity = {
			row_0: { x: 1, y: 0, z: 0, w: 0 },
			row_1: { x: 0, y: 1, z: 0, w: 0 },
			row_2: { x: 0, y: 0, z: 1, w: 0 },
			row_3: { x: 0, y: 0, z: 0, w: 1 },
		}

		# row-major
		to_bytes : Uint4x4 -> List(U8)
		to_bytes = |matrix| [matrix.row_0, matrix.row_1, matrix.row_2, matrix.row_3]
			.join_map(|row| row.to_bytes())
	}

	Uint64x1 := { x : U64 }.{
		is_eq : _

		to_bytes : Uint64x1 -> List(U8)
		to_bytes = |vector| u64_bytes(vector.x)
	}

	Uint64x2 := { x : U64, y : U64 }.{
		is_eq : _

		to_bytes : Uint64x2 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y].join_map(u64_bytes)
	}

	Uint64x3 := { x : U64, y : U64, z : U64 }.{
		is_eq : _

		to_bytes : Uint64x3 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y, vector.z].join_map(u64_bytes)
	}

	Uint64x4 := { x : U64, y : U64, z : U64, w : U64 }.{
		is_eq : _

		to_bytes : Uint64x4 -> List(U8)
		to_bytes = |vector| [vector.x, vector.y, vector.z, vector.w].join_map(u64_bytes)
	}

	Uint64x4x4 := {
		row_0 : Uint64x4,
		row_1 : Uint64x4,
		row_2 : Uint64x4,
		row_3 : Uint64x4,
	}.{
		is_eq : _

		identity : Uint64x4x4
		identity = {
			row_0: { x: 1, y: 0, z: 0, w: 0 },
			row_1: { x: 0, y: 1, z: 0, w: 0 },
			row_2: { x: 0, y: 0, z: 1, w: 0 },
			row_3: { x: 0, y: 0, z: 0, w: 1 },
		}

		# row-major
		to_bytes : Uint64x4x4 -> List(U8)
		to_bytes = |matrix| [matrix.row_0, matrix.row_1, matrix.row_2, matrix.row_3]
			.join_map(|row| row.to_bytes())
	}

	## GPU byte packing. Generated `<Type>.to_bytes` functions build a struct's
	## bytes from these helpers and the offsets reflection reports.

	## Little-endian bytes of a U32.
	u32_bytes : U32 -> List(U8)
	u32_bytes = |value| [
		value.to_u8_wrap(),
		value.shr_zf_wrap(8).to_u8_wrap(),
		value.shr_zf_wrap(16).to_u8_wrap(),
		value.shr_zf_wrap(24).to_u8_wrap(),
	]

	i32_bytes : I32 -> List(U8)
	i32_bytes = |value| u32_bytes(value.to_u32_wrap())

	u64_bytes : U64 -> List(U8)
	u64_bytes = |value| {
		low = u32_bytes(value.to_u32_wrap())

		low.concat(u32_bytes(value.shr_zf_wrap(32).to_u32_wrap()))
	}

	## IEEE 754 single-precision bytes, little-endian.
	f32_bytes : F32 -> List(U8)
	f32_bytes = |value| u32_bytes(value.to_bits())

	## `count` elements of `stride` bytes each, in list order.
	array_bytes : List(a), U64, U64, (a -> List(U8)) -> List(U8)
	array_bytes = |items, count, stride, element_bytes| {
		if items.len() != count {
			crash
				\\GPU byte packing: fixed array expects ${count.to_str()} elements,
				\\got ${items.len().to_str()}
		}

		items.join_map(|item| pad_to(element_bytes(item), stride))
	}

	## A struct of `size` bytes from `(offset, bytes)` fields in ascending
	## offset order. Gaps become zero padding; an overlap crashes.
	pack : U64, List((U64, List(U8))) -> List(U8)
	pack = |size, fields| {
		append_padded = |acc, (offset, bytes)| pad_to(acc, offset).concat(bytes)
		packed = fields.fold([], append_padded)

		pad_to(packed, size)
	}
}

## Zero-pad `bytes` to `size`. Crashes when the bytes already exceed it.
pad_to : List(U8), U64 -> List(U8)
pad_to = |bytes, size| {
	len = bytes.len()

	if len > size {
		crash "GPU byte packing: ${len.to_str()} bytes do not fit in ${size.to_str()}"
	}

	bytes.concat(List.repeat(0, size - len))
}
