//! Roc-only shader generation. Both output directories are tool-owned.
//!
//! All compilation, name/path checks and rendering precede publication. Publication
//! itself is deliberately not transactional: an I/O failure can leave partial output.
//! Logical values have no GPU layout, ownership, binding or validity guarantees.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use askama::Template;
use heck::{ToSnakeCase, ToUpperCamelCase};
pub use mltrs_slang_reflection::OptimizationLevel;
use mltrs_slang_reflection::json::*;
use mltrs_slang_reflection::{
    prepare_reflected_compute_shader_with_optimization, prepare_reflected_shader_with_optimization,
};

#[derive(Debug, Clone)]
pub struct RocConfig {
    pub roc_source_dir: PathBuf,
    pub shaders_source_dir: PathBuf,
    pub compiled_shaders_dir: PathBuf,
    pub project_root: PathBuf,
    pub optimization: OptimizationLevel,
}

/// Match Rust's shared-module ownership: top-level modules own their types;
/// modules one directory below the source root share the directory's module.
fn shared_type_owners(source: &Path) -> Result<BTreeMap<String, String>> {
    let mut modules = Vec::new();
    for entry in fs::read_dir(source)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("non-UTF-8 module name")?;
        if path.is_dir() {
            for entry in fs::read_dir(&path)? {
                let child = entry?.path();
                let Some(stem) = child
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_suffix(".slang"))
                else {
                    continue;
                };

                if child.is_file() {
                    modules.push((format!("{name}/{stem}"), name.to_owned()));
                }
            }
        } else if let Some(stem) = name.strip_suffix(".slang") {
            let is_shader = stem.ends_with(".shader") || stem.ends_with(".compute");
            if !is_shader {
                modules.push((stem.to_owned(), stem.to_owned()));
            }
        }
    }
    modules.sort();
    let mut module_names = Names::default();
    for owner in modules
        .iter()
        .map(|(_, owner)| owner)
        .collect::<BTreeSet<_>>()
    {
        module_names.insert(&identifier(owner, true)?, owner)?;
    }
    let module_refs: Vec<_> = modules
        .iter()
        .map(|(load, owner)| (load.as_str(), owner.as_str()))
        .collect();
    let owners = mltrs_slang_reflection::reflect_shared_module_types(
        &module_refs,
        source.to_str().context("non-UTF-8 source path")?,
    )?;

    owners
        .into_iter()
        .map(|(name, module)| Ok((name, identifier(&module, true)?)))
        .collect()
}

/// Generate Roc and SPIR-V
pub fn write_precompiled_roc_shaders(config: RocConfig) -> Result<()> {
    let root = normalize(&config.project_root)?;
    let source = normalize(&config.shaders_source_dir)?;
    let roc = normalize(&config.roc_source_dir)?;
    let compiled = normalize(&config.compiled_shaders_dir)?;
    for output in [&roc, &compiled] {
        ensure!(
            !root.starts_with(output),
            "output {} contains project root",
            output.display()
        );
        ensure!(
            !overlap(output, &source),
            "output {} overlaps source {}",
            output.display(),
            source.display()
        );
        ensure!(
            !output.exists() || output.is_dir(),
            "output {} is not a directory",
            output.display()
        );
    }

    ensure!(
        !overlap(&roc, &compiled),
        "Roc and compiled output directories overlap"
    );

    let search = source.to_str().context("shader source path is not UTF-8")?;
    let mut inputs = Vec::new();
    for entry in
        fs::read_dir(&source).with_context(|| format!("read shader source {}", source.display()))?
    {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("non-UTF-8 shader source filename"))?;
        let is_shader_source = name.ends_with(".shader.slang") || name.ends_with(".compute.slang");
        if is_shader_source {
            ensure!(entry.path().is_file(), "shader source {name} is not a file");
            inputs.push(name);
        }
    }
    inputs.sort();

    let mut modules = Names::default();
    for name in ["ShaderAtlas", "ShaderTypes", "ShaderReflection"] {
        modules.insert(name, name)?;
    }

    let owners = shared_type_owners(&source)?;
    let mut shared = Logical::default();
    let mut checked = Logical::default();
    let mut roc_files = BTreeMap::new();
    let mut binaries = BTreeMap::new();
    let mut atlas = Vec::new();

    let mut atlas_names = Names::default();

    for input in inputs {
        let compute = input.ends_with(".compute.slang");

        let suffix = if compute {
            ".compute.slang"
        } else {
            ".shader.slang"
        };
        let stem = input.strip_suffix(suffix).expect("discovered suffix");
        let module = identifier(stem, true)?;
        modules.insert(&module, &input)?;
        let value = identifier(stem, false)?;
        atlas_names.insert(&value, &input)?;
        let mut logical = Logical {
            owners: owners.clone(),
            local_module: module.clone(),
            ..Logical::default()
        };
        let mut types = StructTypes::default();
        let mut stages = Vec::new();
        let mut uniforms = Vec::new();
        let mut default_uniform = None;
        let mut vertex_input = None;
        let (reflection_kind, reflection, reflection_json) = if compute {
            let shader = prepare_reflected_compute_shader_with_optimization(
                &input,
                search,
                config.optimization,
            )?;
            logical.compute(&shader.reflection_json)?;
            stages.push((
                "compute",
                shader.compute_shader.shader_bytecode.to_vec(),
                "comp",
            ));
            (
                "ComputeReflection",
                shader.reflection_json.lower(&mut types)?.render(),
                serde_json::to_string_pretty(&shader.reflection_json)?,
            )
        } else {
            let shader =
                prepare_reflected_shader_with_optimization(&input, search, config.optimization)?;
            logical.graphics(&shader.reflection_json)?;
            let (handles, count) =
                uniform_handles(&shader.reflection_json.global_parameters, &mut logical)?;
            let has_single_uniform = count == 1;
            if has_single_uniform {
                default_uniform = handles.first().map(|handle| handle.name.clone());
            }

            uniforms = handles;
            vertex_input = vertex_layout(&shader.reflection_json.vertex_entry_point)?
                .map(|layout| logical.reference(&layout.type_name, &module))
                .transpose()?;
            stages.push((
                "vertex",
                shader.vertex_shader.shader_bytecode.to_vec(),
                "vert",
            ));
            stages.push((
                "fragment",
                shader.fragment_shader.shader_bytecode.to_vec(),
                "frag",
            ));
            (
                "GraphicsReflection",
                shader.reflection_json.lower(&mut types)?.render(),
                serde_json::to_string_pretty(&shader.reflection_json)?,
            )
        };
        let json_filename = if compute {
            format!("{stem}.comp.json")
        } else {
            format!("{stem}.json")
        };
        ensure!(
            !binaries
                .keys()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&json_filename)),
            "reflection filename collision: {json_filename}"
        );
        let reflection_json_path = import_path(&roc, &compiled.join(&json_filename))?;
        binaries.insert(json_filename, reflection_json.into_bytes());
        let mut stage_imports = Vec::new();
        let mut stage_names = Vec::new();
        for (stage, bytes, extension) in stages {
            let filename = format!("{stem}.{extension}.spv");
            ensure!(
                !binaries
                    .keys()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(&filename)),
                "SPIR-V filename collision: {filename}"
            );
            let path = import_path(&roc, &compiled.join(&filename))?;
            stage_imports.push(StageImport { stage, path });
            stage_names.push(stage);
            binaries.insert(filename, bytes);
        }
        let struct_types = types.definitions();
        let mut members = Names::default();
        for name in ["reflection", "stages", "shader"] {
            members.insert(name, "shader module support value")?;
        }
        for definition in &struct_types {
            members.insert(&definition.name, &definition.name)?;
        }
        for handle in &uniforms {
            members.insert(&handle.name, &handle.parameter)?;
        }
        logical.validate_scope(&module)?;
        let logical_definitions = logical.render_definitions(Some(&module));
        let imports = logical.imports(&module);
        let mut exposed = logical.exposed(&module);
        exposed.push(reflection_kind.into());
        if !struct_types.is_empty() {
            exposed.push("StructType".into());
        }
        if !compute {
            exposed.push(match vertex_input {
                Some(_) => "VertexInput".into(),
                None => "NoVertexInput".into(),
            });
        }
        if !uniforms.is_empty() {
            exposed.push("UniformBinding".into());
        }
        exposed.sort();
        exposed.dedup();
        checked.merge(&logical, false)?;
        shared.merge(&logical, true)?;
        let shader_module = ShaderModule {
            module: module.clone(),
            logical_definitions,
            imports,
            exposed,
            stage_imports,
            reflection_json_path: Some(reflection_json_path),
            struct_types,
            reflection_kind,
            reflection: continued(&reflection, 1),
            stages: stage_names,
            shader_name: (!compute).then(|| value.clone()),
            uniforms,
            default_uniform,
            vertex_input,
        };
        roc_files.insert(
            format!("{module}.roc"),
            shader_module.render().expect("static template"),
        );
        atlas.push(AtlasEntry { module, value });
    }

    let shared_modules: BTreeSet<_> = shared
        .definitions
        .values()
        .map(|(original, _)| shared.owner(original).to_owned())
        .collect();
    for module in shared_modules {
        modules.insert(&module, &format!("shared module {module}"))?;
        shared.validate_scope(&module)?;
        let rendered = ShaderTypesTemplate {
            module: module.clone(),
            imports: shared.imports(&module),
            exposed: shared.exposed(&module),
            definitions: shared.render_definitions(Some(&module)),
        }
        .render()
        .expect("static template");
        roc_files.insert(format!("{module}.roc"), rendered);
    }
    let atlas = ShaderAtlas { modules: atlas };
    roc_files.insert(
        "ShaderAtlas.roc".into(),
        atlas.render().expect("static template"),
    );
    // No source-dependent errors may occur after this boundary.
    publish(&compiled, binaries)?;

    let files = roc_files
        .into_iter()
        .map(|(name, text)| (name, canonical_roc_indentation(&text).into_bytes()))
        .collect();

    publish(&roc, files)
}

fn canonical_roc_indentation(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let spaces = line.bytes().take_while(|byte| *byte == b' ').count();
        output.extend(std::iter::repeat_n('\t', spaces / 4));
        output.push_str(&line[spaces..]);
    }

    output
}

fn publish(dir: &Path, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir)
            .with_context(|| format!("clear managed output {}", dir.display()))?;
    }
    fs::create_dir_all(dir)?;
    for (name, bytes) in files {
        fs::write(dir.join(name), bytes)?;
    }

    Ok(())
}

fn overlap(first: &Path, second: &Path) -> bool {
    first.starts_with(second) || second.starts_with(first)
}

/// Resolve existing symlinks before processing `..`, including paths with a
/// not-yet-created suffix. Fail closed for dangling symlinks and inaccessible paths.
fn normalize(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut result = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::RootDir | Component::Prefix(_) => result.push(component.as_os_str()),
            Component::Normal(part) => {
                result.push(part);
                match fs::symlink_metadata(&result) {
                    Ok(_) => {
                        result = fs::canonicalize(&result)
                            .with_context(|| format!("resolve {}", result.display()))?
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).with_context(|| format!("inspect {}", result.display()));
                    }
                }
            }
        }
    }

    Ok(result)
}

fn import_path(from: &Path, to: &Path) -> Result<String> {
    let from = from.components().collect::<Vec<_>>();
    let to = to.components().collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(from_component, to_component)| from_component == to_component)
        .count();
    ensure!(
        common > 0,
        "no relative Roc import path between output directories"
    );
    let mut parts = vec!["..".to_string(); from.len() - common];
    for part in &to[common..] {
        ensure!(
            matches!(part, Component::Normal(_)),
            "unrepresentable Roc import path"
        );
        let part = part
            .as_os_str()
            .to_str()
            .context("non-UTF-8 Roc import path")?;
        ensure!(
            !part.chars().any(|character| {
                character.is_control() || matches!(character, '"' | '\\' | '$')
            }),
            "unrepresentable Roc import path component {part:?}"
        );
        parts.push(part.to_string());
    }

    Ok(parts.join("/"))
}

#[derive(Default)]
struct Names(BTreeMap<String, String>);

impl Names {
    fn insert(&mut self, generated: &str, original: &str) -> Result<()> {
        if let Some(previous) = self
            .0
            .insert(generated.to_ascii_lowercase(), original.into())
        {
            bail!("generated name collision: {original:?} and {previous:?} both use {generated}");
        }

        Ok(())
    }
}

fn identifier(original: &str, upper: bool) -> Result<String> {
    let is_valid = !original.is_empty()
        && original
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && original.as_bytes()[0].is_ascii_alphabetic();
    ensure!(is_valid, "invalid Roc identifier {original:?}");
    let name = if upper {
        original.to_upper_camel_case()
    } else {
        original.to_snake_case()
    };
    ensure!(
        ![
            "and",
            "app",
            "as",
            "break",
            "crash",
            "dbg",
            "else",
            "expect",
            "exposes",
            "exposing",
            "for",
            "generates",
            "has",
            "hosted",
            "if",
            "implements",
            "import",
            "imports",
            "in",
            "interface",
            "match",
            "module",
            "or",
            "package",
            "packages",
            "platform",
            "provides",
            "requires",
            "return",
            "targets",
            "var",
            "where",
            "while",
            "with",
            // Conservatively reserve words used by older/newer surface syntax too.
            "alias",
            "not",
            "type",
        ]
        .contains(&name.as_str()),
        "reserved Roc identifier {original:?}"
    );

    Ok(name)
}

fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' => out.push_str("\\u(24)"),
            character if character.is_control() => {
                out.push_str(&format!("\\u({:x})", u32::from(character)))
            }
            character => out.push(character),
        }
    }
    out.push('"');

    out
}

fn indent_roc(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A Roc value expression before layout. Sibling-value names are literals.
#[derive(Debug, Clone, PartialEq)]
enum RocExpr {
    Literal(String),
    Apply {
        head: String,
        argument: Box<RocExpr>,
    },
    Record(Vec<(String, RocExpr)>),
    List(Vec<RocExpr>),
}

impl RocExpr {
    fn render(&self) -> String {
        match self {
            RocExpr::Literal(text) => text.clone(),
            RocExpr::Apply { head, argument } => {
                let argument = argument.render();
                let hugging = !argument.contains('\n') || argument.starts_with('{');

                fragment(&RocApplyTemplate {
                    head: head.clone(),
                    argument: if hugging {
                        argument
                    } else {
                        continued(&argument, 1)
                    },
                    hugging,
                })
            }
            RocExpr::Record(fields) => fragment(&RocRecordTemplate {
                fields: fields
                    .iter()
                    .map(|(name, value)| RocField {
                        name: name.clone(),
                        value: continued(&value.render(), 1),
                    })
                    .collect(),
            }),
            RocExpr::List(elements) => {
                let elements = elements.iter().map(RocExpr::render).collect::<Vec<_>>();
                let multiline = elements.iter().any(|element| element.contains('\n'));

                fragment(&RocListTemplate {
                    elements: if multiline {
                        elements
                            .iter()
                            .map(|element| continued(element, 1))
                            .collect()
                    } else {
                        elements
                    },
                    multiline,
                })
            }
        }
    }
}

/// Every template renders its first line at column 0 with continuation lines
/// indented relative to it; the embedding site re-indents them.
fn fragment(template: &impl Template) -> String {
    template
        .render()
        .expect("static template")
        .trim_end_matches('\n')
        .to_string()
}

fn continued(text: &str, levels: usize) -> String {
    text.replace('\n', &format!("\n{}", "    ".repeat(levels)))
}

struct RocField {
    name: String,
    value: String,
}

#[derive(Template)]
#[template(path = "record.roc.askama", escape = "none")]
struct RocRecordTemplate {
    fields: Vec<RocField>,
}

#[derive(Template)]
#[template(path = "list.roc.askama", escape = "none")]
struct RocListTemplate {
    elements: Vec<String>,
    multiline: bool,
}

impl RocListTemplate {
    fn one_line(&self) -> String {
        self.elements.join(", ")
    }
}

#[derive(Template)]
#[template(path = "apply.roc.askama", escape = "none")]
struct RocApplyTemplate {
    head: String,
    argument: String,
    hugging: bool,
}

/// Struct types hoisted into named sibling values of one shader module.
#[derive(Default)]
struct StructTypes {
    definitions: BTreeMap<String, StructTypeDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
struct StructTypeDefinition {
    type_name: String,
    fields: RocExpr,
}

impl StructTypes {
    fn reference(&mut self, type_name: &str, fields: &[StructField]) -> Result<RocExpr> {
        let name = identifier(&format!("{type_name}_type"), false)?;
        let fields = fields.lower(self)?;
        match self.definitions.get(&name) {
            Some(previous) => ensure!(
                previous.type_name == type_name && previous.fields == fields,
                "incompatible reflected struct types {type_name:?} and {:?} both use {name}",
                previous.type_name
            ),
            None => {
                let definition = StructTypeDefinition {
                    type_name: type_name.into(),
                    fields,
                };
                self.definitions.insert(name.clone(), definition);
            }
        }

        Ok(RocExpr::Literal(name))
    }

    fn definitions(self) -> Vec<StructTypeDef> {
        self.definitions
            .into_iter()
            .map(|(name, definition)| StructTypeDef {
                name,
                type_name: quoted(&definition.type_name),
                fields: continued(&definition.fields.render(), 1),
            })
            .collect()
    }
}

#[derive(Template)]
#[template(path = "struct_type.roc.askama", escape = "none")]
struct StructTypeDef {
    name: String,
    type_name: String,
    fields: String,
}

impl StructTypeDef {
    fn block(&self) -> String {
        indent_roc(&fragment(self))
    }
}

struct StageImport {
    stage: &'static str,
    path: String,
}

#[derive(Template)]
#[template(path = "shader_module.roc.askama", escape = "none")]
struct ShaderModule {
    module: String,
    logical_definitions: Vec<String>,
    imports: Vec<String>,
    /// `ShaderReflection` type names this module names unqualified
    exposed: Vec<String>,
    stage_imports: Vec<StageImport>,
    /// `None` only for synthetic test modules with no compiled directory
    reflection_json_path: Option<String>,
    struct_types: Vec<StructTypeDef>,
    reflection_kind: &'static str,
    reflection: String,
    stages: Vec<&'static str>,
    /// graphics shaders expose a `shader` record for graph pipelines
    shader_name: Option<String>,
    /// one typed handle per constant buffer, graphics shaders only
    uniforms: Vec<UniformHandle>,
    /// Present only for exactly one constant buffer with a supported packer.
    default_uniform: Option<String>,
    /// the scoped name of the vertex input struct, when the vertex
    /// entry point reads one
    vertex_input: Option<String>,
}

/// A typed handle to one of a graphics shader's constant buffers.
struct UniformHandle {
    /// the value's name in the module
    name: String,
    /// the reflected parameter name
    parameter: String,
    /// position among the shader's constant buffers, in descriptor-set-layout
    /// order
    index: usize,
    /// the element type's scoped name
    type_name: String,
}

/// One handle per parameter block with uniform bytes, in descriptor-set-layout
/// order. A block whose element type has no packer keeps its position but gets
/// no handle.
fn uniform_handles(
    globals: &[GlobalParameter],
    logical: &mut Logical,
) -> Result<(Vec<UniformHandle>, usize)> {
    let mut handles = Vec::new();
    let mut index = 0;
    for global in globals {
        let GlobalParameter::ParameterBlock(block) = global else {
            continue;
        };
        let has_bytes = block
            .element_type
            .fields
            .iter()
            .any(|field| field.binding().is_some_and(Binding::occupies_bytes));
        if !has_bytes {
            continue;
        }
        if logical.has_packer(&block.element_type.type_name)? {
            handles.push(UniformHandle {
                name: identifier(&block.parameter_name, false)?,
                parameter: block.parameter_name.clone(),
                index,
                type_name: logical
                    .reference(&block.element_type.type_name, &logical.local_module.clone())?,
            });
        }
        index += 1;
    }

    Ok((handles, index))
}

struct AtlasEntry {
    module: String,
    value: String,
}

#[derive(Template)]
#[template(path = "shader_atlas.roc.askama", escape = "none")]
struct ShaderAtlas {
    modules: Vec<AtlasEntry>,
}

// Direct exhaustive schema mapping: no JSON intermediate and no numeric floats.
trait Lower {
    fn lower(&self, types: &mut StructTypes) -> Result<RocExpr>;
}

impl Lower for String {
    fn lower(&self, _: &mut StructTypes) -> Result<RocExpr> {
        Ok(RocExpr::Literal(quoted(self)))
    }
}

impl Lower for usize {
    fn lower(&self, _: &mut StructTypes) -> Result<RocExpr> {
        Ok(RocExpr::Literal(
            u64::try_from(*self)
                .context("usize metadata exceeds U64")?
                .to_string(),
        ))
    }
}

impl Lower for u32 {
    fn lower(&self, _: &mut StructTypes) -> Result<RocExpr> {
        Ok(RocExpr::Literal(self.to_string()))
    }
}

impl Lower for i64 {
    fn lower(&self, _: &mut StructTypes) -> Result<RocExpr> {
        Ok(RocExpr::Literal(self.to_string()))
    }
}

impl<T: Lower> Lower for [T] {
    fn lower(&self, types: &mut StructTypes) -> Result<RocExpr> {
        Ok(RocExpr::List(
            self.iter()
                .map(|element| element.lower(types))
                .collect::<Result<_>>()?,
        ))
    }
}

impl<T: Lower> Lower for Option<T> {
    fn lower(&self, types: &mut StructTypes) -> Result<RocExpr> {
        Ok(match self {
            Some(value) => RocExpr::Apply {
                head: "Some".into(),
                argument: Box::new(value.lower(types)?),
            },
            None => RocExpr::Literal("None".into()),
        })
    }
}

impl Lower for [u32; 3] {
    fn lower(&self, _: &mut StructTypes) -> Result<RocExpr> {
        Ok(RocExpr::Record(
            ["x", "y", "z"]
                .iter()
                .zip(self)
                .map(|(name, value)| (name.to_string(), RocExpr::Literal(value.to_string())))
                .collect(),
        ))
    }
}

macro_rules! record {
    ($ty:ty, $($field:ident),+ $(,)?) => {
        impl Lower for $ty {
            fn lower(&self, types: &mut StructTypes) -> Result<RocExpr> {
                Ok(RocExpr::Record(vec![$((stringify!($field).into(), self.$field.lower(types)?)),+]))
            }
        }
    };
}

macro_rules! tags {
    ($ty:ty, $($variant:ident),+ $(,)?) => {
        impl Lower for $ty {
            fn lower(&self, _: &mut StructTypes) -> Result<RocExpr> {
                Ok(RocExpr::Literal(match self { $(Self::$variant => stringify!($variant)),+ }.into()))
            }
        }
    };
}

macro_rules! variants {
    ($ty:ty, $($variant:ident),+ $(,)?) => {
        impl Lower for $ty {
            fn lower(&self, types: &mut StructTypes) -> Result<RocExpr> {
                Ok(match self {
                    $(Self::$variant(value) => RocExpr::Apply {
                        head: stringify!($variant).into(),
                        argument: Box::new(value.lower(types)?),
                    }),+
                })
            }
        }
    };
}

macro_rules! struct_type {
    ($ty:ty) => {
        impl Lower for $ty {
            fn lower(&self, types: &mut StructTypes) -> Result<RocExpr> {
                types.reference(&self.type_name, &self.fields)
            }
        }
    };
}
record!(
    ReflectionJson,
    source_file_name,
    global_parameters,
    vertex_entry_point,
    fragment_entry_point,
    pipeline_layout
);
record!(
    ComputeReflectionJson,
    source_file_name,
    global_parameters,
    compute_entry_point,
    workgroup_size,
    pipeline_layout
);
variants!(GlobalParameter, ParameterBlock, PushConstant);
record!(ParameterBlockGlobalParameter, parameter_name, element_type);
record!(
    PushConstantGlobalParameter,
    parameter_name,
    element_type,
    element_size
);
struct_type!(ParameterBlockElementType);
record!(EntryPoint, entry_point_name, stage, parameters);
tags!(EntryPointStage, Vertex, Fragment, Compute);
variants!(EntryPointParameter, Struct, Scalar);
record!(
    StructEntryPointParameter,
    parameter_name,
    binding,
    type_name,
    fields
);
variants!(ScalarEntryPointParameter, Bound, Semantic);
record!(
    BoundScalarEntryPointParameter,
    parameter_name,
    binding,
    scalar_type
);
record!(
    SemanticScalarEntryPointParameter,
    parameter_name,
    semantic_name,
    scalar_type
);
variants!(
    StructField,
    Scalar,
    Vector,
    Struct,
    Matrix,
    Resource,
    Pointer,
    Array,
    Enum,
    DescriptorHandle
);
variants!(
    Binding,
    Uniform,
    PushConstant,
    DescriptorTableSlot,
    VaryingInput,
    ConstantBuffer
);
record!(OffsetSizeBinding, offset, size);
record!(IndexCountBinding, index, count);
variants!(VectorStructField, Bound, Semantic);
record!(
    SemanticVectorStructField,
    field_name,
    semantic_name,
    element_count,
    element_type
);
record!(ScalarStructField, field_name, binding, scalar_type);
record!(
    BoundVectorStructField,
    field_name,
    binding,
    element_count,
    element_type
);
record!(
    MatrixStructField,
    field_name,
    binding,
    row_count,
    column_count,
    element_type
);
record!(
    ResourceStructField,
    field_name,
    binding,
    resource_shape,
    result_type
);
tags!(ResourceShape, Texture2D, RWTexture2D);
variants!(ResourceResultType, Scalar, Vector, Struct);
record!(ScalarResultType, scalar_type);
record!(VectorResultType, element_count, element_type);
struct_type!(StructResultType);
record!(StructStructField, field_name, binding, struct_type);
struct_type!(StructFieldType);
record!(
    ArrayStructField,
    field_name,
    binding,
    element_scalar_type,
    element_count,
    element_stride
);
record!(
    PointerStructField,
    field_name,
    binding,
    pointee_type,
    pointee_size,
    access
);
tags!(PointerAccess, ReadWrite, Read, Immutable);
record!(DescriptorHandleStructField, field_name, binding, shape);
tags!(DescriptorHandleShape, Sampler2D, RwTexture2D);
variants!(VectorElementType, Scalar);
record!(ScalarVectorElementType, scalar_type);
tags!(ScalarType, Float32, Int32, Uint32, Uint64);
record!(EnumStructField, field_name, binding, enum_type);
record!(EnumFieldType, type_name, tag_type, cases);
record!(EnumCase, name, value);
tags!(EnumTagType, Uint32, Int32);
record!(
    ReflectedPipelineLayout,
    descriptor_set_layouts,
    push_constant_ranges,
    bindless_heap_set
);
record!(ReflectedDescriptorSetLayout, binding_ranges);
record!(
    ReflectedDescriptorSetLayoutBinding,
    binding,
    descriptor_type,
    descriptor_count,
    stage_flags,
    size
);
record!(ReflectedPushConstantRange, stage_flags, offset, size);
tags!(
    ReflectedBindingType,
    Sampler,
    Texture,
    ConstantBuffer,
    CombinedTextureSampler,
    StorageImage
);
tags!(ReflectedStageFlags, Vertex, Fragment, Compute, All, Empty);

#[derive(Default)]
struct Logical {
    /// Original Slang names mapped to generated shared module names.
    owners: BTreeMap<String, String>,
    /// Owner for types declared directly in the shader.
    local_module: String,
    dependencies: BTreeMap<String, BTreeSet<String>>,
    /// ShaderReflection type names each module references.
    exposed: BTreeMap<String, BTreeSet<String>>,
    /// type bodies (record or tag list) keyed by lowercase name
    definitions: BTreeMap<String, (String, String)>,
    /// associated blocks keyed like `definitions`: GPU byte packers and enum
    /// tag functions; a struct with no byte layout (resource elements) has
    /// none
    associated: BTreeMap<String, (String, String)>,
}

/// How a struct's GPU size is known when its packer is generated.
#[derive(Clone, Copy)]
enum PackSize {
    /// reflection states the size (push blocks, pointees, nested fields)
    Known(usize),
    /// a std140 block: the reflected end rounded up to 16
    Std140,
    /// no GPU layout: structured-buffer elements and other non-uniform data
    Skip,
}

impl Logical {
    fn owner(&self, original: &str) -> &str {
        self.owners
            .get(original)
            .map(String::as_str)
            .unwrap_or(&self.local_module)
    }

    fn reference(&mut self, original: &str, scope: &str) -> Result<String> {
        let name = identifier(original, true)?;
        let owner = self.owner(original).to_owned();
        if owner == scope {
            return Ok(name);
        }

        self.dependencies
            .entry(scope.into())
            .or_default()
            .insert(owner.clone());

        Ok(format!("{owner}.{name}"))
    }

    fn imports(&self, module: &str) -> Vec<String> {
        self.dependencies
            .get(module)
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    fn exposed(&self, module: &str) -> Vec<String> {
        self.exposed
            .get(module)
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    /// Record a `ShaderReflection` type the module names unqualified, and
    /// return that bare name.
    fn expose(&mut self, scope: &str, name: String) -> String {
        self.exposed
            .entry(scope.into())
            .or_default()
            .insert(name.clone());

        name
    }

    fn validate_scope(&self, module: &str) -> Result<()> {
        let mut names = Names::default();
        for import in self.imports(module) {
            names.insert(&import, &format!("import {import}"))?;
        }
        for (original, _) in self.definitions.values() {
            if self.owner(original) == module {
                names.insert(&identifier(original, true)?, original)?;
            }
        }

        Ok(())
    }

    fn merge(&mut self, other: &Self, shared_only: bool) -> Result<()> {
        for (key, (original, body)) in &other.definitions {
            let skip_local = shared_only && !other.owners.contains_key(original);
            if skip_local {
                continue;
            }

            self.define(original, body.clone())?;
            if let Some((_, block)) = other.associated.get(key) {
                self.associate(original, block.clone(), "GPU layouts")?;
            }
            self.owners
                .insert(original.clone(), other.owner(original).to_owned());
        }
        for (module, imports) in &other.dependencies {
            self.dependencies
                .entry(module.clone())
                .or_default()
                .extend(imports.iter().cloned());
        }
        for (module, exposed) in &other.exposed {
            self.exposed
                .entry(module.clone())
                .or_default()
                .extend(exposed.iter().cloned());
        }

        Ok(())
    }

    fn graphics(&mut self, shader: &ReflectionJson) -> Result<()> {
        self.globals(&shader.global_parameters)?;

        self.entry(&shader.vertex_entry_point)
    }

    fn compute(&mut self, shader: &ComputeReflectionJson) -> Result<()> {
        self.globals(&shader.global_parameters)?;

        self.entry(&shader.compute_entry_point)
    }

    fn globals(&mut self, globals: &[GlobalParameter]) -> Result<()> {
        let mut names = Names::default();
        for global in globals {
            let (parameter_name, element, size) = match global {
                GlobalParameter::ParameterBlock(parameter) => (
                    &parameter.parameter_name,
                    &parameter.element_type,
                    PackSize::Std140,
                ),
                GlobalParameter::PushConstant(parameter) => (
                    &parameter.parameter_name,
                    &parameter.element_type,
                    PackSize::Known(parameter.element_size),
                ),
            };
            names.insert(&identifier(parameter_name, false)?, parameter_name)?;
            self.structure_sized(&element.type_name, &element.fields, size)?;
        }

        Ok(())
    }

    fn entry(&mut self, entry: &EntryPoint) -> Result<()> {
        identifier(&entry.entry_point_name, false)?;
        let mut names = Names::default();
        for parameter in &entry.parameters {
            let name = match parameter {
                EntryPointParameter::Struct(struct_parameter) => {
                    self.structure_sized(
                        &struct_parameter.type_name,
                        &struct_parameter.fields,
                        PackSize::Skip,
                    )?;
                    &struct_parameter.parameter_name
                }
                EntryPointParameter::Scalar(ScalarEntryPointParameter::Bound(bound)) => {
                    &bound.parameter_name
                }
                EntryPointParameter::Scalar(ScalarEntryPointParameter::Semantic(semantic)) => {
                    &semantic.parameter_name
                }
            };
            names.insert(&identifier(name, false)?, name)?;
        }

        if matches!(entry.stage, EntryPointStage::Vertex)
            && let Some(layout) = vertex_layout(entry)?
        {
            self.vertex_packer(entry, &layout)?;
        }

        Ok(())
    }

    fn vertex_packer(&mut self, entry: &EntryPoint, layout: &VertexLayout) -> Result<()> {
        let fields = entry
            .parameters
            .iter()
            .find_map(|parameter| match parameter {
                EntryPointParameter::Struct(parameter) => Some(&parameter.fields),
                EntryPointParameter::Scalar(_) => None,
            })
            .expect("a vertex layout comes from a struct parameter");
        let scope = self.owner(&layout.type_name).to_owned();
        let mut packed = Vec::new();
        for attribute in &layout.attributes {
            let field = fields
                .iter()
                .find(|field| field.field_name() == attribute.field_name)
                .expect("layout attributes name struct fields");
            let expr = self
                .field_bytes(field, &scope)?
                .expect("layout attributes are scalars or vectors");
            packed.push(PackField {
                offset: attribute.offset as usize,
                expr,
            });
        }

        self.define_packer(&layout.type_name, layout.stride as usize, packed)
    }

    fn define_packer(&mut self, original: &str, size: usize, fields: Vec<PackField>) -> Result<()> {
        let body = fragment(&LogicalPackTemplate {
            name: identifier(original, true)?,
            size,
            fields,
        });

        self.associate(original, body, "GPU layouts")
    }

    fn has_packer(&self, original: &str) -> Result<bool> {
        let key = identifier(original, true)?.to_ascii_lowercase();

        Ok(self.associated.contains_key(&key))
    }

    fn associate(&mut self, original: &str, body: String, what: &str) -> Result<()> {
        let key = identifier(original, true)?.to_ascii_lowercase();
        if let Some((previous, existing)) = self.associated.get(&key) {
            ensure!(
                previous == original && existing == &body,
                "logical type {original:?} has two different {what}"
            );
        } else {
            self.associated.insert(key, (original.into(), body));
        }

        Ok(())
    }

    /// The Roc expression packing `value.<field>`, or `None` for a field
    /// that occupies no uniform bytes.
    fn field_bytes(&mut self, field: &StructField, scope: &str) -> Result<Option<String>> {
        let member = identifier(field.field_name(), false)?;
        let value = format!("value.{member}");

        Ok(Some(match field {
            StructField::Resource(resource_field) => {
                bail!(unsupported_resource(&resource_field.field_name))
            }
            StructField::Scalar(scalar_field) => {
                format!(
                    "ShaderReflection.{}_bytes({value})",
                    scalar_bytes(scalar_field.scalar_type)
                )
            }
            StructField::Vector(VectorStructField::Bound(vector)) => {
                let name = vector_name(vector.element_count, vector_scalar(&vector.element_type));
                format!("{}.to_bytes({value})", self.expose(scope, name))
            }
            StructField::Vector(VectorStructField::Semantic(_)) => return Ok(None),
            StructField::Matrix(matrix_field) => {
                let name = matrix_name(vector_scalar(&matrix_field.element_type));
                format!("{}.to_bytes({value})", self.expose(scope, name))
            }
            StructField::Struct(struct_field) => {
                let nested = identifier(&struct_field.struct_type.type_name, true)?;
                ensure!(
                    self.associated.contains_key(&nested.to_ascii_lowercase()),
                    "nested logical type {:?} has no GPU layout",
                    struct_field.struct_type.type_name
                );
                format!(
                    "{}.to_bytes({value})",
                    self.reference(&struct_field.struct_type.type_name, scope)?
                )
            }
            StructField::Array(array_field) => {
                let element = vector_name(4, array_field.element_scalar_type);
                format!(
                    "ShaderReflection.array_bytes({value}, {}, {}, {}.to_bytes)",
                    array_field.element_count,
                    array_field.element_stride,
                    self.expose(scope, element)
                )
            }
            StructField::Enum(enum_field) => format!(
                "ShaderReflection.{}_bytes({}.tag({value}))",
                match enum_field.enum_type.tag_type {
                    EnumTagType::Uint32 => "u32",
                    EnumTagType::Int32 => "i32",
                },
                self.reference(&enum_field.enum_type.type_name, scope)?
            ),
            StructField::Pointer(_) => {
                format!(
                    "{}.to_bytes({value})",
                    self.expose(scope, "PointerAddress".into())
                )
            }
            StructField::DescriptorHandle(_) => {
                format!(
                    "{}.to_bytes({value})",
                    self.expose(scope, "DescriptorHandle".into())
                )
            }
        }))
    }

    fn define(&mut self, original: &str, definition: String) -> Result<String> {
        let name = identifier(original, true)?;
        let reserved = [
            "PointerAddress",
            "DescriptorHandle",
            "ShaderTypes",
            "Reflection",
            "ShaderAtlas",
            "ShaderReflection",
            "StructType",
            "GraphicsReflection",
            "ComputeReflection",
            "UniformBinding",
            "VertexInput",
            "NoVertexInput",
            "F32",
            "I32",
            "U32",
            "U64",
            "Str",
            "List",
            "Bool",
        ]
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(&name))
            || is_builtin_alias(&name);
        ensure!(
            !reserved,
            "logical type {original:?} collides with support/builtin type"
        );
        if let Some((previous, body)) = self.definitions.get(&name.to_ascii_lowercase()) {
            ensure!(
                previous == original && body == &definition,
                "incompatible or colliding logical definitions for {original:?} and {previous:?}"
            );
        } else {
            self.definitions
                .insert(name.to_ascii_lowercase(), (original.into(), definition));
        }

        Ok(name)
    }

    #[cfg(test)]
    fn structure(&mut self, name: &str, fields: &[StructField]) -> Result<String> {
        self.structure_sized(name, fields, PackSize::Std140)
    }

    fn structure_sized(
        &mut self,
        name: &str,
        fields: &[StructField],
        size: PackSize,
    ) -> Result<String> {
        let scope = self.owner(name).to_owned();
        let mut names = Names::default();
        let mut members = Vec::new();
        for field in fields {
            let original = field.field_name();
            let member = identifier(original, false)?;
            names.insert(&member, original)?;
            let ty = self
                .field(field, &scope)
                .with_context(|| format!("logical {name}.{original}"))?;
            members.push(RocField {
                name: member,
                value: continued(&ty, 1),
            });
        }

        let definition = LogicalStructTemplate { members };
        let defined = self.define(name, fragment(&definition))?;
        self.packer(name, fields, size)
            .with_context(|| format!("GPU byte packer for logical {name}"))?;

        Ok(defined)
    }

    /// Generate the struct's packer when every data field has reflected
    /// uniform bytes. Structs without a byte layout get none.
    fn packer(&mut self, name: &str, fields: &[StructField], size: PackSize) -> Result<()> {
        if matches!(size, PackSize::Skip) {
            return Ok(());
        }

        let scope = self.owner(name).to_owned();
        let mut packed = Vec::new();
        let mut end = 0;
        for field in fields {
            let Some(expr) = self.field_bytes(field, &scope)? else {
                return Ok(());
            };

            let Some(bytes) = field.binding().and_then(Binding::occupied_bytes) else {
                return Ok(());
            };

            end = end.max(bytes.offset + bytes.size);
            packed.push(PackField {
                offset: bytes.offset,
                expr,
            });
        }
        packed.sort_by_key(|field| field.offset);
        let size = match size {
            PackSize::Known(size) => size,
            PackSize::Std140 => end.div_ceil(16) * 16,
            PackSize::Skip => unreachable!(),
        };
        ensure!(
            end <= size,
            "reflected fields of {name:?} end at {end} bytes, past its size {size}"
        );

        self.define_packer(name, size, packed)
    }

    fn field(&mut self, field: &StructField, scope: &str) -> Result<String> {
        Ok(match field {
            StructField::Scalar(scalar_field) => scalar(scalar_field.scalar_type).into(),
            StructField::Vector(vector_field) => {
                let (count, element) = match vector_field {
                    VectorStructField::Bound(bound) => (bound.element_count, &bound.element_type),
                    VectorStructField::Semantic(semantic) => {
                        (semantic.element_count, &semantic.element_type)
                    }
                };

                self.vector(count, vector_scalar(element), scope)?
            }
            StructField::Struct(struct_field) => {
                self.structure_sized(
                    &struct_field.struct_type.type_name,
                    &struct_field.struct_type.fields,
                    match struct_field.binding.occupied_bytes() {
                        Some(bytes) => PackSize::Known(bytes.size),
                        None => PackSize::Skip,
                    },
                )?;

                self.reference(&struct_field.struct_type.type_name, scope)?
            }
            StructField::Matrix(matrix_field) => {
                let scalar = vector_scalar(&matrix_field.element_type);
                let is_supported_matrix = matrix_field.row_count == 4
                    && matrix_field.column_count == 4
                    && !matches!(scalar, ScalarType::Uint64);
                ensure!(
                    is_supported_matrix,
                    "unsupported logical matrix (requires 4x4 F32/I32/U32)"
                );
                self.matrix(scalar, scope)?
            }
            StructField::Resource(resource_field) => {
                bail!(unsupported_resource(&resource_field.field_name))
            }
            StructField::Pointer(pointer_field) => {
                self.structure_sized(
                    &pointer_field.pointee_type.type_name,
                    &pointer_field.pointee_type.fields,
                    PackSize::Known(pointer_field.pointee_size),
                )?;

                self.expose(scope, "PointerAddress".into())
            }
            StructField::Array(array_field) => {
                let is_supported_element = array_field.element_stride == 16
                    && !matches!(array_field.element_scalar_type, ScalarType::Uint64);
                ensure!(
                    is_supported_element,
                    "unsupported logical fixed array element"
                );

                fragment(&RocApplyTemplate {
                    head: "List".into(),
                    argument: self.vector(4, array_field.element_scalar_type, scope)?,
                    hugging: true,
                })
            }
            StructField::Enum(enum_field) => {
                let enum_type = &enum_field.enum_type;
                let name = identifier(&enum_type.type_name, true)?;
                let mut names = Names::default();
                let mut cases = Vec::new();
                ensure!(
                    !enum_type.cases.is_empty(),
                    "empty logical enum {}",
                    enum_type.type_name
                );

                for case in &enum_type.cases {
                    let tag = identifier(&case.name, true)?;
                    names.insert(&tag, &case.name)?;
                    match enum_type.tag_type {
                        EnumTagType::Uint32 => {
                            u32::try_from(case.value).context("enum value outside U32")?;
                        }
                        EnumTagType::Int32 => {
                            i32::try_from(case.value).context("enum value outside I32")?;
                        }
                    }
                    cases.push(EnumArm {
                        tag,
                        value: case.value,
                    });
                }

                let tag = LogicalEnumTemplate {
                    name,
                    numeric: match enum_type.tag_type {
                        EnumTagType::Uint32 => "U32",
                        EnumTagType::Int32 => "I32",
                    },
                    cases,
                };
                self.define(&enum_type.type_name, format!("[{}]", tag.case_list()))?;
                self.associate(&enum_type.type_name, fragment(&tag), "tag functions")?;

                self.reference(&enum_type.type_name, scope)?
            }
            StructField::DescriptorHandle(_) => self.expose(scope, "DescriptorHandle".into()),
        })
    }

    fn render_definitions(&self, module: Option<&str>) -> Vec<String> {
        self.definitions
            .iter()
            .filter(|(_, (original, _))| module.is_none_or(|module| self.owner(original) == module))
            .map(|(key, (original, body))| {
                let definition = LogicalDefinitionTemplate {
                    name: identifier(original, true).expect("defined names are valid"),
                    body: body.clone(),
                    associated: self.associated.get(key).map(|(_, block)| indent_roc(block)),
                };

                indent_roc(&fragment(&definition))
            })
            .collect()
    }

    #[cfg(test)]
    fn shader_types(&self) -> String {
        let exposed: Vec<String> = self
            .exposed
            .values()
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        ShaderTypesTemplate {
            module: "ShaderTypes".into(),
            imports: vec![],
            exposed,
            definitions: self.render_definitions(None),
        }
        .render()
        .expect("static template")
    }
}

#[derive(Template)]
#[template(path = "shader_types.roc.askama", escape = "none")]
struct ShaderTypesTemplate {
    module: String,
    imports: Vec<String>,
    /// `ShaderReflection` type names this module names unqualified
    exposed: Vec<String>,
    definitions: Vec<String>,
}

#[derive(Template)]
#[template(path = "logical_definition.roc.askama", escape = "none")]
struct LogicalDefinitionTemplate {
    name: String,
    body: String,
    associated: Option<String>,
}

struct PackField {
    offset: usize,
    expr: String,
}

#[derive(Template)]
#[template(path = "logical_pack.roc.askama", escape = "none")]
struct LogicalPackTemplate {
    name: String,
    size: usize,
    fields: Vec<PackField>,
}

fn scalar_bytes(scalar: ScalarType) -> &'static str {
    match scalar {
        ScalarType::Float32 => "f32",
        ScalarType::Int32 => "i32",
        ScalarType::Uint32 => "u32",
        ScalarType::Uint64 => "u64",
    }
}

#[derive(Template)]
#[template(path = "logical_struct.roc.askama", escape = "none")]
struct LogicalStructTemplate {
    members: Vec<RocField>,
}

struct EnumArm {
    tag: String,
    value: i64,
}

#[derive(Template)]
#[template(path = "logical_enum.roc.askama", escape = "none")]
struct LogicalEnumTemplate {
    name: String,
    numeric: &'static str,
    cases: Vec<EnumArm>,
}

impl LogicalEnumTemplate {
    fn case_list(&self) -> String {
        self.cases
            .iter()
            .map(|case| case.tag.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn unsupported_resource(field_name: &str) -> String {
    format!(
        "unsupported logical resource field {field_name:?}: a bare Texture2D or \
         RWTexture2D field has no Roc value; declare it as a `.Handle` field"
    )
}

fn scalar(scalar: ScalarType) -> &'static str {
    match scalar {
        ScalarType::Float32 => "F32",
        ScalarType::Int32 => "I32",
        ScalarType::Uint32 => "U32",
        ScalarType::Uint64 => "U64",
    }
}

fn vector_scalar(element: &VectorElementType) -> ScalarType {
    match element {
        VectorElementType::Scalar(scalar) => scalar.scalar_type,
    }
}

fn alias_prefix(scalar: ScalarType) -> &'static str {
    match scalar {
        ScalarType::Float32 => "Float",
        ScalarType::Int32 => "Int",
        ScalarType::Uint32 => "Uint",
        ScalarType::Uint64 => "Uint64x",
    }
}

fn vector_name(count: usize, scalar: ScalarType) -> String {
    format!("{}{count}", alias_prefix(scalar))
}

fn matrix_name(scalar: ScalarType) -> String {
    format!("{}4x4", alias_prefix(scalar))
}

fn is_builtin_alias(name: &str) -> bool {
    let scalars = [
        ScalarType::Float32,
        ScalarType::Int32,
        ScalarType::Uint32,
        ScalarType::Uint64,
    ];

    scalars.into_iter().any(|scalar| {
        matrix_name(scalar).eq_ignore_ascii_case(name)
            || (1..=4).any(|count| vector_name(count, scalar).eq_ignore_ascii_case(name))
    })
}

impl Logical {
    fn vector(&mut self, count: usize, element: ScalarType, scope: &str) -> Result<String> {
        ensure!(
            (1..=4).contains(&count),
            "unsupported logical vector width {count}"
        );

        Ok(self.expose(scope, vector_name(count, element)))
    }

    fn matrix(&mut self, element: ScalarType, scope: &str) -> Result<String> {
        Ok(self.expose(scope, matrix_name(element)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempProject(PathBuf);

    impl TempProject {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("mltrs-roc-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(path.join("source")).unwrap();

            Self(path)
        }

        fn config(&self) -> RocConfig {
            RocConfig {
                roc_source_dir: self.0.join("Generated"),
                shaders_source_dir: self.0.join("source"),
                compiled_shaders_dir: self.0.join("compiled"),
                project_root: self.0.clone(),
                optimization: OptimizationLevel::default(),
            }
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_app_main(generated: &Path) {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let platform = repo
            .join("roc-platform/platform/main.roc")
            .canonicalize()
            .unwrap();
        // An empty-graph app depends on no generated shader module.
        let body = "import pf.Game\nimport pf.RenderGraph\nimport pf.Graphs\n\ngame : Game\ngame = Game.new({ init!, graphs, draw })\n\nOk(graphs) = Graphs.single(RenderGraph.empty)\n\ninit! : {} => Game.Init\ninit! = |_| { window_title: \"codegen test\" }\n\ndraw = |_frame| graphs.draw({})\n";
        fs::write(
            generated.join("main.roc"),
            format!(
                "app [game] {{ pf: platform \"{}\" }}\n\n{body}",
                platform.display()
            ),
        )
        .unwrap();
    }

    #[test]
    fn empty_source_clears_stale_outputs_deterministically() {
        let project = TempProject::new();
        let config = project.config();
        for dir in [&config.roc_source_dir, &config.compiled_shaders_dir] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join("obsolete.json"), "old").unwrap();
        }
        write_precompiled_roc_shaders(config.clone()).unwrap();
        assert_eq!(
            fs::read_dir(&config.compiled_shaders_dir).unwrap().count(),
            0
        );
        let names = fs::read_dir(&config.roc_source_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            names,
            ["ShaderAtlas.roc"].into_iter().map(String::from).collect()
        );
        let first = fs::read(config.roc_source_dir.join("ShaderAtlas.roc")).unwrap();
        write_precompiled_roc_shaders(config.clone()).unwrap();
        assert_eq!(
            first,
            fs::read(config.roc_source_dir.join("ShaderAtlas.roc")).unwrap()
        );
    }

    #[test]
    fn both_outputs_reject_source_overlap_and_preserve_every_tree() {
        for select_roc in [true, false] {
            for bad in ["root", "source", "source/child"] {
                let project = TempProject::new();
                let mut config = project.config();
                let source_sentinel = project.0.join("source/sentinel");
                fs::write(&source_sentinel, "keep source").unwrap();
                let before = seed_outputs(&config);
                let bad_path = match bad {
                    "root" => project.0.clone(),
                    "source" => project.0.join("source"),
                    "source/child" => project.0.join("source/child"),
                    _ => unreachable!(),
                };
                if select_roc {
                    config.roc_source_dir = bad_path;
                } else {
                    config.compiled_shaders_dir = bad_path;
                }
                assert!(write_precompiled_roc_shaders(config).is_err());
                assert_eq!(fs::read_to_string(&source_sentinel).unwrap(), "keep source");
                assert_eq!(tree_contents(&project.config().roc_source_dir), before.0);
                assert_eq!(
                    tree_contents(&project.config().compiled_shaders_dir),
                    before.1
                );
            }
        }
    }

    fn tree_contents(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut contents = BTreeMap::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries {
                let entry = entry.unwrap();
                contents.insert(
                    entry.file_name().into_string().unwrap(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }

        contents
    }

    fn seed_outputs(config: &RocConfig) -> (BTreeMap<String, Vec<u8>>, BTreeMap<String, Vec<u8>>) {
        fs::create_dir_all(&config.roc_source_dir).unwrap();
        fs::create_dir_all(&config.compiled_shaders_dir).unwrap();
        fs::write(config.roc_source_dir.join("old.roc"), "old roc").unwrap();
        fs::write(config.compiled_shaders_dir.join("old.spv"), b"old spv").unwrap();

        (
            tree_contents(&config.roc_source_dir),
            tree_contents(&config.compiled_shaders_dir),
        )
    }

    fn copy_basic_shader(project: &TempProject, name: &str) {
        fs::copy(
            crate::util::manifest_path(["fixtures", "shaders", "basic_triangle.shader.slang"]),
            project.0.join("source").join(name),
        )
        .unwrap();
        fs::copy(
            crate::util::manifest_path(["fixtures", "shaders", "mltrs.slang"]),
            project.0.join("source/mltrs.slang"),
        )
        .unwrap();
    }

    #[test]
    fn shader_uniform_requires_exactly_one_constant_buffer() {
        for count in 0..=2 {
            let project = TempProject::new();
            let config = project.config();
            let declarations = (0..count)
                .map(|index| format!("ParameterBlock<Params> params{index};\n"))
                .collect::<String>();
            let color = (0..count).fold("float4(1.0)".to_owned(), |color, index| {
                format!("{color} + params{index}.color")
            });
            fs::write(
                config.shaders_source_dir.join("uniforms.shader.slang"),
                format!(
                    r#"#language slang 2026
struct Params {{ float4 color; }}
{declarations}
[shader("vertex")]
float4 vertexMain(uint id : SV_VertexID) : SV_Position {{
    return float4(float(id), 0.0, 0.0, 1.0);
}}
[shader("fragment")]
float4 fragmentMain() : SV_Target {{ return {color}; }}
"#
                ),
            )
            .unwrap();
            write_precompiled_roc_shaders(config.clone()).unwrap();
            let module = fs::read_to_string(config.roc_source_dir.join("Uniforms.roc")).unwrap();
            assert_eq!(module.contains("uniform: params0,"), count == 1, "{module}");
            for index in 0..count {
                assert!(module.contains(&format!("params{index} : UniformBinding(Params)")));
            }
        }
    }

    #[test]
    fn late_failures_preserve_both_managed_trees() {
        for case in [
            "compile",
            "keyword",
            "mapping",
            "import_path",
            "shared_module_names",
        ] {
            let project = TempProject::new();
            let mut config = project.config();
            let before = seed_outputs(&config);
            match case {
                "compile" => fs::write(
                    project.0.join("source/broken.shader.slang"),
                    "not valid slang",
                )
                .unwrap(),
                "keyword" => copy_basic_shader(&project, "package.shader.slang"),
                "mapping" => {
                    let shader = |module: &str, ty: &str| {
                        format!(
                            "#language slang 2026\nmodule {module};\nParameterBlock<SharedParams> params;\nstruct SharedParams {{ {ty} value; }}\n[shader(\"vertex\")] float4 vertexMain(uint id : SV_VertexID) : SV_Position {{ return float4(0.0); }}\n[shader(\"fragment\")] float4 fragmentMain() : SV_Target {{ return float4(1.0); }}\n"
                        )
                    };
                    fs::write(
                        project.0.join("source/a.shader.slang"),
                        shader("a", "float"),
                    )
                    .unwrap();
                    fs::write(
                        project.0.join("source/b.shader.slang"),
                        shader("b", "float2"),
                    )
                    .unwrap();
                }
                "shared_module_names" => {
                    copy_basic_shader(&project, "good.shader.slang");
                    for module in ["one_thing", "oneThing"] {
                        fs::write(
                            project.0.join(format!("source/{module}.slang")),
                            format!("module {module};\n"),
                        )
                        .unwrap();
                    }
                }
                "import_path" => {
                    copy_basic_shader(&project, "good.shader.slang");
                    config.compiled_shaders_dir = project.0.join("compiled${bad}");
                    fs::create_dir_all(&config.compiled_shaders_dir).unwrap();
                    fs::write(config.compiled_shaders_dir.join("old.spv"), b"old spv").unwrap();
                }
                _ => unreachable!(),
            }
            let roc_before = tree_contents(&config.roc_source_dir);
            let compiled_before = tree_contents(&config.compiled_shaders_dir);
            let error = write_precompiled_roc_shaders(config.clone()).unwrap_err();
            if case == "keyword" {
                assert!(
                    error.to_string().contains("reserved Roc identifier"),
                    "{error:#}"
                );
            }
            assert_eq!(tree_contents(&config.roc_source_dir), roc_before);
            assert_eq!(tree_contents(&config.compiled_shaders_dir), compiled_before);
            if case != "import_path" {
                assert_eq!((roc_before, compiled_before), before);
            }
        }
    }

    #[test]
    fn overlapping_managed_outputs_are_rejected_without_changes() {
        for nested in [false, true] {
            let project = TempProject::new();
            let mut config = project.config();
            let before = seed_outputs(&config);
            config.compiled_shaders_dir = if nested {
                config.roc_source_dir.join("compiled")
            } else {
                config.roc_source_dir.clone()
            };
            assert!(write_precompiled_roc_shaders(config).is_err());
            assert_eq!(tree_contents(&project.config().roc_source_dir), before.0);
            assert_eq!(
                tree_contents(&project.config().compiled_shaders_dir),
                before.1
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_parent_normalization_preserves_source() {
        for select_roc in [true, false] {
            let project = TempProject::new();
            fs::create_dir_all(project.0.join("source/nested")).unwrap();
            let source_sentinel = project.0.join("source/sentinel");
            fs::write(&source_sentinel, "keep source").unwrap();
            std::os::unix::fs::symlink(project.0.join("source/nested"), project.0.join("alias"))
                .unwrap();
            assert_eq!(
                normalize(&project.0.join("alias/..")).unwrap(),
                fs::canonicalize(project.0.join("source")).unwrap()
            );
            let mut config = project.config();
            let before = seed_outputs(&config);
            if select_roc {
                config.roc_source_dir = project.0.join("alias/../new");
            } else {
                config.compiled_shaders_dir = project.0.join("alias/../new");
            }
            assert!(write_precompiled_roc_shaders(config).is_err());
            assert!(!project.0.join("source/new").exists());
            assert_eq!(fs::read_to_string(&source_sentinel).unwrap(), "keep source");
            assert_eq!(tree_contents(&project.config().roc_source_dir), before.0);
            assert_eq!(
                tree_contents(&project.config().compiled_shaders_dir),
                before.1
            );
        }
    }

    #[test]
    fn names_collisions_and_escaping() {
        assert_eq!(identifier("MyShader", false).unwrap(), "my_shader");
        assert_eq!(identifier("my_shader", true).unwrap(), "MyShader");
        for keyword in [
            "and",
            "app",
            "as",
            "break",
            "crash",
            "dbg",
            "else",
            "expect",
            "exposes",
            "exposing",
            "for",
            "generates",
            "has",
            "hosted",
            "if",
            "implements",
            "import",
            "imports",
            "in",
            "interface",
            "match",
            "module",
            "or",
            "package",
            "packages",
            "platform",
            "provides",
            "requires",
            "return",
            "targets",
            "var",
            "where",
            "while",
            "with",
        ] {
            assert!(identifier(keyword, false).is_err(), "{keyword}");
        }
        assert!(identifier("bad-name", true).is_err());
        let mut names = Names::default();
        names.insert("Foo", "foo").unwrap();
        assert!(names.insert("foo", "FOO").is_err());
        assert_eq!(quoted("\"\\\n${x}"), "\"\\\"\\\\\\n\\u(24){x}\"");
    }

    #[test]
    fn imports_are_relative_and_reject_raw_token_traps() {
        assert_eq!(
            import_path(
                Path::new("/project/Generated"),
                Path::new("/project/shaders with spaces/a.spv")
            )
            .unwrap(),
            "../shaders with spaces/a.spv"
        );
        for bad in ["a\"b", "a\\b", "a${b}", "a\nb"] {
            assert!(import_path(Path::new("/p/gen"), &Path::new("/p").join(bad)).is_err());
        }
    }

    fn text<T: Lower + ?Sized>(value: &T) -> String {
        value.lower(&mut StructTypes::default()).unwrap().render()
    }

    fn synthetic_module(module: &str, kind: &'static str, value: &impl Lower) -> String {
        let mut types = StructTypes::default();
        let reflection = value.lower(&mut types).unwrap().render();
        let struct_types = types.definitions();
        let mut exposed = vec![kind.to_owned()];
        if !struct_types.is_empty() {
            exposed.push("StructType".into());
        }
        exposed.sort();

        ShaderModule {
            module: module.into(),
            logical_definitions: vec![],
            imports: vec![],
            exposed,
            stage_imports: vec![],
            reflection_json_path: None,
            struct_types,
            reflection_kind: kind,
            reflection: continued(&reflection, 1),
            stages: vec![],
            shader_name: None,
            uniforms: vec![],
            default_uniform: None,
            vertex_input: None,
        }
        .render()
        .unwrap()
    }

    #[test]
    fn reflection_numeric_and_optional_boundaries() {
        assert_eq!(
            text(&EnumCase {
                name: "Minimum".into(),
                value: i64::MIN
            }),
            "{\n    name: \"Minimum\",\n    value: -9223372036854775808,\n}"
        );
        assert_eq!(
            text(&EnumCase {
                name: "Maximum".into(),
                value: i64::from(u32::MAX)
            }),
            "{\n    name: \"Maximum\",\n    value: 4294967295,\n}"
        );
        assert_eq!(text(&None::<u32>), "None");
        assert_eq!(text(&Some(0u32)), "Some(0)");
        assert_eq!(text::<[StructField]>(&[]), "[]");
    }

    #[test]
    fn struct_types_share_identical_definitions_and_reject_conflicts() {
        let inner = |scalar_type| {
            StructField::Scalar(ScalarStructField {
                field_name: "inner".into(),
                binding: Binding::Uniform(OffsetSizeBinding { offset: 0, size: 4 }),
                scalar_type,
            })
        };
        let mut types = StructTypes::default();
        assert_eq!(
            types
                .reference("Nested", &[inner(ScalarType::Int32)])
                .unwrap(),
            RocExpr::Literal("nested_type".into())
        );
        types
            .reference("Nested", &[inner(ScalarType::Int32)])
            .unwrap();
        assert_eq!(types.definitions.len(), 1);
        assert!(
            types
                .reference("Nested", &[inner(ScalarType::Float32)])
                .is_err()
        );
        assert!(
            types
                .reference("nested", &[inner(ScalarType::Int32)])
                .is_err()
        );
        let outer = StructField::Struct(StructStructField {
            field_name: "child".into(),
            binding: Binding::Uniform(OffsetSizeBinding { offset: 0, size: 4 }),
            struct_type: StructFieldType {
                type_name: "Nested".into(),
                fields: vec![inner(ScalarType::Int32)],
            },
        });
        types.reference("Outer", &[outer]).unwrap();
        assert_eq!(
            types
                .definitions()
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            ["nested_type", "outer_type"]
        );
    }

    fn all_variant_reflections() -> (ReflectionJson, ComputeReflectionJson) {
        let nested = StructFieldType {
            type_name: "Nested".into(),
            fields: vec![StructField::Scalar(ScalarStructField {
                field_name: "inner".into(),
                binding: Binding::Uniform(OffsetSizeBinding { offset: 0, size: 4 }),
                scalar_type: ScalarType::Int32,
            })],
        };
        let bindings = [
            Binding::Uniform(OffsetSizeBinding { offset: 0, size: 4 }),
            Binding::PushConstant(OffsetSizeBinding { offset: 4, size: 8 }),
            Binding::DescriptorTableSlot(IndexCountBinding { index: 2, count: 0 }),
            Binding::VaryingInput(IndexCountBinding { index: 3, count: 1 }),
            Binding::ConstantBuffer(IndexCountBinding {
                index: 4,
                count: usize::MAX,
            }),
        ];
        let fields = vec![
            StructField::Scalar(ScalarStructField {
                field_name: "scalar".into(),
                binding: bindings[0].clone(),
                scalar_type: ScalarType::Uint64,
            }),
            StructField::Vector(VectorStructField::Bound(BoundVectorStructField {
                field_name: "bound_vec".into(),
                binding: bindings[3].clone(),
                element_count: 4,
                element_type: VectorElementType::Scalar(ScalarVectorElementType {
                    scalar_type: ScalarType::Uint32,
                }),
            })),
            StructField::Vector(VectorStructField::Semantic(SemanticVectorStructField {
                field_name: "semantic_vec".into(),
                semantic_name: "SV_POSITION".into(),
                element_count: 3,
                element_type: VectorElementType::Scalar(ScalarVectorElementType {
                    scalar_type: ScalarType::Float32,
                }),
            })),
            StructField::Struct(StructStructField {
                field_name: "nested".into(),
                binding: bindings[0].clone(),
                struct_type: nested.clone(),
            }),
            StructField::Matrix(MatrixStructField {
                field_name: "matrix".into(),
                binding: bindings[0].clone(),
                row_count: 3,
                column_count: 2,
                element_type: VectorElementType::Scalar(ScalarVectorElementType {
                    scalar_type: ScalarType::Int32,
                }),
            }),
            StructField::Resource(ResourceStructField {
                field_name: "scalar_resource".into(),
                binding: bindings[2].clone(),
                resource_shape: ResourceShape::Texture2D,
                result_type: ResourceResultType::Scalar(ScalarResultType {
                    scalar_type: ScalarType::Float32,
                }),
            }),
            StructField::Resource(ResourceStructField {
                field_name: "vector_resource".into(),
                binding: bindings[2].clone(),
                resource_shape: ResourceShape::RWTexture2D,
                result_type: ResourceResultType::Vector(VectorResultType {
                    element_count: 4,
                    element_type: VectorElementType::Scalar(ScalarVectorElementType {
                        scalar_type: ScalarType::Uint32,
                    }),
                }),
            }),
            StructField::Resource(ResourceStructField {
                field_name: "struct_resource".into(),
                binding: bindings[2].clone(),
                resource_shape: ResourceShape::Texture2D,
                result_type: ResourceResultType::Struct(StructResultType {
                    type_name: nested.type_name.clone(),
                    fields: nested.fields.clone(),
                }),
            }),
            StructField::Pointer(PointerStructField {
                field_name: "read_write_pointer".into(),
                binding: bindings[0].clone(),
                pointee_type: nested.clone(),
                pointee_size: 4,
                access: PointerAccess::ReadWrite,
            }),
            StructField::Pointer(PointerStructField {
                field_name: "read_pointer".into(),
                binding: bindings[0].clone(),
                pointee_type: nested.clone(),
                pointee_size: 4,
                access: PointerAccess::Read,
            }),
            StructField::Pointer(PointerStructField {
                field_name: "immutable_pointer".into(),
                binding: bindings[0].clone(),
                pointee_type: nested.clone(),
                pointee_size: 4,
                access: PointerAccess::Immutable,
            }),
            StructField::Array(ArrayStructField {
                field_name: "array".into(),
                binding: bindings[0].clone(),
                element_scalar_type: ScalarType::Int32,
                element_count: 2,
                element_stride: 16,
            }),
            StructField::Enum(EnumStructField {
                field_name: "signed_enum".into(),
                binding: bindings[0].clone(),
                enum_type: EnumFieldType {
                    type_name: "Signed".into(),
                    tag_type: EnumTagType::Int32,
                    cases: vec![EnumCase {
                        name: "Minimum".into(),
                        value: i64::from(i32::MIN),
                    }],
                },
            }),
            StructField::Enum(EnumStructField {
                field_name: "unsigned_enum".into(),
                binding: bindings[0].clone(),
                enum_type: EnumFieldType {
                    type_name: "Unsigned".into(),
                    tag_type: EnumTagType::Uint32,
                    cases: vec![EnumCase {
                        name: "Maximum".into(),
                        value: i64::from(u32::MAX),
                    }],
                },
            }),
            StructField::DescriptorHandle(DescriptorHandleStructField {
                field_name: "sampled".into(),
                binding: bindings[0].clone(),
                shape: DescriptorHandleShape::Sampler2D,
            }),
            StructField::DescriptorHandle(DescriptorHandleStructField {
                field_name: "storage".into(),
                binding: bindings[0].clone(),
                shape: DescriptorHandleShape::RwTexture2D,
            }),
        ];
        let graphics = ReflectionJson {
            source_file_name: "synthetic.shader.slang".into(),
            global_parameters: vec![
                GlobalParameter::ParameterBlock(ParameterBlockGlobalParameter {
                    parameter_name: "params".into(),
                    element_type: ParameterBlockElementType {
                        type_name: "Params".into(),
                        fields: fields.clone(),
                    },
                }),
                GlobalParameter::PushConstant(PushConstantGlobalParameter {
                    parameter_name: "push".into(),
                    element_type: ParameterBlockElementType {
                        type_name: "Push".into(),
                        fields: vec![],
                    },
                    element_size: 0,
                }),
            ],
            vertex_entry_point: EntryPoint {
                entry_point_name: "vertexOriginal".into(),
                stage: EntryPointStage::Vertex,
                parameters: vec![
                    EntryPointParameter::Struct(StructEntryPointParameter {
                        parameter_name: "vertex".into(),
                        binding: None,
                        type_name: "Vertex".into(),
                        fields: vec![],
                    }),
                    EntryPointParameter::Struct(StructEntryPointParameter {
                        parameter_name: "bound_vertex".into(),
                        binding: Some(bindings[3].clone()),
                        type_name: "BoundVertex".into(),
                        fields: fields.clone(),
                    }),
                    EntryPointParameter::Scalar(ScalarEntryPointParameter::Bound(
                        BoundScalarEntryPointParameter {
                            parameter_name: "bound_scalar".into(),
                            binding: bindings[4].clone(),
                            scalar_type: ScalarType::Int32,
                        },
                    )),
                    EntryPointParameter::Scalar(ScalarEntryPointParameter::Semantic(
                        SemanticScalarEntryPointParameter {
                            parameter_name: "semantic_scalar".into(),
                            semantic_name: "SV_SAMPLEINDEX".into(),
                            scalar_type: ScalarType::Uint32,
                        },
                    )),
                ],
            },
            fragment_entry_point: EntryPoint {
                entry_point_name: "fragmentOriginal".into(),
                stage: EntryPointStage::Fragment,
                parameters: vec![],
            },
            pipeline_layout: ReflectedPipelineLayout {
                descriptor_set_layouts: vec![ReflectedDescriptorSetLayout {
                    binding_ranges: vec![
                        (ReflectedBindingType::Sampler, ReflectedStageFlags::Empty, 0),
                        (
                            ReflectedBindingType::Texture,
                            ReflectedStageFlags::Vertex,
                            1,
                        ),
                        (
                            ReflectedBindingType::ConstantBuffer,
                            ReflectedStageFlags::Fragment,
                            2,
                        ),
                        (
                            ReflectedBindingType::CombinedTextureSampler,
                            ReflectedStageFlags::Compute,
                            3,
                        ),
                        (
                            ReflectedBindingType::StorageImage,
                            ReflectedStageFlags::All,
                            4,
                        ),
                    ]
                    .into_iter()
                    .map(|(descriptor_type, stage_flags, binding)| {
                        ReflectedDescriptorSetLayoutBinding {
                            binding,
                            descriptor_type,
                            descriptor_count: binding,
                            stage_flags,
                            size: usize::try_from(binding).unwrap(),
                        }
                    })
                    .collect(),
                }],
                push_constant_ranges: vec![ReflectedPushConstantRange {
                    stage_flags: ReflectedStageFlags::All,
                    offset: 0,
                    size: 128,
                }],
                bindless_heap_set: Some(0),
            },
        };
        let compute = ComputeReflectionJson {
            source_file_name: "synthetic.compute.slang".into(),
            global_parameters: vec![],
            compute_entry_point: EntryPoint {
                entry_point_name: "computeOriginal".into(),
                stage: EntryPointStage::Compute,
                parameters: vec![],
            },
            workgroup_size: [0, 1, u32::MAX],
            pipeline_layout: ReflectedPipelineLayout {
                descriptor_set_layouts: vec![],
                push_constant_ranges: vec![],
                bindless_heap_set: None,
            },
        };

        (graphics, compute)
    }

    #[test]
    fn reflection_schema_all_variants_snapshot() {
        let (graphics, compute) = all_variant_reflections();
        insta::assert_snapshot!(
            "roc_reflection_all_variants_graphics",
            synthetic_module(
                "GeneratedSyntheticGraphics",
                "GraphicsReflection",
                &graphics
            )
        );
        insta::assert_snapshot!(
            "roc_reflection_all_variants_compute",
            synthetic_module("GeneratedSyntheticCompute", "ComputeReflection", &compute)
        );
    }

    #[test]
    #[ignore = "explicit real-Roc gate; run with just roc-codegen-test"]
    fn emitted_all_variant_reflection_passes_real_roc() {
        use std::process::Command;

        let (graphics, compute) = all_variant_reflections();
        let project = TempProject::new();
        let generated = project.0.join("Generated");
        fs::create_dir_all(&generated).unwrap();
        fs::write(
            generated.join("GeneratedSyntheticGraphics.roc"),
            canonical_roc_indentation(&synthetic_module(
                "GeneratedSyntheticGraphics",
                "GraphicsReflection",
                &graphics,
            )),
        )
        .unwrap();
        fs::write(
            generated.join("GeneratedSyntheticCompute.roc"),
            canonical_roc_indentation(&synthetic_module(
                "GeneratedSyntheticCompute",
                "ComputeReflection",
                &compute,
            )),
        )
        .unwrap();
        fs::write(
            generated.join("GeneratedSyntheticConsumer.roc"),
            canonical_roc_indentation(r#"import GeneratedSyntheticGraphics
import GeneratedSyntheticCompute

GeneratedSyntheticConsumer := {}

expect List.len(GeneratedSyntheticGraphics.reflection.global_parameters) == 2
expect match GeneratedSyntheticGraphics.reflection.global_parameters |> List.get(0) {
    Ok(ParameterBlock(payload)) => List.len(payload.element_type.fields) == 16
    _ => False
}
expect match GeneratedSyntheticGraphics.reflection.global_parameters |> List.get(0) {
    Ok(ParameterBlock(payload)) => match payload.element_type.fields |> List.get(8) {
        Ok(Pointer(pointer)) => pointer.access == ReadWrite
        _ => False
    }
    _ => False
}
expect match GeneratedSyntheticGraphics.reflection.global_parameters |> List.get(0) {
    Ok(ParameterBlock(payload)) => match payload.element_type.fields |> List.get(9) {
        Ok(Pointer(pointer)) => pointer.access == Read
        _ => False
    }
    _ => False
}
expect match GeneratedSyntheticGraphics.reflection.global_parameters |> List.get(0) {
    Ok(ParameterBlock(payload)) => match payload.element_type.fields |> List.get(10) {
        Ok(Pointer(pointer)) => pointer.access == Immutable
        _ => False
    }
    _ => False
}
expect match GeneratedSyntheticGraphics.reflection.global_parameters |> List.get(0) {
    Ok(ParameterBlock(payload)) => match payload.element_type.fields |> List.get(13) {
        Ok(Enum(value)) => value.enum_type.tag_type == Uint32 and value.enum_type.cases |> List.get(0) == Ok({ name: "Maximum", value: 4294967295 })
        _ => False
    }
    _ => False
}
expect GeneratedSyntheticGraphics.reflection.pipeline_layout.bindless_heap_set == Some(0)
expect GeneratedSyntheticCompute.reflection.workgroup_size == { x: 0, y: 1, z: 4294967295 }
expect GeneratedSyntheticCompute.reflection.pipeline_layout.bindless_heap_set == None
"#),
        )
        .unwrap();
        write_app_main(&generated);

        let formatted = Command::new("roc")
            .arg("fmt")
            .arg(&generated)
            .output()
            .expect("format synthetic modules");
        assert!(
            formatted.status.success(),
            "{}{}",
            String::from_utf8_lossy(&formatted.stdout),
            String::from_utf8_lossy(&formatted.stderr)
        );
        let format = Command::new("roc")
            .args(["fmt", "--check"])
            .arg(&generated)
            .output()
            .expect("run roc fmt check");
        assert!(
            format.status.success(),
            "{}{}",
            String::from_utf8_lossy(&format.stdout),
            String::from_utf8_lossy(&format.stderr)
        );
        let output = Command::new("roc")
            .arg("test")
            .arg(format!("--main={}", generated.join("main.roc").display()))
            .arg(generated.join("GeneratedSyntheticConsumer.roc"))
            .current_dir(std::env::temp_dir())
            .output()
            .expect("run roc test");
        // platform modules add their own expectations to the run
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success()
                && stdout.contains("tests passed")
                && !stdout.contains("failed"),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn logical_enums_define_tags_and_a_tag_function() {
        let mut logical = Logical::default();
        let field = StructField::Enum(EnumStructField {
            field_name: "mode".into(),
            binding: Binding::Uniform(OffsetSizeBinding { offset: 0, size: 4 }),
            enum_type: EnumFieldType {
                type_name: "Mode".into(),
                tag_type: EnumTagType::Int32,
                cases: vec![
                    EnumCase {
                        name: "Off".into(),
                        value: -1,
                    },
                    EnumCase {
                        name: "On".into(),
                        value: 1,
                    },
                ],
            },
        });
        assert_eq!(logical.field(&field, "").unwrap(), "Mode");
        assert_eq!(logical.definitions["mode"].1, "[Off, On]");
        assert_eq!(
            logical.associated["mode"].1,
            "tag : Mode -> I32\ntag = |value| {\n    match value {\n        Off => -1\n        On => 1\n    }\n}"
        );
        assert!(logical.shader_types().contains(
            "    Mode := [Off, On].{\n        is_eq : _\n\n        tag : Mode -> I32\n        tag = |value| {\n            match value {\n                Off => -1\n                On => 1\n            }\n        }\n    }"
        ));
    }

    #[test]
    fn logical_packers_follow_reflected_offsets_and_the_vertex_rule() {
        let reflection: ReflectionJson = serde_json::from_str(include_str!(
            "../../slang-reflection/src/fixtures/basic_triangle.json"
        ))
        .unwrap();
        let mut logical = Logical::default();
        logical.graphics(&reflection).unwrap();

        assert_eq!(
            logical.associated["mvpmatrices"].1,
            "gpu_size : U32\ngpu_size = 192\n\nto_bytes : MvpMatrices -> List(U8)\nto_bytes = |value|\n    ShaderReflection.pack(\n        gpu_size.to_u64(),\n        [\n            (0, Float4x4.to_bytes(value.model)),\n            (64, Float4x4.to_bytes(value.view)),\n            (128, Float4x4.to_bytes(value.proj)),\n        ],\n    )"
        );
        assert_eq!(
            logical.associated["vertex"].1,
            "gpu_size : U32\ngpu_size = 32\n\nto_bytes : Vertex -> List(U8)\nto_bytes = |value|\n    ShaderReflection.pack(\n        gpu_size.to_u64(),\n        [\n            (0, Float3.to_bytes(value.position)),\n            (12, Float3.to_bytes(value.color)),\n        ],\n    )"
        );
        // fragment inputs are varying data the CPU never constructs
        assert!(!logical.definitions.contains_key("fraginput"));
        assert!(!logical.associated.contains_key("fraginput"));
        let module = logical.shader_types();
        assert!(!module.contains("FragInput"));
        assert!(module.starts_with(
            "import pf.ShaderReflection exposing [Float3, Float4x4]\n\n## Generated logical values, not GPU layouts.\n## Fixed arrays are lists: required lengths are preserved in reflection.\nShaderTypes := {}.{\n    MvpMatrices := {"
        ));
        assert!(module.contains(
            "    Vertex := {\n        position : Float3,\n        color : Float3,\n    }.{\n        is_eq : _\n\n        gpu_size : U32\n        gpu_size = 32\n\n        to_bytes : Vertex -> List(U8)\n"
        ));
    }

    #[test]
    fn logical_rejects_bare_resource_fields() {
        let fields = [
            StructField::Scalar(ScalarStructField {
                field_name: "scale".into(),
                binding: Binding::Uniform(OffsetSizeBinding { offset: 0, size: 4 }),
                scalar_type: ScalarType::Float32,
            }),
            StructField::Resource(ResourceStructField {
                field_name: "scalar_resource".into(),
                binding: Binding::DescriptorTableSlot(IndexCountBinding { index: 2, count: 0 }),
                resource_shape: ResourceShape::Texture2D,
                result_type: ResourceResultType::Scalar(ScalarResultType {
                    scalar_type: ScalarType::Float32,
                }),
            }),
        ];
        let mut logical = Logical::default();
        let error = logical.structure("Params", &fields).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("logical Params.scalar_resource"),
            "{message}"
        );
        assert!(message.contains("`.Handle`"), "{message}");
        assert!(logical.definitions.is_empty());
        assert!(logical.associated.is_empty());
    }

    #[test]
    fn logical_vectors_and_shared_identity() {
        let mut logical = Logical::default();
        assert_eq!(
            logical
                .vector(2, ScalarType::Uint64, "ShaderTypes")
                .unwrap(),
            "Uint64x2"
        );
        assert!(
            logical
                .vector(5, ScalarType::Float32, "ShaderTypes")
                .is_err()
        );
        assert_eq!(
            logical.matrix(ScalarType::Float32, "ShaderTypes").unwrap(),
            "Float4x4"
        );
        assert!(logical.definitions.is_empty());
        assert!(logical.structure("Float3", &[]).is_err());
        assert!(logical.structure("float4x4", &[]).is_err());
        assert!(logical.structure("ShaderReflection", &[]).is_err());

        logical.structure("Shared", &[]).unwrap();
        logical.structure("Shared", &[]).unwrap();
        assert_eq!(logical.definitions.len(), 1);
        assert!(logical.structure("shared", &[]).is_err());
    }
}
