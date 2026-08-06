//! Slang compilation and reflection, with no graphics API dependency.
//!
//! This is the only crate in the workspace that depends on `shader-slang`, and
//! no `shader_slang` type appears in its public API — the reflected data it
//! hands back (`json::…`) is plain serde structs. Turning that data into vulkan
//! objects is the renderer's job.

use std::collections::HashMap;
use std::ffi::CString;

use shader_slang as slang;

pub mod json;
mod reflection;

use json::*;

/// How hard slang should work on the spir-v it emits.
///
/// A 1:1 mirror of `slang::OptimizationLevel` rather than a re-export, so that
/// no `shader_slang` type appears in this crate's public API: a slang-rs
/// upgrade that renames a variant stops at `to_slang` below instead of
/// rippling straight through into the cli.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptimizationLevel {
    /// don't optimize at all
    None,
    /// balance code quality against compilation time
    Default,
    /// optimize aggressively
    #[default]
    High,
    /// may take a very long time, or trade space for speed severely
    Maximal,
}

impl OptimizationLevel {
    fn to_slang(self) -> slang::OptimizationLevel {
        match self {
            Self::None => slang::OptimizationLevel::None,
            Self::Default => slang::OptimizationLevel::Default,
            Self::High => slang::OptimizationLevel::High,
            Self::Maximal => slang::OptimizationLevel::Maximal,
        }
    }
}

/// The shader stages this engine supports.
///
/// Deliberately a narrowing of `slang::Stage` rather than a mirror of all 18 of
/// its variants: the rest are rejected anyway, and `from_slang` returning
/// `None` is what lets that rejection happen at the point of compilation, where
/// the stage and the source file are both still in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
}

impl ShaderStage {
    fn from_slang(stage: slang::Stage) -> Option<Self> {
        match stage {
            slang::Stage::Vertex => Some(Self::Vertex),
            slang::Stage::Fragment => Some(Self::Fragment),
            slang::Stage::Compute => Some(Self::Compute),

            // raytracing, mesh, tesselation, geometry, hull, domain, …
            _ => None,
        }
    }
}

/// whether to use column-major or row-major matricies with slang
/// https://docs.shader-slang.org/en/latest/external/slang/docs/user-guide/a1-01-matrix-layout.html
const MATRIX_LAYOUT: MatrixLayout = MatrixLayout::RowMajor;

#[derive(Debug, PartialEq, Eq)]
enum MatrixLayout {
    ColumnMajor,
    #[allow(dead_code)]
    RowMajor,
}

fn load_cpu_constants_module(session: &slang::Session) -> anyhow::Result<slang::Module> {
    let column_major = MATRIX_LAYOUT == MatrixLayout::ColumnMajor;
    let src = format!(
        r#"
        #language slang 2026
        module cpu_constants;

        namespace mltrs {{
            export static const bool columnMajor = {column_major};
        }}
        "#,
    );
    Ok(session.load_module_from_source_string("cpu_constants", "cpu_constants.slang", &src)?)
}

pub struct ReflectedShader {
    pub vertex_shader: CompiledShader,
    pub fragment_shader: CompiledShader,
    pub reflection_json: ReflectionJson,
}

pub struct ReflectedComputeShader {
    pub compute_shader: CompiledShader,
    pub reflection_json: ComputeReflectionJson,
}

pub fn prepare_reflected_shader(
    source_file_name: &str,
    search_path: &str,
) -> anyhow::Result<ReflectedShader> {
    prepare_reflected_shader_with_optimization(
        source_file_name,
        search_path,
        OptimizationLevel::High,
    )
}

pub fn prepare_reflected_shader_with_optimization(
    source_file_name: &str,
    search_path: &str,
    optimization: OptimizationLevel,
) -> anyhow::Result<ReflectedShader> {
    let global_session = slang::GlobalSession::new().unwrap();
    let search_path = CString::new(search_path).unwrap();

    let session_options = slang::CompilerOptions::default()
        .vulkan_use_entry_point_name(true)
        .language(slang::SourceLanguage::Slang)
        .optimization(optimization.to_slang())
        .emit_spirv_directly(true);
    let session_options = match MATRIX_LAYOUT {
        MatrixLayout::ColumnMajor => session_options.matrix_layout_column(true),
        MatrixLayout::RowMajor => session_options.matrix_layout_row(true),
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
    let cpu_constants_module = load_cpu_constants_module(&session)?;

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
            source_file_name,
        )?;

        if compiled_shader.stage == ShaderStage::Vertex {
            vertex_shader = Some(compiled_shader)
        } else if compiled_shader.stage == ShaderStage::Fragment {
            fragment_shader = Some(compiled_shader)
        }

        components.push(entry_point.clone().into());
    }

    let Some(vertex_shader) = vertex_shader else {
        anyhow::bail!("no vertex entry point in {source_file_name}");
    };
    let Some(fragment_shader) = fragment_shader else {
        anyhow::bail!("no fragment entry point in {source_file_name}");
    };

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
pub fn dev_compile_slang_shaders(
    source_file_name: &str,
    search_path: &std::path::Path,
) -> anyhow::Result<ReflectedShader> {
    prepare_reflected_shader(source_file_name, search_path.to_str().unwrap())
}

pub fn prepare_reflected_compute_shader(
    source_file_name: &str,
    search_path: &str,
) -> anyhow::Result<ReflectedComputeShader> {
    prepare_reflected_compute_shader_with_optimization(
        source_file_name,
        search_path,
        OptimizationLevel::High,
    )
}

pub fn prepare_reflected_compute_shader_with_optimization(
    source_file_name: &str,
    search_path: &str,
    optimization: OptimizationLevel,
) -> anyhow::Result<ReflectedComputeShader> {
    let global_session = slang::GlobalSession::new().unwrap();
    let search_path = CString::new(search_path).unwrap();

    let session_options = slang::CompilerOptions::default()
        .vulkan_use_entry_point_name(true)
        .language(slang::SourceLanguage::Slang)
        .optimization(optimization.to_slang())
        .emit_spirv_directly(true);
    let session_options = match MATRIX_LAYOUT {
        MatrixLayout::ColumnMajor => session_options.matrix_layout_column(true),
        MatrixLayout::RowMajor => session_options.matrix_layout_row(true),
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
    let cpu_constants_module = load_cpu_constants_module(&session)?;

    // compute shaders have exactly 1 entry point
    debug_assert!(shader_module.entry_points().len() == 1);

    let mut components = vec![shader_module.clone().into()];
    let mut compute_shader: Option<CompiledShader> = None;
    for entry_point in shader_module.entry_points() {
        let compiled_shader = compile_shader(
            &entry_point,
            &session,
            &shader_module,
            &cpu_constants_module,
            source_file_name,
        )?;

        if compiled_shader.stage != ShaderStage::Compute {
            anyhow::bail!(
                "expected a compute entry point in {source_file_name}, got {:?}",
                compiled_shader.stage
            );
        }
        compute_shader = Some(compiled_shader);

        components.push(entry_point.clone().into());
    }

    let Some(compute_shader) = compute_shader else {
        anyhow::bail!("no compute entry point in {source_file_name}");
    };

    let program = session.create_composite_component_type(&components)?;
    let linked_program = program.link()?;
    let program_layout = linked_program.layout(0)?;

    let reflection_json = reflection::compute_reflection_json(source_file_name, program_layout)?;

    Ok(ReflectedComputeShader {
        compute_shader,
        reflection_json,
    })
}

#[cfg(debug_assertions)]
pub fn dev_compile_slang_compute_shaders(
    source_file_name: &str,
    search_path: &std::path::Path,
) -> anyhow::Result<ReflectedComputeShader> {
    prepare_reflected_compute_shader(source_file_name, search_path.to_str().unwrap())
}

/// `modules` pairs a slang load name (eg. "mltrs/addr") with the rust module
/// name its types are recorded under (eg. "mltrs"); the two only differ for
/// modules that live in a subdirectory of the shader source dir.
pub fn reflect_shared_module_types(
    modules: &[(&str, &str)],
    search_path: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let global_session = slang::GlobalSession::new().unwrap();
    let search_path = CString::new(search_path).unwrap();

    let session_options = slang::CompilerOptions::default()
        .language(slang::SourceLanguage::Slang)
        .no_code_gen(true);
    let session_options = match MATRIX_LAYOUT {
        MatrixLayout::ColumnMajor => session_options.matrix_layout_column(true),
        MatrixLayout::RowMajor => session_options.matrix_layout_row(true),
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

    let _cpu_constants_module = load_cpu_constants_module(&session)?;

    let mut type_to_module: HashMap<String, String> = HashMap::new();

    for &(load_name, rust_module_name) in modules {
        let module = session.load_module(load_name)?;
        let module_decl = module.module_reflection();

        collect_struct_and_enum_decls(module_decl, rust_module_name, &mut type_to_module);
    }

    Ok(type_to_module)
}

/// records every struct/enum declared in `decl`'s subtree,
/// looking through namespace declarations (eg. `namespace mltrs { ... }`)
fn collect_struct_and_enum_decls(
    decl: &slang::reflection::Decl,
    rust_module_name: &str,
    type_to_module: &mut HashMap<String, String>,
) {
    for child in decl.children() {
        match child.kind() {
            // enums hoist into a shared module file exactly like structs
            slang::DeclKind::Struct | slang::DeclKind::Enum => {
                if let Some(name) = child.name() {
                    type_to_module.insert(name.to_string(), rust_module_name.to_string());
                }
            }
            slang::DeclKind::Namespace => {
                collect_struct_and_enum_decls(child, rust_module_name, type_to_module);
            }
            _ => {}
        }
    }
}

pub struct CompiledShader {
    pub entry_point_name: CString,
    pub stage: ShaderStage,
    pub shader_bytecode: Vec<u8>,
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
    source_file_name: &str,
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

    let slang_stage = reflection_entry_point.stage();
    let entry_point_name = CString::new(reflection_entry_point.name().unwrap())?;

    let stage = ShaderStage::from_slang(slang_stage).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported shader stage {slang_stage:?} for entry point {entry_point_name:?} \
             in {source_file_name}; only vertex, fragment and compute are supported"
        )
    })?;

    let shader_bytecode: slang::Blob = linked_program.entry_point_code(0, 0)?;
    let shader_bytecode = shader_bytecode.as_slice().to_vec();

    Ok(CompiledShader {
        entry_point_name,
        stage,
        shader_bytecode,
    })
}
