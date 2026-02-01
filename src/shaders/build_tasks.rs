use std::path::PathBuf;

use askama::Template;
use heck::ToSnakeCase;

use crate::util::relative_path;

use super::ReflectedShader;
use super::json::*;
use super::prepare_reflected_shader;

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

    let mut generated_source_files = vec![];

    // generate top-level rust modules
    if config.generate_rust_source {
        add_top_level_rust_modules(&slang_file_names, &mut generated_source_files);
    }

    let search_path = config.shaders_source_dir.to_str().unwrap();

    // generate per-shader files
    for slang_file_name in &slang_file_names {
        let ReflectedShader {
            vertex_shader,
            fragment_shader,
            reflection_json,
        } = prepare_reflected_shader(slang_file_name, search_path)?;

        if config.generate_rust_source {
            let source_file = build_generated_source_file(&reflection_json);
            generated_source_files.push(source_file);
        }

        let source_file_name = &reflection_json.source_file_name;

        std::fs::create_dir_all(&config.compiled_shaders_dir)?;

        let reflection_json = serde_json::to_string_pretty(&reflection_json)?;
        let reflection_json_file_name = source_file_name.replace(SHADER_FILE_SUFFIX, ".json");
        let json_path = &config.compiled_shaders_dir.join(&reflection_json_file_name);
        std::fs::write(json_path, reflection_json)?;

        let spv_vert_file_name = source_file_name.replace(SHADER_FILE_SUFFIX, ".vert.spv");
        let vert_path = &config.compiled_shaders_dir.join(&spv_vert_file_name);
        std::fs::write(vert_path, vertex_shader.shader_bytecode.as_slice())?;

        let spv_frag_file_name = source_file_name.replace(SHADER_FILE_SUFFIX, ".frag.spv");
        let frag_path = &config.compiled_shaders_dir.join(&spv_frag_file_name);
        std::fs::write(frag_path, fragment_shader.shader_bytecode.as_slice())?;
    }

    for source_file in &generated_source_files {
        write_generated_file(&config, source_file)?;
    }

    Ok(())
}

fn add_top_level_rust_modules(
    slang_file_names: &[String],
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

    let shader_atlas_module = ShaderAtlasModule {
        module_names,
        entries,
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

/// generate the matching rust source for a specific slang shader
fn build_generated_source_file(reflection_json: &ReflectionJson) -> GeneratedFile {
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
        // Generate std140 struct fields with proper padding
        let (param_block_fields, _struct_alignment, expected_size) =
            generate_std140_struct_fields(&parameter_block.element_type.fields, &mut struct_defs);

        // Collect required resources (textures, storage buffers, etc.)
        for field in &parameter_block.element_type.fields {
            if let Some(req) = required_resource(field) {
                required_resources.push(req);
            }
        }

        let type_name = &parameter_block.element_type.type_name;
        struct_defs.push(GeneratedStructDefinition {
            type_name: type_name.to_string(),
            fields: param_block_fields,
            trait_derives: vec!["Debug", "Clone", "Serialize"],
            alignment: Some(Alignment::Std140),
            expected_size: Some(expected_size),
        });

        // the default-added parameter block uniform buffer
        let param_name = parameter_block.parameter_name.to_snake_case();
        let element_type_name = parameter_block.element_type.type_name.clone();
        required_resources.push(RequiredResource {
            field_name: format!("{param_name}_buffer"),
            resource_type: RequiredResourceType::UniformBuffer(element_type_name),
        })
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
            };

            GeneratedStructFieldDefinition::new(r.field_name.clone(), type_name)
        })
        .collect();

    let resources_struct = GeneratedStructDefinition {
        type_name: "Resources<'a>".to_string(),
        fields: resources_fields,
        trait_derives: vec![],
        alignment: None,
        expected_size: None,
    };
    struct_defs.push(resources_struct);

    let shader_name = reflection_json
        .source_file_name
        .replace(SHADER_FILE_SUFFIX, "");
    let file_name = reflection_json
        .source_file_name
        .replace(SHADER_FILE_SUFFIX, ".rs");
    let relative_file_path = relative_path(["generated", "shader_atlas", &file_name]);

    // NOTE these must be in descriptor set layout order in the reflection json
    let mut resources_texture_fields: Vec<String> = vec![];
    let mut resources_uniform_buffer_fields: Vec<String> = vec![];
    let mut resources_storage_buffer_fields: Vec<String> = vec![];
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
        }
    }

    let shader_impl = GeneratedShaderImpl {
        shader_name: shader_name.clone(),
        shader_type_name: "Shader".to_string(),
        vertex_type_name,
        resources_texture_fields,
        resources_uniform_buffer_fields,
        resources_storage_buffer_fields,
    };

    let module_doc_lines = vec![format!(
        "generated from slang shader: {}",
        reflection_json.source_file_name
    )];

    let content = ShaderAtlasEntryModule {
        module_doc_lines,
        struct_defs,
        vertex_impl_blocks,
        shader_impl,
    }
    .render()
    .unwrap();

    GeneratedFile {
        relative_path: relative_file_path,
        content,
    }
}

#[derive(Template)]
#[template(path = "shader_atlas.rs.askama", escape = "none")]
struct ShaderAtlasModule {
    module_names: Vec<String>,
    /// field name and type name prefix
    entries: Vec<(String, String)>,
}

#[derive(Template)]
#[template(path = "shader_atlas_entry.rs.askama", escape = "none")]
struct ShaderAtlasEntryModule {
    module_doc_lines: Vec<String>,
    struct_defs: Vec<GeneratedStructDefinition>,
    vertex_impl_blocks: Vec<VertexImplBlock>,
    shader_impl: GeneratedShaderImpl,
}

struct GeneratedShaderImpl {
    shader_name: String,
    shader_type_name: String,
    vertex_type_name: Option<String>,
    resources_texture_fields: Vec<String>,
    resources_uniform_buffer_fields: Vec<String>,
    resources_storage_buffer_fields: Vec<String>,
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
        let Some(gen_field) = gather_struct_defs(source_field, struct_defs, alignment_for_nested)
        else {
            continue;
        };

        // Get the expected offset from reflection
        let Some((expected_offset, field_size)) = field_offset_size(source_field) else {
            // No offset info (e.g. semantic field), just add the field
            generated_fields.push(gen_field);
            continue;
        };

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
        let Some(gen_field) =
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
                ResourceShape::Texture2D => None,

                ResourceShape::StructuredBuffer => {
                    match &res.result_type {
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
            };

            Some(GeneratedStructFieldDefinition::new(
                scalar.field_name.to_snake_case(),
                field_type.to_string(),
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
            let mut generated_sub_fields = vec![];
            for sub_field in &struct_field.struct_type.fields {
                if let Some(field_def) = gather_struct_defs(sub_field, struct_defs, alignment) {
                    generated_sub_fields.push(field_def);
                };
            }

            // Calculate alignment from the nested struct's own fields, not from parent
            let nested_alignment = alignment.map(|a| match a {
                Alignment::Std140 => Alignment::Std140,
                Alignment::Std430 { .. } => Alignment::Std430 {
                    struct_alignment: std430_struct_alignment(&struct_field.struct_type.fields),
                },
            });

            let sub_struct_def = GeneratedStructDefinition {
                type_name: type_name.clone(),
                fields: generated_sub_fields,
                trait_derives: vec!["Debug", "Clone", "Serialize"],
                alignment: nested_alignment,
                expected_size: None,
            };
            try_add_struct_def(struct_defs, sub_struct_def);

            Some(GeneratedStructFieldDefinition::new(
                struct_field.field_name.to_snake_case(),
                type_name,
            ))
        }

        StructField::Matrix(matrix) => {
            let VectorElementType::Scalar(scalar) = &matrix.element_type;

            let field_type = match (scalar.scalar_type, matrix.row_count, matrix.column_count) {
                (ScalarType::Float32, 4, 4) => "glam::Mat4",
                (ScalarType::Float32, 3, 3) => "glam::Mat3",
                (ScalarType::Float32, 2, 2) => "glam::Mat2",
                (s, r, c) => {
                    panic!("matrix not supported: scalar_type: {s:?}, rows: {r}, cols: {c}")
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
        ResourceResultType::Vector(v) => match &v.element_type {
            VectorElementType::Scalar(s) => {
                let element_type = match s.scalar_type {
                    ScalarType::Float32 => "f32",
                    ScalarType::Uint32 => "u32",
                };

                format!("Vec<{element_type}>")
            }
        },

        ResourceResultType::Struct(s) => s.type_name.clone(),
    }
}

#[derive(Debug)]
struct GeneratedStructDefinition {
    type_name: String,
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
}

#[derive(Debug)]
struct GeneratedStructFieldDefinition {
    field_name: String,
    type_name: String,
}

impl GeneratedStructFieldDefinition {
    fn new(field_name: String, type_name: String) -> Self {
        Self {
            field_name,
            type_name,
        }
    }

    fn padding(index: usize, size: usize) -> Self {
        Self {
            field_name: format!("_padding_{index}"),
            type_name: format!("[u8; {size}]"),
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

struct VertexImplBlock {
    type_name: String,
    attribute_descriptions: Vec<VertexAttributeDescription>,
}

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
        "glam::Vec4" | "glam::Mat4" | "glam::Mat3" | "glam::Mat2" => 16,
        "glam::Vec3" => 16, // vec3 has 16-byte alignment in both std140 and std430
        "glam::Vec2" => 8,
        "f32" | "u32" | "i32" => 4,
        _ => 16, // assume 16 for unknown/struct types
    }
}

/// Returns the std430 alignment for a struct field
fn std430_field_alignment(field: &StructField) -> usize {
    match field {
        StructField::Scalar(s) => match s.scalar_type {
            ScalarType::Float32 | ScalarType::Uint32 => 4,
        },
        StructField::Vector(VectorStructField::Bound(v)) => match v.element_count {
            4 => 16,
            3 => 16, // vec3 has 16-byte alignment in std430
            2 => 8,
            _ => 16,
        },
        StructField::Vector(VectorStructField::Semantic(v)) => match v.element_count {
            4 => 16,
            3 => 16, // vec3 has 16-byte alignment in std430
            2 => 8,
            _ => 16,
        },
        StructField::Matrix(_) => 16, // all matrices are 16-byte aligned
        StructField::Struct(s) => std430_struct_alignment(&s.struct_type.fields),
        StructField::Resource(_) => 0, // resources don't contribute to alignment
    }
}

/// Calculates the std430 alignment for a struct from its fields
fn std430_struct_alignment(fields: &[StructField]) -> usize {
    fields.iter().map(std430_field_alignment).max().unwrap_or(4) // minimum alignment is 4 in std430
}

/// Rounds up to the next multiple of alignment
fn align_to(offset: usize, alignment: usize) -> usize {
    offset.div_ceil(alignment) * alignment
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
        // Verify compatibility by comparing fields
        let fields_match = existing.fields.len() == new_def.fields.len()
            && existing
                .fields
                .iter()
                .zip(&new_def.fields)
                .all(|(a, b)| a.field_name == b.field_name && a.type_name == b.type_name);

        if !fields_match {
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
        // this primarily to test the generated const assertions
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
}
