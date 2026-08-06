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

pub struct ComputeParameters {
    pub global_parameters: Vec<GlobalParameter>,
    pub compute_entry_point: EntryPoint,
    pub workgroup_size: [u32; 3],
}

pub fn reflect_entry_points(
    program_layout: &slang::reflection::Shader,
) -> anyhow::Result<Parameters> {
    let mut vertex_entry_point: Option<EntryPoint> = None;
    let mut fragment_entry_point: Option<EntryPoint> = None;

    let mut global_parameters: Vec<GlobalParameter> = vec![];
    for global_param in program_layout.parameters() {
        let parameter_name = global_param.name().unwrap().to_string();

        if global_param.type_layout().unwrap().kind() != slang::TypeKind::ParameterBlock {
            anyhow::bail!(
                "non-ParameterBlock global: {parameter_name}; only ParameterBlock globals are supported"
            )
        }

        let element_type_layout = global_param
            .type_layout()
            .unwrap()
            .element_type_layout()
            .unwrap();

        let element_type = match element_type_layout.kind() {
            slang::TypeKind::Struct => {
                let element_type_name = element_type_layout.name().unwrap().to_string();
                let fields = reflect_struct_fields(element_type_layout, program_layout, false)?;

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
        let entry_point_name = entry_point.name().unwrap().to_string();

        let mut params = vec![];
        for param in entry_point.parameters() {
            let parameter_name = param.name().unwrap().to_string();

            let type_layout = param.type_layout().unwrap();

            let entry_point_param_json = match type_layout.kind() {
                slang::TypeKind::Struct => {
                    let fields = reflect_struct_fields(type_layout, program_layout, false)?;
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
    program_layout: &slang::reflection::Shader,
    in_pointer_pointee: bool,
) -> anyhow::Result<Vec<StructField>> {
    let mut fields = vec![];

    for field in struct_type_layout.fields() {
        let field_name = field.name().unwrap().to_string();
        let field_semantic_name = field.semantic_name().map(str::to_string);
        let field_type_layout = field.type_layout().unwrap();

        let binding = param_binding(field);

        // Slang lays an enum out as its tag type (slang-type-layout.cpp), so
        // field_type_layout.kind() reports Scalar here and the match below would
        // silently degrade the field to its tag. The enum identity survives only
        // on the *declared* type, reached through the variable.
        if let Some(declared) = field.ty()
            && declared.kind() == slang::TypeKind::Enum
        {
            // A vertex input field reflects as VaryingInput, not Uniform. Codegen
            // has no vk::Format for a generated enum, so reject it here with a
            // message that names the field rather than in the format match.
            let Some(binding @ Binding::Uniform(_)) = binding else {
                anyhow::bail!(
                    "enum field '{field_name}': enums are only supported in \
                    uniform/pointee struct fields, not vertex inputs"
                );
            };

            fields.push(StructField::Enum(reflect_enum_field(
                field_name, binding, declared,
            )?));
            continue;
        }

        // DescriptorHandle lowers to a uint2;
        // avoid accidentally treating it as a vector below
        if let Some(declared) = field.ty() {
            let declared_name = declared_full_name(declared);
            if declared_name.starts_with("DescriptorHandle<") {
                anyhow::bail!(
                    "field '{field_name}' ({declared_name}): texture handle fields are \
                    not supported yet; declare the texture as a Texture2D/Sampler2D \
                    resource in the parameter block instead"
                );
            }
        }

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

                let vec_element_type_layout = field_type_layout.element_type_layout().unwrap();

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

                    (Some(field_binding), _optional_semantic) => {
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

                let mat_element_type_layout = field_type_layout.element_type_layout().unwrap();

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
                let field_fields =
                    reflect_struct_fields(field_type_layout, program_layout, in_pointer_pointee)?;
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
                let shape = field_type_layout.resource_shape().unwrap();

                let resource_shape = match shape.base() {
                    slang::BaseShape::Texture2D => {
                        let access = field_type_layout.resource_access();
                        if access == Some(slang::ResourceAccess::ReadWrite) {
                            ResourceShape::RWTexture2D
                        } else {
                            ResourceShape::Texture2D
                        }
                    }
                    slang::BaseShape::StructuredBuffer => anyhow::bail!(
                        "field '{field_name}': StructuredBuffer/RWStructuredBuffer descriptors \
                        are unsupported; use a BDA pointer instead (e.g. mltrs::Addr<T> via \
                        import mltrs, or LayoutPtr<T, Std430DataLayout>)"
                    ),
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

                    slang::TypeKind::Struct => {
                        let element_type_layout = field_type_layout.element_type_layout().unwrap();
                        let element_type_name = element_type_layout.name().unwrap().to_string();

                        let struct_fields =
                            reflect_struct_fields(element_type_layout, program_layout, false)?;

                        let struct_result_type = StructResultType {
                            type_name: element_type_name,
                            fields: struct_fields,
                        };

                        ResourceResultType::Struct(struct_result_type)
                    }

                    slang::TypeKind::Scalar => {
                        let scalar_type = scalar_from_slang(result_type.scalar_type());
                        ResourceResultType::Scalar(ScalarResultType { scalar_type })
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

            slang::TypeKind::Pointer => {
                if in_pointer_pointee {
                    anyhow::bail!(
                        "pointer field '{field_name}': nested pointers \
                        (a pointer inside a pointer's pointee) are not supported"
                    );
                }

                let ptr_type = field_type_layout.ty().unwrap();
                let ptr_type_name = declared_full_name(ptr_type);

                // A default `T*` pointee uses slang's natural (C-like) layout, which
                // reflection misreports for layout-annotated pointers and glam types
                // cannot always express; only std430 pointees are supported.
                let layout_arg = ptr_type_name
                    .trim_end_matches('>')
                    .rsplit(',')
                    .next()
                    .map(str::trim)
                    .unwrap_or_default();
                if layout_arg != "Std430DataLayout" {
                    anyhow::bail!(
                        "pointer field '{field_name}' ({ptr_type_name}): only Std430DataLayout \
                        pointers are supported; declare it as mltrs::Addr<T> (import mltrs;) \
                        or LayoutPtr<T, Std430DataLayout>"
                    );
                }

                // The Access generic argument prints as its enum case name
                // (e.g. `Ptr<T, Access.Read, AddressSpace.Device, Std430DataLayout>`).
                let generic_args: Vec<&str> = ptr_type_name
                    .trim_end_matches('>')
                    .split(',')
                    .map(str::trim)
                    .collect();
                let access = if generic_args.contains(&"Access.Read") {
                    PointerAccess::Read
                } else if generic_args.contains(&"Access.Immutable") {
                    PointerAccess::Immutable
                } else {
                    PointerAccess::ReadWrite
                };

                // The pointer's own element_type_layout() reports default-layout
                // offsets even for Std430DataLayout pointers; query the std430
                // layout explicitly.
                let pointee_ty = field_type_layout
                    .element_type_layout()
                    .unwrap()
                    .ty()
                    .unwrap();
                let pointee_layout = program_layout
                    .type_layout(pointee_ty, slang::LayoutRules::DefaultStructuredBuffer)
                    .expect("failed to lay out pointer pointee type");

                if pointee_layout.kind() != slang::TypeKind::Struct {
                    anyhow::bail!(
                        "pointer field '{field_name}': only struct pointees are supported, got {:?}",
                        pointee_layout.kind()
                    );
                }

                let pointee_type_name = pointee_layout.name().unwrap().to_string();
                let pointee_fields = reflect_struct_fields(pointee_layout, program_layout, true)?;
                let pointee_size = pointee_layout.size(slang::ParameterCategory::Uniform);

                StructField::Pointer(PointerStructField {
                    field_name,
                    binding: binding.expect("pointer field without binding"),
                    pointee_type: StructFieldType {
                        type_name: pointee_type_name,
                        fields: pointee_fields,
                    },
                    pointee_size,
                    access,
                })
            }

            slang::TypeKind::Array => {
                let Some(field_binding @ Binding::Uniform(_)) = binding else {
                    anyhow::bail!(
                        "array field '{field_name}': arrays are only supported in \
                        uniform/pointee struct fields"
                    );
                };

                let element_count = field_type_layout
                    .element_count()
                    .expect("array field without element count");
                let element_type_layout = field_type_layout.element_type_layout().unwrap();
                let element_stride =
                    field_type_layout.element_stride(slang::ParameterCategory::Uniform);

                let element_kind = element_type_layout.kind();
                let component_count = element_type_layout.element_count().unwrap_or(0);
                let element_scalar_type = element_type_layout
                    .element_type_layout()
                    .and_then(|l| l.scalar_type())
                    .map(scalar_from_slang);

                validate_array_element(
                    &field_name,
                    element_kind,
                    component_count,
                    element_scalar_type,
                    element_stride,
                )?;

                StructField::Array(ArrayStructField {
                    field_name,
                    binding: field_binding,
                    element_scalar_type: element_scalar_type
                        .expect("validated as a vector element"),
                    element_count,
                    element_stride,
                })
            }

            k => todo!("field type layout kind not handled: {k:?}"),
        };

        fields.push(field_json);
    }

    Ok(fields)
}

/// A slang type's declared full name, eg.
/// `Ptr<Item, Access.Immutable, AddressSpace.Device, Std430DataLayout>` or
/// `DescriptorHandle<Sampler2D<vector<float,4>>>`. This is the only place some
/// type identities survive: a pointer's layout annotation and a descriptor
/// handle both vanish from the *layout* type.
fn declared_full_name(ty: &slang::reflection::Type) -> String {
    ty.full_name()
        .map(|blob| String::from_utf8_lossy(blob.as_slice()).to_string())
        .unwrap_or_default()
}

/// Only 16-byte vector elements (float4/int4/uint4) have stride == size in
/// BOTH std140 and std430; every other element type would need stride-aware
/// padding the codegen doesn't model (std140 rounds float[N]/float2[N]/
/// float3[N] strides up to 16, and struct elements get struct-size rounding).
fn validate_array_element(
    field_name: &str,
    element_kind: slang::TypeKind,
    component_count: usize,
    scalar_type: Option<ScalarType>,
    reflected_stride: usize,
) -> anyhow::Result<()> {
    let supported_element = element_kind == slang::TypeKind::Vector
        && component_count == 4
        && matches!(
            scalar_type,
            Some(ScalarType::Float32 | ScalarType::Int32 | ScalarType::Uint32)
        );

    if !supported_element || reflected_stride != 16 {
        anyhow::bail!(
            "array field '{field_name}': only float4/int4/uint4 element arrays are \
            supported (16-byte stride); got element kind {element_kind:?} with \
            {component_count} components, scalar type {scalar_type:?}, stride \
            {reflected_stride}; use a BDA buffer of flat structs, or named fields"
        );
    }

    Ok(())
}

/// Reflects a slang enum field off its *declared* type. On an Enum-kind Type,
/// slang overloads the struct-field API to mean enum cases: `fields()` yields
/// the cases and `element_type()` yields the tag type
/// (see spReflectionType_GetFieldByIndex / _GetElementType).
fn reflect_enum_field(
    field_name: String,
    binding: Binding,
    enum_type: &slang::reflection::Type,
) -> anyhow::Result<EnumStructField> {
    // slang synthesizes a name for an anonymous enum rather than reporting none,
    // and that name is neither meaningful to a caller nor a legal Rust type name
    // under clippy's non_camel_case_types
    let Some(type_name) = enum_type
        .name()
        .filter(|n| !n.starts_with("SLANG_anonymous"))
    else {
        anyhow::bail!(
            "enum field '{field_name}' has an anonymous enum type; give the enum a \
            name so the generated Rust enum has one too"
        );
    };
    let type_name = type_name.to_string();

    let Some(tag_type_layout) = enum_type.element_type() else {
        anyhow::bail!("enum '{type_name}' (field '{field_name}') has no reflected tag type");
    };
    let tag_type = enum_tag_from_slang(tag_type_layout.scalar_type(), &type_name)?;

    let mut cases: Vec<EnumCase> = vec![];
    for case in enum_type.fields() {
        let Some(name) = case.name() else {
            anyhow::bail!("enum '{type_name}' has a case with no name");
        };
        let Some(raw_value) = case.default_value_int() else {
            anyhow::bail!(
                "enum '{type_name}' case '{name}' has no constant value; only \
                compile-time constant cases can cross the reflection boundary"
            );
        };
        let value = normalize_case_value(raw_value, tag_type, &type_name, name)?;

        if let Some(clash) = cases.iter().find(|c| c.value == value) {
            anyhow::bail!(
                "enum '{type_name}' cases '{}' and '{name}' share the value {value}; \
                duplicate discriminants cannot be generated as a Rust enum",
                clash.name,
            );
        }

        cases.push(EnumCase {
            name: name.to_string(),
            value,
        });
    }

    if cases.is_empty() {
        anyhow::bail!(
            "enum '{type_name}' has no cases; the generated Rust enum needs at \
            least one case for Default and TryFrom"
        );
    }

    Ok(EnumStructField {
        field_name,
        binding,
        enum_type: EnumFieldType {
            type_name,
            tag_type,
            cases,
        },
    })
}

/// NOTE deliberately not `scalar_from_slang`: widening that one would also start
/// accepting plain `int` scalar fields that codegen does not support.
fn enum_tag_from_slang(scalar: slang::ScalarType, type_name: &str) -> anyhow::Result<EnumTagType> {
    match scalar {
        slang::ScalarType::Uint32 => Ok(EnumTagType::Uint32),
        slang::ScalarType::Int32 => Ok(EnumTagType::Int32),

        // A uint8_t/uint16_t tag lays out fine, but *reading* one makes slang
        // emit Int8/Int16 and UniformAndStorageBuffer{8,16}BitAccess. Those are
        // optional Vulkan feature bits, and requiring them would narrow the
        // supported hardware for a tag width no shader here needs.
        slang::ScalarType::Uint8 | slang::ScalarType::Uint16 | slang::ScalarType::Int16 => {
            anyhow::bail!(
                "enum '{type_name}' has a sub-32-bit tag type {scalar:?}; only uint \
                and int tags are supported, because reading a narrower tag requires \
                the 8/16-bit storage device features"
            )
        }

        other => anyhow::bail!(
            "enum '{type_name}' has an unsupported tag type {other:?}; supported \
            tags are uint and int (or none, which means int)"
        ),
    }
}

/// `default_value_int` returns i64, so an unsigned case may arrive sign-extended
/// (a `uint` case of 0xFFFFFFFF as -1). Emitting that verbatim into a
/// `match value: u32` would not compile, so normalize into the tag's range here.
fn normalize_case_value(
    raw: i64,
    tag_type: EnumTagType,
    type_name: &str,
    case_name: &str,
) -> anyhow::Result<i64> {
    let normalized = match tag_type {
        EnumTagType::Int32 => i32::try_from(raw).map(i64::from).ok(),
        EnumTagType::Uint32 => u32::try_from(raw)
            .map(i64::from)
            .ok()
            .or_else(|| i32::try_from(raw).map(|v| i64::from(v as u32)).ok()),
    };

    normalized.ok_or_else(|| {
        anyhow::anyhow!(
            "enum '{type_name}' case '{case_name}' has value {raw}, which does not \
            fit its {} tag",
            tag_type.rust_type_name(),
        )
    })
}

fn scalar_from_slang(scalar: slang::ScalarType) -> ScalarType {
    match scalar {
        slang::ScalarType::Int32 => ScalarType::Int32,
        slang::ScalarType::Uint32 => ScalarType::Uint32,
        slang::ScalarType::Uint64 => ScalarType::Uint64,
        slang::ScalarType::Float32 => ScalarType::Float32,
        k => todo!("slang scalar type not handled: {k:?}"),
    }
}

// returns None for a param with a semantic,
// where value will be provided by the driver
pub fn reflect_compute_entry_point(
    program_layout: &slang::reflection::Shader,
) -> anyhow::Result<ComputeParameters> {
    let mut global_parameters: Vec<GlobalParameter> = vec![];
    for global_param in program_layout.parameters() {
        let parameter_name = global_param.name().unwrap().to_string();

        if global_param.type_layout().unwrap().kind() != slang::TypeKind::ParameterBlock {
            anyhow::bail!(
                "non-ParameterBlock global: {parameter_name}; only ParameterBlock globals are supported"
            )
        }

        let element_type_layout = global_param
            .type_layout()
            .unwrap()
            .element_type_layout()
            .unwrap();

        let element_type = match element_type_layout.kind() {
            slang::TypeKind::Struct => {
                let element_type_name = element_type_layout.name().unwrap().to_string();
                let fields = reflect_struct_fields(element_type_layout, program_layout, false)?;

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

    let mut compute_entry_point: Option<EntryPoint> = None;
    let mut workgroup_size: Option<[u32; 3]> = None;

    for entry_point in program_layout.entry_points() {
        let entry_point_name = entry_point.name().unwrap().to_string();

        let params = vec![];
        // Compute entry point parameters are system values (SV_DispatchThreadID, etc.)
        // and don't need to be reflected for code generation

        match entry_point.stage() {
            slang::Stage::Compute => {
                let tgs = entry_point.compute_thread_group_size();
                workgroup_size = Some([tgs[0] as u32, tgs[1] as u32, tgs[2] as u32]);

                compute_entry_point = Some(EntryPoint {
                    entry_point_name,
                    stage: EntryPointStage::Compute,
                    parameters: params,
                });
            }

            _ => anyhow::bail!("expected compute entry point"),
        }
    }

    let compute_entry_point =
        compute_entry_point.ok_or_else(|| anyhow::anyhow!("failed to find compute entry point"))?;
    let workgroup_size =
        workgroup_size.ok_or_else(|| anyhow::anyhow!("failed to find workgroup size"))?;

    Ok(ComputeParameters {
        global_parameters,
        compute_entry_point,
        workgroup_size,
    })
}

fn param_binding(param: &slang::reflection::VariableLayout) -> Option<Binding> {
    let category = param.category().unwrap();

    let offset = param.offset(category);
    let size = param.type_layout().unwrap().size(category);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec4_array_elements_are_accepted() {
        for scalar in [ScalarType::Float32, ScalarType::Int32, ScalarType::Uint32] {
            validate_array_element("ok", slang::TypeKind::Vector, 4, Some(scalar), 16)
                .expect("float4/int4/uint4 elements with stride 16 must be accepted");
        }
    }

    #[test]
    fn non_vec4_array_elements_are_rejected() {
        let rejected = [
            // scalar elements: float[N] (std140 rounds the stride up to 16)
            ("bad", slang::TypeKind::Scalar, 0, None, 16),
            // small vector elements: float2[N] / float3[N]
            (
                "bad",
                slang::TypeKind::Vector,
                2,
                Some(ScalarType::Float32),
                16,
            ),
            (
                "bad",
                slang::TypeKind::Vector,
                3,
                Some(ScalarType::Float32),
                16,
            ),
            // unsupported element scalar type
            (
                "bad",
                slang::TypeKind::Vector,
                4,
                Some(ScalarType::Uint64),
                32,
            ),
            // struct elements (struct-size stride rounding)
            ("bad", slang::TypeKind::Struct, 0, None, 32),
            // a 16-byte vector whose reflected stride still disagrees
            (
                "bad",
                slang::TypeKind::Vector,
                4,
                Some(ScalarType::Float32),
                32,
            ),
        ];

        for (name, kind, components, scalar, stride) in rejected {
            let err = validate_array_element(name, kind, components, scalar, stride).expect_err(
                &format!(
                    "must reject element kind {kind:?}, {components} components, \
                    scalar {scalar:?}, stride {stride}"
                ),
            );

            let message = err.to_string();
            assert!(
                message.contains(
                    "array field 'bad': only float4/int4/uint4 element arrays are supported"
                ),
                "unexpected rejection message: {message}"
            );
            assert!(
                message.contains("use a BDA buffer of flat structs, or named fields"),
                "rejection message must point at the alternatives: {message}"
            );
        }
    }
}
