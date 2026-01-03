use std::ffi::CString;

use shader_slang as slang;

pub mod atlas;
pub mod build_tasks;
pub mod json;
mod reflection;

use json::*;

/// whether to use column-major or row-major matricies with slang
const COLUMN_MAJOR: bool = false;

pub struct ReflectedShader {
    pub vertex_shader: CompiledShader,
    pub fragment_shader: CompiledShader,
    pub reflection_json: ReflectionJson,
}

fn prepare_reflected_shader(source_file_name: &str) -> anyhow::Result<ReflectedShader> {
    let global_session = slang::GlobalSession::new().unwrap();
    let search_path = CString::new("shaders/source").unwrap();

    let session_options = slang::CompilerOptions::default()
        .vulkan_use_entry_point_name(true)
        .language(slang::SourceLanguage::Slang)
        .optimization(slang::OptimizationLevel::High)
        .emit_spirv_directly(true);
    let session_options = if COLUMN_MAJOR {
        session_options.matrix_layout_column(true)
    } else {
        session_options.matrix_layout_row(true)
    };

    let target_desc = slang::TargetDesc::default()
        .format(slang::CompileTarget::Spirv)
        .profile(global_session.find_profile("glsl_450+spirv_1_6"));

    let targets = [target_desc];
    let search_paths = [search_path.as_ptr()];
    let session_desc = slang::SessionDesc::default()
        .targets(&targets)
        .search_paths(&search_paths)
        .options(&session_options);

    let session = global_session.create_session(&session_desc).unwrap();

    let shader_module = session.load_module(source_file_name)?;

    let cpu_constants_module_src = format!(
        r#"
        #language slang 2026
        module cpu_constants;

        export static const bool columnMajor = {COLUMN_MAJOR};
        "#,
    );
    let cpu_constants_module = session.load_module_from_source_string(
        "cpu_constants",
        "cpu_constants.slang",
        &cpu_constants_module_src,
    )?;

    // the examples have 1 vert and 1 frag shader
    debug_assert!(shader_module.entry_points().len() == 2);

    let mut components = vec![shader_module.clone().into()];
    let mut vertex_shader: Option<CompiledShader> = None;
    let mut fragment_shader: Option<CompiledShader> = None;
    for entry_point in shader_module.entry_points() {
        let compiled_shader = compile_shader(
            &entry_point,
            &session,
            &shader_module,
            &cpu_constants_module,
        )?;

        if compiled_shader.stage == slang::Stage::Vertex {
            vertex_shader = Some(compiled_shader)
        } else if compiled_shader.stage == slang::Stage::Fragment {
            fragment_shader = Some(compiled_shader)
        }

        components.push(entry_point.clone().into());
    }

    let vertex_shader = vertex_shader
        .unwrap_or_else(|| panic!("failed to load vertex entry point for: {source_file_name}"));
    let fragment_shader = fragment_shader
        .unwrap_or_else(|| panic!("failed to load fragment entry point for: {source_file_name}"));

    let program = session.create_composite_component_type(&components)?;
    let linked_program = program.link()?;
    let program_layout = linked_program.layout(0)?;

    let reflection_json = reflection::reflection_json(source_file_name, program_layout)?;

    let reflected_shader = ReflectedShader {
        vertex_shader,
        fragment_shader,
        reflection_json,
    };

    Ok(reflected_shader)
}

#[cfg(debug_assertions)]
pub fn dev_compile_slang_shaders(source_file_name: &str) -> anyhow::Result<ReflectedShader> {
    prepare_reflected_shader(source_file_name)
}

pub struct CompiledShader {
    pub entry_point_name: CString,
    pub stage: slang::Stage,
    pub shader_bytecode: Vec<u8>,
}

impl CompiledShader {
    /// converts compiled spv to vulkan-readable u32s
    #[cfg(debug_assertions)]
    pub fn spv_bytes(&self) -> Result<Vec<u32>, std::io::Error> {
        let byte_reader = &mut std::io::Cursor::new(self.shader_bytecode.as_slice());
        ash::util::read_spv(byte_reader)
    }
}

impl std::fmt::Debug for CompiledShader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledShader")
            .field("entry_point_name", &self.entry_point_name)
            .field("stage", &self.stage)
            .finish()
    }
}

fn compile_shader(
    entry_point: &slang::EntryPoint,
    session: &slang::Session,
    shader_module: &slang::Module,
    cpu_constants_module: &slang::Module,
) -> anyhow::Result<CompiledShader> {
    let program = session.create_composite_component_type(&[
        shader_module.clone().into(),
        entry_point.clone().into(),
        cpu_constants_module.clone().into(),
    ])?;

    let linked_program = program.link()?;

    let program_layout = linked_program.layout(0)?;

    let mut refl_entry_points = program_layout.entry_points();
    assert!(refl_entry_points.len() == 1);
    let reflection_entry_point = refl_entry_points.next().unwrap();
    let stage = reflection_entry_point.stage();

    let shader_bytecode: slang::Blob = linked_program.entry_point_code(0, 0)?;
    let shader_bytecode = shader_bytecode.as_slice().to_vec();

    let entry_point_name = CString::new(reflection_entry_point.name().unwrap())?;

    Ok(CompiledShader {
        entry_point_name,
        stage,
        shader_bytecode,
    })
}
