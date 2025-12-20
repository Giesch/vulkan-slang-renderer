//! JSON format for global and entrypoint parameters
//!
//! this mostly follows slangc's format, with some exceptions and many limitations

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GlobalParameter {
    ParameterBlock(ParameterBlockGlobalParameter),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterBlockGlobalParameter {
    pub parameter_name: String,
    pub element_type: ParameterBlockElementType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterBlockElementType {
    pub type_name: String,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPoint {
    pub entry_point_name: String,
    pub stage: EntryPointStage,
    pub parameters: Vec<EntryPointParameter>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryPointStage {
    Vertex,
    Fragment,
    Compute,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EntryPointParameter {
    Struct(StructEntryPointParameter),
    Scalar(ScalarEntryPointParameter),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructEntryPointParameter {
    pub parameter_name: String,
    pub binding: Option<Binding>,
    pub type_name: String,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum ScalarEntryPointParameter {
    Bound(BoundScalarEntryPointParameter),
    Semantic(SemanticScalarEntryPointParameter),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundScalarEntryPointParameter {
    pub parameter_name: String,
    pub binding: Binding,
    pub scalar_type: ScalarType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticScalarEntryPointParameter {
    pub parameter_name: String,
    pub semantic_name: String,
    pub scalar_type: ScalarType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StructField {
    Scalar(ScalarStructField),
    Vector(VectorStructField),
    Struct(StructStructField),
    Matrix(MatrixStructField),
    Resource(ResourceStructField),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Binding {
    Uniform(OffsetSizeBinding),
    DescriptorTableSlot(IndexCountBinding),
    VaryingInput(IndexCountBinding),
    ConstantBuffer(IndexCountBinding),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetSizeBinding {
    pub offset: usize,
    pub size: usize,
}

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum VectorStructField {
    Bound(BoundVectorStructField),
    Semantic(SemanticVectorStructField),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticVectorStructField {
    pub field_name: String,
    pub semantic_name: String,
    pub element_count: usize,
    pub element_type: VectorElementType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalarStructField {
    pub field_name: String,
    pub binding: Binding,
    pub scalar_type: ScalarType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundVectorStructField {
    pub field_name: String,
    pub binding: Binding,
    pub element_count: usize,
    pub element_type: VectorElementType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixStructField {
    pub field_name: String,
    pub binding: Binding,
    pub row_count: u32,
    pub column_count: u32,
    pub element_type: VectorElementType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStructField {
    pub field_name: String,
    pub binding: Binding,
    pub resource_shape: ResourceShape,
    pub result_type: ResourceResultType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceShape {
    Texture2D,
    StructuredBuffer,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ResourceResultType {
    Vector(VectorResultType),
    Struct(StructResultType),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorResultType {
    pub element_count: usize,
    pub element_type: VectorElementType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructResultType {
    pub type_name: String,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructStructField {
    pub field_name: String,
    pub binding: Binding,
    pub struct_type: StructFieldType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructFieldType {
    pub type_name: String,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VectorElementType {
    Scalar(ScalarVectorElementType),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalarVectorElementType {
    pub scalar_type: ScalarType,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum ScalarType {
    Float32,
    Uint32,
}
