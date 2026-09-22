//! This module calculates the vertex input layout of a vertex entry point's
//! struct parameter.
//!
//! Slang reflection records no byte offsets for varying inputs. This module
//! assigns them by three rules:
//!
//! - Attributes follow the declaration order of the struct fields.
//! - Each attribute starts at an offset aligned to its Rust field type.
//! - The stride is a multiple of 16 bytes.
//!
//! The code generator builds the Rust vertex struct and the Roc packer from
//! this layout. The generated Rust asserts `size_of` and `offset_of!` against
//! the layout. Runtime pipeline creation reads the offsets from the generated
//! Rust struct.

use super::{
    EntryPoint, EntryPointParameter, ScalarType, StructField, VectorElementType, VectorStructField,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexFormat {
    R32Sfloat,
    R32G32Sfloat,
    R32G32B32Sfloat,
    R32G32B32A32Sfloat,
    R32Sint,
    R32G32B32A32Sint,
    R32Uint,
    R32G32B32A32Uint,
}

impl VertexFormat {
    pub fn size(self) -> u32 {
        match self {
            Self::R32Sfloat | Self::R32Sint | Self::R32Uint => 4,
            Self::R32G32Sfloat => 8,
            Self::R32G32B32Sfloat => 12,
            Self::R32G32B32A32Sfloat | Self::R32G32B32A32Sint | Self::R32G32B32A32Uint => 16,
        }
    }

    /// The Rust alignment of the generated field type; only `glam::Vec4` is
    /// SIMD aligned.
    fn alignment(self) -> u32 {
        match self {
            Self::R32G32B32A32Sfloat => 16,
            _ => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VertexAttribute {
    pub field_name: String,
    pub location: u32,
    pub offset: u32,
    pub format: VertexFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VertexLayout {
    pub type_name: String,
    pub stride: u32,
    pub attributes: Vec<VertexAttribute>,
}

const STRUCT_ALIGNMENT: u32 = 16;

/// The layout of the entry point's struct parameter, or `None` when the entry
/// point takes no struct (a vertex-count shader).
pub fn vertex_layout(entry_point: &EntryPoint) -> anyhow::Result<Option<VertexLayout>> {
    let mut layout = None;
    for parameter in &entry_point.parameters {
        let EntryPointParameter::Struct(parameter) = parameter else {
            continue;
        };
        anyhow::ensure!(
            layout.is_none(),
            "vertex entry point '{}' has more than one struct parameter",
            entry_point.entry_point_name
        );

        let mut attributes = vec![];
        let mut cursor = 0u32;
        for field in &parameter.fields {
            let Some((field_name, format)) = attribute_format(field)? else {
                continue;
            };

            let offset = align_to(cursor, format.alignment());
            attributes.push(VertexAttribute {
                field_name,
                location: attributes.len() as u32,
                offset,
                format,
            });
            cursor = offset + format.size();
        }

        layout = Some(VertexLayout {
            type_name: parameter.type_name.clone(),
            stride: align_to(cursor, STRUCT_ALIGNMENT),
            attributes,
        });
    }

    Ok(layout)
}

fn attribute_format(field: &StructField) -> anyhow::Result<Option<(String, VertexFormat)>> {
    let (name, scalar, count) = match field {
        StructField::Scalar(scalar) => (&scalar.field_name, scalar.scalar_type, 1),
        StructField::Vector(VectorStructField::Bound(vector)) => {
            let VectorElementType::Scalar(element) = &vector.element_type;
            (
                &vector.field_name,
                element.scalar_type,
                vector.element_count,
            )
        }
        StructField::Vector(VectorStructField::Semantic(_)) | StructField::Resource(_) => {
            return Ok(None);
        }
        other => anyhow::bail!(
            "vertex field '{}' is not a scalar or vector",
            other.field_name()
        ),
    };

    let format = match (scalar, count) {
        (ScalarType::Float32, 1) => VertexFormat::R32Sfloat,
        (ScalarType::Float32, 2) => VertexFormat::R32G32Sfloat,
        (ScalarType::Float32, 3) => VertexFormat::R32G32B32Sfloat,
        (ScalarType::Float32, 4) => VertexFormat::R32G32B32A32Sfloat,
        (ScalarType::Int32, 1) => VertexFormat::R32Sint,
        (ScalarType::Int32, 4) => VertexFormat::R32G32B32A32Sint,
        (ScalarType::Uint32, 1) => VertexFormat::R32Uint,
        (ScalarType::Uint32, 4) => VertexFormat::R32G32B32A32Uint,
        (scalar, count) => {
            anyhow::bail!("vertex field '{name}' has unsupported element type {scalar:?} x {count}")
        }
    };

    Ok(Some((name.clone(), format)))
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::{
        Binding, BoundVectorStructField, EntryPointStage, IndexCountBinding, ReflectionJson,
        ScalarStructField, ScalarVectorElementType, SemanticVectorStructField,
        StructEntryPointParameter,
    };

    fn basic_triangle() -> ReflectionJson {
        let raw = include_str!("../fixtures/basic_triangle.json");

        serde_json::from_str(raw).unwrap()
    }

    fn varying(index: usize) -> Binding {
        Binding::VaryingInput(IndexCountBinding { index, count: 1 })
    }

    fn element_type(scalar_type: ScalarType) -> VectorElementType {
        VectorElementType::Scalar(ScalarVectorElementType { scalar_type })
    }

    fn vector(
        field_name: &str,
        index: usize,
        scalar_type: ScalarType,
        element_count: usize,
    ) -> StructField {
        StructField::Vector(VectorStructField::Bound(BoundVectorStructField {
            field_name: field_name.to_string(),
            binding: varying(index),
            element_count,
            element_type: element_type(scalar_type),
        }))
    }

    fn semantic_vector(
        field_name: &str,
        semantic_name: &str,
        scalar_type: ScalarType,
        element_count: usize,
    ) -> StructField {
        StructField::Vector(VectorStructField::Semantic(SemanticVectorStructField {
            field_name: field_name.to_string(),
            semantic_name: semantic_name.to_string(),
            element_count,
            element_type: element_type(scalar_type),
        }))
    }

    fn scalar(field_name: &str, index: usize, scalar_type: ScalarType) -> StructField {
        StructField::Scalar(ScalarStructField {
            field_name: field_name.to_string(),
            binding: varying(index),
            scalar_type,
        })
    }

    fn vertex_entry_point(fields: Vec<StructField>) -> EntryPoint {
        EntryPoint {
            entry_point_name: "vertexMain".to_string(),
            stage: EntryPointStage::Vertex,
            parameters: vec![EntryPointParameter::Struct(StructEntryPointParameter {
                parameter_name: "vertex".to_string(),
                binding: Some(varying(0)),
                type_name: "Vertex".to_string(),
                fields,
            })],
        }
    }

    fn attributes(layout: &VertexLayout) -> Vec<(&str, u32, u32, VertexFormat)> {
        layout
            .attributes
            .iter()
            .map(|attribute| {
                (
                    attribute.field_name.as_str(),
                    attribute.location,
                    attribute.offset,
                    attribute.format,
                )
            })
            .collect()
    }

    fn varying_input_indices(entry_point: &EntryPoint) -> Vec<u32> {
        let mut indices = vec![];
        for parameter in &entry_point.parameters {
            let EntryPointParameter::Struct(parameter) = parameter else {
                continue;
            };
            for field in &parameter.fields {
                if let Some(Binding::VaryingInput(binding)) = field.binding() {
                    indices.push(binding.index as u32);
                }
            }
        }

        indices
    }

    #[test]
    fn vec3_fields_pack_tightly() {
        let reflection = basic_triangle();
        let layout = vertex_layout(&reflection.vertex_entry_point)
            .unwrap()
            .unwrap();

        assert_eq!(layout.type_name, "Vertex");
        assert_eq!(layout.stride, 32);
        assert_eq!(
            attributes(&layout),
            vec![
                ("position", 0, 0, VertexFormat::R32G32B32Sfloat),
                ("color", 1, 12, VertexFormat::R32G32B32Sfloat),
            ]
        );
    }

    #[test]
    fn locations_match_the_reflected_varying_indices() {
        let reflection = basic_triangle();
        let layout = vertex_layout(&reflection.vertex_entry_point)
            .unwrap()
            .unwrap();

        let locations: Vec<_> = layout
            .attributes
            .iter()
            .map(|attribute| attribute.location)
            .collect();
        assert_eq!(
            locations,
            varying_input_indices(&reflection.vertex_entry_point)
        );
    }

    #[test]
    fn a_vertex_count_entry_point_has_no_layout() {
        let raw = include_str!("../fixtures/koch_curve.json");
        let reflection: ReflectionJson = serde_json::from_str(raw).unwrap();
        assert!(
            vertex_layout(&reflection.vertex_entry_point)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn field_offsets_follow_rust_alignment() {
        use ScalarType::{Float32, Int32, Uint32};
        use VertexFormat::*;

        let cases = [
            (
                "a vec4 starts at 16",
                vec![
                    vector("position", 0, Float32, 3),
                    vector("color", 1, Float32, 4),
                ],
                vec![
                    ("position", 0, 0, R32G32B32Sfloat),
                    ("color", 1, 16, R32G32B32A32Sfloat),
                ],
                32,
            ),
            (
                "a float after a vec4 leaves 12 bytes of stride padding",
                vec![vector("color", 0, Float32, 4), scalar("weight", 1, Float32)],
                vec![
                    ("color", 0, 0, R32G32B32A32Sfloat),
                    ("weight", 1, 16, R32Sfloat),
                ],
                32,
            ),
            (
                "integer vectors start at 4",
                vec![
                    vector("position", 0, Float32, 3),
                    vector("joints", 1, Uint32, 4),
                    vector("offsets", 2, Int32, 4),
                ],
                vec![
                    ("position", 0, 0, R32G32B32Sfloat),
                    ("joints", 1, 12, R32G32B32A32Uint),
                    ("offsets", 2, 28, R32G32B32A32Sint),
                ],
                48,
            ),
            (
                "a vec2 starts at 4",
                vec![
                    scalar("index", 0, Uint32),
                    vector("uv", 1, Float32, 2),
                    scalar("id", 2, Int32),
                ],
                vec![
                    ("index", 0, 0, R32Uint),
                    ("uv", 1, 4, R32G32Sfloat),
                    ("id", 2, 12, R32Sint),
                ],
                16,
            ),
            (
                "a semantic field takes no offset and no location",
                vec![
                    vector("position", 0, Float32, 3),
                    semantic_vector("clip_position", "SV_Position", Float32, 4),
                    vector("uv", 1, Float32, 2),
                ],
                vec![
                    ("position", 0, 0, R32G32B32Sfloat),
                    ("uv", 1, 12, R32G32Sfloat),
                ],
                32,
            ),
        ];

        for (description, fields, expected_attributes, expected_stride) in cases {
            let layout = vertex_layout(&vertex_entry_point(fields)).unwrap().unwrap();

            assert_eq!(attributes(&layout), expected_attributes, "{description}");
            assert_eq!(layout.stride, expected_stride, "{description}");
        }
    }

    #[test]
    fn more_than_one_struct_parameter_is_rejected() {
        let mut entry_point =
            vertex_entry_point(vec![vector("position", 0, ScalarType::Float32, 3)]);
        let parameter = entry_point.parameters[0].clone();
        entry_point.parameters.push(parameter);

        let error = vertex_layout(&entry_point).unwrap_err();
        assert!(
            error.to_string().contains("more than one struct parameter"),
            "{error}"
        );
    }

    #[test]
    fn an_unsupported_vector_is_rejected() {
        let entry_point = vertex_entry_point(vec![vector("offsets", 0, ScalarType::Int32, 2)]);

        let error = vertex_layout(&entry_point).unwrap_err();
        assert!(
            error.to_string().contains("unsupported element type"),
            "{error}"
        );
    }
}
