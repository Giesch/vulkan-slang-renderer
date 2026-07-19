use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use askama::Template;
use heck::ToSnakeCase;

use crate::util::relative_path;

use super::json::*;
use super::{ReflectedComputeShader, ReflectedShader};
use super::{prepare_reflected_compute_shader, prepare_reflected_shader};

pub struct Config {
    /// whether to write rust code (or only shader spirv & json)
    pub generate_rust_source: bool,
    /// the directory to write the 'generated' module into
    pub rust_source_dir: PathBuf,
    /// the directory to read slang files from
    pub shaders_source_dir: PathBuf,
    /// the directory to write shader spriv & json to
    pub compiled_shaders_dir: PathBuf,
}

const SHADER_FILE_SUFFIX: &str = ".shader.slang";
const COMPUTE_SHADER_FILE_SUFFIX: &str = ".compute.slang";

pub fn write_precompiled_shaders(config: Config) -> anyhow::Result<()> {
    let slang_file_names: Vec<_> = std::fs::read_dir(&config.shaders_source_dir)?
        .filter_map(|entry_res| entry_res.ok())
        .map(|dir_entry| dir_entry.path())
        .filter(|path| {
            let file_name = path.file_name().unwrap().to_str().unwrap();
            file_name.ends_with(SHADER_FILE_SUFFIX)
        })
        .filter_map(|path| {
            path.file_name()
                .and_then(|os_str| os_str.to_str())
                .map(|s| s.to_string())
        })
        .collect();

    let compute_slang_file_names: Vec<_> = std::fs::read_dir(&config.shaders_source_dir)?
        .filter_map(|entry_res| entry_res.ok())
        .map(|dir_entry| dir_entry.path())
        .filter(|path| {
            let file_name = path.file_name().unwrap().to_str().unwrap();
            file_name.ends_with(COMPUTE_SHADER_FILE_SUFFIX)
        })
        .filter_map(|path| {
            path.file_name()
                .and_then(|os_str| os_str.to_str())
                .map(|s| s.to_string())
        })
        .collect();

    // Build type→module map from shared slang modules
    let type_to_module = reflect_slang_module_types(&config.shaders_source_dir);

    let search_path = config.shaders_source_dir.to_str().unwrap();

    // Pass 1: Compile all shaders, write SPIR-V/JSON, collect intermediate build data
    let mut graphics_data: Vec<GraphicsShaderData> = vec![];
    let mut compute_data: Vec<ComputeShaderData> = vec![];

    for slang_file_name in &slang_file_names {
        let ReflectedShader {
            vertex_shader,
            fragment_shader,
            reflection_json,
        } = prepare_reflected_shader(slang_file_name, search_path)?;

        let source_file_name = &reflection_json.source_file_name;
        std::fs::create_dir_all(&config.compiled_shaders_dir)?;

        let reflection_json_str = serde_json::to_string_pretty(&reflection_json)?;
        let reflection_json_file_name = source_file_name.replace(SHADER_FILE_SUFFIX, ".json");
        std::fs::write(
            config.compiled_shaders_dir.join(&reflection_json_file_name),
            reflection_json_str,
        )?;

        let spv_vert_file_name = source_file_name.replace(SHADER_FILE_SUFFIX, ".vert.spv");
        std::fs::write(
            config.compiled_shaders_dir.join(&spv_vert_file_name),
            vertex_shader.shader_bytecode.as_slice(),
        )?;

        let spv_frag_file_name = source_file_name.replace(SHADER_FILE_SUFFIX, ".frag.spv");
        std::fs::write(
            config.compiled_shaders_dir.join(&spv_frag_file_name),
            fragment_shader.shader_bytecode.as_slice(),
        )?;

        if config.generate_rust_source {
            graphics_data.push(collect_graphics_shader_data(
                &reflection_json,
                &type_to_module,
            ));
        }
    }

    for slang_file_name in &compute_slang_file_names {
        let ReflectedComputeShader {
            compute_shader,
            reflection_json,
        } = prepare_reflected_compute_shader(slang_file_name, search_path)?;

        let source_file_name = &reflection_json.source_file_name;
        std::fs::create_dir_all(&config.compiled_shaders_dir)?;

        let reflection_json_str = serde_json::to_string_pretty(&reflection_json)?;
        let reflection_json_file_name =
            source_file_name.replace(COMPUTE_SHADER_FILE_SUFFIX, ".comp.json");
        std::fs::write(
            config.compiled_shaders_dir.join(&reflection_json_file_name),
            reflection_json_str,
        )?;

        let spv_comp_file_name = source_file_name.replace(COMPUTE_SHADER_FILE_SUFFIX, ".comp.spv");
        std::fs::write(
            config.compiled_shaders_dir.join(&spv_comp_file_name),
            compute_shader.shader_bytecode.as_slice(),
        )?;

        if config.generate_rust_source {
            compute_data.push(collect_compute_shader_data(
                &reflection_json,
                &type_to_module,
            ));
        }
    }

    if config.generate_rust_source {
        let mut generated_source_files = vec![];

        // Pass 2: Identify shared modules from all shader struct defs
        let all_shader_defs: Vec<(String, Vec<GeneratedStructDefinition>)> = graphics_data
            .iter()
            .map(|d| (d.shader_name.clone(), d.struct_defs.clone()))
            .chain(
                compute_data
                    .iter()
                    .map(|d| (d.shader_name.clone(), d.struct_defs.clone())),
            )
            .collect();

        let shared_modules = collect_shared_modules(&all_shader_defs);

        // Generate shared module files
        for (module_name, module_defs) in &shared_modules {
            let cross_imports = cross_module_imports(module_name, module_defs, &shared_modules);

            let template = SharedModuleTemplate {
                module_doc_lines: vec![format!(
                    "shared types from slang module: {module_name}.slang"
                )],
                cross_module_imports: cross_imports,
                struct_defs: module_defs.clone(),
            };

            let file_name = format!("{module_name}.rs");
            generated_source_files.push(GeneratedFile {
                relative_path: relative_path(["generated", "shader_atlas", &file_name]),
                content: template.render().unwrap(),
            });
        }

        // Generate per-shader files with shared types filtered out
        for data in &graphics_data {
            let file = render_graphics_shader_file(data, &shared_modules);
            generated_source_files.push(file);
        }

        for data in &compute_data {
            let file = render_compute_shader_file(data, &shared_modules);
            generated_source_files.push(file);
        }

        // Generate top-level module files
        let shared_module_names: Vec<String> = shared_modules.keys().cloned().collect();
        add_top_level_rust_modules(
            &slang_file_names,
            &compute_slang_file_names,
            &shared_module_names,
            &mut generated_source_files,
        );

        for source_file in &generated_source_files {
            write_generated_file(&config, source_file)?;
        }
    }

    Ok(())
}

fn add_top_level_rust_modules(
    slang_file_names: &[String],
    compute_slang_file_names: &[String],
    shared_module_names: &[String],
    generated_source_files: &mut Vec<GeneratedFile>,
) {
    let module_names: Vec<String> = slang_file_names
        .iter()
        .map(|file_name| file_name.replace(SHADER_FILE_SUFFIX, ""))
        .collect();
    let entries: Vec<(String, String)> = module_names
        .iter()
        .map(|module_name| {
            let field_name = module_name.clone();
            let type_prefix = format!("{module_name}::");
            (field_name, type_prefix)
        })
        .collect();

    let compute_module_names: Vec<String> = compute_slang_file_names
        .iter()
        .map(|file_name| file_name.replace(COMPUTE_SHADER_FILE_SUFFIX, "_compute"))
        .collect();
    let compute_entries: Vec<(String, String)> = compute_module_names
        .iter()
        .map(|module_name| {
            let field_name = module_name.clone();
            let type_prefix = format!("{module_name}::");
            (field_name, type_prefix)
        })
        .collect();

    let shader_atlas_module = ShaderAtlasModule {
        shared_module_names: shared_module_names.to_vec(),
        module_names,
        entries,
        compute_module_names,
        compute_entries,
    };

    let shader_atlas_file = GeneratedFile {
        relative_path: relative_path(["generated", "shader_atlas.rs"]),
        content: shader_atlas_module.render().unwrap(),
    };
    generated_source_files.push(shader_atlas_file);

    let top_generated_module = GeneratedFile {
        relative_path: relative_path(["generated.rs"]),
        content: "pub mod shader_atlas;".to_string(),
    };
    generated_source_files.push(top_generated_module);
}

/// Intermediate data collected from a graphics shader before rendering
struct GraphicsShaderData {
    shader_name: String,
    struct_defs: Vec<GeneratedStructDefinition>,
    vertex_impl_blocks: Vec<VertexImplBlock>,
    shader_impl: GeneratedShaderImpl,
    source_file_name: String,
}

/// Collect struct definitions and template data from a graphics shader (without rendering)
fn collect_graphics_shader_data(
    reflection_json: &ReflectionJson,
    type_to_module: &HashMap<String, String>,
) -> GraphicsShaderData {
    let mut struct_defs = vec![];
    let mut vertex_impl_blocks = vec![];

    let has_vertex_struct = reflection_json
        .vertex_entry_point
        .parameters
        .iter()
        .any(|param| matches!(param, EntryPointParameter::Struct(_)));

    let mut required_resources = if has_vertex_struct {
        vec![
            RequiredResource {
                field_name: "vertices".to_string(),
                resource_type: RequiredResourceType::VertexBuffer,
            },
            RequiredResource {
                field_name: "indices".to_string(),
                resource_type: RequiredResourceType::IndexBuffer,
            },
        ]
    } else {
        vec![]
    };

    let mut vertex_type_name = None;
    for vert_param in &reflection_json.vertex_entry_point.parameters {
        match vert_param {
            EntryPointParameter::Scalar(ScalarEntryPointParameter::Semantic(_)) => {}
            EntryPointParameter::Scalar(ScalarEntryPointParameter::Bound(_)) => todo!(),

            EntryPointParameter::Struct(struct_param) => {
                vertex_type_name = Some(struct_param.type_name.to_string());

                let mut generated_fields = vec![];
                for field in &struct_param.fields {
                    if let Some(generated_field) =
                        gather_struct_defs(field, &mut struct_defs, Some(Alignment::Std140))
                    {
                        generated_fields.push(generated_field);
                    };
                }

                let def = GeneratedStructDefinition {
                    type_name: struct_param.type_name.to_string(),
                    source_module: None,
                    fields: generated_fields,
                    trait_derives: vec!["Debug", "Clone", "Serialize"],
                    alignment: Some(Alignment::Std140),
                    expected_size: None,
                };

                let mut attribute_descriptions = vec![];
                for (location, field) in def.fields.iter().enumerate() {
                    let format = match field.type_name.as_str() {
                        "glam::Vec3" => "ash::vk::Format::R32G32B32_SFLOAT",
                        "glam::Vec2" => "ash::vk::Format::R32G32_SFLOAT",
                        "u32" => "ash::vk::Format::R32_UINT",
                        other => todo!("field without vk format in entry point parameter: {other}"),
                    };

                    let attr = VertexAttributeDescription {
                        field_name: field.field_name.to_snake_case(),
                        format: format.to_string(),
                        location,
                    };

                    attribute_descriptions.push(attr);
                }
                let vert_block = VertexImplBlock {
                    type_name: def.type_name.clone(),
                    attribute_descriptions,
                };
                vertex_impl_blocks.push(vert_block);

                struct_defs.push(def);
            }
        }
    }

    for GlobalParameter::ParameterBlock(parameter_block) in &reflection_json.global_parameters {
        let (param_block_fields, _struct_alignment, expected_size) =
            generate_std140_struct_fields(&parameter_block.element_type.fields, &mut struct_defs);

        for field in &parameter_block.element_type.fields {
            if let Some(req) = required_resource(field) {
                required_resources.push(req);
            }
        }

        let has_uniform_fields = !param_block_fields.is_empty();

        let type_name = &parameter_block.element_type.type_name;
        struct_defs.push(GeneratedStructDefinition {
            type_name: type_name.to_string(),
            source_module: None,
            fields: param_block_fields,
            trait_derives: vec!["Debug", "Clone", "Serialize"],
            alignment: Some(Alignment::Std140),
            expected_size: Some(expected_size),
        });

        let param_name = parameter_block.parameter_name.to_snake_case();
        let element_type_name = parameter_block.element_type.type_name.clone();
        if has_uniform_fields {
            required_resources.push(RequiredResource {
                field_name: format!("{param_name}_buffer"),
                resource_type: RequiredResourceType::UniformBuffer(element_type_name),
            })
        }
    }

    struct_defs.reverse();

    let resources_fields = required_resources
        .iter()
        .map(|r| {
            let type_name = match &r.resource_type {
                RequiredResourceType::VertexBuffer => {
                    let vertex_type_name = vertex_type_name
                        .as_ref()
                        .expect("no struct parameter for vertex entry point");
                    format!("Vec<{vertex_type_name}>")
                }
                RequiredResourceType::IndexBuffer => "Vec<u32>".to_string(),
                RequiredResourceType::Texture => "&'a TextureHandle".to_string(),
                RequiredResourceType::UniformBuffer(element_type_name) => {
                    format!("&'a UniformBufferHandle<{element_type_name}>")
                }
                RequiredResourceType::StructuredBuffer(element_type_name) => {
                    format!("&'a StorageBufferHandle<{element_type_name}>")
                }
                RequiredResourceType::StorageTexture2D => "&'a StorageTextureHandle".to_string(),
            };

            GeneratedStructFieldDefinition::new(r.field_name.clone(), type_name)
        })
        .collect();

    let resources_struct = GeneratedStructDefinition {
        type_name: "Resources<'a>".to_string(),
        source_module: None,
        fields: resources_fields,
        trait_derives: vec![],
        alignment: None,
        expected_size: None,
    };
    struct_defs.push(resources_struct);

    let shader_name = reflection_json
        .source_file_name
        .replace(SHADER_FILE_SUFFIX, "");

    // NOTE these must be in descriptor set layout order in the reflection json
    let mut resources_texture_fields: Vec<String> = vec![];
    let mut resources_uniform_buffer_fields: Vec<String> = vec![];
    let mut resources_storage_buffer_fields: Vec<String> = vec![];
    let mut resources_storage_texture_fields: Vec<String> = vec![];
    for res in &required_resources {
        match res.resource_type {
            RequiredResourceType::VertexBuffer => {}
            RequiredResourceType::IndexBuffer => {}
            RequiredResourceType::Texture => {
                resources_texture_fields.push(res.field_name.clone());
            }
            RequiredResourceType::UniformBuffer(_) => {
                resources_uniform_buffer_fields.push(res.field_name.clone());
            }
            RequiredResourceType::StructuredBuffer(_) => {
                resources_storage_buffer_fields.push(res.field_name.clone());
            }
            RequiredResourceType::StorageTexture2D => {
                resources_storage_texture_fields.push(res.field_name.clone());
            }
        }
    }

    let shader_impl = GeneratedShaderImpl {
        shader_name: shader_name.clone(),
        shader_type_name: "Shader".to_string(),
        vertex_type_name,
        resources_texture_fields,
        resources_uniform_buffer_fields,
        resources_storage_buffer_fields,
        resources_storage_texture_fields,
    };

    // Tag struct defs with source module info
    tag_source_modules(&mut struct_defs, type_to_module, &shader_name);

    GraphicsShaderData {
        shader_name,
        struct_defs,
        vertex_impl_blocks,
        shader_impl,
        source_file_name: reflection_json.source_file_name.clone(),
    }
}

/// Render a graphics shader file, filtering out shared types and adding imports
fn render_graphics_shader_file(
    data: &GraphicsShaderData,
    shared_modules: &BTreeMap<String, Vec<GeneratedStructDefinition>>,
) -> GeneratedFile {
    let shared_module_imports = shared_imports_for_shader(&data.struct_defs, shared_modules);

    // Filter out shared types — they're in their own module files
    let local_struct_defs: Vec<GeneratedStructDefinition> = data
        .struct_defs
        .iter()
        .filter(|d| d.source_module.is_none())
        .cloned()
        .collect();

    let module_doc_lines = vec![format!(
        "generated from slang shader: {}",
        data.source_file_name
    )];

    let content = ShaderAtlasEntryModule {
        module_doc_lines,
        shared_module_imports,
        struct_defs: local_struct_defs,
        vertex_impl_blocks: data.vertex_impl_blocks.clone(),
        shader_impl: data.shader_impl.clone(),
    }
    .render()
    .unwrap();

    let file_name = data.source_file_name.replace(SHADER_FILE_SUFFIX, ".rs");
    GeneratedFile {
        relative_path: relative_path(["generated", "shader_atlas", &file_name]),
        content,
    }
}

#[derive(Template)]
#[template(path = "shader_atlas.rs.askama", escape = "none")]
struct ShaderAtlasModule {
    shared_module_names: Vec<String>,
    module_names: Vec<String>,
    /// field name and type name prefix
    entries: Vec<(String, String)>,
    compute_module_names: Vec<String>,
    /// field name and type name prefix for compute shaders
    compute_entries: Vec<(String, String)>,
}

#[derive(Template)]
#[template(path = "shader_atlas_entry.rs.askama", escape = "none")]
struct ShaderAtlasEntryModule {
    module_doc_lines: Vec<String>,
    shared_module_imports: Vec<SharedModuleImport>,
    struct_defs: Vec<GeneratedStructDefinition>,
    vertex_impl_blocks: Vec<VertexImplBlock>,
    shader_impl: GeneratedShaderImpl,
}

#[derive(Template)]
#[template(path = "shader_compute_entry.rs.askama", escape = "none")]
struct ShaderComputeEntryModule {
    module_doc_lines: Vec<String>,
    shared_module_imports: Vec<SharedModuleImport>,
    struct_defs: Vec<GeneratedStructDefinition>,
    shader_impl: GeneratedComputeShaderImpl,
}

#[derive(Clone)]
struct GeneratedComputeShaderImpl {
    shader_name: String,
    shader_type_name: String,
    workgroup_size: [u32; 3],
    resources_texture_fields: Vec<String>,
    resources_uniform_buffer_fields: Vec<String>,
    resources_storage_buffer_fields: Vec<String>,
    resources_storage_texture_fields: Vec<String>,
}

#[derive(Clone)]
struct GeneratedShaderImpl {
    shader_name: String,
    shader_type_name: String,
    vertex_type_name: Option<String>,
    resources_texture_fields: Vec<String>,
    resources_uniform_buffer_fields: Vec<String>,
    resources_storage_buffer_fields: Vec<String>,
    resources_storage_texture_fields: Vec<String>,
}

impl GeneratedShaderImpl {
    fn draw_call(&self) -> &str {
        if self.vertex_type_name.is_some() {
            "DrawIndexed"
        } else {
            "DrawVertexCount"
        }
    }

    fn vertex_type_or_never(&self) -> &str {
        self.vertex_type_name.as_deref().unwrap_or("NoVertex")
    }
}

/// Intermediate data collected from a compute shader before rendering
struct ComputeShaderData {
    shader_name: String,
    struct_defs: Vec<GeneratedStructDefinition>,
    shader_impl: GeneratedComputeShaderImpl,
    source_file_name: String,
}

/// Collect struct definitions and template data from a compute shader (without rendering)
fn collect_compute_shader_data(
    reflection_json: &ComputeReflectionJson,
    type_to_module: &HashMap<String, String>,
) -> ComputeShaderData {
    let mut struct_defs = vec![];
    let mut required_resources = vec![];

    for GlobalParameter::ParameterBlock(parameter_block) in &reflection_json.global_parameters {
        let (param_block_fields, _struct_alignment, expected_size) =
            generate_std140_struct_fields(&parameter_block.element_type.fields, &mut struct_defs);

        for field in &parameter_block.element_type.fields {
            if let Some(req) = required_resource(field) {
                required_resources.push(req);
            }
        }

        let has_uniform_fields = !param_block_fields.is_empty();

        let type_name = &parameter_block.element_type.type_name;
        struct_defs.push(GeneratedStructDefinition {
            type_name: type_name.to_string(),
            source_module: None,
            fields: param_block_fields,
            trait_derives: vec!["Debug", "Clone", "Serialize"],
            alignment: Some(Alignment::Std140),
            expected_size: Some(expected_size),
        });

        let param_name = parameter_block.parameter_name.to_snake_case();
        let element_type_name = parameter_block.element_type.type_name.clone();
        if has_uniform_fields {
            required_resources.push(RequiredResource {
                field_name: format!("{param_name}_buffer"),
                resource_type: RequiredResourceType::UniformBuffer(element_type_name),
            })
        }
    }

    struct_defs.reverse();

    let resources_fields = required_resources
        .iter()
        .map(|r| {
            let type_name = match &r.resource_type {
                RequiredResourceType::VertexBuffer | RequiredResourceType::IndexBuffer => {
                    unreachable!("compute shaders don't have vertex/index buffers")
                }
                RequiredResourceType::Texture => "&'a TextureHandle".to_string(),
                RequiredResourceType::UniformBuffer(element_type_name) => {
                    format!("&'a UniformBufferHandle<{element_type_name}>")
                }
                RequiredResourceType::StructuredBuffer(element_type_name) => {
                    format!("&'a StorageBufferHandle<{element_type_name}>")
                }
                RequiredResourceType::StorageTexture2D => "&'a StorageTextureHandle".to_string(),
            };

            GeneratedStructFieldDefinition::new(r.field_name.clone(), type_name)
        })
        .collect();

    let resources_struct = GeneratedStructDefinition {
        type_name: "Resources<'a>".to_string(),
        source_module: None,
        fields: resources_fields,
        trait_derives: vec![],
        alignment: None,
        expected_size: None,
    };
    struct_defs.push(resources_struct);

    let shader_name = reflection_json
        .source_file_name
        .replace(COMPUTE_SHADER_FILE_SUFFIX, "");

    // NOTE these must be in descriptor set layout order in the reflection json
    let mut resources_texture_fields: Vec<String> = vec![];
    let mut resources_uniform_buffer_fields: Vec<String> = vec![];
    let mut resources_storage_buffer_fields: Vec<String> = vec![];
    let mut resources_storage_texture_fields: Vec<String> = vec![];
    for res in &required_resources {
        match res.resource_type {
            RequiredResourceType::VertexBuffer | RequiredResourceType::IndexBuffer => {}
            RequiredResourceType::Texture => {
                resources_texture_fields.push(res.field_name.clone());
            }
            RequiredResourceType::UniformBuffer(_) => {
                resources_uniform_buffer_fields.push(res.field_name.clone());
            }
            RequiredResourceType::StructuredBuffer(_) => {
                resources_storage_buffer_fields.push(res.field_name.clone());
            }
            RequiredResourceType::StorageTexture2D => {
                resources_storage_texture_fields.push(res.field_name.clone());
            }
        }
    }

    let shader_impl = GeneratedComputeShaderImpl {
        shader_name: shader_name.clone(),
        shader_type_name: "Shader".to_string(),
        workgroup_size: reflection_json.workgroup_size,
        resources_texture_fields,
        resources_uniform_buffer_fields,
        resources_storage_buffer_fields,
        resources_storage_texture_fields,
    };

    // Tag struct defs with source module info
    tag_source_modules(&mut struct_defs, type_to_module, &shader_name);

    ComputeShaderData {
        shader_name,
        struct_defs,
        shader_impl,
        source_file_name: reflection_json.source_file_name.clone(),
    }
}

/// Render a compute shader file, filtering out shared types and adding imports
fn render_compute_shader_file(
    data: &ComputeShaderData,
    shared_modules: &BTreeMap<String, Vec<GeneratedStructDefinition>>,
) -> GeneratedFile {
    let shared_module_imports = shared_imports_for_shader(&data.struct_defs, shared_modules);

    let local_struct_defs: Vec<GeneratedStructDefinition> = data
        .struct_defs
        .iter()
        .filter(|d| d.source_module.is_none())
        .cloned()
        .collect();

    let module_doc_lines = vec![format!(
        "generated from slang compute shader: {}",
        data.source_file_name
    )];

    let content = ShaderComputeEntryModule {
        module_doc_lines,
        shared_module_imports,
        struct_defs: local_struct_defs,
        shader_impl: data.shader_impl.clone(),
    }
    .render()
    .unwrap();

    let module_name = format!("{}_compute", data.shader_name);
    let file_name = format!("{module_name}.rs");
    GeneratedFile {
        relative_path: relative_path(["generated", "shader_atlas", &file_name]),
        content,
    }
}

/// Generates fields for a std430 storage buffer struct, inserting padding as needed.
/// Returns (fields, struct_alignment, expected_size).
fn generate_std430_struct_fields(
    source_fields: &[StructField],
    struct_defs: &mut Vec<GeneratedStructDefinition>,
) -> (Vec<GeneratedStructFieldDefinition>, usize, usize) {
    let mut generated_fields = Vec::new();
    let mut current_offset: usize = 0;
    let mut max_alignment: usize = 4; // minimum alignment
    let mut padding_index: usize = 0;

    for source_field in source_fields {
        // Get the generated field (and recurse for nested structs)
        let alignment_for_nested = Some(Alignment::Std430 {
            struct_alignment: 16,
        });
        let Some(mut gen_field) =
            gather_struct_defs(source_field, struct_defs, alignment_for_nested)
        else {
            continue;
        };

        // Get the expected offset from reflection
        let Some((expected_offset, field_size)) = field_offset_size(source_field) else {
            // No offset info (e.g. semantic field), just add the field
            generated_fields.push(gen_field);
            continue;
        };

        check_rust_placeable(&gen_field, expected_offset);
        gen_field.offset = Some(expected_offset);
        gen_field.size = Some(field_size);

        // Track max alignment for struct alignment calculation
        let field_align = field_alignment(&gen_field.type_name);
        max_alignment = max_alignment.max(field_align);

        // Insert padding if needed
        if expected_offset > current_offset {
            let padding_size = expected_offset - current_offset;
            generated_fields.push(GeneratedStructFieldDefinition::padding(
                padding_index,
                padding_size,
            ));
            padding_index += 1;
        }

        generated_fields.push(gen_field);
        current_offset = expected_offset + field_size;
    }

    // Calculate final struct size (round up to struct alignment for array stride)
    let expected_size = align_to(current_offset, max_alignment);

    // Add trailing padding if needed
    if expected_size > current_offset {
        let padding_size = expected_size - current_offset;
        generated_fields.push(GeneratedStructFieldDefinition::padding(
            padding_index,
            padding_size,
        ));
    }

    (generated_fields, max_alignment, expected_size)
}

/// Generates fields for a std140 uniform buffer struct, inserting padding as needed.
/// Returns (fields, struct_alignment, expected_size).
/// Key difference from std430: nested structs always have 16-byte alignment in std140.
fn generate_std140_struct_fields(
    source_fields: &[StructField],
    struct_defs: &mut Vec<GeneratedStructDefinition>,
) -> (Vec<GeneratedStructFieldDefinition>, usize, usize) {
    let mut generated_fields = Vec::new();
    let mut current_offset: usize = 0;
    let mut padding_index: usize = 0;

    for source_field in source_fields {
        // Skip resources - they don't have offset/size and don't contribute to layout
        if matches!(source_field, StructField::Resource(_)) {
            // Still need to gather struct definitions for StructuredBuffer element types
            let _ = gather_struct_defs(source_field, struct_defs, Some(Alignment::Std140));
            continue;
        }

        // Get the generated field (and recurse for nested structs)
        let Some(mut gen_field) =
            gather_struct_defs(source_field, struct_defs, Some(Alignment::Std140))
        else {
            continue;
        };

        // Get the expected offset from reflection
        let Some((expected_offset, field_size)) = field_offset_size(source_field) else {
            // No offset info (e.g. semantic field), just add the field
            generated_fields.push(gen_field);
            continue;
        };

        check_rust_placeable(&gen_field, expected_offset);
        gen_field.offset = Some(expected_offset);
        gen_field.size = Some(field_size);

        // Insert padding if needed
        if expected_offset > current_offset {
            let padding_size = expected_offset - current_offset;
            generated_fields.push(GeneratedStructFieldDefinition::padding(
                padding_index,
                padding_size,
            ));
            padding_index += 1;
        }

        generated_fields.push(gen_field);
        current_offset = expected_offset + field_size;
    }

    // std140 always uses 16-byte struct alignment
    let struct_alignment = 16;

    // Calculate final struct size (round up to struct alignment)
    let expected_size = align_to(current_offset, struct_alignment);

    // Add trailing padding if needed
    if expected_size > current_offset {
        let padding_size = expected_size - current_offset;
        generated_fields.push(GeneratedStructFieldDefinition::padding(
            padding_index,
            padding_size,
        ));
    }

    (generated_fields, struct_alignment, expected_size)
}

fn gather_struct_defs(
    field: &StructField,
    struct_defs: &mut Vec<GeneratedStructDefinition>,
    alignment: Option<Alignment>,
) -> Option<GeneratedStructFieldDefinition> {
    match field {
        StructField::Resource(res) => {
            match &res.resource_shape {
                ResourceShape::Texture2D | ResourceShape::RWTexture2D => None,

                ResourceShape::StructuredBuffer => {
                    match &res.result_type {
                        ResourceResultType::Scalar(_) => {}

                        ResourceResultType::Vector(vector_result_type) => {
                            match vector_result_type.element_type {
                                VectorElementType::Scalar(_) => {}
                            };
                        }

                        ResourceResultType::Struct(struct_result_type) => {
                            // Storage buffer struct - need to calculate std430 layout
                            let (fields, struct_alignment, expected_size) =
                                generate_std430_struct_fields(
                                    &struct_result_type.fields,
                                    struct_defs,
                                );

                            let alignment = Some(Alignment::Std430 { struct_alignment });

                            try_add_struct_def(
                                struct_defs,
                                GeneratedStructDefinition {
                                    type_name: struct_result_type.type_name.clone(),
                                    source_module: None,
                                    fields,
                                    trait_derives: vec!["Debug", "Clone", "Serialize"],
                                    alignment,
                                    expected_size: Some(expected_size),
                                },
                            );
                        }
                    }

                    // NOTE handled via resoruces;
                    // not a field of the uniform buffer struct
                    None
                }
            }
        }

        StructField::Scalar(scalar) => {
            let field_type = match scalar.scalar_type {
                ScalarType::Float32 => "f32",
                ScalarType::Uint32 => "u32",
                ScalarType::Uint64 => "u64",
            };

            Some(GeneratedStructFieldDefinition::new(
                scalar.field_name.to_snake_case(),
                field_type.to_string(),
            ))
        }

        StructField::Pointer(ptr) => {
            // The pointee struct is emitted like a StructuredBuffer element
            // (std430 per the reflected offsets); the pointer field itself is
            // 8 bytes of uniform data holding a buffer device address, written
            // per-frame via Gpu::device_address — no descriptor, no Resources
            // entry.
            let (fields, struct_alignment, expected_size) =
                generate_std430_struct_fields(&ptr.pointee_type.fields, struct_defs);

            assert_eq!(
                expected_size, ptr.pointee_size,
                "computed std430 size of pointee '{}' disagrees with slang reflection",
                ptr.pointee_type.type_name,
            );

            try_add_struct_def(
                struct_defs,
                GeneratedStructDefinition {
                    type_name: ptr.pointee_type.type_name.clone(),
                    source_module: None,
                    fields,
                    trait_derives: vec!["Debug", "Clone", "Serialize"],
                    alignment: Some(Alignment::Std430 { struct_alignment }),
                    expected_size: Some(expected_size),
                },
            );

            Some(GeneratedStructFieldDefinition::new(
                ptr.field_name.to_snake_case(),
                format!("Addr<{}>", ptr.pointee_type.type_name),
            ))
        }

        StructField::Vector(VectorStructField::Semantic(_)) => None,
        StructField::Vector(VectorStructField::Bound(vector)) => {
            let VectorElementType::Scalar(element_type) = &vector.element_type;
            let field_type = match (element_type.scalar_type, vector.element_count) {
                (ScalarType::Float32, 4) => "glam::Vec4",
                (ScalarType::Float32, 3) => "glam::Vec3",
                (ScalarType::Float32, 2) => "glam::Vec2",
                (t, c) => panic!("vector not supported: type: {t:?}, count: {c}"),
            };

            Some(GeneratedStructFieldDefinition::new(
                vector.field_name.to_snake_case(),
                field_type.to_string(),
            ))
        }

        StructField::Struct(struct_field) => {
            let type_name = struct_field.struct_type.type_name.to_string();

            // Use the same offset-based padding logic as top-level structs
            let (generated_sub_fields, nested_alignment, expected_size) = match alignment {
                Some(Alignment::Std140) => {
                    let (fields, _align, size) = generate_std140_struct_fields(
                        &struct_field.struct_type.fields,
                        struct_defs,
                    );
                    (fields, Some(Alignment::Std140), Some(size))
                }
                Some(Alignment::Std430 { .. }) => {
                    let (fields, align, size) = generate_std430_struct_fields(
                        &struct_field.struct_type.fields,
                        struct_defs,
                    );
                    (
                        fields,
                        Some(Alignment::Std430 {
                            struct_alignment: align,
                        }),
                        Some(size),
                    )
                }
                None => {
                    let mut fields = vec![];
                    for sub_field in &struct_field.struct_type.fields {
                        if let Some(field_def) =
                            gather_struct_defs(sub_field, struct_defs, alignment)
                        {
                            fields.push(field_def);
                        };
                    }
                    (fields, None, None)
                }
            };

            let sub_struct_def = GeneratedStructDefinition {
                type_name: type_name.clone(),
                source_module: None,
                fields: generated_sub_fields,
                trait_derives: vec!["Debug", "Clone", "Serialize"],
                alignment: nested_alignment,
                expected_size,
            };
            try_add_struct_def(struct_defs, sub_struct_def);

            Some(GeneratedStructFieldDefinition::new(
                struct_field.field_name.to_snake_case(),
                type_name,
            ))
        }

        StructField::Matrix(matrix) => {
            let VectorElementType::Scalar(scalar) = &matrix.element_type;

            // Only float4x4 is supported: it is 64 contiguous bytes under every GPU
            // layout rule set, matching glam::Mat4 exactly. Smaller matrices have
            // interior column-stride padding on the GPU (std140 mat3 = 48 bytes vs
            // glam::Mat3's contiguous 36) that a Rust field of a glam type cannot
            // express, producing silently wrong data.
            let field_type = match (scalar.scalar_type, matrix.row_count, matrix.column_count) {
                (ScalarType::Float32, 4, 4) => "glam::Mat4",
                (s, r, c) => {
                    panic!(
                        "matrix field '{}' not supported in parameter blocks: \
                        scalar_type: {s:?}, rows: {r}, cols: {c}; \
                        use float4x4, or padded float4 rows",
                        matrix.field_name,
                    )
                }
            };

            Some(GeneratedStructFieldDefinition::new(
                matrix.field_name.to_snake_case(),
                field_type.to_string(),
            ))
        }
    }
}

fn required_resource(field: &StructField) -> Option<RequiredResource> {
    match field {
        StructField::Resource(res) => match &res.resource_shape {
            ResourceShape::Texture2D => Some(RequiredResource {
                field_name: res.field_name.to_snake_case(),
                resource_type: RequiredResourceType::Texture,
            }),

            ResourceShape::RWTexture2D => Some(RequiredResource {
                field_name: res.field_name.to_snake_case(),
                resource_type: RequiredResourceType::StorageTexture2D,
            }),

            ResourceShape::StructuredBuffer => Some(RequiredResource {
                field_name: res.field_name.to_snake_case(),
                resource_type: RequiredResourceType::StructuredBuffer(resource_type_name(
                    &res.result_type,
                )),
            }),
        },

        _ => None,
    }
}

fn resource_type_name(result_type: &ResourceResultType) -> String {
    match result_type {
        ResourceResultType::Scalar(s) => match s.scalar_type {
            ScalarType::Float32 => "f32".to_string(),
            ScalarType::Uint32 => "u32".to_string(),
            ScalarType::Uint64 => "u64".to_string(),
        },

        ResourceResultType::Vector(v) => match &v.element_type {
            VectorElementType::Scalar(s) => {
                let element_type = match s.scalar_type {
                    ScalarType::Float32 => "f32",
                    ScalarType::Uint32 => "u32",
                    ScalarType::Uint64 => "u64",
                };

                format!("Vec<{element_type}>")
            }
        },

        ResourceResultType::Struct(s) => s.type_name.clone(),
    }
}

#[derive(Debug, Clone)]
struct GeneratedStructDefinition {
    type_name: String,
    /// Which slang module this type originated from (None = local to the shader)
    source_module: Option<String>,
    fields: Vec<GeneratedStructFieldDefinition>,
    trait_derives: Vec<&'static str>,
    alignment: Option<Alignment>, // None = CPU only
    expected_size: Option<usize>, // For compile-time size assertion
}

impl GeneratedStructDefinition {
    fn trait_derive_line(&self) -> Option<String> {
        if self.trait_derives.is_empty() {
            return None;
        }

        let trait_list = self.trait_derives.join(", ");

        Some(format!("#[derive({trait_list})]"))
    }

    fn gpu_write(&self) -> bool {
        self.alignment.is_some()
    }

    fn repr(&self) -> Option<String> {
        self.alignment.as_ref().map(Alignment::annotation)
    }

    fn expected_size(&self) -> Option<usize> {
        self.expected_size
    }

    /// Per-field layout assertion lines for the generated source.
    /// Offsets check field placement; sizes check field extent (interior
    /// stride padding always changes a type's total size, which offset
    /// asserts alone cannot see). Layout bugs reached through raw device
    /// addresses produce no validation errors, so mismatches must fail
    /// at cargo check instead.
    fn layout_assert_lines(&self) -> Vec<String> {
        let mut lines = vec![];

        for field in &self.fields {
            let Some(offset) = field.offset else {
                continue;
            };

            lines.push(format!(
                "const _: () = assert!(std::mem::offset_of!({}, {}) == {offset});",
                self.type_name, field.field_name,
            ));

            if let Some(size) = field.size {
                lines.push(format!(
                    "const _: () = assert!(std::mem::size_of::<{}>() == {size});",
                    field.type_name,
                ));
            }
        }

        lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedStructFieldDefinition {
    field_name: String,
    type_name: String,
    /// reflected offset within the GPU struct; None for padding fields
    /// and fields outside GPU layout (vertex inputs, CPU-only structs)
    offset: Option<usize>,
    /// reflected size within the GPU struct; None when offset is None
    size: Option<usize>,
}

impl GeneratedStructFieldDefinition {
    fn new(field_name: String, type_name: String) -> Self {
        Self {
            field_name,
            type_name,
            offset: None,
            size: None,
        }
    }

    fn padding(index: usize, size: usize) -> Self {
        Self {
            field_name: format!("_padding_{index}"),
            type_name: format!("[u8; {size}]"),
            offset: None,
            size: None,
        }
    }
}

struct GeneratedFile {
    /// the path relative to the rust 'src' dir
    relative_path: PathBuf,
    content: String,
}

fn write_generated_file(config: &Config, source_file: &GeneratedFile) -> anyhow::Result<()> {
    let absolute_path = config.rust_source_dir.join(&source_file.relative_path);

    std::fs::create_dir_all(absolute_path.parent().unwrap())?;
    std::fs::write(&absolute_path, &source_file.content)?;

    Ok(())
}

#[derive(Clone)]
struct VertexImplBlock {
    type_name: String,
    attribute_descriptions: Vec<VertexAttributeDescription>,
}

#[derive(Clone)]
struct VertexAttributeDescription {
    field_name: String,
    format: String,
    location: usize,
}

struct RequiredResource {
    field_name: String,
    resource_type: RequiredResourceType,
}

enum RequiredResourceType {
    VertexBuffer,
    IndexBuffer,
    Texture,
    StorageTexture2D,
    UniformBuffer(String),
    StructuredBuffer(String),
}

/// Extracts offset and size from a StructField's binding
fn field_offset_size(field: &StructField) -> Option<(usize, usize)> {
    let binding = match field {
        StructField::Scalar(s) => Some(&s.binding),
        StructField::Vector(VectorStructField::Bound(v)) => Some(&v.binding),
        StructField::Vector(VectorStructField::Semantic(_)) => None,
        StructField::Matrix(m) => Some(&m.binding),
        StructField::Struct(s) => Some(&s.binding),
        StructField::Pointer(p) => Some(&p.binding),
        StructField::Resource(_) => None,
    };

    binding.and_then(|b| match b {
        Binding::Uniform(u) => Some((u.offset, u.size)),
        _ => None,
    })
}

/// Returns the alignment for a given Rust type name.
/// These rules are the same for both std140 and std430 for basic types.
fn field_alignment(type_name: &str) -> usize {
    match type_name {
        "glam::Vec4" | "glam::Mat4" => 16,
        "glam::Vec3" => 16, // vec3 has 16-byte alignment in both std140 and std430
        "glam::Vec2" | "u64" => 8,
        "f32" | "u32" | "i32" => 4,
        s if s.starts_with("Addr<") => 8, // repr(transparent) over u64
        _ => 16,                          // assume 16 for unknown/struct types
    }
}

/// Rounds up to the next multiple of alignment
fn align_to(offset: usize, alignment: usize) -> usize {
    offset.div_ceil(alignment) * alignment
}

/// The actual Rust alignment of an emitted leaf type, or None for generated
/// struct types (whose #[repr(C, align(N))] matches their GPU alignment by
/// construction, so their reflected offsets are always placeable).
fn rust_type_alignment(type_name: &str) -> Option<usize> {
    Some(match type_name {
        "f32" => std::mem::align_of::<f32>(),
        "u32" => std::mem::align_of::<u32>(),
        "u64" => std::mem::align_of::<u64>(),
        "glam::Vec2" => std::mem::align_of::<glam::Vec2>(),
        "glam::Vec3" => std::mem::align_of::<glam::Vec3>(),
        "glam::Vec4" => std::mem::align_of::<glam::Vec4>(),
        "glam::Mat4" => std::mem::align_of::<glam::Mat4>(),
        // Addr<T> is repr(transparent) over u64 for every T
        s if s.starts_with("Addr<") => std::mem::align_of::<u64>(),
        _ => return None,
    })
}

/// A reflected offset that isn't a multiple of the emitted Rust type's alignment
/// cannot be reproduced with a #[repr(C)] field of that type — unreachable under
/// std140/std430, so it means a non-std GPU layout leaked into codegen.
fn check_rust_placeable(gen_field: &GeneratedStructFieldDefinition, expected_offset: usize) {
    if let Some(align) = rust_type_alignment(&gen_field.type_name)
        && !expected_offset.is_multiple_of(align)
    {
        panic!(
            "field '{}' has reflected offset {expected_offset}, which is not a multiple of \
            {}'s Rust alignment ({align}); non-std GPU layout detected",
            gen_field.field_name, gen_field.type_name,
        );
    }
}

/// Two generated definitions of the same type must agree exactly — field names,
/// Rust types, and reflected offsets/sizes. A same-size mismatch here means two
/// shaders see the same struct with different GPU layouts.
fn struct_defs_compatible(a: &GeneratedStructDefinition, b: &GeneratedStructDefinition) -> bool {
    a.fields == b.fields
}

/// Adds a struct definition if it doesn't already exist.
/// Panics if a struct with the same name exists but has incompatible fields.
fn try_add_struct_def(
    struct_defs: &mut Vec<GeneratedStructDefinition>,
    new_def: GeneratedStructDefinition,
) {
    if let Some(existing) = struct_defs
        .iter()
        .find(|d| d.type_name == new_def.type_name)
    {
        if !struct_defs_compatible(existing, &new_def) {
            panic!(
                "Incompatible struct definitions for '{}': fields differ",
                new_def.type_name
            );
        }
        // Already exists with matching fields, skip
    } else {
        struct_defs.push(new_def);
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Alignment {
    /// #[repr(C, align(16))] - used for uniform buffers
    Std140,
    /// #[repr(C, align(N))] - used for storage buffers with calculated alignment
    Std430 { struct_alignment: usize },
}

impl Alignment {
    fn annotation(&self) -> String {
        match self {
            Self::Std140 => "#[repr(C, align(16))]".to_string(),
            Self::Std430 { struct_alignment } => {
                format!("#[repr(C, align({struct_alignment}))]")
            }
        }
    }
}

/// Reflects all `.slang` files in the source directory that are NOT shader files
/// (i.e., shared/utility modules), extracting `struct` declarations via the Slang reflection API.
/// Returns a map of `type_name → module_name`.
fn reflect_slang_module_types(shaders_source_dir: &Path) -> HashMap<String, String> {
    let mut module_names = Vec::new();

    for entry in std::fs::read_dir(shaders_source_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let file_name = path.file_name().unwrap().to_str().unwrap();

        if file_name.ends_with(SHADER_FILE_SUFFIX)
            || file_name.ends_with(COMPUTE_SHADER_FILE_SUFFIX)
        {
            continue;
        }
        if !file_name.ends_with(".slang") {
            continue;
        }

        let module_name = file_name.strip_suffix(".slang").unwrap().to_string();
        module_names.push(module_name);
    }

    let search_path = shaders_source_dir.to_str().unwrap();
    let module_name_refs: Vec<&str> = module_names.iter().map(|s| s.as_str()).collect();
    super::reflect_shared_module_types(&module_name_refs, search_path)
        .unwrap_or_else(|e| panic!("failed to reflect shared modules: {e}"))
}

/// Tag struct definitions with their source module based on the type→module map.
/// Recursively tags nested structs.
fn tag_source_modules(
    struct_defs: &mut [GeneratedStructDefinition],
    type_to_module: &HashMap<String, String>,
    current_shader_module: &str,
) {
    for def in struct_defs.iter_mut() {
        if let Some(module) = type_to_module.get(&def.type_name)
            && module != current_shader_module
        {
            def.source_module = Some(module.clone());
        }
    }
}

struct SharedModuleImport {
    module_name: String,
    type_names: Vec<String>,
}

/// Collect shared type definitions from all shaders into per-module groups.
/// Returns (module_name → definitions) for types that appear in more than one shader.
fn collect_shared_modules(
    all_shader_defs: &[(String, Vec<GeneratedStructDefinition>)],
) -> BTreeMap<String, Vec<GeneratedStructDefinition>> {
    let mut modules: BTreeMap<String, Vec<GeneratedStructDefinition>> = BTreeMap::new();

    for (shader_name, defs) in all_shader_defs {
        for def in defs {
            if let Some(ref module_name) = def.source_module {
                let module_defs = modules.entry(module_name.clone()).or_default();
                match module_defs.iter().find(|d| d.type_name == def.type_name) {
                    Some(existing) => {
                        // a shared type must have the same layout in every shader
                        // that uses it; first-definition-wins would silently drop
                        // one of two diverging layouts
                        if !struct_defs_compatible(existing, def) {
                            panic!(
                                "shared type '{}' (module '{module_name}') has an \
                                incompatible layout in shader '{shader_name}'",
                                def.type_name,
                            );
                        }
                    }
                    None => module_defs.push(def.clone()),
                }
            }
        }
    }

    modules
}

/// Determine which shared modules a shader needs to import, and which type names.
/// Only imports types directly referenced by local (non-shared) struct fields.
fn shared_imports_for_shader(
    struct_defs: &[GeneratedStructDefinition],
    shared_modules: &BTreeMap<String, Vec<GeneratedStructDefinition>>,
) -> Vec<SharedModuleImport> {
    let mut imports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    // Check which shared types are directly referenced by local struct fields
    for def in struct_defs {
        if def.source_module.is_some() {
            continue; // skip shared types themselves
        }
        for field in &def.fields {
            for (module_name, module_defs) in shared_modules {
                for module_def in module_defs {
                    // Check exact match or contained within generic type (e.g., StorageBufferHandle<Cube>)
                    if field.type_name == module_def.type_name
                        || field.type_name.contains(&module_def.type_name)
                    {
                        imports
                            .entry(module_name.clone())
                            .or_default()
                            .insert(module_def.type_name.clone());
                    }
                }
            }
        }
    }

    imports
        .into_iter()
        .map(|(module_name, type_names)| SharedModuleImport {
            module_name,
            type_names: type_names.into_iter().collect(),
        })
        .collect()
}

/// Determine cross-module imports for a shared module.
/// For example, ray_march_camera.rs needs to import Projection from projection.rs.
fn cross_module_imports(
    module_name: &str,
    module_defs: &[GeneratedStructDefinition],
    all_shared_modules: &BTreeMap<String, Vec<GeneratedStructDefinition>>,
) -> Vec<SharedModuleImport> {
    let mut imports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for def in module_defs {
        for field in &def.fields {
            for (other_module, other_defs) in all_shared_modules {
                if other_module == module_name {
                    continue;
                }
                for other_def in other_defs {
                    if field.type_name == other_def.type_name {
                        imports
                            .entry(other_module.clone())
                            .or_default()
                            .insert(other_def.type_name.clone());
                    }
                }
            }
        }
    }

    imports
        .into_iter()
        .map(|(module_name, type_names)| SharedModuleImport {
            module_name,
            type_names: type_names.into_iter().collect(),
        })
        .collect()
}

#[derive(Template)]
#[template(path = "shader_shared_module.rs.askama", escape = "none")]
struct SharedModuleTemplate {
    module_doc_lines: Vec<String>,
    cross_module_imports: Vec<SharedModuleImport>,
    struct_defs: Vec<GeneratedStructDefinition>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::util::manifest_path;

    // the tmp_path.strip_prefix() is broken for windows' '\\?\' extended paths
    #[cfg(not(windows))]
    #[test]
    fn generated_files() {
        let tmp_prefix = format!("shader-test-{}", uuid::Uuid::new_v4());
        let tmp_dir_path = std::env::temp_dir().join(tmp_prefix);

        let config = Config {
            generate_rust_source: true,
            rust_source_dir: tmp_dir_path.join("src"),
            shaders_source_dir: manifest_path(["shaders", "source"]),
            compiled_shaders_dir: tmp_dir_path.join(relative_path(["shaders", "compiled"])),
        };

        write_precompiled_shaders(config).unwrap();

        insta::glob!(&tmp_dir_path, "**/*.{rs,json}", |tmp_path| {
            let relative_path = tmp_path.strip_prefix(&tmp_dir_path).unwrap();

            let info = serde_json::json!({
                "relative_path": &relative_path
            });

            let content = std::fs::read_to_string(tmp_path).unwrap();

            insta::with_settings!({ info => &info, omit_expression => true }, {
                insta::assert_snapshot!(content);
            });
        });
    }

    // Tests for std140 and std430 alignment edge cases
    #[cfg(not(windows))]
    #[test]
    fn alignment_tests() {
        let tmp_prefix = format!("shader-test-{}", uuid::Uuid::new_v4());
        let tmp_dir_path = std::env::temp_dir().join(tmp_prefix);

        let config = Config {
            generate_rust_source: true,
            rust_source_dir: tmp_dir_path.join("src"),
            shaders_source_dir: manifest_path(["shaders", "test"]),
            compiled_shaders_dir: tmp_dir_path.join(relative_path(["shaders", "compiled"])),
        };

        write_precompiled_shaders(config).unwrap();

        // Run cargo check on the generated code to verify it compiles
        // this is primarily to test the generated const layout assertions
        {
            use std::fmt::Write;

            let check_crate = manifest_path(["shaders", "test", "check_crate"]);
            let check_crate_src = check_crate.join("src/generated");
            let check_crate_shaders = check_crate.join("shaders/compiled");

            std::fs::create_dir_all(&check_crate_src).unwrap();
            std::fs::create_dir_all(&check_crate_shaders).unwrap();

            // Copy .rs files and build mod.rs
            let mut mod_contents = String::new();
            let shader_atlas_dir = tmp_dir_path.join("src/generated/shader_atlas");
            for entry in std::fs::read_dir(&shader_atlas_dir).unwrap() {
                let entry = entry.unwrap();
                if entry.path().extension() == Some(std::ffi::OsStr::new("rs")) {
                    let filename = entry.file_name();
                    std::fs::copy(entry.path(), check_crate_src.join(&filename)).unwrap();
                    let mod_name = filename.to_str().unwrap().strip_suffix(".rs").unwrap();
                    writeln!(mod_contents, "pub mod {};", mod_name).unwrap();
                }
            }
            std::fs::write(check_crate_src.join("mod.rs"), mod_contents).unwrap();

            // Copy compiled shader files (.json and .spv)
            let compiled_dir = tmp_dir_path.join("shaders/compiled");
            for entry in std::fs::read_dir(&compiled_dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json" || e == "spv") {
                    std::fs::copy(&path, check_crate_shaders.join(entry.file_name())).unwrap();
                }
            }

            // Run cargo check
            let output = std::process::Command::new("cargo")
                .args(["check"])
                .current_dir(&check_crate)
                .output()
                .expect("failed to run cargo check");

            // Cleanup before asserting (so we don't leave files on failure)
            std::fs::remove_dir_all(&check_crate_src).unwrap();
            std::fs::remove_dir_all(&check_crate_shaders).unwrap();

            assert!(
                output.status.success(),
                "generated code failed to compile:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        insta::glob!(&tmp_dir_path, "**/*.{rs,json}", |tmp_path| {
            let relative_path = tmp_path.strip_prefix(&tmp_dir_path).unwrap();

            let info = serde_json::json!({
                "relative_path": &relative_path
            });

            let content = std::fs::read_to_string(tmp_path).unwrap();

            insta::with_settings!({ info => &info, omit_expression => true }, {
                insta::assert_snapshot!(content);
            });
        });
    }

    // glam::Mat3/Mat2 have no interior column-stride padding, so they can never
    // match the GPU layout of a float3x3/float2x2 in a parameter block; the codegen
    // must reject them rather than emit silently wrong data.
    #[test]
    #[should_panic(expected = "matrix field 'bad' not supported in parameter blocks")]
    fn small_matrix_fields_are_rejected() {
        let field = StructField::Matrix(MatrixStructField {
            field_name: "bad".to_string(),
            binding: Binding::Uniform(OffsetSizeBinding {
                offset: 0,
                size: 48,
            }),
            row_count: 3,
            column_count: 3,
            element_type: VectorElementType::Scalar(ScalarVectorElementType {
                scalar_type: ScalarType::Float32,
            }),
        });

        gather_struct_defs(&field, &mut Vec::new(), None);
    }

    /// Returns the size of the Rust type the codegen emits for a given type name,
    /// or None for generated struct types (whose interiors are checked field-by-field).
    fn rust_size_of(rust_type_name: &str) -> Option<usize> {
        let size = match rust_type_name {
            "f32" => std::mem::size_of::<f32>(),
            "u32" => std::mem::size_of::<u32>(),
            "u64" => std::mem::size_of::<u64>(),
            "glam::Vec2" => std::mem::size_of::<glam::Vec2>(),
            "glam::Vec3" => std::mem::size_of::<glam::Vec3>(),
            "glam::Vec4" => std::mem::size_of::<glam::Vec4>(),
            "glam::Mat2" => std::mem::size_of::<glam::Mat2>(),
            "glam::Mat3" => std::mem::size_of::<glam::Mat3>(),
            "glam::Mat4" => std::mem::size_of::<glam::Mat4>(),
            _ => return None,
        };

        Some(size)
    }

    fn check_field_sizes(fields: &[StructField], context: &str, mismatches: &mut Vec<String>) {
        for field in fields {
            match field {
                StructField::Struct(s) => {
                    let context = format!("{context}.{}", s.field_name);
                    check_field_sizes(&s.struct_type.fields, &context, mismatches);
                    continue;
                }

                StructField::Resource(res) => {
                    if let ResourceResultType::Struct(s) = &res.result_type {
                        let context = format!("{context}.{}", res.field_name);
                        check_field_sizes(&s.fields, &context, mismatches);
                    }
                    continue;
                }

                StructField::Pointer(ptr) => {
                    // check the pointee's fields, then fall through to the
                    // leaf check for the pointer's own u64
                    let pointee_context = format!("{context}.{}", ptr.field_name);
                    check_field_sizes(&ptr.pointee_type.fields, &pointee_context, mismatches);
                }

                StructField::Scalar(_) | StructField::Vector(_) | StructField::Matrix(_) => {}
            }

            let Some((_offset, reflected_size)) = field_offset_size(field) else {
                continue;
            };
            let Some(generated) = gather_struct_defs(field, &mut Vec::new(), None) else {
                continue;
            };
            let Some(rust_size) = rust_size_of(&generated.type_name) else {
                continue;
            };

            if rust_size != reflected_size {
                mismatches.push(format!(
                    "{context}.{}: {} is {rust_size} bytes in Rust, but the reflected GPU size is {reflected_size}",
                    generated.field_name, generated.type_name,
                ));
            }
        }
    }

    /// Asserts that every uniform-bound field's emitted Rust type has exactly the
    /// reflected GPU size. This catches interior-stride mismatches (e.g. glam::Mat3 =
    /// 36 contiguous bytes vs std430's 48 with 16-byte column stride) that total-size
    /// and per-field offset asserts cannot see, because stride padding always changes
    /// a field's total size.
    #[cfg(not(windows))]
    #[test]
    fn field_size_tripwire() {
        let tmp_prefix = format!("shader-test-{}", uuid::Uuid::new_v4());
        let tmp_dir_path = std::env::temp_dir().join(tmp_prefix);

        let config = Config {
            generate_rust_source: false,
            rust_source_dir: tmp_dir_path.join("src"),
            shaders_source_dir: manifest_path(["shaders", "test"]),
            compiled_shaders_dir: tmp_dir_path.join(relative_path(["shaders", "compiled"])),
        };
        let compiled_dir = config.compiled_shaders_dir.clone();

        write_precompiled_shaders(config).unwrap();

        let mut mismatches = Vec::new();
        for entry in std::fs::read_dir(&compiled_dir).unwrap() {
            let entry = entry.unwrap();
            let file_name = entry.file_name().to_string_lossy().to_string();

            let json_str;
            let global_parameters = if file_name.ends_with(".comp.json") {
                json_str = std::fs::read_to_string(entry.path()).unwrap();
                let json: ComputeReflectionJson = serde_json::from_str(&json_str).unwrap();
                json.global_parameters
            } else if file_name.ends_with(".json") {
                json_str = std::fs::read_to_string(entry.path()).unwrap();
                let json: ReflectionJson = serde_json::from_str(&json_str).unwrap();
                json.global_parameters
            } else {
                continue;
            };

            for global_parameter in &global_parameters {
                let GlobalParameter::ParameterBlock(block) = global_parameter;
                let context = format!("{file_name}: {}", block.parameter_name);
                check_field_sizes(&block.element_type.fields, &context, &mut mismatches);
            }
        }

        assert!(
            mismatches.is_empty(),
            "field size mismatches (Rust type byte image != reflected GPU size):\n{}",
            mismatches.join("\n"),
        );
    }

    /// Pins the SPIR-V layout of Std430DataLayout pointer pointees. The generated
    /// Rust structs assert the *reflected* offsets; this test asserts the *emitted*
    /// offsets match them, closing the loop reflection alone cannot close (the
    /// pointer's own element_type_layout() misreports layout-annotated pointees).
    /// This is the regression guard for slang upgrades changing pointer layout.
    #[cfg(not(windows))]
    #[test]
    fn pointer_pointee_spirv_layout() {
        use rspirv::dr::Operand;
        use rspirv::spirv::{Decoration, Op, StorageClass};

        let search_path = manifest_path(["shaders", "test"]);
        let reflected = prepare_reflected_shader(
            "pointer_pointee_layout.shader.slang",
            search_path.to_str().unwrap(),
        )
        .unwrap();

        let module = rspirv::dr::load_bytes(&reflected.vertex_shader.shader_bytecode)
            .expect("failed to parse SPIR-V");

        // the PhysicalStorageBuffer pointer type identifies the pointee struct
        let (ptr_type_id, pointee_struct_id) = module
            .types_global_values
            .iter()
            .find_map(|inst| {
                if inst.class.opcode != Op::TypePointer {
                    return None;
                }
                match inst.operands.as_slice() {
                    [
                        Operand::StorageClass(StorageClass::PhysicalStorageBuffer),
                        Operand::IdRef(pointee),
                    ] => Some((inst.result_id.unwrap(), *pointee)),
                    _ => None,
                }
            })
            .expect("no PhysicalStorageBuffer pointer type in SPIR-V");

        let member_offsets = |struct_id: u32| -> Vec<(u32, u32)> {
            let mut offsets: Vec<(u32, u32)> = module
                .annotations
                .iter()
                .filter(|inst| inst.class.opcode == Op::MemberDecorate)
                .filter_map(|inst| match inst.operands.as_slice() {
                    [
                        Operand::IdRef(target),
                        Operand::LiteralBit32(member),
                        Operand::Decoration(Decoration::Offset),
                        Operand::LiteralBit32(offset),
                    ] if *target == struct_id => Some((*member, *offset)),
                    _ => None,
                })
                .collect();
            offsets.sort_unstable();
            offsets
        };

        // HostileData under std430 (see pointer_pointee_layout.shader.slang)
        assert_eq!(
            member_offsets(pointee_struct_id),
            vec![
                (0, 0),
                (1, 12),
                (2, 16),
                (3, 32),
                (4, 48),
                (5, 80),
                (6, 88),
                (7, 104)
            ],
        );

        // pointer indexing stride == std430 struct size
        let array_stride = module
            .annotations
            .iter()
            .filter(|inst| inst.class.opcode == Op::Decorate)
            .find_map(|inst| match inst.operands.as_slice() {
                [
                    Operand::IdRef(target),
                    Operand::Decoration(Decoration::ArrayStride),
                    Operand::LiteralBit32(stride),
                ] if *target == ptr_type_id => Some(*stride),
                _ => None,
            })
            .expect("no ArrayStride on the PhysicalStorageBuffer pointer type");
        assert_eq!(array_stride, 112);

        // nested pointee structs: InnerA (member 4), InnerB (member 6)
        let struct_def = module
            .types_global_values
            .iter()
            .find(|inst| {
                inst.class.opcode == Op::TypeStruct && inst.result_id == Some(pointee_struct_id)
            })
            .expect("pointee struct type not found");
        let member_type = |index: usize| match struct_def.operands[index] {
            Operand::IdRef(id) => id,
            _ => unreachable!("struct member operands are type ids"),
        };
        // natural layout would put InnerA.v at 4
        assert_eq!(member_offsets(member_type(4)), vec![(0, 0), (1, 16)]);
        assert_eq!(member_offsets(member_type(6)), vec![(0, 0), (1, 8)]);
    }

    // A bare `T*` pointee uses slang's natural layout, which codegen would
    // silently mis-generate as std430; the reflection hard error is the only
    // guard, so it gets its own pin. The fixture lives in a temp dir because
    // every shader in shaders/test must compile.
    #[cfg(not(windows))]
    #[test]
    fn default_layout_pointer_is_rejected() {
        let tmp_dir = std::env::temp_dir().join(format!("shader-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let source = r#"#language slang 2026

module default_layout_pointer;

struct Item {
    float4 value;
}

struct Params {
    Item* items;
}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return params.items[id].value;
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
"#;
        std::fs::write(tmp_dir.join("default_layout_pointer.shader.slang"), source).unwrap();

        let result = prepare_reflected_shader(
            "default_layout_pointer.shader.slang",
            tmp_dir.to_str().unwrap(),
        );

        std::fs::remove_dir_all(&tmp_dir).ok();

        let err = match result {
            Ok(_) => panic!("a default-layout pointer field must be rejected"),
            Err(err) => err,
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("Std430DataLayout") && message.contains("Addr<"),
            "unexpected error message: {message}"
        );
    }

    fn count_branch_instructions(spv_bytes: &[u8]) -> usize {
        let module = rspirv::dr::load_bytes(spv_bytes).expect("Failed to parse SPIR-V module");
        module
            .all_inst_iter()
            .filter(|inst| {
                matches!(
                    inst.class.opcode,
                    rspirv::spirv::Op::BranchConditional | rspirv::spirv::Op::Switch
                )
            })
            .count()
    }

    #[cfg(not(windows))]
    #[test]
    fn shader_branching_snapshots() {
        let compiled_dir = manifest_path(["shaders", "compiled"]);
        let mut entries: Vec<_> = std::fs::read_dir(&compiled_dir)
            .expect("Failed to read compiled shaders directory")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "spv"))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        let mut summary = String::new();
        for entry in &entries {
            let bytes = std::fs::read(entry.path()).expect("Failed to read .spv file");
            let count = count_branch_instructions(&bytes);
            let name = entry.file_name();
            summary.push_str(&format!("{}: {}\n", name.to_string_lossy(), count));
        }

        insta::assert_snapshot!(summary);
    }
}
