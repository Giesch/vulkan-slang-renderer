//! Typed errors for slang compilation and reflection.
//!
//! Every failure is a small type carrying the data a caller needs to react
//! to it. Public functions return them as `eros` unions; the aliases at the
//! bottom of this module name the sets.
//!
//! No `shader_slang` type appears here (see the crate rule in AGENTS.md):
//! slang errors are converted to [`SlangError`], and slang enums that a
//! message mentions are stored as their `Debug` text.

use std::fmt;

use shader_slang as slang;

use crate::ShaderStage;
use crate::json::{Binding, EnumTagType, ScalarType};

// ---------------------------------------------------------------------------
// slang compilation
// ---------------------------------------------------------------------------

/// The slang API call that failed; see [`SlangError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlangOperation {
    /// Loading a module, from the search path or from an in-memory source.
    LoadModule {
        module_name: String,
    },
    CreateCompositeComponentType,
    Link,
    /// Querying the linked program's reflection layout.
    Layout,
    /// Fetching an entry point's compiled SPIR-V.
    EntryPointCode,
}

impl fmt::Display for SlangOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LoadModule { module_name } => write!(f, "load module '{module_name}'"),
            Self::CreateCompositeComponentType => f.write_str("create composite component type"),
            Self::Link => f.write_str("link"),
            Self::Layout => f.write_str("program layout"),
            Self::EntryPointCode => f.write_str("entry point code generation"),
        }
    }
}

/// A slang API call failed. `diagnostics` is slang's diagnostic output (for
/// a compile error, the rendered compiler messages), or its result code when
/// slang produced no diagnostics.
#[derive(Debug, thiserror::Error)]
#[error("slang {operation} failed: {diagnostics}")]
pub struct SlangError {
    pub operation: SlangOperation,
    pub diagnostics: String,
}

impl SlangError {
    pub(crate) fn new(operation: SlangOperation, error: slang::Error) -> Self {
        let diagnostics = match error {
            slang::Error::Code(code) => format!("slang result code {code:#010x}"),
            slang::Error::Blob(blob) => blob.as_str().unwrap_or_default().trim_end().to_string(),
        };

        Self {
            operation,
            diagnostics,
        }
    }
}

/// Converts a `shader_slang` result into one carrying [`SlangError`].
pub(crate) trait SlangResultExt<T> {
    fn slang(self, operation: SlangOperation) -> Result<T, SlangError>;
}

impl<T> SlangResultExt<T> for slang::Result<T> {
    fn slang(self, operation: SlangOperation) -> Result<T, SlangError> {
        self.map_err(|error| SlangError::new(operation, error))
    }
}

/// An entry point's stage is not vertex, fragment or compute.
#[derive(Debug, thiserror::Error)]
#[error(
    "unsupported shader stage {stage} for entry point {entry_point_name:?} in {source_file_name}; \
     only vertex, fragment and compute are supported"
)]
pub struct UnsupportedShaderStage {
    /// slang's stage, as its `Debug` text.
    pub stage: String,
    pub entry_point_name: String,
    pub source_file_name: String,
}

/// The shader declares no entry point for a stage its pipeline needs.
#[derive(Debug, thiserror::Error)]
#[error("no {stage} entry point in {source_file_name}")]
pub struct MissingEntryPoint {
    pub stage: ShaderStage,
    pub source_file_name: String,
}

/// The shader declares an entry point of a stage its pipeline cannot use,
/// eg. a vertex entry point in a compute shader.
#[derive(Debug, thiserror::Error)]
#[error("expected a {expected} entry point in {source_file_name}, got {found}")]
pub struct UnexpectedEntryPointStage {
    pub expected: ShaderStage,
    /// The stage found, as slang's `Debug` text (eg. `Vertex`).
    pub found: String,
    pub source_file_name: String,
}

// ---------------------------------------------------------------------------
// reflection: shapes codegen cannot represent
// ---------------------------------------------------------------------------

/// An entry point parameter that is neither a varying nor a system value.
/// Slang would silently turn it into a binding that codegen never sees.
#[derive(Debug, thiserror::Error)]
pub enum UnsupportedEntryPointParameter {
    /// A `uniform` parameter, which slang promotes to an implicit push
    /// constant range.
    #[error(
        "entry point parameter '{parameter_name}' on '{entry_point_name}': a uniform \
        entry point parameter is promoted to an implicit push constant range that no \
        generated code writes; declare a [[vk::push_constant]] ConstantBuffer<T> \
        global instead"
    )]
    Uniform {
        parameter_name: String,
        entry_point_name: String,
    },

    /// A parameter holding a resource, which slang binds as a descriptor in a
    /// set of its own.
    #[error(
        "entry point parameter '{parameter_name}' on '{entry_point_name}': a parameter \
        bound as {category} is a descriptor that nothing binds, and it takes a descriptor \
        set of its own — which shifts the set index of every ParameterBlock after it; \
        declare it in a ParameterBlock<T> global instead"
    )]
    Descriptor {
        parameter_name: String,
        entry_point_name: String,
        /// slang's parameter category, as its `Debug` text.
        category: String,
    },
}

/// A global parameter that is neither a `ParameterBlock<T>` nor a
/// `[[vk::push_constant]] ConstantBuffer<T>`.
#[derive(Debug, thiserror::Error)]
#[error(
    "non-ParameterBlock global: {parameter_name}; only ParameterBlock globals \
    and [[vk::push_constant]] ConstantBuffer blocks are supported"
)]
pub struct UnsupportedGlobalParameter {
    pub parameter_name: String,
}

/// A `[[vk::push_constant]]` block the renderer cannot bind.
#[derive(Debug, thiserror::Error)]
pub enum InvalidPushConstantBlock {
    /// A second push constant block; the renderer supports one per shader.
    #[error(
        "push constant block '{parameter_name}': '{first}' is already declared, \
        and only one push constant block per shader is supported"
    )]
    Duplicate {
        parameter_name: String,
        first: String,
    },

    /// A descriptor-backed resource field (at any nesting depth) inside the block.
    #[error(
        "push constant block '{block_name}': field '{field_name}' is a texture or buffer \
        descriptor, which cannot live in a push block; use a bindless handle \
        (Sampler2D.Handle) or a BDA pointer (mltrs::Addr<T>) instead"
    )]
    DescriptorField {
        block_name: String,
        field_name: String,
    },
}

/// A field kind that is only supported in uniform/pointee struct fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniformOnlyFieldKind {
    Enum,
    Array,
    DescriptorHandle,
}

/// An enum, array or texture handle field outside a uniform or pointee
/// struct, eg. as a vertex input.
#[derive(Debug, thiserror::Error)]
pub struct FieldOutsideUniformLayout {
    pub field_name: String,
    pub kind: UniformOnlyFieldKind,
}

impl fmt::Display for FieldOutsideUniformLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let field_name = &self.field_name;
        match self.kind {
            UniformOnlyFieldKind::Enum => write!(
                f,
                "enum field '{field_name}': enums are only supported in \
                uniform/pointee struct fields, not vertex inputs"
            ),
            UniformOnlyFieldKind::Array => write!(
                f,
                "array field '{field_name}': arrays are only supported in \
                uniform/pointee struct fields"
            ),
            UniformOnlyFieldKind::DescriptorHandle => write!(
                f,
                "handle field '{field_name}': texture handles are only supported in \
                uniform/pointee struct fields, not vertex inputs"
            ),
        }
    }
}

/// A vector field with neither a binding nor a semantic.
#[derive(Debug, thiserror::Error)]
#[error(
    "vector field '{field_name}': unexpected combination of vector binding and semantic \
    {binding:?}, {semantic:?}"
)]
pub struct UnexpectedVectorBinding {
    pub field_name: String,
    pub binding: Option<Binding>,
    pub semantic: Option<String>,
}

/// A `StructuredBuffer`/`RWStructuredBuffer` descriptor field.
#[derive(Debug, thiserror::Error)]
#[error(
    "field '{field_name}': StructuredBuffer/RWStructuredBuffer descriptors \
    are unsupported; use a BDA pointer instead (e.g. mltrs::Addr<T> via \
    import mltrs, or LayoutPtr<T, Std430DataLayout>)"
)]
pub struct StructuredBufferField {
    pub field_name: String,
}

/// An array field whose element is not a 16-byte vector (float4/int4/uint4),
/// the only element type whose stride is the same under std140 and std430.
#[derive(Debug, thiserror::Error)]
#[error(
    "array field '{field_name}': only float4/int4/uint4 element arrays are \
    supported (16-byte stride); got element kind {element_kind} with \
    {component_count} components, scalar type {scalar_type:?}, stride \
    {stride}; use a BDA buffer of flat structs, or named fields"
)]
pub struct UnsupportedArrayElement {
    pub field_name: String,
    /// slang's element type kind, as its `Debug` text.
    pub element_kind: String,
    pub component_count: usize,
    pub scalar_type: Option<ScalarType>,
    pub stride: usize,
}

/// A pointer field the reflection JSON cannot represent.
#[derive(Debug, thiserror::Error)]
pub enum UnsupportedPointer {
    /// A pointer without the `Std430DataLayout` layout argument.
    #[error(
        "pointer field '{field_name}' ({pointer_type}): only Std430DataLayout \
        pointers are supported; declare it as mltrs::Addr<T> (import mltrs;) \
        or LayoutPtr<T, Std430DataLayout>"
    )]
    NonStd430Layout {
        field_name: String,
        pointer_type: String,
    },

    #[error("pointer field '{field_name}': only struct pointees are supported, got {pointee_kind}")]
    NonStructPointee {
        field_name: String,
        /// slang's pointee type kind, as its `Debug` text.
        pointee_kind: String,
    },

    /// Pointee fields embed inline, so a pointer cycle cannot be represented.
    /// `chain` is the pointee type names from the outermost pointer inwards.
    #[error(
        "pointer field '{field_name}': pointee '{pointee_type_name}' already \
        appears on the pointee chain ({} -> {pointee_type_name})",
        chain.join(" -> ")
    )]
    Cycle {
        field_name: String,
        pointee_type_name: String,
        chain: Vec<String>,
    },
}

/// A `DescriptorHandle<T>` field the bindless heap cannot serve.
#[derive(Debug, thiserror::Error)]
pub enum UnsupportedDescriptorHandle {
    #[error(
        "field '{field_name}' ({declared_name}): arrays of texture handles are not \
        supported. A handle is an 8-byte element, whose array stride is 16 under \
        std140 but 8 under std430, so it cannot be laid out identically in a \
        ParameterBlock and in a Std430DataLayout pointee; use named fields, or a \
        BDA buffer of structs that each hold one handle"
    )]
    Array {
        field_name: String,
        declared_name: String,
    },

    #[error("field '{field_name}' ({declared_name}): unparseable handle type")]
    Unparseable {
        field_name: String,
        declared_name: String,
    },

    #[error(
        "field '{field_name}' ({declared_name}): only Sampler2D.Handle and \
        RWTexture2D.Handle texture handles are supported; the bindless heap has \
        a combined-image-sampler binding and a storage-image binding only"
    )]
    UnsupportedShape {
        field_name: String,
        declared_name: String,
    },
}

/// A slang enum type that cannot be generated as a Rust enum.
#[derive(Debug, thiserror::Error)]
pub enum UnsupportedEnum {
    #[error(
        "enum field '{field_name}' has an anonymous enum type; give the enum a \
        name so the generated Rust enum has one too"
    )]
    Anonymous { field_name: String },

    #[error("enum '{type_name}' (field '{field_name}') has no reflected tag type")]
    MissingTagType {
        type_name: String,
        field_name: String,
    },

    /// A `uint8_t`/`uint16_t`/`int16_t` tag; reading one needs the optional
    /// 8/16-bit storage device features.
    #[error(
        "enum '{type_name}' has a sub-32-bit tag type {tag_type}; only uint \
        and int tags are supported, because reading a narrower tag requires \
        the 8/16-bit storage device features"
    )]
    NarrowTagType {
        type_name: String,
        /// slang's scalar type, as its `Debug` text.
        tag_type: String,
    },

    #[error(
        "enum '{type_name}' has an unsupported tag type {tag_type}; supported \
        tags are uint and int (or none, which means int)"
    )]
    UnsupportedTagType {
        type_name: String,
        /// slang's scalar type, as its `Debug` text.
        tag_type: String,
    },

    #[error("enum '{type_name}' has a case with no name")]
    UnnamedCase { type_name: String },

    #[error(
        "enum '{type_name}' case '{case_name}' has no constant value; only \
        compile-time constant cases can cross the reflection boundary"
    )]
    NonConstantCase {
        type_name: String,
        case_name: String,
    },

    #[error(
        "enum '{type_name}' case '{case_name}' has value {value}, which does not \
        fit its {} tag",
        tag_type.rust_type_name()
    )]
    CaseOutOfRange {
        type_name: String,
        case_name: String,
        value: i64,
        tag_type: EnumTagType,
    },

    #[error(
        "enum '{type_name}' cases '{first_case}' and '{second_case}' share the value {value}; \
        duplicate discriminants cannot be generated as a Rust enum"
    )]
    DuplicateDiscriminant {
        type_name: String,
        first_case: String,
        second_case: String,
        value: i64,
    },

    #[error(
        "enum '{type_name}' has no cases; the generated Rust enum needs at \
        least one case for Default and TryFrom"
    )]
    Empty { type_name: String },
}

// ---------------------------------------------------------------------------
// error sets
// ---------------------------------------------------------------------------

/// Failures reflecting one struct's fields (recursively).
pub type StructFieldErrors = (
    FieldOutsideUniformLayout,
    UnexpectedVectorBinding,
    StructuredBufferField,
    UnsupportedArrayElement,
    UnsupportedPointer,
    UnsupportedDescriptorHandle,
    UnsupportedEnum,
);

/// Failures turning a linked program's layout into reflection JSON.
pub type ReflectionErrors = (
    MissingEntryPoint,
    UnexpectedEntryPointStage,
    UnsupportedEntryPointParameter,
    UnsupportedGlobalParameter,
    InvalidPushConstantBlock,
    FieldOutsideUniformLayout,
    UnexpectedVectorBinding,
    StructuredBufferField,
    UnsupportedArrayElement,
    UnsupportedPointer,
    UnsupportedDescriptorHandle,
    UnsupportedEnum,
);

/// Failures compiling one entry point to SPIR-V.
pub type CompileShaderErrors = (SlangError, std::ffi::NulError, UnsupportedShaderStage);

/// Failures of the `prepare_reflected_*` / `dev_compile_*` functions:
/// [`CompileShaderErrors`] plus [`ReflectionErrors`].
pub type PrepareShaderErrors = (
    SlangError,
    std::ffi::NulError,
    UnsupportedShaderStage,
    MissingEntryPoint,
    UnexpectedEntryPointStage,
    UnsupportedEntryPointParameter,
    UnsupportedGlobalParameter,
    InvalidPushConstantBlock,
    FieldOutsideUniformLayout,
    UnexpectedVectorBinding,
    StructuredBufferField,
    UnsupportedArrayElement,
    UnsupportedPointer,
    UnsupportedDescriptorHandle,
    UnsupportedEnum,
);
