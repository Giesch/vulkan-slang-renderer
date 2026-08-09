//! JSON format for global and entrypoint parameters
//!
//! this mostly follows slangc's format, with some exceptions and many limitations

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GlobalParameter {
    ParameterBlock(ParameterBlockGlobalParameter),
    PushConstant(PushConstantGlobalParameter),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterBlockGlobalParameter {
    pub parameter_name: String,
    pub element_type: ParameterBlockElementType,
}

/// A global `[[vk::push_constant]] ConstantBuffer<T>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushConstantGlobalParameter {
    pub parameter_name: String,
    pub element_type: ParameterBlockElementType,
    pub element_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterBlockElementType {
    pub type_name: String,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPoint {
    pub entry_point_name: String,
    pub stage: EntryPointStage,
    pub parameters: Vec<EntryPointParameter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryPointStage {
    Vertex,
    Fragment,
    Compute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EntryPointParameter {
    Struct(StructEntryPointParameter),
    Scalar(ScalarEntryPointParameter),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructEntryPointParameter {
    pub parameter_name: String,
    pub binding: Option<Binding>,
    pub type_name: String,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum ScalarEntryPointParameter {
    Bound(BoundScalarEntryPointParameter),
    Semantic(SemanticScalarEntryPointParameter),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundScalarEntryPointParameter {
    pub parameter_name: String,
    pub binding: Binding,
    pub scalar_type: ScalarType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticScalarEntryPointParameter {
    pub parameter_name: String,
    pub semantic_name: String,
    pub scalar_type: ScalarType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StructField {
    Scalar(ScalarStructField),
    Vector(VectorStructField),
    Struct(StructStructField),
    Matrix(MatrixStructField),
    Resource(ResourceStructField),
    Pointer(PointerStructField),
    Array(ArrayStructField),
    Enum(EnumStructField),
    DescriptorHandle(DescriptorHandleStructField),
}

impl GlobalParameter {
    /// Whether this parameter declares a bindless texture handle anywhere
    pub fn declares_bindless_handle(&self) -> bool {
        let fields = match self {
            Self::ParameterBlock(block) => &block.element_type.fields,
            Self::PushConstant(push) => &push.element_type.fields,
        };

        fields_declare_bindless_handle(fields)
    }
}

impl StructField {
    pub fn binding(&self) -> Option<&Binding> {
        match self {
            Self::Scalar(s) => Some(&s.binding),
            Self::Vector(VectorStructField::Bound(v)) => Some(&v.binding),
            Self::Vector(VectorStructField::Semantic(_)) => None,
            Self::Struct(s) => Some(&s.binding),
            Self::Matrix(m) => Some(&m.binding),
            Self::Resource(r) => Some(&r.binding),
            Self::Pointer(p) => Some(&p.binding),
            Self::Array(a) => Some(&a.binding),
            Self::Enum(e) => Some(&e.binding),
            Self::DescriptorHandle(h) => Some(&h.binding),
        }
    }

    pub fn field_name(&self) -> &str {
        match self {
            Self::Scalar(s) => &s.field_name,
            Self::Vector(VectorStructField::Bound(v)) => &v.field_name,
            Self::Vector(VectorStructField::Semantic(v)) => &v.field_name,
            Self::Struct(s) => &s.field_name,
            Self::Matrix(m) => &m.field_name,
            Self::Resource(r) => &r.field_name,
            Self::Pointer(p) => &p.field_name,
            Self::Array(a) => &a.field_name,
            Self::Enum(e) => &e.field_name,
            Self::DescriptorHandle(h) => &h.field_name,
        }
    }
}

/// Recursively check whether any field contains a bindless handle
fn fields_declare_bindless_handle(fields: &[StructField]) -> bool {
    fields.iter().any(|field| match field {
        StructField::DescriptorHandle(_) => true,

        StructField::Struct(s) => fields_declare_bindless_handle(&s.struct_type.fields),
        StructField::Pointer(p) => fields_declare_bindless_handle(&p.pointee_type.fields),
        StructField::Resource(r) => match &r.result_type {
            ResourceResultType::Struct(s) => fields_declare_bindless_handle(&s.fields),
            ResourceResultType::Scalar(_) | ResourceResultType::Vector(_) => false,
        },

        StructField::Scalar(_)
        | StructField::Vector(_)
        | StructField::Matrix(_)
        | StructField::Array(_)
        | StructField::Enum(_) => false,
    })
}

/// maps to a Slang `ParameterCategory`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Binding {
    Uniform(OffsetSizeBinding),
    PushConstant(OffsetSizeBinding),
    DescriptorTableSlot(IndexCountBinding),
    VaryingInput(IndexCountBinding),
    ConstantBuffer(IndexCountBinding),
}

impl Binding {
    /// The offset and size a binding occupies in its enclosing block. Uniform
    /// and push-constant fields measure bytes; every other category measures an
    /// index into a descriptor set or a varying slot, and occupies no bytes.
    ///
    /// This is the line between a slang field that becomes a `#[repr(C)]` field
    /// in the generated struct and one that does not.
    pub fn occupied_bytes(&self) -> Option<&OffsetSizeBinding> {
        match self {
            Self::Uniform(bytes) | Self::PushConstant(bytes) => Some(bytes),

            Self::DescriptorTableSlot(_) | Self::VaryingInput(_) | Self::ConstantBuffer(_) => None,
        }
    }

    pub fn occupies_bytes(&self) -> bool {
        self.occupied_bytes().is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetSizeBinding {
    pub offset: usize,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexCountBinding {
    pub index: usize,
    // NOTE slangc omits a count of 1,
    // and replaces 'bitwise not 0' with the string 'unbounded'
    // see SLANG_UNBOUNDED_SIZE
    // https://github.com/shader-slang/slang/blob/04093bcbaea9784cdffe55f3931f50db7ad9f808/source/slang/slang-reflection-json.cpp#L124
    // https://github.com/shader-slang/slang/blob/04093bcbaea9784cdffe55f3931f50db7ad9f808/include/slang.h#L2167
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum VectorStructField {
    Bound(BoundVectorStructField),
    Semantic(SemanticVectorStructField),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticVectorStructField {
    pub field_name: String,
    pub semantic_name: String,
    pub element_count: usize,
    pub element_type: VectorElementType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalarStructField {
    pub field_name: String,
    pub binding: Binding,
    pub scalar_type: ScalarType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundVectorStructField {
    pub field_name: String,
    pub binding: Binding,
    pub element_count: usize,
    pub element_type: VectorElementType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixStructField {
    pub field_name: String,
    pub binding: Binding,
    pub row_count: u32,
    pub column_count: u32,
    pub element_type: VectorElementType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStructField {
    pub field_name: String,
    pub binding: Binding,
    pub resource_shape: ResourceShape,
    pub result_type: ResourceResultType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceShape {
    Texture2D,
    RWTexture2D,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ResourceResultType {
    Scalar(ScalarResultType),
    Vector(VectorResultType),
    Struct(StructResultType),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalarResultType {
    pub scalar_type: ScalarType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorResultType {
    pub element_count: usize,
    pub element_type: VectorElementType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructResultType {
    pub type_name: String,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructStructField {
    pub field_name: String,
    pub binding: Binding,
    pub struct_type: StructFieldType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructFieldType {
    pub type_name: String,
    pub fields: Vec<StructField>,
}

/// A fixed-length array field. Restricted to 16-byte vector elements
/// (float4/int4/uint4), the only element types whose stride equals their size
/// in both std140 and std430 — so a contiguous Rust array matches the GPU
/// layout exactly, with no inter-element padding to model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrayStructField {
    pub field_name: String,
    pub binding: Binding,
    pub element_scalar_type: ScalarType,
    pub element_count: usize,
    /// reflected element stride; the reflection gate guarantees 16
    pub element_stride: usize,
}

/// A physical-storage-buffer pointer field (slang `Ptr<T, ..., Std430DataLayout>`).
/// 8 bytes of uniform data holding a buffer device address; consumes no
/// descriptor slot. The pointee fields carry std430 offsets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerStructField {
    pub field_name: String,
    pub binding: Binding,
    pub pointee_type: StructFieldType,
    /// reflected std430 size of the pointee — cross-checked against the
    /// codegen's computed struct size
    pub pointee_size: usize,
    #[serde(default)]
    pub access: PointerAccess,
}

/// The access mode of a pointer field
/// (eg. Slang's `Access.ReadWrite` / `Access.Read` / `Access.Immutable`)
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PointerAccess {
    /// The pointer is writable
    #[default]
    ReadWrite,
    /// The pointer is read-only
    Read,
    /// The underlying data is immutable during shader execution
    Immutable,
}

/// A slang `DescriptorHandle<T>` field (eg. `Sampler2D.Handle`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptorHandleStructField {
    pub field_name: String,
    pub binding: Binding,
    pub shape: DescriptorHandleShape,
}

/// The descriptor type a handle resolves to. We only support combined image samplers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DescriptorHandleShape {
    Sampler2D,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VectorElementType {
    Scalar(ScalarVectorElementType),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalarVectorElementType {
    pub scalar_type: ScalarType,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum ScalarType {
    Float32,
    Int32,
    Uint32,
    Uint64,
}

/// A slang enum field. Slang lays an enum out as its tag type, so the GPU bytes
/// are identical to the equivalent scalar field; the enum identity survives only
/// on the declared type, and is carried here so codegen can emit a Rust enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumStructField {
    pub field_name: String,
    pub binding: Binding,
    pub enum_type: EnumFieldType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumFieldType {
    pub type_name: String,
    pub tag_type: EnumTagType,
    pub cases: Vec<EnumCase>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnumCase {
    pub name: String,
    /// normalized to the tag type's range by the reflection layer, so an
    /// unsigned case never arrives here sign-extended
    pub value: i64,
}

/// The integer type a slang enum is laid out as.
///
/// 32-bit tags only. A `uint8_t`/`uint16_t` tag would be laid out at its natural
/// 1/2-byte alignment, but reading one makes slang emit Int8/Int16 and the
/// UniformAndStorageBuffer{8,16}BitAccess capabilities — optional Vulkan feature
/// bits the renderer deliberately does not require. See `enum_tag_from_slang`.
///
/// Deliberately separate from ScalarType: widening ScalarType to carry Int32
/// would also start accepting plain `int` *scalar* fields, which codegen's
/// vk::Format and vector-type matches do not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnumTagType {
    Uint32,
    Int32,
}

impl EnumTagType {
    pub fn rust_type_name(self) -> &'static str {
        match self {
            Self::Uint32 => "u32",
            Self::Int32 => "i32",
        }
    }

    pub fn repr(self) -> String {
        format!("#[repr({})]", self.rust_type_name())
    }

    /// Alignment equals size for every tag type.
    pub fn size(self) -> usize {
        match self {
            Self::Uint32 | Self::Int32 => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform(offset: usize, size: usize) -> Binding {
        Binding::Uniform(OffsetSizeBinding { offset, size })
    }

    fn handle_field() -> StructField {
        StructField::DescriptorHandle(DescriptorHandleStructField {
            field_name: "tex".to_string(),
            binding: uniform(0, 8),
            shape: DescriptorHandleShape::Sampler2D,
        })
    }

    fn scalar_field() -> StructField {
        StructField::Scalar(ScalarStructField {
            field_name: "scale".to_string(),
            binding: uniform(0, 4),
            scalar_type: ScalarType::Float32,
        })
    }

    fn block(fields: Vec<StructField>) -> GlobalParameter {
        GlobalParameter::ParameterBlock(ParameterBlockGlobalParameter {
            parameter_name: "params".to_string(),
            element_type: ParameterBlockElementType {
                type_name: "Params".to_string(),
                fields,
            },
        })
    }

    fn push_block(fields: Vec<StructField>) -> GlobalParameter {
        GlobalParameter::PushConstant(PushConstantGlobalParameter {
            parameter_name: "draw".to_string(),
            element_type: ParameterBlockElementType {
                type_name: "Draw".to_string(),
                fields,
            },
            element_size: 8,
        })
    }

    #[test]
    fn a_handle_free_block_declares_no_handle() {
        assert!(!block(vec![scalar_field()]).declares_bindless_handle());
    }

    #[test]
    fn a_top_level_handle_is_found() {
        assert!(block(vec![scalar_field(), handle_field()]).declares_bindless_handle());
    }

    #[test]
    fn a_handle_in_a_push_block_is_found() {
        assert!(push_block(vec![handle_field()]).declares_bindless_handle());
        assert!(!push_block(vec![scalar_field()]).declares_bindless_handle());
    }

    #[test]
    fn a_handle_in_a_nested_struct_is_found() {
        let nested = StructField::Struct(StructStructField {
            field_name: "inner".to_string(),
            binding: uniform(0, 16),
            struct_type: StructFieldType {
                type_name: "Inner".to_string(),
                fields: vec![handle_field()],
            },
        });

        assert!(block(vec![nested]).declares_bindless_handle());
    }

    #[test]
    fn a_handle_in_a_pointer_pointee_is_found() {
        let pointer = StructField::Pointer(PointerStructField {
            field_name: "materials".to_string(),
            binding: uniform(0, 8),
            pointee_type: StructFieldType {
                type_name: "Material".to_string(),
                fields: vec![handle_field()],
            },
            pointee_size: 16,
            access: PointerAccess::Immutable,
        });

        assert!(block(vec![pointer]).declares_bindless_handle());
    }
}
