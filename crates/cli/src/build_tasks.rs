use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use askama::Template;
use heck::{ToSnakeCase, ToUpperCamelCase};

use mltrs_slang_reflection::json::*;
use mltrs_slang_reflection::{ReflectedComputeShader, ReflectedShader};
use mltrs_slang_reflection::{
    prepare_reflected_compute_shader_with_optimization, prepare_reflected_shader_with_optimization,
};

pub use mltrs_slang_reflection::OptimizationLevel;

use crate::util::relative_path;

#[derive(Debug, Clone)]
pub struct Config {
    /// whether to write rust code (or only shader spirv & json)
    pub generate_rust_source: bool,
    /// the directory to write the 'generated' module into
    pub rust_source_dir: PathBuf,
    /// the directory to read slang files from
    pub shaders_source_dir: PathBuf,
    /// the directory to write shader spriv & json to
    pub compiled_shaders_dir: PathBuf,
    /// the path prefix generated imports resolve the engine crate through:
    /// `"crate"` when the generated code lives in the engine itself (or in a
    /// stub crate like the check_crate fixture), `"mltrs"` for consumers
    pub import_root: String,
    /// the slang optimization level to compile shader bytecode at
    pub optimization: OptimizationLevel,
}

const SHADER_FILE_SUFFIX: &str = ".shader.slang";
const COMPUTE_SHADER_FILE_SUFFIX: &str = ".compute.slang";

/// Collect the names of the `.slang` files in `dir` whose name ends with `suffix`.
///
/// The result is sorted: `read_dir` yields entries in filesystem order, which
/// differs between machines and between two states of the same directory, and
/// this order reaches the generated `shader_atlas.rs` (module list, `ShaderAtlas`
/// fields, `init()` body). Sorting keeps generated output a pure function of the
/// source tree so a snapshot diff always means a real change.
fn collect_slang_file_names(dir: &Path, suffix: &str) -> anyhow::Result<Vec<String>> {
    let mut file_names: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|entry_res| entry_res.ok())
        .map(|dir_entry| dir_entry.path())
        .filter_map(|path| {
            path.file_name()
                .and_then(|os_str| os_str.to_str())
                .map(|s| s.to_string())
        })
        .filter(|file_name| file_name.ends_with(suffix))
        .collect();
    file_names.sort();
    Ok(file_names)
}

pub fn write_precompiled_shaders(config: Config) -> anyhow::Result<()> {
    let slang_file_names =
        collect_slang_file_names(&config.shaders_source_dir, SHADER_FILE_SUFFIX)?;
    let compute_slang_file_names =
        collect_slang_file_names(&config.shaders_source_dir, COMPUTE_SHADER_FILE_SUFFIX)?;

    let type_to_module = reflect_slang_module_types(&config.shaders_source_dir);

    let search_path = config.shaders_source_dir.to_str().unwrap();

    let mut graphics_data: Vec<GraphicsShaderData> = vec![];
    let mut compute_data: Vec<ComputeShaderData> = vec![];
    let mut compiled_files: Vec<(String, Vec<u8>)> = vec![];

    for slang_file_name in &slang_file_names {
        let ReflectedShader {
            vertex_shader,
            fragment_shader,
            reflection_json,
        } = prepare_reflected_shader_with_optimization(
            slang_file_name,
            search_path,
            config.optimization,
        )?;

        let source_file_name = &reflection_json.source_file_name;

        let reflection_json_str = serde_json::to_string_pretty(&reflection_json)?;
        compiled_files.push((
            source_file_name.replace(SHADER_FILE_SUFFIX, ".json"),
            reflection_json_str.into_bytes(),
        ));
        compiled_files.push((
            source_file_name.replace(SHADER_FILE_SUFFIX, ".vert.spv"),
            vertex_shader.shader_bytecode.to_vec(),
        ));
        compiled_files.push((
            source_file_name.replace(SHADER_FILE_SUFFIX, ".frag.spv"),
            fragment_shader.shader_bytecode.to_vec(),
        ));

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
        } = prepare_reflected_compute_shader_with_optimization(
            slang_file_name,
            search_path,
            config.optimization,
        )?;

        let source_file_name = &reflection_json.source_file_name;

        let reflection_json_str = serde_json::to_string_pretty(&reflection_json)?;
        compiled_files.push((
            source_file_name.replace(COMPUTE_SHADER_FILE_SUFFIX, ".comp.json"),
            reflection_json_str.into_bytes(),
        ));
        compiled_files.push((
            source_file_name.replace(COMPUTE_SHADER_FILE_SUFFIX, ".comp.spv"),
            compute_shader.shader_bytecode.to_vec(),
        ));

        if config.generate_rust_source {
            compute_data.push(collect_compute_shader_data(
                &reflection_json,
                &type_to_module,
            ));
        }
    }

    if config.compiled_shaders_dir.exists() {
        std::fs::remove_dir_all(&config.compiled_shaders_dir)?;
    }
    std::fs::create_dir_all(&config.compiled_shaders_dir)?;
    for (file_name, bytes) in compiled_files {
        std::fs::write(config.compiled_shaders_dir.join(file_name), bytes)?;
    }

    if config.generate_rust_source {
        let mut generated_source_files = vec![];

        let all_shader_defs: Vec<(String, GeneratedTypeDefs)> = graphics_data
            .iter()
            .map(|d| (d.shader_name.clone(), d.defs.clone()))
            .chain(
                compute_data
                    .iter()
                    .map(|d| (d.shader_name.clone(), d.defs.clone())),
            )
            .collect();

        let shared_modules = collect_shared_modules(&all_shader_defs);

        for (module_name, module) in &shared_modules {
            let cross_imports = cross_module_imports(module_name, module, &shared_modules);

            let template = SharedModuleTemplate {
                import_root: config.import_root.clone(),
                module_doc_lines: vec![format!(
                    "shared types from slang module: {module_name}.slang"
                )],
                cross_module_imports: cross_imports,
                enum_defs: module.enum_defs.clone(),
                struct_defs: module.struct_defs.clone(),
            };

            let file_name = format!("{module_name}.rs");
            generated_source_files.push(GeneratedFile {
                relative_path: relative_path(["generated", "shader_atlas", &file_name]),
                content: template.render().unwrap(),
            });
        }

        for data in &graphics_data {
            let file = render_graphics_shader_file(data, &shared_modules, &config.import_root);
            generated_source_files.push(file);
        }

        for data in &compute_data {
            let file = render_compute_shader_file(data, &shared_modules, &config.import_root);
            generated_source_files.push(file);
        }

        let shared_module_names: Vec<String> = shared_modules.keys().cloned().collect();
        add_top_level_rust_modules(
            &slang_file_names,
            &compute_slang_file_names,
            &shared_module_names,
            &config.import_root,
            &mut generated_source_files,
        );

        let shader_atlas_dir = config
            .rust_source_dir
            .join(relative_path(["generated", "shader_atlas"]));
        if shader_atlas_dir.exists() {
            std::fs::remove_dir_all(&shader_atlas_dir)?;
        }

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
    import_root: &str,
    generated_source_files: &mut Vec<GeneratedFile>,
) {
    let module_names: Vec<String> = slang_file_names
        .iter()
        .map(|file_name| file_name.replace(SHADER_FILE_SUFFIX, ""))
        .collect();
    let entries: Vec<ShaderAtlasField> = module_names
        .iter()
        .map(|module_name| ShaderAtlasField {
            field_name: module_name.clone(),
            type_prefix: format!("{module_name}::"),
        })
        .collect();

    let compute_module_names: Vec<String> = compute_slang_file_names
        .iter()
        .map(|file_name| file_name.replace(COMPUTE_SHADER_FILE_SUFFIX, "_compute"))
        .collect();
    let compute_entries: Vec<ShaderAtlasField> = compute_module_names
        .iter()
        .map(|module_name| ShaderAtlasField {
            field_name: module_name.clone(),
            type_prefix: format!("{module_name}::"),
        })
        .collect();

    let engine_root = match import_root {
        "crate" | "self" | "super" => import_root.to_string(),
        // we need the leading :: because the local 'mltrs' module
        // from mtlrs.slang shadows the crate name
        external_crate => format!("::{external_crate}"),
    };

    let mut all_module_names: Vec<String> = shared_module_names
        .iter()
        .chain(&module_names)
        .chain(&compute_module_names)
        .cloned()
        .collect();
    all_module_names.sort();

    let shader_atlas_module = ShaderAtlasModule {
        engine_root,
        module_names: all_module_names,
        entries,
        compute_entries,
    };

    let shader_atlas_file = GeneratedFile {
        relative_path: relative_path(["generated", "shader_atlas.rs"]),
        content: shader_atlas_module.render().unwrap(),
    };
    generated_source_files.push(shader_atlas_file);

    let top_generated_module = GeneratedFile {
        relative_path: relative_path(["generated.rs"]),
        content: "#[allow(dead_code)]\npub mod shader_atlas;\n".to_string(),
    };
    generated_source_files.push(top_generated_module);
}

/// Intermediate data collected from a graphics shader before rendering
struct GraphicsShaderData {
    shader_name: String,
    defs: GeneratedTypeDefs,
    vertex_impl_blocks: Vec<VertexImplBlock>,
    shader_impl: GeneratedShaderImpl,
    source_file_name: String,
}

/// Collect struct definitions and template data from a graphics shader (without rendering)
fn collect_graphics_shader_data(
    reflection_json: &ReflectionJson,
    type_to_module: &HashMap<String, String>,
) -> GraphicsShaderData {
    let mut defs = GeneratedTypeDefs::default();
    let mut vertex_impl_blocks = vec![];

    // NOTE vertex/index data is not a Resources field: whether a pipeline owns
    // its buffers or borrows a shared mesh is a call-site decision, made with
    // PipelineConfig::with_vertices / with_shared_mesh.
    let mut required_resources = vec![];

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
                        gather_struct_defs(field, &mut defs, Some(Alignment::Std140))
                    {
                        generated_fields.push(generated_field);
                    };
                }

                let def = GeneratedStructDefinition::gpu_layout(
                    struct_param.type_name.to_string(),
                    generated_fields,
                    Some(Alignment::Std140),
                    None,
                );

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

                defs.struct_defs.push(def);
            }
        }
    }

    let mut push_constant_type_name = None;
    for global_parameter in &reflection_json.global_parameters {
        match global_parameter {
            GlobalParameter::ParameterBlock(parameter_block) => {
                collect_parameter_block(parameter_block, &mut defs, &mut required_resources);
            }

            GlobalParameter::PushConstant(push_constant) => {
                let type_name = collect_push_constant_block(
                    push_constant,
                    &mut defs,
                    &reflection_json.source_file_name,
                );
                push_constant_type_name = Some(type_name);
            }
        }
    }

    defs.struct_defs.reverse();

    defs.struct_defs.push(resources_struct(&required_resources));

    let shader_name = reflection_json
        .source_file_name
        .replace(SHADER_FILE_SUFFIX, "");

    // NOTE these must be in descriptor set layout order in the reflection json
    let mut resources_texture_fields: Vec<String> = vec![];
    let mut resources_uniform_buffer_fields: Vec<String> = vec![];
    let mut resources_storage_texture_fields: Vec<String> = vec![];
    for res in &required_resources {
        match res.resource_type {
            RequiredResourceType::Texture => {
                resources_texture_fields.push(res.field_name.clone());
            }
            RequiredResourceType::UniformBuffer(_) => {
                resources_uniform_buffer_fields.push(res.field_name.clone());
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
        push_constant_type_name,
        resources_texture_fields,
        resources_uniform_buffer_fields,
        resources_storage_texture_fields,
    };

    // Tag struct defs with source module info
    tag_source_modules(&mut defs, type_to_module, &shader_name);

    GraphicsShaderData {
        shader_name,
        defs,
        vertex_impl_blocks,
        shader_impl,
        source_file_name: reflection_json.source_file_name.clone(),
    }
}

/// Render a graphics shader file, filtering out shared types and adding imports
fn render_graphics_shader_file(
    data: &GraphicsShaderData,
    shared_modules: &BTreeMap<String, GeneratedTypeDefs>,
    import_root: &str,
) -> GeneratedFile {
    let shared_module_imports = shared_imports_for_shader(&data.defs, shared_modules);

    // Filter out shared types — they're in their own module files
    let local = local_type_defs(&data.defs);

    let module_doc_lines = vec![format!(
        "generated from slang shader: {}",
        data.source_file_name
    )];

    let content = ShaderAtlasEntryModule {
        import_root: import_root.to_string(),
        module_doc_lines,
        shared_module_imports,
        enum_defs: local.enum_defs,
        struct_defs: local.struct_defs,
        vertex_impl_blocks: data.vertex_impl_blocks.clone(),
        shader_impl: data.shader_impl.clone(),
        push_constant_budget: MAX_PUSH_CONSTANT_BYTES,
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
    /// `import_root`, made absolute so a shared slang module cannot shadow it
    engine_root: String,
    /// every submodule, sorted — rustfmt sorts a `mod` list alphabetically
    module_names: Vec<String>,
    entries: Vec<ShaderAtlasField>,
    compute_entries: Vec<ShaderAtlasField>,
}

/// One `ShaderAtlas` field: its name and the module path its `Shader` lives in.
struct ShaderAtlasField {
    field_name: String,
    type_prefix: String,
}

impl ShaderAtlasField {
    /// The whole line, indented. rustfmt breaks a struct-literal field after the
    /// colon when the value does not fit, and the field name comes from the
    /// shader file name, so the width is only known here.
    fn init_line(&self) -> String {
        let Self {
            field_name,
            type_prefix,
        } = self;

        let one_line = format!("            {field_name}: {type_prefix}Shader::init(),");
        if one_line.len() <= RUSTFMT_MAX_WIDTH {
            one_line
        } else {
            format!("            {field_name}:\n                {type_prefix}Shader::init(),")
        }
    }
}

/// rustfmt's default `max_width`; the repo's rustfmt.toml sets only the edition
const RUSTFMT_MAX_WIDTH: usize = 100;

#[derive(Template)]
#[template(path = "shader_atlas_entry.rs.askama", escape = "none")]
struct ShaderAtlasEntryModule {
    import_root: String,
    module_doc_lines: Vec<String>,
    shared_module_imports: Vec<SharedModuleImport>,
    enum_defs: Vec<GeneratedEnumDefinition>,
    struct_defs: Vec<GeneratedStructDefinition>,
    vertex_impl_blocks: Vec<VertexImplBlock>,
    shader_impl: GeneratedShaderImpl,
    push_constant_budget: usize,
}

#[derive(Template)]
#[template(path = "shader_compute_entry.rs.askama", escape = "none")]
struct ShaderComputeEntryModule {
    import_root: String,
    module_doc_lines: Vec<String>,
    shared_module_imports: Vec<SharedModuleImport>,
    enum_defs: Vec<GeneratedEnumDefinition>,
    struct_defs: Vec<GeneratedStructDefinition>,
    shader_impl: GeneratedComputeShaderImpl,
}

/// The types a shader emits into its own file — everything not hoisted into a
/// shared slang module's file.
fn local_type_defs(defs: &GeneratedTypeDefs) -> GeneratedTypeDefs {
    GeneratedTypeDefs {
        struct_defs: defs
            .struct_defs
            .iter()
            .filter(|d| d.source_module.is_none())
            .cloned()
            .collect(),
        enum_defs: defs
            .enum_defs
            .iter()
            .filter(|d| d.source_module.is_none())
            .cloned()
            .collect(),
    }
}

#[derive(Clone)]
struct GeneratedComputeShaderImpl {
    shader_name: String,
    shader_type_name: String,
    workgroup_size: [u32; 3],
    resources_texture_fields: Vec<String>,
    resources_uniform_buffer_fields: Vec<String>,
    resources_storage_texture_fields: Vec<String>,
}

#[derive(Clone)]
struct GeneratedShaderImpl {
    shader_name: String,
    shader_type_name: String,
    vertex_type_name: Option<String>,
    push_constant_type_name: Option<String>,
    resources_texture_fields: Vec<String>,
    resources_uniform_buffer_fields: Vec<String>,
    resources_storage_texture_fields: Vec<String>,
}

impl GeneratedShaderImpl {
    /// What `pipeline_config()` returns. An indexed shader hands back an
    /// `IndexedPipelineConfig`, which is not yet a `PipelineConfig`: the caller
    /// has to pick a vertex source first. A shader with no vertex input is
    /// already complete.
    fn config_return_type(&self) -> String {
        let push_slot = match &self.push_constant_type_name {
            Some(block) => format!("PushBlock<{block}>"),
            None => "NoPush".to_string(),
        };

        match &self.vertex_type_name {
            Some(vertex_type_name) => {
                format!("IndexedPipelineConfig<'a, {vertex_type_name}, {push_slot}>")
            }
            None => format!("PipelineConfig<'a, NoVertex, DrawVertexCount, {push_slot}>"),
        }
    }

    /// The matching `PipelineConfigBuilder` terminal call. Must stay in sync
    /// with [`Self::config_return_type`] — both key off `vertex_type_name`.
    fn build_method(&self) -> &str {
        if self.vertex_type_name.is_some() {
            "build_indexed"
        } else {
            "build_vertex_count"
        }
    }
}

/// Intermediate data collected from a compute shader before rendering
struct ComputeShaderData {
    shader_name: String,
    defs: GeneratedTypeDefs,
    shader_impl: GeneratedComputeShaderImpl,
    source_file_name: String,
}

/// Collect struct definitions and template data from a compute shader (without rendering)
fn collect_compute_shader_data(
    reflection_json: &ComputeReflectionJson,
    type_to_module: &HashMap<String, String>,
) -> ComputeShaderData {
    let mut defs = GeneratedTypeDefs::default();
    let mut required_resources = vec![];

    for global_parameter in &reflection_json.global_parameters {
        match global_parameter {
            GlobalParameter::ParameterBlock(parameter_block) => {
                collect_parameter_block(parameter_block, &mut defs, &mut required_resources);
            }

            GlobalParameter::PushConstant(push_constant) => unreachable!(
                "push constant block '{}' in a compute shader",
                push_constant.parameter_name,
            ),
        }
    }

    defs.struct_defs.reverse();

    defs.struct_defs.push(resources_struct(&required_resources));

    let shader_name = reflection_json
        .source_file_name
        .replace(COMPUTE_SHADER_FILE_SUFFIX, "");

    // NOTE these must be in descriptor set layout order in the reflection json
    let mut resources_texture_fields: Vec<String> = vec![];
    let mut resources_uniform_buffer_fields: Vec<String> = vec![];
    let mut resources_storage_texture_fields: Vec<String> = vec![];
    for res in &required_resources {
        match res.resource_type {
            RequiredResourceType::Texture => {
                resources_texture_fields.push(res.field_name.clone());
            }
            RequiredResourceType::UniformBuffer(_) => {
                resources_uniform_buffer_fields.push(res.field_name.clone());
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
        resources_storage_texture_fields,
    };

    // Tag struct defs with source module info
    tag_source_modules(&mut defs, type_to_module, &shader_name);

    ComputeShaderData {
        shader_name,
        defs,
        shader_impl,
        source_file_name: reflection_json.source_file_name.clone(),
    }
}

/// Render a compute shader file, filtering out shared types and adding imports
fn render_compute_shader_file(
    data: &ComputeShaderData,
    shared_modules: &BTreeMap<String, GeneratedTypeDefs>,
    import_root: &str,
) -> GeneratedFile {
    let shared_module_imports = shared_imports_for_shader(&data.defs, shared_modules);

    let local = local_type_defs(&data.defs);

    let module_doc_lines = vec![format!(
        "generated from slang compute shader: {}",
        data.source_file_name
    )];

    let content = ShaderComputeEntryModule {
        import_root: import_root.to_string(),
        module_doc_lines,
        shared_module_imports,
        enum_defs: local.enum_defs,
        struct_defs: local.struct_defs,
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
    defs: &mut GeneratedTypeDefs,
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
        let Some(mut gen_field) = gather_struct_defs(source_field, defs, alignment_for_nested)
        else {
            assert_no_occupied_bytes_dropped(source_field);
            continue;
        };

        // Get the expected offset from reflection
        let Some(OffsetSizeBinding {
            offset: expected_offset,
            size: field_size,
        }) = field_offset_size(source_field)
        else {
            // No offset info (e.g. semantic field), just add the field
            generated_fields.push(gen_field);
            continue;
        };

        check_rust_placeable(&gen_field, expected_offset);
        gen_field.offset = Some(expected_offset);
        gen_field.size = Some(field_size);

        // Track max alignment for struct alignment calculation
        let field_align = field_alignment(&gen_field);
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
    defs: &mut GeneratedTypeDefs,
) -> (Vec<GeneratedStructFieldDefinition>, usize, usize) {
    let mut generated_fields = Vec::new();
    let mut current_offset: usize = 0;
    let mut padding_index: usize = 0;

    for source_field in source_fields {
        // Skip resources - they don't have offset/size and don't contribute to layout
        if matches!(source_field, StructField::Resource(_)) {
            // Still need to gather struct definitions for StructuredBuffer element types
            let _ = gather_struct_defs(source_field, defs, Some(Alignment::Std140));
            assert_no_occupied_bytes_dropped(source_field);
            continue;
        }

        // Get the generated field (and recurse for nested structs)
        let Some(mut gen_field) = gather_struct_defs(source_field, defs, Some(Alignment::Std140))
        else {
            assert_no_occupied_bytes_dropped(source_field);
            continue;
        };

        let Some(OffsetSizeBinding {
            offset: expected_offset,
            size: field_size,
        }) = field_offset_size(source_field)
        else {
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
    defs: &mut GeneratedTypeDefs,
    alignment: Option<Alignment>,
) -> Option<GeneratedStructFieldDefinition> {
    match field {
        // textures are handled via resources; not a field of the uniform buffer struct
        StructField::Resource(_) => None,

        StructField::Scalar(scalar) => {
            let field_type = match scalar.scalar_type {
                ScalarType::Float32 => "f32",
                ScalarType::Int32 => "i32",
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
                generate_std430_struct_fields(&ptr.pointee_type.fields, defs);

            assert_eq!(
                expected_size, ptr.pointee_size,
                "computed std430 size of pointee '{}' disagrees with slang reflection",
                ptr.pointee_type.type_name,
            );

            try_add_struct_def(
                &mut defs.struct_defs,
                GeneratedStructDefinition::gpu_layout(
                    ptr.pointee_type.type_name.clone(),
                    fields,
                    Some(Alignment::Std430 { struct_alignment }),
                    Some(expected_size),
                ),
            );

            let addr_type = match ptr.access {
                PointerAccess::ReadWrite => "Addr",
                PointerAccess::Read => "ReadAddr",
                PointerAccess::Immutable => "ImmutableAddr",
            };
            Some(GeneratedStructFieldDefinition::new(
                ptr.field_name.to_snake_case(),
                format!("{addr_type}<{}>", ptr.pointee_type.type_name),
            ))
        }

        StructField::Vector(VectorStructField::Semantic(_)) => None,
        StructField::Vector(VectorStructField::Bound(vector)) => {
            let VectorElementType::Scalar(element_type) = &vector.element_type;
            // Integer vectors are 4-component only: 2/3-component integer
            // vectors aren't needed and UVec3-style types have the same
            // vec3 padding trap as Vec3 without the existing precedent.
            let field_type = match (element_type.scalar_type, vector.element_count) {
                (ScalarType::Float32, 4) => "glam::Vec4",
                (ScalarType::Float32, 3) => "glam::Vec3",
                (ScalarType::Float32, 2) => "glam::Vec2",
                (ScalarType::Uint32, 4) => "glam::UVec4",
                (ScalarType::Int32, 4) => "glam::IVec4",
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
                    let (fields, _align, size) =
                        generate_std140_struct_fields(&struct_field.struct_type.fields, defs);
                    (fields, Some(Alignment::Std140), Some(size))
                }
                Some(Alignment::Std430 { .. }) => {
                    let (fields, align, size) =
                        generate_std430_struct_fields(&struct_field.struct_type.fields, defs);
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
                        if let Some(field_def) = gather_struct_defs(sub_field, defs, alignment) {
                            fields.push(field_def);
                        };
                    }
                    (fields, None, None)
                }
            };

            let sub_struct_def = GeneratedStructDefinition::gpu_layout(
                type_name.clone(),
                generated_sub_fields,
                nested_alignment,
                expected_size,
            );
            try_add_struct_def(&mut defs.struct_defs, sub_struct_def);

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

        StructField::Array(array) => {
            let element_type = match array.element_scalar_type {
                ScalarType::Float32 => "glam::Vec4",
                ScalarType::Int32 => "glam::IVec4",
                ScalarType::Uint32 => "glam::UVec4",
                ScalarType::Uint64 => unreachable!("rejected by the array reflection gate"),
            };

            Some(GeneratedStructFieldDefinition::new(
                array.field_name.to_snake_case(),
                format!("[{element_type}; {}]", array.element_count),
            ))
        }

        StructField::DescriptorHandle(handle) => {
            // a bindless texture handle
            let shape_marker = match handle.shape {
                DescriptorHandleShape::Sampler2D => "Sampler2D",
                DescriptorHandleShape::RwTexture2D => "RwTexture2D",
            };

            Some(GeneratedStructFieldDefinition::new(
                handle.field_name.to_snake_case(),
                format!("BindlessHandle<{shape_marker}>"),
            ))
        }

        StructField::Enum(enum_field) => {
            try_add_enum_def(&mut defs.enum_defs, &enum_field.enum_type);

            // A slang enum is laid out as its tag type, whose alignment equals
            // its size. The name-based alignment helpers can't know that, so it
            // travels with the field.
            Some(GeneratedStructFieldDefinition::new_with_align(
                enum_field.field_name.to_snake_case(),
                enum_field.enum_type.type_name.clone(),
                enum_field.enum_type.tag_type.size(),
            ))
        }
    }
}

/// Collects a `ParameterBlock<T>` global: one std140 uniform buffer struct, plus
/// its descriptor-backed resources in descriptor set layout order.
fn collect_parameter_block(
    parameter_block: &ParameterBlockGlobalParameter,
    defs: &mut GeneratedTypeDefs,
    required_resources: &mut Vec<RequiredResource>,
) {
    let (param_block_fields, _struct_alignment, expected_size) =
        generate_std140_struct_fields(&parameter_block.element_type.fields, defs);

    let parameter_block_resources = parameter_block
        .element_type
        .fields
        .iter()
        .filter_map(required_resource);
    required_resources.extend(parameter_block_resources);

    let has_uniform_fields = !param_block_fields.is_empty();

    defs.struct_defs.push(GeneratedStructDefinition::gpu_layout(
        parameter_block.element_type.type_name.clone(),
        param_block_fields,
        Some(Alignment::Std140),
        Some(expected_size),
    ));

    if has_uniform_fields {
        let param_name = parameter_block.parameter_name.to_snake_case();
        required_resources.push(RequiredResource {
            field_name: format!("{param_name}_buffer"),
            resource_type: RequiredResourceType::UniformBuffer(
                parameter_block.element_type.type_name.clone(),
            ),
        })
    }
}

/// Collects a `[[vk::push_constant]]` block, returning the emitted type's name.
fn collect_push_constant_block(
    push_constant: &PushConstantGlobalParameter,
    defs: &mut GeneratedTypeDefs,
    source_file_name: &str,
) -> String {
    let (fields, struct_alignment, expected_size) =
        generate_std430_struct_fields(&push_constant.element_type.fields, defs);

    let type_name = push_constant.element_type.type_name.clone();

    // if this assert fails, it indicates either a user annotating a struct
    // field with [[vk::offset]], or a bug in slang
    assert_eq!(
        expected_size, push_constant.element_size,
        "computed std430 size of push constant block '{type_name}' \
        disagrees with slang reflection",
    );

    assert_push_constant_size(&type_name, expected_size, source_file_name);

    defs.struct_defs.push(GeneratedStructDefinition::gpu_layout(
        type_name.clone(),
        fields,
        Some(Alignment::Std430 { struct_alignment }),
        Some(expected_size),
    ));

    type_name
}

/// The generated code asserts this too, but a failing const assert reports only
/// "evaluation of constant value failed", so we also check here
fn assert_push_constant_size(type_name: &str, size: usize, source_file_name: &str) {
    assert!(
        size <= MAX_PUSH_CONSTANT_BYTES,
        "push constant block '{type_name}' ({source_file_name}) is {size} bytes, over the \
        {MAX_PUSH_CONSTANT_BYTES}-byte guaranteed budget; move a field behind a BDA pointer \
        (mltrs::Addr<T>) or into the ParameterBlock",
    );
}

/// The `Resources<'a>` fields for a shader's descriptor-backed resources.
fn resources_struct(required_resources: &[RequiredResource]) -> GeneratedStructDefinition {
    let fields = required_resources
        .iter()
        .map(|r| {
            let type_name = match &r.resource_type {
                RequiredResourceType::Texture => "&'a TextureHandle".to_string(),
                RequiredResourceType::UniformBuffer(element_type_name) => {
                    format!("&'a UniformBufferHandle<{element_type_name}>")
                }
                RequiredResourceType::StorageTexture2D => "&'a StorageTextureHandle".to_string(),
            };

            GeneratedStructFieldDefinition::new(r.field_name.clone(), type_name)
        })
        .collect();

    // borrowed handles, not GPU bytes: no layout, no derives
    GeneratedStructDefinition {
        type_name: "Resources<'a>".to_string(),
        source_module: None,
        fields,
        trait_derives: vec![],
        alignment: None,
        expected_size: None,
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
        },

        _ => None,
    }
}

/// The generated type definitions gathered from one shader. Structs and enums
/// travel together because a slang module can contribute either, and the shared
/// module map is keyed by module name regardless of which kind it holds.
#[derive(Debug, Clone, Default)]
struct GeneratedTypeDefs {
    struct_defs: Vec<GeneratedStructDefinition>,
    enum_defs: Vec<GeneratedEnumDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
struct GeneratedEnumDefinition {
    type_name: String,
    /// Which slang module this type originated from (None = local to the shader)
    source_module: Option<String>,
    tag_type: EnumTagType,
    cases: Vec<GeneratedEnumCase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedEnumCase {
    variant_name: String,
    value: i64,
}

impl GeneratedEnumDefinition {
    fn trait_derive_line(&self) -> String {
        // Debug/Clone/Copy/Serialize match what generated structs derive;
        // PartialEq/Eq/Default/Facet are added because an enum is a value type
        // callers compare, construct, and edit through the facet-driven UI.
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default, Facet)]".to_string()
    }

    fn repr(&self) -> String {
        self.tag_type.repr()
    }

    fn tag_rust_type(&self) -> &'static str {
        self.tag_type.rust_type_name()
    }

    fn expected_size(&self) -> usize {
        self.tag_type.size()
    }

    fn try_from_arms(&self) -> Vec<String> {
        self.cases
            .iter()
            .map(|case| format!("{} => Ok(Self::{}),", case.value, case.variant_name))
            .collect()
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
    /// A struct mirroring a GPU memory layout.
    ///
    /// NOTE: `source_module` starts empty for every generated type;
    /// `tag_source_modules` fills it in later from the type→module map.
    fn gpu_layout(
        type_name: String,
        fields: Vec<GeneratedStructFieldDefinition>,
        alignment: Option<Alignment>,
        expected_size: Option<usize>,
    ) -> Self {
        Self {
            type_name,
            source_module: None,
            fields,
            // reflection only allows copyable types here
            trait_derives: vec!["Debug", "Clone", "Copy", "Serialize"],
            alignment,
            expected_size,
        }
    }

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
    /// the emitted Rust type's alignment, when the name-based helpers can't
    /// derive it (generated enums). None means "look it up by type name".
    rust_align: Option<usize>,
}

impl GeneratedStructFieldDefinition {
    fn new(field_name: String, type_name: String) -> Self {
        Self {
            field_name,
            type_name,
            offset: None,
            size: None,
            rust_align: None,
        }
    }

    fn new_with_align(field_name: String, type_name: String, rust_align: usize) -> Self {
        Self {
            field_name,
            type_name,
            offset: None,
            size: None,
            rust_align: Some(rust_align),
        }
    }

    fn padding(index: usize, size: usize) -> Self {
        Self {
            field_name: format!("_padding_{index}"),
            type_name: format!("[u8; {size}]"),
            offset: None,
            size: None,
            rust_align: None,
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
    Texture,
    StorageTexture2D,
    UniformBuffer(String),
}

/// Extracts offset and size from a StructField's binding. Only a binding that
/// occupies bytes — a uniform block or push block field — maps to the GPU
/// struct; the layout generators do not care which of the two it is.
fn field_offset_size(field: &StructField) -> Option<OffsetSizeBinding> {
    field.binding()?.occupied_bytes().cloned()
}

/// A field that occupies bytes (uniform or push constant) maps to the GPU struct.
/// Dropping it from the generated struct emits no size assert for it.
fn assert_no_occupied_bytes_dropped(source_field: &StructField) {
    assert!(
        field_offset_size(source_field).is_none(),
        "field '{}' occupies bytes in the GPU struct but was dropped from the \
        generated struct",
        source_field.field_name(),
    );
}

/// Parses an emitted Rust array type string `"[T; N]"` into `(T, N)`.
fn parse_array_type(type_name: &str) -> Option<(&str, usize)> {
    let inner = type_name.strip_prefix('[')?.strip_suffix(']')?;
    let (element, count) = inner.rsplit_once("; ")?;
    Some((element, count.trim().parse().ok()?))
}

/// Returns the GPU alignment of a generated field, preferring an alignment the
/// field carries explicitly. A generated enum's type name says nothing about its
/// tag width, and the by-name fallback would assume 16 for it.
fn field_alignment(field: &GeneratedStructFieldDefinition) -> usize {
    field
        .rust_align
        .unwrap_or_else(|| field_alignment_by_name(&field.type_name))
}

/// Returns the alignment for a given Rust type name.
/// These rules are the same for both std140 and std430 for basic types.
fn field_alignment_by_name(type_name: &str) -> usize {
    // arrays share their element's alignment in both std140 and std430
    if let Some((element, _count)) = parse_array_type(type_name) {
        return field_alignment_by_name(element);
    }

    match type_name {
        "glam::Vec4" | "glam::UVec4" | "glam::IVec4" | "glam::Mat4" => 16,
        "glam::Vec3" => 16, // vec3 has 16-byte alignment in both std140 and std430
        "glam::Vec2" | "u64" => 8,
        "f32" | "u32" | "i32" => 4,
        // Addr<T> / ReadAddr<T> / ImmutableAddr<T> are repr(transparent) over u64
        s if s.starts_with("Addr<")
            || s.starts_with("ReadAddr<")
            || s.starts_with("ImmutableAddr<") =>
        {
            8
        }
        // a DescriptorHandle is a uint2, which aligns to 8 in std140 and std430
        s if s.starts_with("BindlessHandle<") => 8,
        _ => 16, // assume 16 for unknown/struct types
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
    if let Some((element, _count)) = parse_array_type(type_name) {
        return rust_type_alignment(element);
    }

    Some(match type_name {
        "f32" => std::mem::align_of::<f32>(),
        "i32" => std::mem::align_of::<i32>(),
        "u32" => std::mem::align_of::<u32>(),
        "u64" => std::mem::align_of::<u64>(),
        "glam::Vec2" => std::mem::align_of::<glam::Vec2>(),
        "glam::Vec3" => std::mem::align_of::<glam::Vec3>(),
        "glam::Vec4" => std::mem::align_of::<glam::Vec4>(),
        // UVec4/IVec4 are repr(C) align-4 (no SIMD), unlike Vec4's align-16;
        // that's fine — 4 divides every 16-multiple offset, and exact placement
        // is proven by the emitted offset_of! asserts, which is also why the
        // generated-file preamble's align_of assert stays Vec4-only.
        "glam::UVec4" => std::mem::align_of::<glam::UVec4>(),
        "glam::IVec4" => std::mem::align_of::<glam::IVec4>(),
        "glam::Mat4" => std::mem::align_of::<glam::Mat4>(),
        // Addr<T> / ReadAddr<T> / ImmutableAddr<T> are repr(transparent) over u64 for every T
        s if s.starts_with("Addr<")
            || s.starts_with("ReadAddr<")
            || s.starts_with("ImmutableAddr<") =>
        {
            std::mem::align_of::<u64>()
        }
        // BindlessHandle<T> is repr(transparent) over u64 for every T
        s if s.starts_with("BindlessHandle<") => std::mem::align_of::<u64>(),
        _ => return None,
    })
}

/// A reflected offset that isn't a multiple of the emitted Rust type's alignment
/// cannot be reproduced with a #[repr(C)] field of that type — unreachable under
/// std140/std430, so it means a non-std GPU layout leaked into codegen.
fn check_rust_placeable(gen_field: &GeneratedStructFieldDefinition, expected_offset: usize) {
    // an explicit alignment wins: a generated enum's type name isn't in the
    // by-name table, which would otherwise skip the check entirely
    let alignment = gen_field
        .rust_align
        .or_else(|| rust_type_alignment(&gen_field.type_name));

    if let Some(align) = alignment
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

/// The tag is part of the ABI, so it's compared alongside the cases.
fn enum_defs_compatible(a: &GeneratedEnumDefinition, b: &GeneratedEnumDefinition) -> bool {
    a.tag_type == b.tag_type && a.cases == b.cases
}

/// Adds an enum definition if it doesn't already exist. Like try_add_struct_def
/// this only dedups *within one shader* — enum_defs is allocated per shader, and
/// cross-shader agreement is enforced for hoisted types in collect_shared_modules.
fn try_add_enum_def(enum_defs: &mut Vec<GeneratedEnumDefinition>, enum_type: &EnumFieldType) {
    let mut cases: Vec<GeneratedEnumCase> = vec![];
    for case in &enum_type.cases {
        let variant_name = case.name.to_upper_camel_case();

        // rustc's duplicate-variant error doesn't mention the slang spelling
        // the two cases came from, which is the only actionable part
        if let Some(clash) = cases.iter().find(|c| c.variant_name == variant_name) {
            panic!(
                "enum '{}' cases '{}' and '{}' both generate the Rust variant \
                '{variant_name}'; rename one in the shader",
                enum_type.type_name, clash.variant_name, case.name,
            );
        }

        cases.push(GeneratedEnumCase {
            variant_name,
            value: case.value,
        });
    }

    let new_def = GeneratedEnumDefinition {
        type_name: enum_type.type_name.clone(),
        source_module: None,
        tag_type: enum_type.tag_type,
        cases,
    };

    if let Some(existing) = enum_defs.iter().find(|d| d.type_name == new_def.type_name) {
        if !enum_defs_compatible(existing, &new_def) {
            panic!(
                "Incompatible enum definitions for '{}': cases or tag type differ",
                new_def.type_name
            );
        }
    } else {
        enum_defs.push(new_def);
    }
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
    // (slang load name, rust module name); these differ for modules in a
    // subdirectory, whose types all collapse into one rust module named
    // after the directory (eg. mltrs/addr.slang → "mltrs/addr" / "mltrs").
    let mut modules: Vec<(String, String)> = Vec::new();
    let mut subdirs = Vec::new();

    for entry in std::fs::read_dir(shaders_source_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let file_name = path.file_name().unwrap().to_str().unwrap();

        if path.is_dir() {
            subdirs.push(file_name.to_string());
            continue;
        }
        if file_name.ends_with(SHADER_FILE_SUFFIX)
            || file_name.ends_with(COMPUTE_SHADER_FILE_SUFFIX)
        {
            continue;
        }
        if !file_name.ends_with(".slang") {
            continue;
        }

        let module_name = file_name.strip_suffix(".slang").unwrap().to_string();
        modules.push((module_name.clone(), module_name));
    }

    // A top-level module sharing a subdirectory's name (eg. mltrs.slang next
    // to mltrs/) is reflected like any other.
    // In the 'mltrs' case, it's a re-export prelude with no types of its own,
    // but if the top module declares types,
    // they'll collapse into the same rust module
    for subdir in &subdirs {
        for entry in std::fs::read_dir(shaders_source_dir.join(subdir)).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let file_name = path.file_name().unwrap().to_str().unwrap();

            if !path.is_file() || !file_name.ends_with(".slang") {
                continue;
            }

            let stem = file_name.strip_suffix(".slang").unwrap();
            modules.push((format!("{subdir}/{stem}"), subdir.clone()));
        }
    }

    modules.sort();

    let search_path = shaders_source_dir.to_str().unwrap();
    let module_refs: Vec<(&str, &str)> = modules
        .iter()
        .map(|(load, rust)| (load.as_str(), rust.as_str()))
        .collect();
    mltrs_slang_reflection::reflect_shared_module_types(&module_refs, search_path)
        .unwrap_or_else(|e| panic!("failed to reflect shared modules: {e}"))
}

/// Tag type definitions with their source module based on the type→module map.
/// The flat lists already contain nested types, so no recursion is needed.
fn tag_source_modules(
    defs: &mut GeneratedTypeDefs,
    type_to_module: &HashMap<String, String>,
    current_shader_module: &str,
) {
    for def in defs.struct_defs.iter_mut() {
        if let Some(module) = type_to_module.get(&def.type_name)
            && module != current_shader_module
        {
            def.source_module = Some(module.clone());
        }
    }
    for def in defs.enum_defs.iter_mut() {
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

impl SharedModuleImport {
    /// rustfmt strips the braces around a single-name import list
    fn use_path(&self) -> String {
        let module_name = &self.module_name;
        match self.type_names.as_slice() {
            [type_name] => format!("super::{module_name}::{type_name}"),
            type_names => format!("super::{module_name}::{{{}}}", type_names.join(", ")),
        }
    }
}

/// Collect shared type definitions from all shaders into per-module groups.
/// Returns (module_name → definitions) for types declared in a shared slang module.
fn collect_shared_modules(
    all_shader_defs: &[(String, GeneratedTypeDefs)],
) -> BTreeMap<String, GeneratedTypeDefs> {
    let mut modules: BTreeMap<String, GeneratedTypeDefs> = BTreeMap::new();

    for (shader_name, defs) in all_shader_defs {
        for def in &defs.struct_defs {
            if let Some(ref module_name) = def.source_module {
                let module = modules.entry(module_name.clone()).or_default();
                match module
                    .struct_defs
                    .iter()
                    .find(|d| d.type_name == def.type_name)
                {
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
                    None => module.struct_defs.push(def.clone()),
                }
            }
        }

        for def in &defs.enum_defs {
            if let Some(ref module_name) = def.source_module {
                let module = modules.entry(module_name.clone()).or_default();
                match module
                    .enum_defs
                    .iter()
                    .find(|d| d.type_name == def.type_name)
                {
                    Some(existing) => {
                        if !enum_defs_compatible(existing, def) {
                            panic!(
                                "shared enum '{}' (module '{module_name}') has \
                                incompatible cases or tag type in shader '{shader_name}'",
                                def.type_name,
                            );
                        }
                    }
                    None => module.enum_defs.push(def.clone()),
                }
            }
        }
    }

    modules
}

/// Determine which shared modules a shader needs to import, and which type names.
/// Only imports types directly referenced by local (non-shared) struct fields.
fn shared_imports_for_shader(
    defs: &GeneratedTypeDefs,
    shared_modules: &BTreeMap<String, GeneratedTypeDefs>,
) -> Vec<SharedModuleImport> {
    let mut imports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    // Check which shared types are directly referenced by local struct fields.
    // Enums have no fields, so they are only ever import targets, never sources.
    for def in &defs.struct_defs {
        if def.source_module.is_some() {
            continue; // skip shared types themselves
        }
        for field in &def.fields {
            for (module_name, module) in shared_modules {
                for module_def in &module.struct_defs {
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

                // exact match only: an enum is never a generic argument (a pointee
                // must be a struct) nor an array element, and short enum names are
                // exactly the substrings likeliest to collide — `contains` would
                // import an enum `View` for a field of type `DebugView`
                for module_def in &module.enum_defs {
                    if field.type_name == module_def.type_name {
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
    module: &GeneratedTypeDefs,
    all_shared_modules: &BTreeMap<String, GeneratedTypeDefs>,
) -> Vec<SharedModuleImport> {
    let mut imports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for def in &module.struct_defs {
        for field in &def.fields {
            for (other_module, other) in all_shared_modules {
                if other_module == module_name {
                    continue;
                }
                for other_def in &other.struct_defs {
                    if field.type_name == other_def.type_name {
                        imports
                            .entry(other_module.clone())
                            .or_default()
                            .insert(other_def.type_name.clone());
                    }
                }
                for other_def in &other.enum_defs {
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
    import_root: String,
    module_doc_lines: Vec<String>,
    cross_module_imports: Vec<SharedModuleImport>,
    enum_defs: Vec<GeneratedEnumDefinition>,
    struct_defs: Vec<GeneratedStructDefinition>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::util::manifest_path;
    use mltrs_slang_reflection::{prepare_reflected_compute_shader, prepare_reflected_shader};

    /// Shader discovery must be sorted, not in `read_dir` order: that order reaches
    /// the generated `shader_atlas.rs` and its snapshots, so an unsorted walk makes
    /// them fail spuriously on a pristine tree.
    #[test]
    fn slang_file_names_are_sorted() {
        let source_dir = manifest_path(["fixtures", "shaders"]);

        for suffix in [SHADER_FILE_SUFFIX, COMPUTE_SHADER_FILE_SUFFIX] {
            let file_names = collect_slang_file_names(&source_dir, suffix).unwrap();

            assert!(
                !file_names.is_empty(),
                "no '{suffix}' files found in {source_dir:?} — the sort assertion \
                 below would pass vacuously",
            );
            assert!(
                file_names.is_sorted(),
                "'{suffix}' files came back unsorted: {file_names:?}",
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn generated_files() {
        let tmp_prefix = format!("shader-test-{}", uuid::Uuid::new_v4());
        let tmp_dir_path = std::env::temp_dir().join(tmp_prefix);

        let config = Config {
            generate_rust_source: true,
            rust_source_dir: tmp_dir_path.join("src"),
            shaders_source_dir: manifest_path(["fixtures", "shaders"]),
            compiled_shaders_dir: tmp_dir_path.join(relative_path(["shaders", "compiled"])),
            import_root: "crate".to_string(),
            optimization: OptimizationLevel::High,
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

    /// No fixture has a shader name long enough to reach the wrapped branch, so
    /// the width rule is pinned here instead.
    #[test]
    fn atlas_init_line_wraps_only_past_the_rustfmt_width() {
        let short = ShaderAtlasField {
            field_name: "sdf_2d".to_string(),
            type_prefix: "sdf_2d::".to_string(),
        };
        assert_eq!(
            short.init_line(),
            "            sdf_2d: sdf_2d::Shader::init(),",
        );

        let name = "wc_advect_and_transfer_pigment_compute";
        let long = ShaderAtlasField {
            field_name: name.to_string(),
            type_prefix: format!("{name}::"),
        };
        assert_eq!(
            long.init_line(),
            format!("            {name}:\n                {name}::Shader::init(),"),
        );

        for line in long.init_line().lines() {
            assert!(line.len() <= RUSTFMT_MAX_WIDTH, "too wide: {line}");
        }
    }

    /// The templates must emit rustfmt-clean rust. The snapshots record template
    /// output verbatim, so any drift here makes them disagree with the formatted
    /// files an example crate commits.
    #[cfg(not(windows))]
    #[test]
    fn generated_rust_source_is_rustfmt_clean() {
        // rustfmt walks into each `mod` declaration on its own, so passing the
        // crate roots covers every generated file exactly once
        fn collect_module_roots(dir: &Path) -> Vec<PathBuf> {
            let mut roots: Vec<PathBuf> = std::fs::read_dir(dir)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension() == Some(std::ffi::OsStr::new("rs")))
                .collect();
            roots.sort();
            roots
        }

        for fixture in ["shaders", "alignment"] {
            let tmp_dir_path =
                std::env::temp_dir().join(format!("shader-test-{}", uuid::Uuid::new_v4()));

            let config = Config {
                generate_rust_source: true,
                rust_source_dir: tmp_dir_path.join("src"),
                shaders_source_dir: manifest_path(["fixtures", fixture]),
                compiled_shaders_dir: tmp_dir_path.join(relative_path(["shaders", "compiled"])),
                import_root: "crate".to_string(),
                optimization: OptimizationLevel::High,
            };

            write_precompiled_shaders(config).unwrap();

            let rust_files = collect_module_roots(&tmp_dir_path.join("src"));
            assert!(
                !rust_files.is_empty(),
                "fixtures/{fixture} generated no rust — the check below would pass \
                 vacuously",
            );

            // The temp dir is outside the repo, so rustfmt does not find the root
            // rustfmt.toml. That file sets the edition and nothing else.
            let output = std::process::Command::new("rustfmt")
                .args(["--check", "--edition", "2024"])
                .args(&rust_files)
                .output()
                .expect("failed to run rustfmt");

            std::fs::remove_dir_all(&tmp_dir_path).unwrap();

            assert!(
                output.status.success(),
                "fixtures/{fixture} generated rust that rustfmt would reformat:\n{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn compiled_dir_is_regenerated_only_after_every_compile_succeeds() {
        fn copy_dir(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let dest = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_dir(&entry.path(), &dest);
                } else {
                    std::fs::copy(entry.path(), dest).unwrap();
                }
            }
        }

        // name -> content hash, for catching a half-replaced directory
        fn contents(dir: &Path) -> BTreeMap<String, u64> {
            use std::hash::{DefaultHasher, Hash, Hasher};

            let Ok(entries) = std::fs::read_dir(dir) else {
                return BTreeMap::new();
            };
            entries
                .map(|entry| {
                    let entry = entry.unwrap();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let mut hasher = DefaultHasher::new();
                    std::fs::read(entry.path()).unwrap().hash(&mut hasher);
                    (name, hasher.finish())
                })
                .collect()
        }

        let tmp_dir_path =
            std::env::temp_dir().join(format!("shader-test-{}", uuid::Uuid::new_v4()));
        let source_dir = tmp_dir_path.join("source");
        copy_dir(&manifest_path(["fixtures", "shaders"]), &source_dir);

        let compiled_dir = tmp_dir_path.join(relative_path(["shaders", "compiled"]));
        let config = Config {
            generate_rust_source: false,
            rust_source_dir: tmp_dir_path.join("src"),
            shaders_source_dir: source_dir.clone(),
            compiled_shaders_dir: compiled_dir.clone(),
            import_root: "crate".to_string(),
            optimization: OptimizationLevel::High,
        };

        write_precompiled_shaders(config.clone()).unwrap();
        let baseline = contents(&compiled_dir);
        assert!(
            !baseline.is_empty(),
            "the fixture produced no compiled output — the asserts below would \
             pass vacuously",
        );

        // An output whose shader is gone does not survive a clean run.
        let orphan = compiled_dir.join("removed_shader.vert.spv");
        std::fs::write(&orphan, b"stale").unwrap();
        write_precompiled_shaders(config.clone()).unwrap();
        assert!(!orphan.exists(), "orphaned output survived a clean run");
        assert_eq!(contents(&compiled_dir), baseline);

        // A shader that cannot compile aborts before anything is deleted.
        let broken = source_dir.join("broken.shader.slang");
        std::fs::write(&broken, "this is not valid slang").unwrap();
        write_precompiled_shaders(config.clone())
            .expect_err("a shader that cannot compile must fail the run");
        assert_eq!(
            contents(&compiled_dir),
            baseline,
            "a failed compile must leave the previous outputs untouched",
        );

        std::fs::remove_dir_all(&tmp_dir_path).unwrap();
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
            shaders_source_dir: manifest_path(["fixtures", "alignment"]),
            compiled_shaders_dir: tmp_dir_path.join(relative_path(["shaders", "compiled"])),
            import_root: "crate".to_string(),
            optimization: OptimizationLevel::High,
        };

        write_precompiled_shaders(config).unwrap();

        // Run cargo check on the generated code to verify it compiles
        // this is primarily to test the generated const layout assertions
        {
            use std::fmt::Write;

            let check_crate = manifest_path(["fixtures", "check_crate"]);
            let check_crate_src = check_crate.join("src/generated");
            let check_crate_shaders = check_crate.join("shaders/compiled");

            std::fs::create_dir_all(&check_crate_src).unwrap();
            std::fs::create_dir_all(&check_crate_shaders).unwrap();

            // Copy .rs files and build mod.rs
            let mut mod_contents = String::new();
            let shader_atlas_dir = tmp_dir_path.join("src/generated/shader_atlas");
            let mut atlas_entries: Vec<_> = std::fs::read_dir(&shader_atlas_dir)
                .unwrap()
                .map(|entry| entry.unwrap())
                .collect();
            atlas_entries.sort_by_key(|entry| entry.file_name());
            for entry in atlas_entries {
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

        gather_struct_defs(&field, &mut GeneratedTypeDefs::default(), None);
    }

    #[test]
    fn parse_array_type_round_trips_emitted_arrays() {
        assert_eq!(
            parse_array_type("[glam::UVec4; 8]"),
            Some(("glam::UVec4", 8))
        );
        assert_eq!(parse_array_type("[glam::Vec4; 4]"), Some(("glam::Vec4", 4)));
        assert_eq!(parse_array_type("glam::Vec4"), None);
        assert_eq!(parse_array_type("[glam::Vec4]"), None);
    }

    #[test]
    fn array_fields_use_element_alignment() {
        assert_eq!(field_alignment_by_name("[glam::IVec4; 3]"), 16);
        assert_eq!(rust_type_alignment("[glam::Vec4; 4]"), Some(16));
        assert_eq!(rust_type_alignment("[glam::UVec4; 8]"), Some(4));
    }

    // an example of incorrect generated code we should catch
    fn dropped_field_occupying_bytes() -> StructField {
        StructField::Resource(ResourceStructField {
            field_name: "dropped".to_string(),
            binding: Binding::Uniform(OffsetSizeBinding { offset: 0, size: 8 }),
            resource_shape: ResourceShape::Texture2D,
            result_type: ResourceResultType::Scalar(ScalarResultType {
                scalar_type: ScalarType::Float32,
            }),
        })
    }

    #[test]
    #[should_panic(expected = "field 'dropped' occupies bytes in the GPU struct")]
    fn std140_rejects_dropping_a_field_that_occupies_bytes() {
        let field = dropped_field_occupying_bytes();
        generate_std140_struct_fields(&[field], &mut GeneratedTypeDefs::default());
    }

    #[test]
    #[should_panic(expected = "field 'dropped' occupies bytes in the GPU struct")]
    fn std430_rejects_dropping_a_field_that_occupies_bytes() {
        let field = dropped_field_occupying_bytes();
        generate_std430_struct_fields(&[field], &mut GeneratedTypeDefs::default());
    }

    /// The `(member index, byte offset)` pairs SPIR-V decorates a struct type with,
    /// sorted by member. This is the *emitted* layout, as opposed to the reflected
    /// one the generated `offset_of!` asserts pin.
    #[cfg(not(windows))]
    fn member_offsets(module: &rspirv::dr::Module, struct_id: u32) -> Vec<(u32, u32)> {
        use rspirv::dr::Operand;
        use rspirv::spirv::{Decoration, Op};

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
    }

    /// The type ids of a struct type's members, in declaration order.
    #[cfg(not(windows))]
    fn member_type_ids(module: &rspirv::dr::Module, struct_id: u32) -> Vec<u32> {
        use rspirv::dr::Operand;
        use rspirv::spirv::Op;

        module
            .types_global_values
            .iter()
            .find(|inst| inst.class.opcode == Op::TypeStruct && inst.result_id == Some(struct_id))
            .expect("struct type not found")
            .operands
            .iter()
            .map(|operand| match operand {
                Operand::IdRef(id) => *id,
                _ => unreachable!("struct member operands are type ids"),
            })
            .collect()
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

        let search_path = manifest_path(["fixtures", "alignment"]);
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

        let member_offsets = |struct_id: u32| member_offsets(&module, struct_id);

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
        let member_types = member_type_ids(&module, pointee_struct_id);
        // natural layout would put InnerA.v at 4
        assert_eq!(member_offsets(member_types[4]), vec![(0, 0), (1, 16)]);
        assert_eq!(member_offsets(member_types[6]), vec![(0, 0), (1, 8)]);
    }

    /// The push-block twin of `pointer_pointee_spirv_layout`. Reflection reporting
    /// std430 and slang *emitting* std430 are two different claims: the generated
    /// `offset_of!` asserts only pin the first, since they are generated from the
    /// same reflection they would agree with. A std140-shaped emission would round
    /// `DrawInner` up to 16 and shift every member after it.
    #[cfg(not(windows))]
    #[test]
    fn push_constant_spirv_layout() {
        use rspirv::dr::Operand;
        use rspirv::spirv::{Op, StorageClass};

        let search_path = manifest_path(["fixtures", "alignment"]);
        let reflected =
            prepare_reflected_shader("push_constants.shader.slang", search_path.to_str().unwrap())
                .unwrap();

        // the fragment stage is the one that reads every member of the block
        let module = rspirv::dr::load_bytes(&reflected.fragment_shader.shader_bytecode)
            .expect("failed to parse SPIR-V");

        let push_constant_ptr_type = module
            .types_global_values
            .iter()
            .find_map(|inst| match (inst.class.opcode, inst.operands.first()) {
                (Op::Variable, Some(Operand::StorageClass(StorageClass::PushConstant))) => {
                    inst.result_type
                }
                _ => None,
            })
            .expect("no PushConstant variable in SPIR-V");

        let block_struct_id = module
            .types_global_values
            .iter()
            .find(|inst| {
                inst.class.opcode == Op::TypePointer
                    && inst.result_id == Some(push_constant_ptr_type)
            })
            .and_then(|inst| match inst.operands.as_slice() {
                [
                    Operand::StorageClass(StorageClass::PushConstant),
                    Operand::IdRef(pointee),
                ] => Some(*pointee),
                _ => None,
            })
            .expect("the PushConstant variable's type is not a pointer to a block");

        // DrawConstants under std430 (see push_constants.shader.slang)
        assert_eq!(
            member_offsets(&module, block_struct_id),
            vec![(0, 0), (1, 8), (2, 16), (3, 24), (4, 32)],
        );

        // the nested struct is the std430/std140 discriminator: std140 would give
        // it size 16 and push `tail` from 24 to 32
        let member_types = member_type_ids(&module, block_struct_id);
        assert_eq!(member_offsets(&module, member_types[2]), vec![(0, 0)]);
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

    #[cfg(not(windows))]
    #[test]
    fn structured_buffer_is_rejected() {
        let tmp_dir = std::env::temp_dir().join(format!("shader-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let source = r#"#language slang 2026

module structured_buffer;

struct Item {
    float4 value;
}

struct Params {
    StructuredBuffer<Item> items;
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
        std::fs::write(tmp_dir.join("structured_buffer.shader.slang"), source).unwrap();

        let result =
            prepare_reflected_shader("structured_buffer.shader.slang", tmp_dir.to_str().unwrap());

        std::fs::remove_dir_all(&tmp_dir).ok();

        let err = match result {
            Ok(_) => panic!("a StructuredBuffer field must be rejected"),
            Err(err) => err,
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("StructuredBuffer") && message.contains("BDA pointer"),
            "unexpected error message: {message}"
        );
    }

    // Access.Immutable is only sound with the ImmutableBufferHandle allocation
    // kind, so reflection must map it to its own PointerAccess variant rather
    // than falling through to ReadWrite (which would happen silently if slang's
    // full_name() stopped printing the Access generic arg as `Access.Immutable`).
    #[cfg(not(windows))]
    #[test]
    fn immutable_pointer_is_reflected() {
        let tmp_dir = std::env::temp_dir().join(format!("shader-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let source = r#"#language slang 2026

module immutable_pointer;

struct Item {
    float4 value;
}

struct Params {
    Ptr<Item, Access.Immutable, AddressSpace.Device, Std430DataLayout> items;
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
        std::fs::write(tmp_dir.join("immutable_pointer.shader.slang"), source).unwrap();

        let result =
            prepare_reflected_shader("immutable_pointer.shader.slang", tmp_dir.to_str().unwrap());

        std::fs::remove_dir_all(&tmp_dir).ok();

        let reflected = result.expect("an Access.Immutable pointer field must be accepted");
        let block = only_parameter_block(&reflected);
        let access = block
            .element_type
            .fields
            .iter()
            .find_map(|field| match field {
                StructField::Pointer(ptr) if ptr.field_name == "items" => Some(ptr.access),
                _ => None,
            })
            .expect("pointer field 'items' not reflected");
        assert_eq!(access, PointerAccess::Immutable);
    }

    // Slang lays an enum out as its tag type, so an enum field's *layout* kind is
    // Scalar and reflection would silently degrade it to a bare uint. This pins
    // the declared-type lookup that recovers the enum, including a non-contiguous
    // case list (which is what makes the values worth carrying at all).
    #[cfg(not(windows))]
    #[test]
    fn enum_field_is_reflected() {
        let tmp_dir = std::env::temp_dir().join(format!("shader-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let source = r#"#language slang 2026

module enum_field;

enum Mode : uint {
    First = 0,
    Skipped = 7,
}

struct Params {
    float scale;
    Mode mode;
}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(params.scale, 0.0, 0.0, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    if (params.mode == Mode.Skipped) {
        return float4(0.0);
    }
    return float4(1.0);
}
"#;
        std::fs::write(tmp_dir.join("enum_field.shader.slang"), source).unwrap();

        let result = prepare_reflected_shader("enum_field.shader.slang", tmp_dir.to_str().unwrap());

        std::fs::remove_dir_all(&tmp_dir).ok();

        let reflected = result.expect("an enum field must be accepted");
        let block = only_parameter_block(&reflected);
        let enum_field = block
            .element_type
            .fields
            .iter()
            .find_map(|field| match field {
                StructField::Enum(e) if e.field_name == "mode" => Some(e),
                _ => None,
            })
            .expect("enum field 'mode' not reflected");

        assert_eq!(enum_field.enum_type.type_name, "Mode");
        assert_eq!(enum_field.enum_type.tag_type, EnumTagType::Uint32);
        assert_eq!(
            enum_field.enum_type.cases,
            vec![
                EnumCase {
                    name: "First".to_string(),
                    value: 0
                },
                EnumCase {
                    name: "Skipped".to_string(),
                    value: 7
                },
            ]
        );
        // the enum still occupies exactly its tag type's 4 bytes, right after the float
        let Binding::Uniform(binding) = &enum_field.binding else {
            panic!("enum field must have a uniform binding");
        };
        assert_eq!((binding.offset, binding.size), (4, 4));
    }

    // An array field's declared type is an Array whose *element* is the enum, so
    // the enum guard doesn't fire and it falls through to the array path. The
    // element's layout is its tag's (a scalar), which the vec4-only array gate
    // rejects. Pinned because the failure mode if it ever changes is a silent
    // degrade to [u32; N], not a panic.
    #[cfg(not(windows))]
    #[test]
    fn enum_arrays_are_rejected() {
        let tmp_dir = std::env::temp_dir().join(format!("shader-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let source = r#"#language slang 2026

module enum_array;

enum Mode : uint {
    First = 0,
    Second = 1,
}

struct Params {
    Mode modes[4];
}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(float(uint(params.modes[0])), 0.0, 0.0, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
"#;
        std::fs::write(tmp_dir.join("enum_array.shader.slang"), source).unwrap();

        let result = prepare_reflected_shader("enum_array.shader.slang", tmp_dir.to_str().unwrap());

        std::fs::remove_dir_all(&tmp_dir).ok();

        let err = match result {
            Ok(_) => panic!("an array of enums must be rejected"),
            Err(err) => err,
        };
        let message = format!("{err:#}");
        assert!(
            message.contains("array field 'modes'")
                && message.contains("only float4/int4/uint4 element arrays are supported"),
            "unexpected error message: {message}"
        );
    }

    /// The first global parameter of a reflected shader, which every caller here
    /// declares as a `ParameterBlock`.
    #[cfg(not(windows))]
    fn only_parameter_block(reflected: &ReflectedShader) -> &ParameterBlockGlobalParameter {
        match &reflected.reflection_json.global_parameters[0] {
            GlobalParameter::ParameterBlock(block) => block,
            GlobalParameter::PushConstant(push) => {
                panic!(
                    "expected a ParameterBlock global, got push block '{}'",
                    push.parameter_name
                )
            }
        }
    }

    /// Compiles an inline shader that is expected to be rejected, and returns the
    /// rendered error. The fixture lives in a temp dir because every shader in
    /// shaders/test must compile.
    #[cfg(not(windows))]
    fn reflect_rejected_shader(module_name: &str, source: &str) -> String {
        let tmp_dir = std::env::temp_dir().join(format!("shader-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let file_name = format!("{module_name}.shader.slang");
        std::fs::write(tmp_dir.join(&file_name), source).unwrap();

        let result = prepare_reflected_shader(&file_name, tmp_dir.to_str().unwrap());

        std::fs::remove_dir_all(&tmp_dir).ok();

        match result {
            Ok(_) => panic!("'{module_name}' must be rejected"),
            Err(err) => format!("{err:#}"),
        }
    }

    /// The compute twin of [`reflect_rejected_shader`].
    #[cfg(not(windows))]
    fn reflect_rejected_compute_shader(module_name: &str, source: &str) -> String {
        let tmp_dir = std::env::temp_dir().join(format!("shader-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let file_name = format!("{module_name}.compute.slang");
        std::fs::write(tmp_dir.join(&file_name), source).unwrap();

        let result = prepare_reflected_compute_shader(&file_name, tmp_dir.to_str().unwrap());

        std::fs::remove_dir_all(&tmp_dir).ok();

        match result {
            Ok(_) => panic!("'{module_name}' must be rejected"),
            Err(err) => format!("{err:#}"),
        }
    }

    /// Wraps an enum declaration in the minimal shader that uses it as a
    /// ParameterBlock field, so reflection actually reaches the enum.
    #[cfg(not(windows))]
    fn enum_fixture_source(module_name: &str, enum_decl: &str) -> String {
        format!(
            r#"#language slang 2026

module {module_name};

{enum_decl}

struct Params {{
    Bad bad;
}}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {{
    return float4(1.0);
}}

[shader("fragment")]
float4 fragMain() : SV_Target {{
    return float4(1.0);
}}
"#
        )
    }

    #[cfg(not(windows))]
    #[test]
    fn duplicate_enum_values_are_rejected() {
        let message = reflect_rejected_shader(
            "duplicate_enum_values",
            &enum_fixture_source(
                "duplicate_enum_values",
                "enum Bad : uint {\n    First = 3,\n    Second = 3,\n}",
            ),
        );
        assert!(
            message.contains("enum 'Bad'")
                && message.contains("share the value 3")
                && message.contains("duplicate discriminants"),
            "unexpected error message: {message}"
        );
    }

    // slang accepts `enum Bad : uint {}`, but Default and TryFrom both need a
    // first case, so codegen would emit an uninhabited enum in a GPU struct.
    #[cfg(not(windows))]
    #[test]
    fn empty_enums_are_rejected() {
        let message = reflect_rejected_shader(
            "empty_enum",
            &enum_fixture_source("empty_enum", "enum Bad : uint {\n}"),
        );
        assert!(
            message.contains("enum 'Bad'") && message.contains("has no cases"),
            "unexpected error message: {message}"
        );
    }

    // slang accepts an anonymous enum and synthesizes `SLANG_anonymous_N` for it,
    // which would generate a Rust type name that clippy's non_camel_case_types
    // rejects — and that no caller could meaningfully name anyway.
    #[cfg(not(windows))]
    #[test]
    fn anonymous_enums_are_rejected() {
        let source = r#"#language slang 2026

module anon_enum;

struct Params {
    enum { A = 0 } bad;
}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
"#;
        let message = reflect_rejected_shader("anon_enum", source);
        assert!(
            message.contains("enum field 'bad'") && message.contains("anonymous enum type"),
            "unexpected error message: {message}"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_enum_tag_is_rejected() {
        let message = reflect_rejected_shader(
            "unsupported_enum_tag",
            &enum_fixture_source(
                "unsupported_enum_tag",
                "enum Bad : uint64_t {\n    Only = 0,\n}",
            ),
        );
        assert!(
            message.contains("enum 'Bad'") && message.contains("unsupported tag type"),
            "unexpected error message: {message}"
        );
    }

    // A narrow tag lays out fine, but reading one makes slang emit Int8/Int16 and
    // UniformAndStorageBuffer{8,16}BitAccess — optional Vulkan feature bits that
    // create_logical_device deliberately does not request. Rejecting at reflection
    // keeps that a compile-time error instead of a device-creation failure.
    #[cfg(not(windows))]
    #[test]
    fn sub_32_bit_enum_tags_are_rejected() {
        for tag in ["uint8_t", "uint16_t"] {
            let message = reflect_rejected_shader(
                "narrow_enum_tag",
                &enum_fixture_source(
                    "narrow_enum_tag",
                    &format!("enum Bad : {tag} {{\n    Only = 0,\n}}"),
                ),
            );
            assert!(
                message.contains("enum 'Bad'")
                    && message.contains("sub-32-bit tag type")
                    && message.contains("8/16-bit storage device features"),
                "unexpected error message for {tag}: {message}"
            );
        }
    }

    // The guard that recovers the enum sits before the layout-kind match, so it
    // also intercepts vertex inputs, where there is no uniform binding to carry.
    #[cfg(not(windows))]
    #[test]
    fn enum_vertex_inputs_are_rejected() {
        let source = r#"#language slang 2026

module enum_vertex_input;

enum Bad : uint {
    Only = 0,
}

struct Vertex {
    float3 position;
    Bad bad;
}

[shader("vertex")]
float4 vertMain(Vertex vertex) : SV_Position {
    return float4(vertex.position, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
"#;
        let message = reflect_rejected_shader("enum_vertex_input", source);
        assert!(
            message.contains("enum field 'bad'") && message.contains("not vertex inputs"),
            "unexpected error message: {message}"
        );
    }

    // The accepted path is covered by the handle_* alignment fixtures; what
    // stays here are the shapes the bindless heap cannot serve.
    //
    // A separate Texture2D.Handle or SamplerState.Handle lights up slang's heap
    // bindings 2 and 0, and DescriptorHeap creates bindings 1 (combined image
    // sampler) and 3 (storage image). Accepting one would produce a shader
    // reading a descriptor array that was never declared, with no reflection or
    // validation signal.
    #[cfg(not(windows))]
    #[test]
    fn unsupported_handle_shapes_are_rejected() {
        for (module, decl) in [
            ("texture_handle", "Texture2D.Handle tex;"),
            ("sampler_handle", "SamplerState.Handle samp;"),
        ] {
            let source = format!(
                r#"#language slang 2026

module {module};

struct Params {{
    {decl}
    float4 tint;
}}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {{
    return float4(1.0);
}}

[shader("fragment")]
float4 fragMain() : SV_Target {{
    return params.tint;
}}
"#
            );
            let message = reflect_rejected_shader(module, &source);
            assert!(
                message.contains("DescriptorHandle<")
                    && message.contains(
                        "only Sampler2D.Handle and RWTexture2D.Handle texture handles \
                         are supported"
                    ),
                "unexpected error message for {module}: {message}"
            );
        }
    }

    // An array's declared full_name() is the element's with `[N]` appended, so
    // the prefix guard fires on an array of handles too — which is why the
    // accept path has to split that suffix off before trusting the prefix.
    // Worth pinning: the TypeKind::Array arm never recurses into
    // reflect_struct_fields, so if that suffix form ever changes the only
    // remaining gate is the generic vec4-only array check — a much vaguer error
    // for the same mistake.
    #[cfg(not(windows))]
    #[test]
    fn handle_arrays_are_rejected() {
        let source = r#"#language slang 2026

module handle_array;

struct Params {
    Sampler2D.Handle textures[4];
}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(1.0);
}

[shader("fragment")]
float4 fragMain(float2 uv) : SV_Target {
    return params.textures[0].Sample(uv);
}
"#;
        let message = reflect_rejected_shader("handle_array", source);
        assert!(
            message.contains("field 'textures'")
                && message.contains("DescriptorHandle<Sampler2D<vector<float,4>>>[4]")
                && message.contains("arrays of texture handles are not supported"),
            "unexpected error message: {message}"
        );
        assert!(
            message.contains("16 under std140 but 8 under std430"),
            "the message must name the stride reason, not just refuse: {message}"
        );
    }

    // The handle guard sits before the layout-kind match and is deliberately not
    // gated on the binding, so it also intercepts vertex inputs — where there is
    // no uniform binding and the Vector arm would have accepted the uint2.
    #[cfg(not(windows))]
    #[test]
    fn handle_vertex_inputs_are_rejected() {
        let source = r#"#language slang 2026

module handle_vertex_input;

struct Vertex {
    float3 position;
    Sampler2D.Handle tex;
}

[shader("vertex")]
float4 vertMain(Vertex vertex) : SV_Position {
    return float4(vertex.position, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
"#;
        let message = reflect_rejected_shader("handle_vertex_input", source);
        assert!(
            message.contains("handle field 'tex'") && message.contains("not vertex inputs"),
            "unexpected error message: {message}"
        );
    }

    // The push-constant gate keys on the parameter's *category*, not just its type
    // kind: a plain `ConstantBuffer<T>` global has the same TypeKind::ConstantBuffer
    // but is a descriptor-backed UBO with no push-constant range, which codegen
    // would emit as a std430 struct nothing ever writes.
    #[cfg(not(windows))]
    #[test]
    fn a_plain_constant_buffer_global_is_rejected() {
        let source = r#"#language slang 2026

module plain_constant_buffer;

struct Params {
    float4 tint;
}

ConstantBuffer<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return params.tint;
}
"#;
        let message = reflect_rejected_shader("plain_constant_buffer", source);
        assert!(
            message.contains("non-ParameterBlock global: params"),
            "unexpected error message: {message}"
        );
    }

    // add_push_constatant_range_for_constant_buffer hard-codes offset 0 on the
    // assumption slang emits one range per shader, so a second block would overlap
    // the first rather than follow it.
    #[cfg(not(windows))]
    #[test]
    fn a_second_push_constant_block_is_rejected() {
        let source = r#"#language slang 2026

module two_push_blocks;

struct First {
    float a;
}

struct Second {
    float b;
}

[[vk::push_constant]] ConstantBuffer<First> one;
[[vk::push_constant]] ConstantBuffer<Second> two;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(one.a, two.b, 0.0, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
"#;
        let message = reflect_rejected_shader("two_push_blocks", source);
        assert!(
            message.contains("push constant block 'two'")
                && message.contains("'one' is already declared")
                && message.contains("only one push constant block per shader"),
            "unexpected error message: {message}"
        );
    }

    // A descriptor in a push block reflects with a DescriptorTableSlot binding, so
    // assert_no_occupied_bytes_dropped does not fire and the reflected block size —
    // uniform bytes only — still matches the generated struct. Without this gate
    // the field would simply disappear, with nothing downstream saying so.
    #[cfg(not(windows))]
    #[test]
    fn a_descriptor_in_a_push_block_is_rejected() {
        let source = r#"#language slang 2026

module push_block_descriptor;

struct DrawConstants {
    float scale;
    Texture2D<float4> albedo;
}

[[vk::push_constant]] ConstantBuffer<DrawConstants> draw;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(draw.scale);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return draw.albedo.Load(int3(0, 0, 0));
}
"#;
        let message = reflect_rejected_shader("push_block_descriptor", source);
        assert!(
            message.contains("push constant block 'draw'")
                && message.contains("field 'albedo'")
                && message.contains("cannot live in a push block"),
            "unexpected error message: {message}"
        );
        assert!(
            message.contains("Sampler2D.Handle") && message.contains("Addr<T>"),
            "the message must point at the alternatives: {message}"
        );
    }

    // Nothing in the dispatch path calls cmd_push_constants, so a compute push
    // block would reflect and generate cleanly while never being written — the
    // shader would read whatever the last graphics draw happened to leave behind.
    #[cfg(not(windows))]
    #[test]
    fn a_compute_push_constant_block_is_rejected() {
        let source = r#"#language slang 2026

module compute_push_block;

struct DrawConstants {
    uint count;
}

struct Params {
    LayoutPtr<DrawConstants, Std430DataLayout> data;
}

ParameterBlock<Params> params;
[[vk::push_constant]] ConstantBuffer<DrawConstants> draw;

[numthreads(64, 1, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    params.data[dispatchThreadID.x].count = draw.count;
}
"#;
        let message = reflect_rejected_compute_shader("compute_push_block", source);
        assert!(
            message.contains("push constant block 'draw'")
                && message.contains("only supported in graphics shaders"),
            "unexpected error message: {message}"
        );
    }

    // Slang promotes a `uniform` entry point parameter to a push constant range
    // (a2-01-spirv-target-specific.md), and it never reaches globalParameters — so
    // every guard around [[vk::push_constant]] globals is blind to it. Codegen reads
    // only the *vertex* entry point's parameters, so this range gets no generated
    // type, no PushConstants alias and no Resources entry, while a real
    // vk::PushConstantRange still reaches the pipeline layout.
    #[cfg(not(windows))]
    #[test]
    fn a_fragment_entry_point_uniform_is_rejected() {
        let source = r#"#language slang 2026

module frag_uniform;

struct Tint {
    float scale;
}

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(1.0);
}

[shader("fragment")]
float4 fragMain(uniform Tint tint) : SV_Target {
    return float4(tint.scale);
}
"#;
        let message = reflect_rejected_shader("frag_uniform", source);
        assert!(
            message.contains("entry point parameter 'tint' on 'fragMain'")
                && message.contains("implicit push constant range"),
            "unexpected error message: {message}"
        );
        assert!(
            message.contains("[[vk::push_constant]] ConstantBuffer<T>"),
            "the message must point at the alternative: {message}"
        );
    }

    // Two entry point uniforms reflect as two ranges — {vertex, 0, N} and
    // {fragment, 0, N} — with no global block involved at all. Phase 8's
    // "at most one push constant range" assert rests on this being rejected;
    // the ranges are legal Vulkan (distinct stages may overlap), just invisible.
    #[cfg(not(windows))]
    #[test]
    fn entry_point_uniforms_on_both_stages_are_rejected() {
        let source = r#"#language slang 2026

module both_stage_uniforms;

struct VertConstants {
    float scale;
}

struct FragConstants {
    float tint;
}

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID, uniform VertConstants vc) : SV_Position {
    return float4(vc.scale);
}

[shader("fragment")]
float4 fragMain(uniform FragConstants fc) : SV_Target {
    return float4(fc.tint);
}
"#;
        let message = reflect_rejected_shader("both_stage_uniforms", source);
        assert!(
            message.contains("entry point parameter 'vc' on 'vertMain'"),
            "unexpected error message: {message}"
        );
    }

    // The invalid case: the global block reflects as one `all`-stage range at
    // offset 0 and the entry point uniform as a `fragment` range at offset 0.
    // `all` includes FRAGMENT, so two ranges cover the same stage —
    // VUID-VkPipelineLayoutCreateInfo-pPushConstantRanges-00292, a
    // vkCreatePipelineLayout failure with nothing upstream explaining it.
    #[cfg(not(windows))]
    #[test]
    fn an_entry_point_uniform_beside_a_push_block_is_rejected() {
        let source = r#"#language slang 2026

module block_plus_uniform;

struct DrawConstants {
    float scale;
}

struct Tint {
    float tint;
}

[[vk::push_constant]] ConstantBuffer<DrawConstants> draw;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(draw.scale);
}

[shader("fragment")]
float4 fragMain(uniform Tint t) : SV_Target {
    return float4(t.tint);
}
"#;
        let message = reflect_rejected_shader("block_plus_uniform", source);
        assert!(
            message.contains("entry point parameter 't' on 'fragMain'"),
            "the entry point guard must fire, not the global one: {message}"
        );
    }

    // The pre-existing bug, older than push constants and reachable without them:
    // collect_graphics_shader_data treats *any* struct parameter on the vertex
    // entry point as the vertex input type. Every field here has a vk::Format, so
    // the `todo!()` in that format match does not fire and this would silently
    // generate an `impl VertexDescription` for a push constant block. The guard
    // returning an Err — rather than the test panicking — is what proves the
    // rejection came from the binding category and not from the format match.
    #[cfg(not(windows))]
    #[test]
    fn a_vertex_entry_point_uniform_is_rejected() {
        let source = r#"#language slang 2026

module vert_uniform;

struct Small {
    float3 offset;
    float2 uv_scale;
    uint flags;
}

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID, uniform Small s) : SV_Position {
    return float4(s.offset, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
"#;
        let message = reflect_rejected_shader("vert_uniform", source);
        assert!(
            message.contains("entry point parameter 's' on 'vertMain'")
                && message.contains("implicit push constant range"),
            "unexpected error message: {message}"
        );
    }

    // The compute twin. reflect_compute_entry_point discards entry point parameters
    // entirely — the promotion applies there too, and the dispatch path has no
    // cmd_push_constants to write the range with.
    #[cfg(not(windows))]
    #[test]
    fn a_compute_entry_point_uniform_is_rejected() {
        let source = r#"#language slang 2026

module compute_uniform;

struct DrawConstants {
    uint count;
}

struct Params {
    LayoutPtr<DrawConstants, Std430DataLayout> data;
}

ParameterBlock<Params> params;

[numthreads(64, 1, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID, uniform DrawConstants draw) {
    params.data[dispatchThreadID.x].count = draw.count;
}
"#;
        let message = reflect_rejected_compute_shader("compute_uniform", source);
        assert!(
            message.contains("entry point parameter 'draw' on 'computeMain'")
                && message.contains("implicit push constant range"),
            "unexpected error message: {message}"
        );
    }

    // The descriptor half of the same hole, and the reason the guard is an
    // allow-list: a uniform parameter holding a *resource* carries no uniform
    // bytes, so no push constant range is created and a check for uniform bytes
    // does not fire. Slang gives it a descriptor set instead. Measured before the
    // allow-list landed: this reflected as `{binding 0, texture, compute}` in set
    // 0, with the ParameterBlock displaced to set 1 — so it corrupts the set
    // indices codegen does know about, on top of declaring one it cannot fill.
    #[cfg(not(windows))]
    #[test]
    fn a_compute_entry_point_descriptor_is_rejected() {
        let source = r#"#language slang 2026

module compute_descriptor;

struct Cell {
    uint value;
}

struct Params {
    LayoutPtr<Cell, Std430DataLayout> data;
}

ParameterBlock<Params> params;

[numthreads(64, 1, 1)]
[shader("compute")]
void computeMain(uint3 tid : SV_DispatchThreadID, uniform Texture2D<float4> tex) {
    params.data[tid.x].value = uint(tex.Load(int3(0, 0, 0)).x);
}
"#;
        let message = reflect_rejected_compute_shader("compute_descriptor", source);
        assert!(
            message.contains("entry point parameter 'tex' on 'computeMain'")
                && message.contains("descriptor that nothing binds"),
            "unexpected error message: {message}"
        );
    }

    // A struct wrapping the descriptor is the same hole with a different shape:
    // its only category is the descriptor's, so nothing about the parameter says
    // "uniform". On the vertex entry point it is also the vertex-input confusion
    // again — collect_graphics_shader_data would take a struct parameter here as
    // the vertex input type.
    #[cfg(not(windows))]
    #[test]
    fn a_vertex_entry_point_descriptor_struct_is_rejected() {
        let source = r#"#language slang 2026

module vert_descriptor;

struct TexHolder {
    Texture2D<float4> tex;
}

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID, uniform TexHolder th) : SV_Position {
    return th.tex.Load(int3(0, 0, 0));
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
"#;
        let message = reflect_rejected_shader("vert_descriptor", source);
        assert!(
            message.contains("entry point parameter 'th' on 'vertMain'")
                && message.contains("descriptor that nothing binds"),
            "unexpected error message: {message}"
        );
        assert!(
            message.contains("ParameterBlock<T> global"),
            "the message must point at the alternative: {message}"
        );
    }

    // 128 bytes is the vulkan-guaranteed maxPushConstantsSize. The generated code
    // asserts this too, but a failing const assert reports only "evaluation of
    // constant value failed" — no shader, no block, no size.
    #[test]
    #[should_panic(expected = "push constant block 'TooBig' (huge.shader.slang) is 144 bytes")]
    fn an_oversized_push_constant_block_is_rejected() {
        assert_push_constant_size("TooBig", 144, "huge.shader.slang");
    }

    #[test]
    fn a_push_constant_block_at_the_budget_is_accepted() {
        assert_push_constant_size("Exact", MAX_PUSH_CONSTANT_BYTES, "exact.shader.slang");
    }

    // heck folds SCREAMING_CASE and UpperCamel onto the same variant name;
    // rustc's own duplicate-variant error names neither slang case.
    #[test]
    #[should_panic(expected = "both generate the Rust variant 'WetAreaMask'")]
    fn colliding_enum_variant_names_are_rejected() {
        try_add_enum_def(
            &mut Vec::new(),
            &EnumFieldType {
                type_name: "Bad".to_string(),
                tag_type: EnumTagType::Uint32,
                cases: vec![
                    EnumCase {
                        name: "WET_AREA_MASK".to_string(),
                        value: 0,
                    },
                    EnumCase {
                        name: "WetAreaMask".to_string(),
                        value: 1,
                    },
                ],
            },
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
        let tmp_prefix = format!("shader-test-{}", uuid::Uuid::new_v4());
        let tmp_dir_path = std::env::temp_dir().join(tmp_prefix);

        let config = Config {
            generate_rust_source: false,
            rust_source_dir: tmp_dir_path.join("src"),
            shaders_source_dir: manifest_path(["fixtures", "shaders"]),
            compiled_shaders_dir: tmp_dir_path.join(relative_path(["shaders", "compiled"])),
            import_root: "crate".to_string(),
            // This test uses -O0 on purpose.
            // The goal is to catch unintentionally added branches,
            // but the optimizer can obscure this via inlining decisions.
            optimization: OptimizationLevel::None,
        };
        let compiled_dir = config.compiled_shaders_dir.clone();

        write_precompiled_shaders(config).unwrap();

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
