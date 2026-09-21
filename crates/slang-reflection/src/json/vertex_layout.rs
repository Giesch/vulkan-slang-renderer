//! The vertex input layout implied by a vertex entry point's struct parameter.
//!
//! Reflection records no byte offsets for varying inputs. This rule defines
//! them once for every consumer: generated Rust vertex structs, generated Roc
//! packers, and runtime pipeline creation must agree with it.

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
    use crate::json::ReflectionJson;

    #[test]
    fn basic_triangle_vertex_is_two_vec3s_in_32_bytes() {
        let raw = include_str!("../fixtures/basic_triangle.json");
        let reflection: ReflectionJson = serde_json::from_str(raw).unwrap();
        let layout = vertex_layout(&reflection.vertex_entry_point)
            .unwrap()
            .unwrap();

        assert_eq!(layout.type_name, "Vertex");
        assert_eq!(layout.stride, 32);
        let attributes: Vec<_> = layout
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
            .collect();
        assert_eq!(
            attributes,
            vec![
                ("position", 0, 0, VertexFormat::R32G32B32Sfloat),
                ("color", 1, 12, VertexFormat::R32G32B32Sfloat),
            ]
        );
    }

    #[test]
    fn fragment_entry_point_without_struct_has_no_layout() {
        let raw = include_str!("../fixtures/basic_triangle.json");
        let reflection: ReflectionJson = serde_json::from_str(raw).unwrap();
        let mut entry = reflection.vertex_entry_point.clone();
        entry.parameters.clear();
        assert!(vertex_layout(&entry).unwrap().is_none());
    }

    #[test]
    fn vec4_after_vec3_aligns_to_16() {
        let raw = include_str!("../fixtures/basic_triangle.json");
        let mut reflection: ReflectionJson = serde_json::from_str(raw).unwrap();
        let EntryPointParameter::Struct(parameter) =
            &mut reflection.vertex_entry_point.parameters[0]
        else {
            unreachable!()
        };
        let StructField::Vector(VectorStructField::Bound(color)) = &mut parameter.fields[1] else {
            unreachable!()
        };
        color.element_count = 4;

        let layout = vertex_layout(&reflection.vertex_entry_point)
            .unwrap()
            .unwrap();
        assert_eq!(layout.attributes[1].offset, 16);
        assert_eq!(layout.stride, 32);
    }
}
