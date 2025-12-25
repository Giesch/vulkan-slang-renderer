use std::path::PathBuf;

use askama::Template;
use heck::ToSnakeCase;

use crate::util::relative_path;

use super::{ReflectedShader, json::*, prepare_reflected_shader};

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
            // path.extension().is_some_and(|ext| ext == "slang")
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

    // generate per-shader files
    for slang_file_name in &slang_file_names {
        let ReflectedShader {
            vertex_shader,
            fragment_shader,
            reflection_json,
        } = prepare_reflected_shader(slang_file_name)?;

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
        vec![RequiredResource {
            field_name: "vertex_count".to_string(),
            resource_type: RequiredResourceType::VertexCount,
        }]
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
        let mut param_block_fields = vec![];
        for field in &parameter_block.element_type.fields {
            if let Some(generated_field) =
                gather_struct_defs(field, &mut struct_defs, Some(Alignment::Std140))
            {
                param_block_fields.push(generated_field);
            };

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
                RequiredResourceType::VertexCount => "u32".to_string(),
                RequiredResourceType::Texture => "&'a TextureHandle".to_string(),
                RequiredResourceType::UniformBuffer(element_type_name) => {
                    format!("&'a UniformBufferHandle<{element_type_name}>")
                }
                RequiredResourceType::StructuredBuffer(element_type_name) => {
                    format!("&'a StorageBufferHandle<{element_type_name}>")
                }
            };

            GeneratedStructFieldDefinition {
                field_name: r.field_name.clone(),
                type_name,
            }
        })
        .collect();

    let resources_struct = GeneratedStructDefinition {
        type_name: "Resources<'a>".to_string(),
        fields: resources_fields,
        trait_derives: vec![],
        alignment: None,
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
            RequiredResourceType::VertexCount => {}
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
                            // NOTE for now, the only resource this can be is a storage buffer
                            let alignment = Some(Alignment::Std430);

                            let fields = struct_result_type
                                .fields
                                .iter()
                                .filter_map(|sf| gather_struct_defs(sf, struct_defs, alignment))
                                .collect();

                            struct_defs.push(GeneratedStructDefinition {
                                type_name: struct_result_type.type_name.clone(),
                                fields,
                                trait_derives: vec!["Debug", "Clone", "Serialize"],
                                alignment,
                            });
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

            Some(GeneratedStructFieldDefinition {
                field_name: scalar.field_name.to_snake_case(),
                type_name: field_type.to_string(),
            })
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

            Some(GeneratedStructFieldDefinition {
                field_name: vector.field_name.to_snake_case(),
                type_name: field_type.to_string(),
            })
        }

        StructField::Struct(struct_field) => {
            let type_name = struct_field.struct_type.type_name.to_string();
            let mut generated_sub_fields = vec![];
            for sub_field in &struct_field.struct_type.fields {
                if let Some(field_def) = gather_struct_defs(sub_field, struct_defs, alignment) {
                    generated_sub_fields.push(field_def);
                };
            }
            let sub_struct_def = GeneratedStructDefinition {
                type_name: type_name.clone(),
                fields: generated_sub_fields,
                trait_derives: vec!["Debug", "Clone", "Serialize"],
                alignment,
            };
            struct_defs.push(sub_struct_def);

            Some(GeneratedStructFieldDefinition {
                field_name: struct_field.field_name.to_snake_case(),
                type_name,
            })
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

            Some(GeneratedStructFieldDefinition {
                field_name: matrix.field_name.to_snake_case(),
                type_name: field_type.to_string(),
            })
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

    fn repr(&self) -> Option<&str> {
        self.alignment.as_ref().map(Alignment::annotation)
    }
}

#[derive(Debug)]
struct GeneratedStructFieldDefinition {
    field_name: String,
    type_name: String,
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
    VertexCount,
    Texture,
    UniformBuffer(String),
    StructuredBuffer(String),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Alignment {
    // #[repr(C, align(16))]
    // used for uniform buffers
    Std140,
    // #[repr(C)]
    // used for storage buffers
    // manual padding may be necessary
    Std430,
}

impl Alignment {
    fn annotation(&self) -> &str {
        match self {
            Self::Std140 => "#[repr(C, align(16))]",
            Self::Std430 => "#[repr(C)]",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::util::manifest_path;

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
}
