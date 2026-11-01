//! A shader entry built at runtime from precompiled SPIR-V and reflection JSON,
//! for hosts whose shaders are not generated Rust bindings.

use std::ffi::CString;

use ash::vk;

use crate::renderer::LayoutDescription;

use super::atlas::{PrecompiledShader, PrecompiledShaders, ShaderAtlasEntry};
use super::json::{
    ReflectedPipelineLayout, ReflectionJson, VertexFormat, VertexLayout,
    layout_bindings_from_pipeline_layout, vertex_layout,
};

pub struct RuntimeShader {
    reflection: ReflectionJson,
    vertex_spv: Vec<u32>,
    fragment_spv: Vec<u32>,
    vertex_layout: Option<VertexLayout>,
}

impl RuntimeShader {
    /// `reflection_json` is the compiler's reflection output for the shader;
    /// the SPIR-V slices are its vertex and fragment stages. Hot reload never
    /// touches an entry created here.
    pub fn new(
        reflection_json: &str,
        vertex_spv: &[u8],
        fragment_spv: &[u8],
    ) -> anyhow::Result<Self> {
        let reflection: ReflectionJson = serde_json::from_str(reflection_json)?;
        let vertex_layout = vertex_layout(&reflection.vertex_entry_point)?;

        Ok(Self {
            vertex_spv: read_spv(vertex_spv, "vertex")?,
            fragment_spv: read_spv(fragment_spv, "fragment")?,
            vertex_layout,
            reflection,
        })
    }

    /// The vertex stride the entry's binding description declares, or `None`
    /// for a shader without vertex input.
    pub fn vertex_stride(&self) -> Option<u32> {
        self.vertex_layout.as_ref().map(|layout| layout.stride)
    }
}

fn read_spv(bytes: &[u8], stage: &str) -> anyhow::Result<Vec<u32>> {
    let mut cursor = std::io::Cursor::new(bytes);
    ash::util::read_spv(&mut cursor)
        .map_err(|err| anyhow::anyhow!("{stage} SPIR-V is not a valid module: {err}"))
}

fn vk_format(format: VertexFormat) -> vk::Format {
    match format {
        VertexFormat::R32Sfloat => vk::Format::R32_SFLOAT,
        VertexFormat::R32G32Sfloat => vk::Format::R32G32_SFLOAT,
        VertexFormat::R32G32B32Sfloat => vk::Format::R32G32B32_SFLOAT,
        VertexFormat::R32G32B32A32Sfloat => vk::Format::R32G32B32A32_SFLOAT,
        VertexFormat::R32Sint => vk::Format::R32_SINT,
        VertexFormat::R32G32B32A32Sint => vk::Format::R32G32B32A32_SINT,
        VertexFormat::R32Uint => vk::Format::R32_UINT,
        VertexFormat::R32G32B32A32Uint => vk::Format::R32G32B32A32_UINT,
    }
}

impl ShaderAtlasEntry for RuntimeShader {
    fn source_file_name(&self) -> &str {
        &self.reflection.source_file_name
    }

    fn hot_reload(&self) -> bool {
        false
    }

    fn reflection_json(&self) -> &ReflectionJson {
        &self.reflection
    }

    fn vertex_binding_descriptions(&self) -> Vec<vk::VertexInputBindingDescription> {
        self.vertex_layout
            .iter()
            .map(|layout| {
                vk::VertexInputBindingDescription::default()
                    .binding(0)
                    .stride(layout.stride)
                    .input_rate(vk::VertexInputRate::VERTEX)
            })
            .collect()
    }

    fn vertex_attribute_descriptions(&self) -> Vec<vk::VertexInputAttributeDescription> {
        self.vertex_layout
            .iter()
            .flat_map(|layout| &layout.attributes)
            .map(|attribute| {
                vk::VertexInputAttributeDescription::default()
                    .binding(0)
                    .location(attribute.location)
                    .offset(attribute.offset)
                    .format(vk_format(attribute.format))
            })
            .collect()
    }

    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>> {
        layout_bindings_from_pipeline_layout(&self.reflection.pipeline_layout)
    }

    fn precompiled_shaders(&self) -> PrecompiledShaders {
        PrecompiledShaders {
            vert: PrecompiledShader {
                entry_point_name: entry_point_name(
                    &self.reflection.vertex_entry_point.entry_point_name,
                ),
                spv_bytes: self.vertex_spv.clone(),
            },
            frag: PrecompiledShader {
                entry_point_name: entry_point_name(
                    &self.reflection.fragment_entry_point.entry_point_name,
                ),
                spv_bytes: self.fragment_spv.clone(),
            },
        }
    }

    fn pipeline_layout(&self) -> &ReflectedPipelineLayout {
        &self.reflection.pipeline_layout
    }
}

fn entry_point_name(name: &str) -> CString {
    CString::new(name).expect("reflected entry point names contain no NUL byte")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFLECTION: &str =
        include_str!("../../../slang-reflection/src/fixtures/basic_triangle.json");

    fn spv_module() -> Vec<u8> {
        // magic number, version, generator, bound, schema
        [0x0723_0203u32, 0x0001_0000, 0, 1, 0]
            .iter()
            .flat_map(|word| word.to_ne_bytes())
            .collect()
    }

    #[test]
    fn runtime_shader_describes_the_reflected_vertex_layout() {
        let module = spv_module();
        let shader = RuntimeShader::new(REFLECTION, &module, &module).unwrap();

        assert_eq!(shader.vertex_stride(), Some(32));
        let bindings = shader.vertex_binding_descriptions();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].stride, 32);
        let attributes = shader.vertex_attribute_descriptions();
        assert_eq!(
            attributes
                .iter()
                .map(|attribute| (attribute.location, attribute.offset, attribute.format))
                .collect::<Vec<_>>(),
            vec![
                (0, 0, vk::Format::R32G32B32_SFLOAT),
                (1, 12, vk::Format::R32G32B32_SFLOAT),
            ]
        );
        assert!(!shader.hot_reload());
        assert_eq!(shader.layout_bindings().len(), 1);
        assert_eq!(
            shader
                .precompiled_shaders()
                .vert
                .entry_point_name
                .to_str()
                .unwrap(),
            "vertexMain"
        );
    }

    #[test]
    fn misaligned_spirv_is_rejected() {
        let mut module = spv_module();
        module.push(0);
        let err = match RuntimeShader::new(REFLECTION, &module, &spv_module()) {
            Ok(_) => panic!("misaligned SPIR-V was accepted"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("vertex SPIR-V"), "{err}");
    }
}
