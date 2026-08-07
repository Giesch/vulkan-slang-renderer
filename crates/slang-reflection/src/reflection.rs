use shader_slang as slang;

use crate::json::*;

mod parameters;
use parameters::*;

mod pipeline_layout;
use pipeline_layout::*;

/// Whether the shader declares any `DescriptorHandle` field, which is what
/// decides whether the bindless texture heap set is appended to this shader's
/// pipeline layout and bound before its draws.
///
/// Always false for now: `reflect_struct_fields` still rejects every handle
/// field outright.
const DECLARES_BINDLESS_HANDLE: bool = false;

pub fn reflection_json(
    source_file_name: &str,
    program_layout: &slang::reflection::Shader,
) -> anyhow::Result<ReflectionJson> {
    let parameters = reflect_entry_points(program_layout)?;

    let pipeline_layout = reflect_pipeline_layout(program_layout, DECLARES_BINDLESS_HANDLE);

    let reflection_json = ReflectionJson {
        source_file_name: source_file_name.to_string(),
        global_parameters: parameters.global_parameters,
        vertex_entry_point: parameters.entry_points.vertex_entry_point,
        fragment_entry_point: parameters.entry_points.fragment_entry_point,
        pipeline_layout,
    };

    Ok(reflection_json)
}

pub fn compute_reflection_json(
    source_file_name: &str,
    program_layout: &slang::reflection::Shader,
) -> anyhow::Result<ComputeReflectionJson> {
    let result = reflect_compute_entry_point(program_layout)?;
    let pipeline_layout = reflect_pipeline_layout(program_layout, DECLARES_BINDLESS_HANDLE);

    Ok(ComputeReflectionJson {
        source_file_name: source_file_name.to_string(),
        global_parameters: result.global_parameters,
        compute_entry_point: result.compute_entry_point,
        workgroup_size: result.workgroup_size,
        pipeline_layout,
    })
}
