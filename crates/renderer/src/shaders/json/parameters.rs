//! JSON format for global and entrypoint parameters
//!
//! this mostly follows slangc's format, with some exceptions and many limitations

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GlobalParameter {
    ParameterBlock(ParameterBlockGlobalParameter),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterBlockGlobalParameter {
    pub parameter_name: String,
    pub element_type: ParameterBlockElementType,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Binding {
    Uniform(OffsetSizeBinding),
    DescriptorTableSlot(IndexCountBinding),
    VaryingInput(IndexCountBinding),
    ConstantBuffer(IndexCountBinding),
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
