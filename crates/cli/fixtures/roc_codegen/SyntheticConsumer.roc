import SyntheticReflection

SyntheticConsumer := {}

expect List.len(SyntheticReflection.all_bindings) == 5
expect List.get(SyntheticReflection.all_bindings, 0) == Ok(Uniform({ offset: 0, size: 4 }))
expect List.get(SyntheticReflection.all_bindings, 4) == Ok(ConstantBuffer({ index: 4, count: 18446744073709551615 }))
expect List.len(SyntheticReflection.all_fields) == 16
expect match List.get(SyntheticReflection.all_fields, 0) {
	Ok(Scalar(payload)) => payload.field_name == "scalar" and payload.scalar_type == Uint64
	_ => False
}
expect match List.get(SyntheticReflection.all_fields, 2) {
	Ok(Vector(Semantic(payload))) => payload.semantic_name == "SV_POSITION" and payload.element_count == 3
	_ => False
}
expect match List.get(SyntheticReflection.all_fields, 4) {
	Ok(Matrix(payload)) => payload.row_count == 3 and payload.column_count == 2
	_ => False
}
expect match List.get(SyntheticReflection.all_fields, 7) {
	Ok(Resource(payload)) => payload.field_name == "struct_texture" and match payload.result_type {
		Struct(nested) => nested.type_name == "Nested" and List.len(nested.fields) == 1
		_ => False
	}
	_ => False
}
expect match List.get(SyntheticReflection.all_fields, 10) {
	Ok(Pointer(payload)) => payload.access == Immutable and payload.pointee_size == 4 and payload.pointee_type.type_name == "Nested"
	_ => False
}
expect match List.get(SyntheticReflection.all_fields, 11) {
	Ok(Array(payload)) => payload.element_count == 2 and payload.element_stride == 16 and payload.element_scalar_type == Int32
	_ => False
}
expect match List.get(SyntheticReflection.all_fields, 12) {
	Ok(Enum(payload)) => payload.enum_type.tag_type == Int32 and List.get(payload.enum_type.cases, 0) == Ok({ name: "Minimum", value: -2147483648 })
	_ => False
}
expect match List.get(SyntheticReflection.all_fields, 13) {
	Ok(Enum(payload)) => payload.enum_type.tag_type == Uint32 and List.get(payload.enum_type.cases, 0) == Ok({ name: "Maximum", value: 4294967295 })
	_ => False
}
expect match List.get(SyntheticReflection.all_fields, 14) {
	Ok(DescriptorHandle(payload)) => payload.shape == Sampler2D
	_ => False
}
expect match List.get(SyntheticReflection.graphics.global_parameters, 1) {
	Ok(PushConstant(payload)) => payload.parameter_name == "push" and payload.element_size == 0 and payload.element_type.type_name == "Push" and List.is_empty(payload.element_type.fields)
	_ => False
}
expect match SyntheticReflection.graphics.vertex_entry_point.parameters |> List.get(0) {
	Ok(Struct(payload)) => payload.parameter_name == "vertex" and payload.binding == None and List.is_empty(payload.fields)
	_ => False
}
expect match SyntheticReflection.graphics.vertex_entry_point.parameters |> List.get(1) {
	Ok(Struct(payload)) => payload.parameter_name == "bound_vertex" and payload.binding == Some(VaryingInput({ index: 0, count: 0 })) and List.len(payload.fields) == 16
	_ => False
}
expect SyntheticReflection.graphics.pipeline_layout.bindless_heap_set == Some(0)
expect match SyntheticReflection.graphics.pipeline_layout.descriptor_set_layouts |> List.get(0) {
	Ok(set) => List.len(set.binding_ranges) == 5
	Err(_) => False
}
expect SyntheticReflection.graphics.pipeline_layout.push_constant_ranges == [{ stage_flags: All, offset: 0, size: 128 }]
expect SyntheticReflection.compute.workgroup_size == { x: 0, y: 1, z: 4294967295 }
expect SyntheticReflection.compute.pipeline_layout.bindless_heap_set == None
expect SyntheticReflection.compute.pipeline_layout.descriptor_set_layouts == []
