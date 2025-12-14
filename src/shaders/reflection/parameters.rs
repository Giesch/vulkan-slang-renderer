//! reflection generating a json format based on slangc's, implemented originally here:
//! https://github.com/shader-slang/slang/blob/master/source/slang/slang-reflection-json.cpp

use shader_slang as slang;

use crate::shaders::json::*;

pub struct Parameters {
    pub global_parameters: Vec<GlobalParameter>,
    pub entry_points: VertFragEntryPoints,
}

pub struct VertFragEntryPoints {
    pub vertex_entry_point: EntryPoint,
    pub fragment_entry_point: EntryPoint,
}

pub fn reflect_entry_points(
    program_layout: &slang::reflection::Shader,
) -> anyhow::Result<Parameters> {
    let mut vertex_entry_point: Option<EntryPoint> = None;
    let mut fragment_entry_point: Option<EntryPoint> = None;

    let mut global_parameters: Vec<GlobalParameter> = vec![];
    for global_param in program_layout.parameters() {
        let parameter_name = global_param.name().unwrap().to_string();

        if global_param.type_layout().kind() != slang::TypeKind::ParameterBlock {
            anyhow::bail!(
                "non-ParameterBlock global: {parameter_name}; only ParameterBlock globals are supported"
            )
        }

        let element_type_layout = global_param.type_layout().element_type_layout();

        let element_type = match element_type_layout.kind() {
            slang::TypeKind::Struct => {
                let element_type_name = element_type_layout.name().unwrap().to_string();
                let fields = reflect_struct_fields(element_type_layout)?;

                ParameterBlockElementType {
                    type_name: element_type_name,
                    fields,
                }
            }

            k => unimplemented!("type kind reflection not implemented: {k:?}"),
        };

        let parameter_block = ParameterBlockGlobalParameter {
            parameter_name,
            element_type,
        };
        let global_parameter = GlobalParameter::ParameterBlock(parameter_block);

        global_parameters.push(global_parameter);
    }

    for entry_point in program_layout.entry_points() {
        let entry_point_name = entry_point.name().to_string();

        let mut params = vec![];
        for param in entry_point.parameters() {
            let parameter_name = param.name().unwrap().to_string();

            let type_layout = param.type_layout();

            let entry_point_param_json = match type_layout.kind() {
                slang::TypeKind::Struct => {
                    let fields = reflect_struct_fields(type_layout)?;
                    let type_name = type_layout.name().unwrap().to_string();

                    EntryPointParameter::Struct(StructEntryPointParameter {
                        parameter_name,
                        binding: param_binding(param),
                        type_name,
                        fields,
                    })
                }

                slang::TypeKind::Scalar => {
                    let semantic = param.semantic_name().map(str::to_string);
                    let scalar_type = scalar_from_slang(type_layout.scalar_type().unwrap());

                    let scalar_param = match semantic {
                        Some(semantic_name) => {
                            ScalarEntryPointParameter::Semantic(SemanticScalarEntryPointParameter {
                                parameter_name,
                                scalar_type,
                                semantic_name,
                            })
                        }

                        None => {
                            let binding = param_binding(param).unwrap();
                            ScalarEntryPointParameter::Bound(BoundScalarEntryPointParameter {
                                parameter_name,
                                scalar_type,
                                binding,
                            })
                        }
                    };

                    EntryPointParameter::Scalar(scalar_param)
                }

                k => todo!("type kind reflection not implemented: {k:?}"),
            };

            params.push(entry_point_param_json);
        }

        match entry_point.stage() {
            slang::Stage::Vertex => {
                vertex_entry_point = Some(EntryPoint {
                    entry_point_name,
                    stage: EntryPointStage::Vertex,
                    parameters: params,
                });
            }

            slang::Stage::Fragment => {
                fragment_entry_point = Some(EntryPoint {
                    entry_point_name,
                    stage: EntryPointStage::Fragment,
                    parameters: params,
                });
            }

            _ => todo!(),
        }
    }

    let (vertex_entry_point, fragment_entry_point) =
        match (vertex_entry_point, fragment_entry_point) {
            (Some(v), Some(f)) => (v, f),
            _ => anyhow::bail!("failed to load vertex and fragment entry points"),
        };

    let entry_points = VertFragEntryPoints {
        vertex_entry_point,
        fragment_entry_point,
    };

    let parameters = Parameters {
        global_parameters,
        entry_points,
    };

    Ok(parameters)
}

fn reflect_struct_fields(
    struct_type_layout: &slang::reflection::TypeLayout,
) -> anyhow::Result<Vec<StructField>> {
    let mut fields = vec![];

    for field in struct_type_layout.fields() {
        let field_name = field.name().unwrap().to_string();
        let field_semantic_name = field.semantic_name().map(str::to_string);
        let field_type_layout = field.type_layout();

        // TODO handle this being optional in a better way; avoid the unwraps() below
        let binding = param_binding(field);

        let field_json = match field_type_layout.kind() {
            slang::TypeKind::Scalar => {
                let slang_scalar_type = field_type_layout.scalar_type().unwrap();
                let scalar_type = scalar_from_slang(slang_scalar_type);

                StructField::Scalar(ScalarStructField {
                    field_name,
                    binding: binding.unwrap(),
                    scalar_type,
                })
            }

            slang::TypeKind::Vector => {
                let vec_elem_count = field_type_layout.element_count().unwrap();

                let vec_element_type_layout = field_type_layout.element_type_layout();

                let slang_scalar_type = vec_element_type_layout.scalar_type().unwrap();
                let scalar_type = scalar_from_slang(slang_scalar_type);
                let vec_elem_type =
                    VectorElementType::Scalar(ScalarVectorElementType { scalar_type });

                let vec_struct_field = match (binding, field_semantic_name) {
                    (None, Some(field_semantic)) => {
                        VectorStructField::Semantic(SemanticVectorStructField {
                            field_name,
                            semantic_name: field_semantic,
                            element_count: vec_elem_count,
                            element_type: vec_elem_type,
                        })
                    }

                    (Some(field_binding), None) => {
                        VectorStructField::Bound(BoundVectorStructField {
                            field_name,
                            binding: field_binding,
                            element_count: vec_elem_count,
                            element_type: vec_elem_type,
                        })
                    }

                    (b, s) => {
                        anyhow::bail!(
                            "unexpected combination of vector binding and semantic {b:?}, {s:?}"
                        )
                    }
                };

                StructField::Vector(vec_struct_field)
            }

            slang::TypeKind::Matrix => {
                let row_count = field_type_layout.row_count().unwrap();
                let column_count = field_type_layout.column_count().unwrap();

                let mat_element_type_layout = field_type_layout.element_type_layout();

                let scalar_type = scalar_from_slang(mat_element_type_layout.scalar_type().unwrap());
                let element_type =
                    VectorElementType::Scalar(ScalarVectorElementType { scalar_type });

                StructField::Matrix(MatrixStructField {
                    field_name,
                    binding: binding.expect("matrix field without binding"),
                    row_count,
                    column_count,
                    element_type,
                })
            }

            slang::TypeKind::Struct => {
                let field_fields = reflect_struct_fields(field_type_layout)?;
                let field_type_name = field_type_layout.name().unwrap().to_string();

                StructField::Struct(StructStructField {
                    field_name,
                    binding: binding.expect("struct field without binding"),
                    struct_type: StructFieldType {
                        type_name: field_type_name,
                        fields: field_fields,
                    },
                })
            }

            slang::TypeKind::Resource => {
                let shape_with_flags = field_type_layout.resource_shape().unwrap();
                let slang_base_shape = slang_base_shape(shape_with_flags);

                let resource_shape = match slang_base_shape {
                    slang::ResourceShape::SlangTexture2d => ResourceShape::Texture2D,
                    s => todo!("unhandled slang base shape: {s:?}"),
                };

                let result_type = field_type_layout.resource_result_type().unwrap();
                let result_type = match result_type.kind() {
                    slang::TypeKind::Vector => {
                        let element_count = result_type.element_count();

                        let scalar_type = scalar_from_slang(result_type.scalar_type());
                        let element_type =
                            VectorElementType::Scalar(ScalarVectorElementType { scalar_type });

                        ResourceResultType::Vector(VectorResultType {
                            element_count,
                            element_type,
                        })
                    }
                    k => todo!("result type kind not handled: {k:?}"),
                };

                StructField::Resource(ResourceStructField {
                    field_name,
                    binding: binding.expect("resource struct field without binding"),
                    resource_shape,
                    result_type,
                })
            }

            k => todo!("field type layout kind not handled: {k:?}"),
        };

        fields.push(field_json);
    }

    Ok(fields)
}

fn slang_base_shape(shape_with_flags: slang::ResourceShape) -> slang::ResourceShape {
    // this is reproducing the way the base shape mask is used here:
    // https://github.com/shader-slang/slang/blob/9f9d28c1f496132dc71b80252b0eeddfa28cc8bc/source/slang/slang-reflection-json.cpp#L470
    let base_shape =
        shape_with_flags as u32 & slang::ResourceShape::SlangResourceBaseShapeMask as u32;
    unsafe { std::mem::transmute(base_shape) }
}

fn scalar_from_slang(scalar: slang::ScalarType) -> ScalarType {
    match scalar {
        slang::ScalarType::Uint32 => ScalarType::Uint32,
        slang::ScalarType::Float32 => ScalarType::Float32,
        k => todo!("slang scalar type not handled: {k:?}"),
    }
}

// returns None for a param with a semantic,
// where value will be provided by the driver
fn param_binding(param: &slang::reflection::VariableLayout) -> Option<Binding> {
    let category = param.category();

    let offset = param.offset(category);
    let size = param.type_layout().size(category);

    match category {
        slang::ParameterCategory::Uniform => {
            Some(Binding::Uniform(OffsetSizeBinding { offset, size }))
        }

        slang::ParameterCategory::DescriptorTableSlot => {
            Some(Binding::DescriptorTableSlot(IndexCountBinding {
                index: offset,
                count: size,
            }))
        }
        slang::ParameterCategory::VaryingInput => Some(Binding::VaryingInput(IndexCountBinding {
            index: offset,
            count: size,
        })),
        slang::ParameterCategory::ConstantBuffer => {
            Some(Binding::ConstantBuffer(IndexCountBinding {
                index: offset,
                count: size,
            }))
        }

        slang::ParameterCategory::None => None,

        c => todo!("param category not handled: {c:?}"),
    }
}
