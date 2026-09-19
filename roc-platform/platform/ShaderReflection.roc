## Shader reflection schema and logical shader value types.
##
## Generated shader modules import this module as `pf.ShaderReflection`.
## Every size or count that originates as a Rust `usize` is a `U64`.
##
## Logical values are application data. They do not describe GPU memory
## layout, and handles, addresses, and references carry no GPU validity,
## ownership, lookup, or dereference guarantee.
ShaderReflection := {}.{
	Optional(a) : [None, Some(a)]

	ScalarType : [Float32, Int32, Uint32, Uint64]

	VectorElementType : [
		Scalar(
			{
				scalar_type : ScalarType,
			},
		),
	]

	Stage : [Vertex, Fragment, Compute]

	StageFlags : [Vertex, Fragment, Compute, All, Empty]

	Binding : [
		Uniform(
			{
				offset : U64,
				size : U64,
			},
		),
		PushConstant(
			{
				offset : U64,
				size : U64,
			},
		),
		DescriptorTableSlot(
			{
				index : U64,
				count : U64,
			},
		),
		VaryingInput(
			{
				index : U64,
				count : U64,
			},
		),
		ConstantBuffer(
			{
				index : U64,
				count : U64,
			},
		),
	]

	StructType := {
		type_name : Str,
		fields : List(Field),
	}

	EnumType : {
		type_name : Str,
		tag_type : [Uint32, Int32],
		cases : List(
			{
				name : Str,
				value : I64,
			},
		),
	}

	ResourceResult : [
		Scalar(
			{
				scalar_type : ScalarType,
			},
		),
		Vector(
			{
				element_count : U64,
				element_type : VectorElementType,
			},
		),
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
		descriptor_type : [Sampler, Texture, ConstantBuffer, CombinedTextureSampler, StorageImage],
		descriptor_count : U32,
		stage_flags : StageFlags,
		size : U64,
	}

	PipelineLayout : {
		descriptor_set_layouts : List(
			{
				binding_ranges : List(DescriptorBinding),
			},
		),
		push_constant_ranges : List(
			{
				stage_flags : StageFlags,
				offset : U32,
				size : U32,
			},
		),
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
		workgroup_size : {
			x : U32,
			y : U32,
			z : U32,
		},
		pipeline_layout : PipelineLayout,
	}

	## A physical buffer address.
	PointerAddress : [PointerAddress(U64)]

	## A bindless descriptor index.
	DescriptorHandle : [DescriptorHandle(U64)]

	## A texture or storage resource, named by its reflected identifier.
	ResourceReference : [ResourceReference(Str)]

	Float1 : {
		x : F32,
	}

	Float2 : {
		x : F32,
		y : F32,
	}

	Float3 : {
		x : F32,
		y : F32,
		z : F32,
	}

	Float4 : {
		x : F32,
		y : F32,
		z : F32,
		w : F32,
	}

	Float4x4 : {
		row_0 : Float4,
		row_1 : Float4,
		row_2 : Float4,
		row_3 : Float4,
	}

	Int1 : {
		x : I32,
	}

	Int2 : {
		x : I32,
		y : I32,
	}

	Int3 : {
		x : I32,
		y : I32,
		z : I32,
	}

	Int4 : {
		x : I32,
		y : I32,
		z : I32,
		w : I32,
	}

	Int4x4 : {
		row_0 : Int4,
		row_1 : Int4,
		row_2 : Int4,
		row_3 : Int4,
	}

	Uint1 : {
		x : U32,
	}

	Uint2 : {
		x : U32,
		y : U32,
	}

	Uint3 : {
		x : U32,
		y : U32,
		z : U32,
	}

	Uint4 : {
		x : U32,
		y : U32,
		z : U32,
		w : U32,
	}

	Uint4x4 : {
		row_0 : Uint4,
		row_1 : Uint4,
		row_2 : Uint4,
		row_3 : Uint4,
	}

	Uint64x1 : {
		x : U64,
	}

	Uint64x2 : {
		x : U64,
		y : U64,
	}

	Uint64x3 : {
		x : U64,
		y : U64,
		z : U64,
	}

	Uint64x4 : {
		x : U64,
		y : U64,
		z : U64,
		w : U64,
	}

	Uint64x4x4 : {
		row_0 : Uint64x4,
		row_1 : Uint64x4,
		row_2 : Uint64x4,
		row_3 : Uint64x4,
	}
}
