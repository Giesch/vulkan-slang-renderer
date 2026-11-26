# Generated; logical values are not GPU memory layouts.
import pf.ShaderReflection
import "../shaders/compiled/basic_triangle.vert.spv" as vertex_bytes : List(U8)
import "../shaders/compiled/basic_triangle.frag.spv" as fragment_bytes : List(U8)

BasicTriangle := {}.{
	mvp_matrices_type : ShaderReflection.StructType
	mvp_matrices_type = ShaderReflection.StructType.{
		type_name: "MVPMatrices",
		fields: [
			Matrix({
				field_name: "model",
				binding: Uniform({
					offset: 0,
					size: 64,
				}),
				row_count: 4,
				column_count: 4,
				element_type: Scalar({
					scalar_type: Float32,
				}),
			}),
			Matrix({
				field_name: "view",
				binding: Uniform({
					offset: 64,
					size: 64,
				}),
				row_count: 4,
				column_count: 4,
				element_type: Scalar({
					scalar_type: Float32,
				}),
			}),
			Matrix({
				field_name: "proj",
				binding: Uniform({
					offset: 128,
					size: 64,
				}),
				row_count: 4,
				column_count: 4,
				element_type: Scalar({
					scalar_type: Float32,
				}),
			}),
		],
	}

	triangle_params_type : ShaderReflection.StructType
	triangle_params_type = ShaderReflection.StructType.{
		type_name: "TriangleParams",
		fields: [
			Struct({
				field_name: "mvp",
				binding: Uniform({
					offset: 0,
					size: 192,
				}),
				struct_type: mvp_matrices_type,
			}),
			Pointer({
				field_name: "vertices",
				binding: Uniform({
					offset: 192,
					size: 8,
				}),
				pointee_type: vertex_type,
				pointee_size: 32,
				access: Immutable,
			}),
		],
	}

	vertex_type : ShaderReflection.StructType
	vertex_type = ShaderReflection.StructType.{
		type_name: "Vertex",
		fields: [
			Vector(
				Bound({
					field_name: "position",
					binding: Uniform({
						offset: 0,
						size: 12,
					}),
					element_count: 3,
					element_type: Scalar({
						scalar_type: Float32,
					}),
				}),
			),
			Vector(
				Bound({
					field_name: "color",
					binding: Uniform({
						offset: 16,
						size: 12,
					}),
					element_count: 3,
					element_type: Scalar({
						scalar_type: Float32,
					}),
				}),
			),
		],
	}

	reflection : ShaderReflection.GraphicsReflection
	reflection = {
		source_file_name: "basic_triangle.shader.slang",
		global_parameters: [
			ParameterBlock({
				parameter_name: "params",
				element_type: triangle_params_type,
			}),
		],
		vertex_entry_point: {
			entry_point_name: "vertexMain",
			stage: Vertex,
			parameters: [
				Scalar(
					Semantic({
						parameter_name: "vertexId",
						semantic_name: "SV_VERTEXID",
						scalar_type: Uint32,
					}),
				),
			],
		},
		fragment_entry_point: {
			entry_point_name: "fragmentMain",
			stage: Fragment,
			parameters: [
				Struct({
					parameter_name: "fragVertex",
					binding: Some(
						VaryingInput({
							index: 0,
							count: 1,
						}),
					),
					type_name: "FragInput",
					fields: [
						Vector(
							Semantic({
								field_name: "position",
								semantic_name: "SV_POSITION",
								element_count: 4,
								element_type: Scalar({
									scalar_type: Float32,
								}),
							}),
						),
						Vector(
							Bound({
								field_name: "color",
								binding: VaryingInput({
									index: 0,
									count: 1,
								}),
								element_count: 3,
								element_type: Scalar({
									scalar_type: Float32,
								}),
							}),
						),
					],
				}),
			],
		},
		pipeline_layout: {
			descriptor_set_layouts: [
				{
					binding_ranges: [
						{
							binding: 0,
							descriptor_type: ConstantBuffer,
							descriptor_count: 1,
							stage_flags: All,
							size: 208,
						},
					],
				},
			],
			push_constant_ranges: [],
			bindless_heap_set: None,
		},
	}

	stages = {
		vertex: vertex_bytes,
		fragment: fragment_bytes,
	}
}
