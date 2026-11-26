import pf.ShaderReflection exposing [BlockSchema, ElementPacker, Float3, GraphicsReflection, GraphicsShader, ImmutableBufferBinding, NoPush, PointerAddress, StructType, UniformSlot, UniformSpec]

import Mltrs

import "../shaders/compiled/creative_coding.vert.spv" as vertex_bytes : List(U8)
import "../shaders/compiled/creative_coding.frag.spv" as fragment_bytes : List(U8)
import "../shaders/compiled/creative_coding.json" as reflection_json : Str

## Generated shader types, GPU byte packers, block schemas, and reflection.
CreativeCoding := {}.{
	Params := {
		matrices : Mltrs.MvpMatrices,
		vertices : PointerAddress,
		time : F32,
	}.{
		is_eq : _

		gpu_size : U32
		gpu_size = 208

		to_bytes : Params -> List(U8)
		to_bytes = |value|
			ShaderReflection.pack(
				gpu_size.to_u64(),
				[
					(0, Mltrs.MvpMatrices.to_bytes(value.matrices)),
					(192, PointerAddress.to_bytes(value.vertices)),
					(200, ShaderReflection.f32_bytes(value.time)),
				],
			)

		packer : ElementPacker(Params)
		packer = { stride: gpu_size, to_bytes }

		schema : BlockSchema(ParamsData, ParamsBindings)
		schema = {
			name: "Params",
			gpu_size: 208,
			data_size: 196,
			resource_fields: [BufAddr],
			fields: [
				{ key: 1, offset: 0, len: 192, kind: Data({ src_offset: 0 }) },
				{ key: 0, offset: 192, len: 8, kind: BufAddr },
				{ key: 2, offset: 200, len: 4, kind: Data({ src_offset: 192 }) },
			],
			pack_data: ParamsData.to_bytes,
			extract_bindings: |bindings| [
				(0, bindings.vertices.erase()),
			],
		}
	}

	ParamsBindings : {
		vertices : ImmutableBufferBinding(Vertex),
	}

	ParamsData := {
		matrices : Mltrs.MvpMatrices,
		time : F32,
	}.{
		is_eq : _

		gpu_size : U32
		gpu_size = 196

		to_bytes : ParamsData -> List(U8)
		to_bytes = |value|
			ShaderReflection.pack(
				gpu_size.to_u64(),
				[
					(0, Mltrs.MvpMatrices.to_bytes(value.matrices)),
					(192, ShaderReflection.f32_bytes(value.time)),
				],
			)

		packer : ElementPacker(ParamsData)
		packer = { stride: gpu_size, to_bytes }
	}

	Vertex := {
		position : Float3,
		color : Float3,
	}.{
		is_eq : _

		gpu_size : U32
		gpu_size = 32

		to_bytes : Vertex -> List(U8)
		to_bytes = |value|
			ShaderReflection.pack(
				gpu_size.to_u64(),
				[
					(0, Float3.to_bytes(value.position)),
					(16, Float3.to_bytes(value.color)),
				],
			)

		packer : ElementPacker(Vertex)
		packer = { stride: gpu_size, to_bytes }
	}

	mvp_matrices_type : StructType
	mvp_matrices_type = StructType.{
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

	params_type : StructType
	params_type = StructType.{
		type_name: "Params",
		fields: [
			Struct({
				field_name: "matrices",
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
			Scalar({
				field_name: "time",
				binding: Uniform({
					offset: 200,
					size: 4,
				}),
				scalar_type: Float32,
			}),
		],
	}

	vertex_type : StructType
	vertex_type = StructType.{
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

	reflection : GraphicsReflection
	reflection = {
		source_file_name: "creative_coding.shader.slang",
		global_parameters: [
			ParameterBlock({
				parameter_name: "params",
				element_type: params_type,
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

	params : UniformSpec(ParamsData, ParamsBindings)
	params = {
		parameter_name: "params",
		index: 0,
		schema: Params.schema,
	}

	shader : GraphicsShader(
		{
			params : UniformSlot(ParamsData, ParamsBindings),
		},
		NoPush,
	)
	shader = GraphicsShader.{
		name: "creative_coding",
		vertex_spv: vertex_bytes,
		fragment_spv: fragment_bytes,
		reflection_json,
		reflection,
		uniform_slots: |slots| [slots.params.index()],
		push: NoPush.{},
	}
}
