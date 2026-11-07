//! Roc-only shader generation. Both output directories are tool-owned.
//!
//! All compilation, name/path checks and rendering precede publication. Publication
//! itself is deliberately not transactional: an I/O failure can leave partial output.
//! Logical values have no GPU layout, ownership, binding or validity guarantees.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use std::num::TryFromIntError;

use askama::Template;
use eros::{Context, IntoUnion, ReshapeUnion};
use heck::{ToSnakeCase, ToUpperCamelCase};
pub use mltrs_slang_reflection::OptimizationLevel;
use mltrs_slang_reflection::json::*;
use mltrs_slang_reflection::{
    prepare_reflected_compute_shader_with_optimization, prepare_reflected_shader_with_optimization,
};

use crate::error::{PathIoError, ShaderReflectionFailed};

/// The configured source and output directories cannot be used together.
#[derive(Debug, thiserror::Error)]
pub enum InvalidProjectLayout {
    #[error("output {} contains project root", output.display())]
    OutputContainsProjectRoot { output: PathBuf },
    #[error("output {} overlaps source {}", output.display(), source_dir.display())]
    OutputOverlapsSource {
        output: PathBuf,
        source_dir: PathBuf,
    },
    #[error("output {} is not a directory", output.display())]
    OutputNotADirectory { output: PathBuf },
    #[error("Roc and compiled output directories overlap")]
    OutputsOverlap { roc: PathBuf, compiled: PathBuf },
    #[error("shader source {name} is not a file")]
    SourceNotAFile { name: String },
}

/// A shader source path or file name is not UTF-8, which slang requires.
#[derive(Debug, thiserror::Error)]
#[error("{what} is not UTF-8: {}", path.display())]
pub struct NonUtf8Path {
    pub what: &'static str,
    pub path: PathBuf,
}

/// Two inputs map to the same generated name (compared case-insensitively).
#[derive(Debug, thiserror::Error)]
pub enum NameCollision {
    #[error("generated name collision: {original:?} and {previous:?} both use {generated}")]
    Generated {
        original: String,
        previous: String,
        generated: String,
    },
    #[error("SPIR-V filename collision: {filename}")]
    SpirvFilename { filename: String },
}

/// A reflected name cannot become a Roc identifier.
#[derive(Debug, thiserror::Error)]
pub enum InvalidRocIdentifier {
    #[error("invalid Roc identifier {original:?}")]
    Invalid { original: String },
    #[error("reserved Roc identifier {original:?}")]
    Reserved { original: String },
}

/// The compiled output directory cannot be named by a relative Roc import path
/// from the Roc output directory.
#[derive(Debug, thiserror::Error)]
pub enum UnrepresentableImportPath {
    #[error("no relative Roc import path between output directories")]
    NoCommonAncestor { from: PathBuf, to: PathBuf },
    #[error("unrepresentable Roc import path")]
    NonNormalComponent { to: PathBuf },
    #[error("non-UTF-8 Roc import path")]
    NonUtf8 { to: PathBuf },
    #[error("unrepresentable Roc import path component {component:?}")]
    ForbiddenCharacter { component: String },
}

/// Two reflected struct types with different shapes map to one Roc type value.
#[derive(Debug, thiserror::Error)]
#[error("incompatible reflected struct types {type_name:?} and {previous:?} both use {generated}")]
pub struct IncompatibleStructTypes {
    pub type_name: String,
    pub previous: String,
    pub generated: String,
}

/// A logical type name collides with a support/builtin type or with a
/// different logical definition.
#[derive(Debug, thiserror::Error)]
pub enum LogicalTypeCollision {
    #[error("logical type {original:?} collides with support/builtin type")]
    Builtin { original: String },
    #[error("incompatible or colliding logical definitions for {original:?} and {previous:?}")]
    Incompatible { original: String, previous: String },
}

/// A reflected field has no logical Roc representation.
#[derive(Debug, thiserror::Error)]
pub enum UnsupportedLogicalType {
    #[error("unsupported logical matrix (requires 4x4 F32/I32/U32)")]
    Matrix {
        rows: u32,
        columns: u32,
        scalar: ScalarType,
    },
    #[error("unsupported logical fixed array element")]
    ArrayElement { stride: usize, scalar: ScalarType },
    #[error("unsupported logical vector width {count}")]
    VectorWidth { count: usize },
    #[error("empty logical enum {type_name}")]
    EmptyEnum { type_name: String },
    #[error("enum value outside {tag_type}: {type_name}.{case} = {value}")]
    EnumValueOutOfRange {
        type_name: String,
        case: String,
        value: i64,
        tag_type: &'static str,
    },
}

/// Failures lowering reflection JSON to Roc values.
pub type LowerErrors = (
    InvalidRocIdentifier,
    IncompatibleStructTypes,
    TryFromIntError,
);

/// Failures deriving logical Roc types from reflection.
pub type LogicalErrors = (
    InvalidRocIdentifier,
    NameCollision,
    LogicalTypeCollision,
    UnsupportedLogicalType,
);

/// A Roc-specific naming, typing or lowering failure: one variant per kind.
///
/// [`write_precompiled_roc_shaders`] classifies [`LowerErrors`] and
/// [`LogicalErrors`] into this, along with its own name and import-path checks.
/// Each variant's message is the wrapped error's.
#[derive(Debug, thiserror::Error)]
pub enum RocCodegenError {
    #[error(transparent)]
    InvalidIdentifier(#[from] InvalidRocIdentifier),
    #[error(transparent)]
    NameCollision(#[from] NameCollision),
    #[error(transparent)]
    IncompatibleStructTypes(#[from] IncompatibleStructTypes),
    #[error(transparent)]
    LogicalTypeCollision(#[from] LogicalTypeCollision),
    #[error(transparent)]
    UnsupportedLogicalType(#[from] UnsupportedLogicalType),
    #[error(transparent)]
    UnrepresentableImportPath(#[from] UnrepresentableImportPath),
    /// a `usize` reflection value does not fit the Roc `U64` it lowers to
    #[error(transparent)]
    IntConversion(#[from] TryFromIntError),
}

/// Every [`RocCodegenError`] payload type, which
/// [`RocCodegenError::classify`] checks in this order.
type RocCodegenMembers = (
    InvalidRocIdentifier,
    NameCollision,
    IncompatibleStructTypes,
    LogicalTypeCollision,
    UnsupportedLogicalType,
    UnrepresentableImportPath,
    TryFromIntError,
);

// Compile-time check that [`RocCodegenError::classify`] covers the inner sets:
// adding a member to `LowerErrors` or `LogicalErrors` without a variant (and a
// `RocCodegenMembers` entry) fails to widen here instead of reaching the
// `unreachable!` at runtime.
const _: () = {
    #[allow(dead_code)]
    fn inner_sets_are_classified(
        lower: eros::ErrorUnion<LowerErrors>,
        logical: eros::ErrorUnion<LogicalErrors>,
    ) -> [eros::ErrorUnion<RocCodegenMembers>; 2] {
        [lower.widen(), logical.widen()]
    }
};

impl RocCodegenError {
    /// The `classify` callback for a set whose members are all in
    /// [`RocCodegenMembers`], such as [`LowerErrors`] and [`LogicalErrors`].
    fn classify(inner: eros::InnerError) -> Self {
        macro_rules! classify_as {
            ($inner:ident: $($variant:ident($ty:ty)),* $(,)?) => {
                $(let $inner = match $inner.downcast::<$ty>() {
                    Ok(error) => return Self::$variant(error),
                    Err(other) => other,
                };)*
            };
        }
        classify_as!(inner:
            InvalidIdentifier(InvalidRocIdentifier),
            NameCollision(NameCollision),
            IncompatibleStructTypes(IncompatibleStructTypes),
            LogicalTypeCollision(LogicalTypeCollision),
            UnsupportedLogicalType(UnsupportedLogicalType),
            UnrepresentableImportPath(UnrepresentableImportPath),
            IntConversion(TryFromIntError),
        );
        unreachable!("not a Roc codegen error: {}", inner.as_error())
    }
}

/// Failures of [`write_precompiled_roc_shaders`]: compiling and reflecting a
/// shader ([`ShaderReflectionFailed`], which wraps the slang-reflection error),
/// filesystem access, the project layout, and Roc code generation.
pub type WriteRocShadersErrors = (
    ShaderReflectionFailed,
    PathIoError,
    InvalidProjectLayout,
    NonUtf8Path,
    RocCodegenError,
);

#[derive(Debug, Clone)]
pub struct RocConfig {
    pub roc_source_dir: PathBuf,
    pub shaders_source_dir: PathBuf,
    pub compiled_shaders_dir: PathBuf,
    pub project_root: PathBuf,
    pub optimization: OptimizationLevel,
}

/// Generate Roc and SPIR-V
// eager: `config` is owned, so the lazy form cannot borrow it after the body
#[eros::eager_context(
    "Generate Roc shader bindings from {} into {}",
    config.shaders_source_dir.display(),
    config.roc_source_dir.display()
)]
pub fn write_precompiled_roc_shaders(config: RocConfig) -> eros::Result<(), WriteRocShadersErrors> {
    let root = normalize(&config.project_root).union()?;
    let source = normalize(&config.shaders_source_dir).union()?;
    let roc = normalize(&config.roc_source_dir).union()?;
    let compiled = normalize(&config.compiled_shaders_dir).union()?;
    for output in [&roc, &compiled] {
        let invalid = if root.starts_with(output) {
            Some(InvalidProjectLayout::OutputContainsProjectRoot {
                output: output.clone(),
            })
        } else if overlap(output, &source) {
            Some(InvalidProjectLayout::OutputOverlapsSource {
                output: output.clone(),
                source_dir: source.clone(),
            })
        } else if output.exists() && !output.is_dir() {
            Some(InvalidProjectLayout::OutputNotADirectory {
                output: output.clone(),
            })
        } else {
            None
        };
        if let Some(invalid) = invalid {
            return Err(invalid).union();
        }
    }

    if overlap(&roc, &compiled) {
        return Err(InvalidProjectLayout::OutputsOverlap { roc, compiled }).union();
    }

    let Some(search) = source.to_str() else {
        return Err(NonUtf8Path {
            what: "shader source path",
            path: source,
        })
        .union();
    };
    let mut inputs = Vec::new();
    let entries = fs::read_dir(&source)
        .and_then(Iterator::collect::<std::io::Result<Vec<_>>>)
        .map_err(PathIoError::at("read shader source", &source))
        .union()?;
    for entry in entries {
        let Ok(name) = entry.file_name().into_string() else {
            return Err(NonUtf8Path {
                what: "shader source filename",
                path: entry.path(),
            })
            .union();
        };
        let is_shader_source = name.ends_with(".shader.slang") || name.ends_with(".compute.slang");
        if is_shader_source {
            if !entry.path().is_file() {
                return Err(InvalidProjectLayout::SourceNotAFile { name }).union();
            }
            inputs.push(name);
        }
    }
    inputs.sort();

    let mut modules = Names::default();
    for name in ["ShaderAtlas", "ShaderTypes", "ShaderReflection"] {
        modules
            .insert(name, name)
            .map_err(RocCodegenError::from)
            .union()?;
    }

    let mut logical = Logical::default();
    let mut roc_files = BTreeMap::new();
    let mut binaries = BTreeMap::new();
    let mut atlas = Vec::new();

    let mut atlas_names = Names::default();
    atlas_names
        .insert("shader_names", "atlas support value")
        .map_err(RocCodegenError::from)
        .union()?;

    for input in inputs {
        let compute = input.ends_with(".compute.slang");

        let suffix = if compute {
            ".compute.slang"
        } else {
            ".shader.slang"
        };
        let stem = input.strip_suffix(suffix).expect("discovered suffix");
        let module = identifier(stem, true)
            .map_err(RocCodegenError::from)
            .union()?;
        modules
            .insert(&module, &input)
            .map_err(RocCodegenError::from)
            .union()?;
        let value = identifier(stem, false)
            .map_err(RocCodegenError::from)
            .union()?;
        atlas_names
            .insert(&value, &input)
            .map_err(RocCodegenError::from)
            .union()?;
        let mut types = StructTypes::default();
        let mut stages = Vec::new();
        let (reflection_kind, reflection) = if compute {
            let shader = prepare_reflected_compute_shader_with_optimization(
                &input,
                search,
                config.optimization,
            )
            .classify(ShaderReflectionFailed::of(&input))?;
            logical
                .compute(&shader.reflection_json)
                .with_context(|| format!("Derive logical Roc types for {input}"))
                .classify(RocCodegenError::classify)?;
            stages.push((
                "compute",
                shader.compute_shader.shader_bytecode.to_vec(),
                "comp",
            ));
            (
                "ComputeReflection",
                shader
                    .reflection_json
                    .lower(&mut types)
                    .with_context(|| format!("Lower reflection of {input} to Roc"))
                    .classify(RocCodegenError::classify)?
                    .render(),
            )
        } else {
            let shader =
                prepare_reflected_shader_with_optimization(&input, search, config.optimization)
                    .classify(ShaderReflectionFailed::of(&input))?;
            logical
                .graphics(&shader.reflection_json)
                .with_context(|| format!("Derive logical Roc types for {input}"))
                .classify(RocCodegenError::classify)?;
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
                shader
                    .reflection_json
                    .lower(&mut types)
                    .with_context(|| format!("Lower reflection of {input} to Roc"))
                    .classify(RocCodegenError::classify)?
                    .render(),
            )
        };
        let mut stage_imports = Vec::new();
        let mut stage_names = Vec::new();
        for (stage, bytes, extension) in stages {
            let filename = format!("{stem}.{extension}.spv");
            if binaries
                .keys()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&filename))
            {
                return Err(RocCodegenError::from(NameCollision::SpirvFilename {
                    filename,
                }))
                .union();
            }
            let path = import_path(&roc, &compiled.join(&filename))
                .map_err(RocCodegenError::from)
                .union()?;
            stage_imports.push(StageImport { stage, path });
            stage_names.push(stage);
            binaries.insert(filename, bytes);
        }
        let shader_module = ShaderModule {
            module: module.clone(),
            stage_imports,
            struct_types: types.definitions(),
            reflection_kind,
            reflection: continued(&reflection, 1),
            stages: stage_names,
        };
        roc_files.insert(
            format!("{module}.roc"),
            shader_module.render().expect("static template"),
        );
        atlas.push(AtlasEntry { module, value });
    }

    roc_files.insert("ShaderTypes.roc".into(), logical.shader_types());
    let atlas = ShaderAtlas { modules: atlas };
    roc_files.insert(
        "ShaderAtlas.roc".into(),
        atlas.render().expect("static template"),
    );
    // No source-dependent errors may occur after this boundary.
    publish(&compiled, binaries).union()?;

    let files = roc_files
        .into_iter()
        .map(|(name, text)| (name, canonical_roc_indentation(&text).into_bytes()))
        .collect();

    publish(&roc, files).union()
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

fn publish(dir: &Path, files: BTreeMap<String, Vec<u8>>) -> Result<(), PathIoError> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(PathIoError::at("clear managed output", dir))?;
    }
    fs::create_dir_all(dir).map_err(PathIoError::at("create managed output", dir))?;
    for (name, bytes) in files {
        let path = dir.join(name);
        fs::write(&path, bytes).map_err(PathIoError::at("write managed output", &path))?;
    }

    Ok(())
}

fn overlap(first: &Path, second: &Path) -> bool {
    first.starts_with(second) || second.starts_with(first)
}

/// Resolve existing symlinks before processing `..`, including paths with a
/// not-yet-created suffix. Fail closed for dangling symlinks and inaccessible paths.
fn normalize(path: &Path) -> Result<PathBuf, PathIoError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(PathIoError::at("resolve the working directory for", path))?
            .join(path)
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
                            .map_err(PathIoError::at("resolve", &result))?
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(PathIoError::at("inspect", &result)(error));
                    }
                }
            }
        }
    }

    Ok(result)
}

fn import_path(from_path: &Path, to_path: &Path) -> Result<String, UnrepresentableImportPath> {
    let from = from_path.components().collect::<Vec<_>>();
    let to = to_path.components().collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(from_component, to_component)| from_component == to_component)
        .count();
    if common == 0 {
        return Err(UnrepresentableImportPath::NoCommonAncestor {
            from: from_path.to_path_buf(),
            to: to_path.to_path_buf(),
        });
    }
    let mut parts = vec!["..".to_string(); from.len() - common];
    for part in &to[common..] {
        if !matches!(part, Component::Normal(_)) {
            return Err(UnrepresentableImportPath::NonNormalComponent {
                to: to_path.to_path_buf(),
            });
        }
        let Some(part) = part.as_os_str().to_str() else {
            return Err(UnrepresentableImportPath::NonUtf8 {
                to: to_path.to_path_buf(),
            });
        };
        if part
            .chars()
            .any(|character| character.is_control() || matches!(character, '"' | '\\' | '$'))
        {
            return Err(UnrepresentableImportPath::ForbiddenCharacter {
                component: part.to_string(),
            });
        }
        parts.push(part.to_string());
    }

    Ok(parts.join("/"))
}

#[derive(Default)]
struct Names(BTreeMap<String, String>);

impl Names {
    fn insert(&mut self, generated: &str, original: &str) -> Result<(), NameCollision> {
        if let Some(previous) = self
            .0
            .insert(generated.to_ascii_lowercase(), original.into())
        {
            return Err(NameCollision::Generated {
                original: original.into(),
                previous,
                generated: generated.into(),
            });
        }

        Ok(())
    }
}

fn identifier(original: &str, upper: bool) -> Result<String, InvalidRocIdentifier> {
    let is_valid = !original.is_empty()
        && original
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && original.as_bytes()[0].is_ascii_alphabetic();
    if !is_valid {
        return Err(InvalidRocIdentifier::Invalid {
            original: original.into(),
        });
    }
    let name = if upper {
        original.to_upper_camel_case()
    } else {
        original.to_snake_case()
    };
    let is_reserved = [
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
    .contains(&name.as_str());
    if is_reserved {
        return Err(InvalidRocIdentifier::Reserved {
            original: original.into(),
        });
    }

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
    fn reference(
        &mut self,
        type_name: &str,
        fields: &[StructField],
    ) -> eros::Result<RocExpr, LowerErrors> {
        let name = identifier(&format!("{type_name}_type"), false).union()?;
        let fields = fields.lower(self)?;
        match self.definitions.get(&name) {
            Some(previous) => {
                if previous.type_name != type_name || previous.fields != fields {
                    return Err(IncompatibleStructTypes {
                        type_name: type_name.into(),
                        previous: previous.type_name.clone(),
                        generated: name,
                    })
                    .union();
                }
            }
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
    stage_imports: Vec<StageImport>,
    struct_types: Vec<StructTypeDef>,
    reflection_kind: &'static str,
    reflection: String,
    stages: Vec<&'static str>,
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

impl ShaderAtlas {
    fn shader_names(&self) -> String {
        self.modules
            .iter()
            .map(|entry| quoted(&entry.value))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

// Direct exhaustive schema mapping: no JSON intermediate and no numeric floats.
trait Lower {
    fn lower(&self, types: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors>;
}

impl Lower for String {
    fn lower(&self, _: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
        Ok(RocExpr::Literal(quoted(self)))
    }
}

impl Lower for usize {
    fn lower(&self, _: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
        Ok(RocExpr::Literal(u64::try_from(*self).union()?.to_string()))
    }
}

impl Lower for u32 {
    fn lower(&self, _: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
        Ok(RocExpr::Literal(self.to_string()))
    }
}

impl Lower for i64 {
    fn lower(&self, _: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
        Ok(RocExpr::Literal(self.to_string()))
    }
}

impl<T: Lower> Lower for [T] {
    fn lower(&self, types: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
        Ok(RocExpr::List(
            self.iter()
                .map(|element| element.lower(types))
                .collect::<eros::Result<_, LowerErrors>>()?,
        ))
    }
}

impl<T: Lower> Lower for Option<T> {
    fn lower(&self, types: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
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
    fn lower(&self, _: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
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
            fn lower(&self, types: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
                Ok(RocExpr::Record(vec![$((stringify!($field).into(), self.$field.lower(types)?)),+]))
            }
        }
    };
}

macro_rules! tags {
    ($ty:ty, $($variant:ident),+ $(,)?) => {
        impl Lower for $ty {
            fn lower(&self, _: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
                Ok(RocExpr::Literal(match self { $(Self::$variant => stringify!($variant)),+ }.into()))
            }
        }
    };
}

macro_rules! variants {
    ($ty:ty, $($variant:ident),+ $(,)?) => {
        impl Lower for $ty {
            fn lower(&self, types: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
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
            fn lower(&self, types: &mut StructTypes) -> eros::Result<RocExpr, LowerErrors> {
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
    definitions: BTreeMap<String, (String, String)>,
}

impl Logical {
    fn graphics(&mut self, shader: &ReflectionJson) -> eros::Result<(), LogicalErrors> {
        self.globals(&shader.global_parameters)?;
        self.entry(&shader.vertex_entry_point)?;

        self.entry(&shader.fragment_entry_point)
    }

    fn compute(&mut self, shader: &ComputeReflectionJson) -> eros::Result<(), LogicalErrors> {
        self.globals(&shader.global_parameters)?;

        self.entry(&shader.compute_entry_point)
    }

    fn globals(&mut self, globals: &[GlobalParameter]) -> eros::Result<(), LogicalErrors> {
        let mut names = Names::default();
        for global in globals {
            let (parameter_name, element) = match global {
                GlobalParameter::ParameterBlock(parameter) => {
                    (&parameter.parameter_name, &parameter.element_type)
                }
                GlobalParameter::PushConstant(parameter) => {
                    (&parameter.parameter_name, &parameter.element_type)
                }
            };
            names
                .insert(&identifier(parameter_name, false).union()?, parameter_name)
                .union()?;
            self.structure(&element.type_name, &element.fields)?;
        }

        Ok(())
    }

    fn entry(&mut self, entry: &EntryPoint) -> eros::Result<(), LogicalErrors> {
        identifier(&entry.entry_point_name, false).union()?;
        let mut names = Names::default();
        for parameter in &entry.parameters {
            let name = match parameter {
                EntryPointParameter::Struct(struct_parameter) => {
                    self.structure(&struct_parameter.type_name, &struct_parameter.fields)?;
                    &struct_parameter.parameter_name
                }
                EntryPointParameter::Scalar(ScalarEntryPointParameter::Bound(bound)) => {
                    &bound.parameter_name
                }
                EntryPointParameter::Scalar(ScalarEntryPointParameter::Semantic(semantic)) => {
                    &semantic.parameter_name
                }
            };
            names
                .insert(&identifier(name, false).union()?, name)
                .union()?;
        }

        Ok(())
    }

    fn define(
        &mut self,
        original: &str,
        definition: String,
    ) -> eros::Result<String, LogicalErrors> {
        let name = identifier(original, true).union()?;
        let reserved = [
            "PointerAddress",
            "DescriptorHandle",
            "ResourceReference",
            "ShaderTypes",
            "Reflection",
            "ShaderAtlas",
            "ShaderReflection",
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
        if reserved {
            return Err(LogicalTypeCollision::Builtin {
                original: original.into(),
            })
            .union();
        }
        if let Some((previous, body)) = self.definitions.get(&name.to_ascii_lowercase()) {
            if previous != original || body != &definition {
                return Err(LogicalTypeCollision::Incompatible {
                    original: original.into(),
                    previous: previous.clone(),
                })
                .union();
            }
        } else {
            self.definitions
                .insert(name.to_ascii_lowercase(), (original.into(), definition));
        }

        Ok(name)
    }

    fn structure(
        &mut self,
        name: &str,
        fields: &[StructField],
    ) -> eros::Result<String, LogicalErrors> {
        let mut names = Names::default();
        let mut members = Vec::new();
        for field in fields {
            let original = field.field_name();
            let member = identifier(original, false).union()?;
            names.insert(&member, original).union()?;
            let ty = self
                .field(field)
                .with_context(|| format!("logical {name}.{original}"))?;
            members.push(RocField {
                name: member,
                value: continued(&ty, 1),
            });
        }

        let definition = LogicalStructTemplate {
            name: identifier(name, true).union()?,
            members,
        };

        self.define(name, fragment(&definition))
    }

    fn field(&mut self, field: &StructField) -> eros::Result<String, LogicalErrors> {
        Ok(match field {
            StructField::Scalar(scalar_field) => scalar(scalar_field.scalar_type).into(),
            StructField::Vector(vector_field) => {
                let (count, element) = match vector_field {
                    VectorStructField::Bound(bound) => (bound.element_count, &bound.element_type),
                    VectorStructField::Semantic(semantic) => {
                        (semantic.element_count, &semantic.element_type)
                    }
                };

                self.vector(count, vector_scalar(element)).union()?
            }
            StructField::Struct(struct_field) => self.structure(
                &struct_field.struct_type.type_name,
                &struct_field.struct_type.fields,
            )?,
            StructField::Matrix(matrix_field) => {
                let scalar = vector_scalar(&matrix_field.element_type);
                let is_supported_matrix = matrix_field.row_count == 4
                    && matrix_field.column_count == 4
                    && !matches!(scalar, ScalarType::Uint64);
                if !is_supported_matrix {
                    return Err(UnsupportedLogicalType::Matrix {
                        rows: matrix_field.row_count,
                        columns: matrix_field.column_count,
                        scalar,
                    })
                    .union();
                }
                self.matrix(scalar)?
            }
            StructField::Resource(resource_field) => {
                if let ResourceResultType::Struct(result) = &resource_field.result_type {
                    self.structure(&result.type_name, &result.fields)?;
                }

                "ShaderReflection.ResourceReference".into()
            }
            StructField::Pointer(pointer_field) => {
                self.structure(
                    &pointer_field.pointee_type.type_name,
                    &pointer_field.pointee_type.fields,
                )?;

                "ShaderReflection.PointerAddress".into()
            }
            StructField::Array(array_field) => {
                let is_supported_element = array_field.element_stride == 16
                    && !matches!(array_field.element_scalar_type, ScalarType::Uint64);
                if !is_supported_element {
                    return Err(UnsupportedLogicalType::ArrayElement {
                        stride: array_field.element_stride,
                        scalar: array_field.element_scalar_type,
                    })
                    .union();
                }

                fragment(&RocApplyTemplate {
                    head: "List".into(),
                    argument: self.vector(4, array_field.element_scalar_type).union()?,
                    hugging: true,
                })
            }
            StructField::Enum(enum_field) => {
                let enum_type = &enum_field.enum_type;
                let name = identifier(&enum_type.type_name, true).union()?;
                let mut names = Names::default();
                let mut cases = Vec::new();
                if enum_type.cases.is_empty() {
                    return Err(UnsupportedLogicalType::EmptyEnum {
                        type_name: enum_type.type_name.clone(),
                    })
                    .union();
                }

                for case in &enum_type.cases {
                    let tag = identifier(&case.name, true).union()?;
                    names.insert(&tag, &case.name).union()?;
                    let (in_range, tag_type) = match enum_type.tag_type {
                        EnumTagType::Uint32 => (u32::try_from(case.value).is_ok(), "U32"),
                        EnumTagType::Int32 => (i32::try_from(case.value).is_ok(), "I32"),
                    };
                    if !in_range {
                        return Err(UnsupportedLogicalType::EnumValueOutOfRange {
                            type_name: enum_type.type_name.clone(),
                            case: case.name.clone(),
                            value: case.value,
                            tag_type,
                        })
                        .union();
                    }
                    cases.push(EnumArm {
                        tag,
                        value: case.value,
                    });
                }

                let definition = LogicalEnumTemplate {
                    name,
                    value_name: identifier(&enum_type.type_name, false).union()?,
                    numeric: match enum_type.tag_type {
                        EnumTagType::Uint32 => "U32",
                        EnumTagType::Int32 => "I32",
                    },
                    cases,
                };

                self.define(&enum_type.type_name, fragment(&definition))?
            }
            StructField::DescriptorHandle(_) => "ShaderReflection.DescriptorHandle".into(),
        })
    }

    fn shader_types(&self) -> String {
        let module = ShaderTypesTemplate {
            definitions: self
                .definitions
                .values()
                .map(|(_, body)| indent_roc(body))
                .collect(),
        };

        module.render().expect("static template")
    }
}

#[derive(Template)]
#[template(path = "shader_types.roc.askama", escape = "none")]
struct ShaderTypesTemplate {
    definitions: Vec<String>,
}

#[derive(Template)]
#[template(path = "logical_struct.roc.askama", escape = "none")]
struct LogicalStructTemplate {
    name: String,
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
    value_name: String,
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
    fn vector(&self, count: usize, element: ScalarType) -> Result<String, UnsupportedLogicalType> {
        if !(1..=4).contains(&count) {
            return Err(UnsupportedLogicalType::VectorWidth { count });
        }

        Ok(format!("ShaderReflection.{}", vector_name(count, element)))
    }

    fn matrix(&self, element: ScalarType) -> eros::Result<String, LogicalErrors> {
        Ok(format!("ShaderReflection.{}", matrix_name(element)))
    }
}

#[cfg(test)]
#[path = "../tests/support/roc_app.rs"]
mod roc_app;

#[cfg(test)]
mod tests {
    use super::roc_app::write_app_main;
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
            ["ShaderAtlas.roc", "ShaderTypes.roc"]
                .into_iter()
                .map(String::from)
                .collect()
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
    fn late_failures_preserve_both_managed_trees() {
        for case in ["compile", "keyword", "mapping", "import_path"] {
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
            assert_eq!(tree_contents(&config.roc_source_dir), roc_before);
            assert_eq!(tree_contents(&config.compiled_shaders_dir), compiled_before);
            if case != "import_path" {
                assert_eq!((roc_before, compiled_before), before);
            }
            let debug = format!("{error:#?}");
            assert!(debug.contains("Generate Roc shader bindings"), "{debug}");
            if case == "keyword" {
                assert!(
                    error.to_string().contains("reserved Roc identifier"),
                    "{debug}"
                );
            }
            if case == "compile" {
                let Ok(failed) = error.narrow::<ShaderReflectionFailed, _>() else {
                    panic!("a slang failure must classify as ShaderReflectionFailed: {debug}");
                };
                assert_eq!(failed.shader, "broken.shader.slang");
                assert!(
                    failed
                        .downcast_ref::<mltrs_slang_reflection::SlangError>()
                        .is_some(),
                    "{failed:?}"
                );
                let source = std::error::Error::source(&failed).unwrap();
                assert!(source.is::<mltrs_slang_reflection::SlangError>());
                continue;
            }
            let Ok(roc_error) = error.narrow::<RocCodegenError, _>() else {
                panic!("{case} must classify as RocCodegenError: {debug}");
            };
            match (case, &roc_error) {
                (
                    "keyword",
                    RocCodegenError::InvalidIdentifier(InvalidRocIdentifier::Reserved { original }),
                ) if original == "package" => {}
                (
                    "mapping",
                    RocCodegenError::LogicalTypeCollision(LogicalTypeCollision::Incompatible {
                        ..
                    }),
                ) => {}
                (
                    "import_path",
                    RocCodegenError::UnrepresentableImportPath(
                        UnrepresentableImportPath::ForbiddenCharacter { .. },
                    ),
                ) => {}
                _ => panic!("{case}: unexpected {roc_error:?}"),
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

        ShaderModule {
            module: module.into(),
            stage_imports: vec![],
            struct_types: types.definitions(),
            reflection_kind: kind,
            reflection: continued(&reflection, 1),
            stages: vec![],
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
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("All (9) tests passed"),
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
        assert_eq!(logical.field(&field).unwrap(), "Mode");
        assert_eq!(
            logical.definitions["mode"].1,
            "Mode : [Off, On]\n\nmode_tag : Mode -> I32\nmode_tag = |value| {\n    match value {\n        Off => -1\n        On => 1\n    }\n}"
        );
    }

    #[test]
    fn logical_vectors_and_shared_identity() {
        let mut logical = Logical::default();
        assert_eq!(
            logical.vector(2, ScalarType::Uint64).unwrap(),
            "ShaderReflection.Uint64x2"
        );
        assert!(logical.vector(5, ScalarType::Float32).is_err());
        assert_eq!(
            logical.matrix(ScalarType::Float32).unwrap(),
            "ShaderReflection.Float4x4"
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
