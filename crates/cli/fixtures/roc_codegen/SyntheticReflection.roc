import pf.ShaderReflection

SyntheticReflection := {}.{
	all_bindings = [
		Uniform({ offset: 0, size: 4 }),
		PushConstant({ offset: 4, size: 8 }),
		DescriptorTableSlot({ index: 2, count: 0 }),
		VaryingInput({ index: 3, count: 1 }),
		ConstantBuffer({ index: 4, count: 18446744073709551615 }),
	]

	nested = ShaderReflection.StructType.{
		type_name: "Nested",
		fields: [Scalar({ field_name: "inner", binding: Uniform({ offset: 0, size: 4 }), scalar_type: Int32 })],
	}

	all_fields : List(ShaderReflection.Field)
	all_fields = [
		Scalar({ field_name: "scalar", binding: Uniform({ offset: 0, size: 4 }), scalar_type: Uint64 }),
		Vector(Bound({ field_name: "bound_vec", binding: VaryingInput({ index: 0, count: 1 }), element_count: 4, element_type: Scalar({ scalar_type: Uint32 }) })),
		Vector(Semantic({ field_name: "semantic_vec", semantic_name: "SV_POSITION", element_count: 3, element_type: Scalar({ scalar_type: Float32 }) })),
		Struct({ field_name: "nested", binding: Uniform({ offset: 16, size: 4 }), struct_type: nested }),
		Matrix({ field_name: "matrix", binding: Uniform({ offset: 32, size: 64 }), row_count: 3, column_count: 2, element_type: Scalar({ scalar_type: Int32 }) }),
		Resource({ field_name: "scalar_texture", binding: DescriptorTableSlot({ index: 0, count: 1 }), resource_shape: Texture2D, result_type: Scalar({ scalar_type: Float32 }) }),
		Resource({ field_name: "vector_texture", binding: DescriptorTableSlot({ index: 1, count: 1 }), resource_shape: RWTexture2D, result_type: Vector({ element_count: 4, element_type: Scalar({ scalar_type: Uint32 }) }) }),
		Resource({ field_name: "struct_texture", binding: DescriptorTableSlot({ index: 2, count: 1 }), resource_shape: Texture2D, result_type: Struct(nested) }),
		Pointer({ field_name: "read_write", binding: Uniform({ offset: 96, size: 8 }), pointee_type: nested, pointee_size: 4, access: ReadWrite }),
		Pointer({ field_name: "read_only", binding: Uniform({ offset: 104, size: 8 }), pointee_type: nested, pointee_size: 4, access: Read }),
		Pointer({ field_name: "immutable", binding: Uniform({ offset: 112, size: 8 }), pointee_type: nested, pointee_size: 4, access: Immutable }),
		Array({ field_name: "array", binding: Uniform({ offset: 128, size: 32 }), element_scalar_type: Int32, element_count: 2, element_stride: 16 }),
		Enum({ field_name: "signed_enum", binding: Uniform({ offset: 160, size: 4 }), enum_type: { type_name: "Signed", tag_type: Int32, cases: [{ name: "Minimum", value: -2147483648 }, { name: "Zero", value: 0 }] } }),
		Enum({ field_name: "unsigned_enum", binding: Uniform({ offset: 164, size: 4 }), enum_type: { type_name: "Unsigned", tag_type: Uint32, cases: [{ name: "Maximum", value: 4294967295 }] } }),
		DescriptorHandle({ field_name: "sampled", binding: Uniform({ offset: 168, size: 8 }), shape: Sampler2D }),
		DescriptorHandle({ field_name: "storage", binding: Uniform({ offset: 176, size: 8 }), shape: RwTexture2D }),
	]

	graphics : ShaderReflection.GraphicsReflection
	graphics = {
		source_file_name: "synthetic.shader.slang",
		global_parameters: [
			ParameterBlock({ parameter_name: "params", element_type: ShaderReflection.StructType.{ type_name: "Params", fields: all_fields } }),
			PushConstant({ parameter_name: "push", element_type: ShaderReflection.StructType.{ type_name: "Push", fields: [] }, element_size: 0 }),
		],
		vertex_entry_point: {
			entry_point_name: "vertexOriginal",
			stage: Vertex,
			parameters: [
				Struct({ parameter_name: "vertex", binding: None, type_name: "Vertex", fields: [] }),
				Struct({ parameter_name: "bound_vertex", binding: Some(VaryingInput({ index: 0, count: 0 })), type_name: "BoundVertex", fields: all_fields }),
				Scalar(Bound({ parameter_name: "bound_scalar", binding: ConstantBuffer({ index: 0, count: 1 }), scalar_type: Int32 })),
				Scalar(Semantic({ parameter_name: "semantic_scalar", semantic_name: "SV_SAMPLEINDEX", scalar_type: Uint32 })),
			],
		},
		fragment_entry_point: { entry_point_name: "fragmentOriginal", stage: Fragment, parameters: [] },
		pipeline_layout: {
			descriptor_set_layouts: [
				{
					binding_ranges: [
						{ binding: 0, descriptor_type: Sampler, descriptor_count: 0, stage_flags: Empty, size: 0 },
						{ binding: 1, descriptor_type: Texture, descriptor_count: 1, stage_flags: Vertex, size: 4 },
						{ binding: 2, descriptor_type: ConstantBuffer, descriptor_count: 2, stage_flags: Fragment, size: 8 },
						{ binding: 3, descriptor_type: CombinedTextureSampler, descriptor_count: 3, stage_flags: Compute, size: 16 },
						{ binding: 4, descriptor_type: StorageImage, descriptor_count: 4, stage_flags: All, size: 18446744073709551615 },
					],
				},
			],
			push_constant_ranges: [{ stage_flags: All, offset: 0, size: 128 }],
			bindless_heap_set: Some(0),
		},
	}

	compute : ShaderReflection.ComputeReflection
	compute = {
		source_file_name: "synthetic.compute.slang",
		global_parameters: [],
		compute_entry_point: { entry_point_name: "computeOriginal", stage: Compute, parameters: [] },
		workgroup_size: { x: 0, y: 1, z: 4294967295 },
		pipeline_layout: { descriptor_set_layouts: [], push_constant_ranges: [], bindless_heap_set: None },
	}
}
