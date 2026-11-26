//! Shader entries built at runtime from precompiled SPIR-V and reflection JSON,
//! for hosts whose shaders are not generated Rust bindings.

use std::ffi::CString;

use crate::renderer::LayoutDescription;

use super::atlas::{
    ComputeShaderAtlasEntry, PrecompiledShader, PrecompiledShaders, ShaderAtlasEntry,
};
use super::json::{
    ComputeReflectionJson, ReflectedPipelineLayout, ReflectionJson,
    layout_bindings_from_pipeline_layout,
};

pub struct RuntimeShader {
    reflection: ReflectionJson,
    vertex_spv: Vec<u32>,
    fragment_spv: Vec<u32>,
}

impl RuntimeShader {
    /// `reflection_json` is the compiler's reflection output for the shader;
    /// the SPIR-V slices are its vertex and fragment stages. When shader hot
    /// reload is on, the renderer recompiles it from its `source_file_name` in
    /// the hot reload source dir like any generated entry.
    pub fn new(
        reflection_json: &str,
        vertex_spv: &[u8],
        fragment_spv: &[u8],
    ) -> anyhow::Result<Self> {
        let reflection: ReflectionJson = serde_json::from_str(reflection_json)?;

        Ok(Self {
            vertex_spv: read_spv(vertex_spv, "vertex")?,
            fragment_spv: read_spv(fragment_spv, "fragment")?,
            reflection,
        })
    }
}

fn read_spv(bytes: &[u8], stage: &str) -> anyhow::Result<Vec<u32>> {
    let mut cursor = std::io::Cursor::new(bytes);

    ash::util::read_spv(&mut cursor)
        .map_err(|err| anyhow::anyhow!("{stage} SPIR-V is not a valid module: {err}"))
}

impl ShaderAtlasEntry for RuntimeShader {
    fn source_file_name(&self) -> &str {
        &self.reflection.source_file_name
    }

    fn reflection_json(&self) -> &ReflectionJson {
        &self.reflection
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

pub struct RuntimeComputeShader {
    reflection: ComputeReflectionJson,
    compute_spv: Vec<u32>,
}

impl RuntimeComputeShader {
    /// `reflection_json` is the compiler's compute reflection output for the
    /// shader; `compute_spv` is its compute stage. When shader hot reload is
    /// on, the renderer recompiles it from its `source_file_name` in the hot
    /// reload source dir like any generated entry.
    pub fn new(reflection_json: &str, compute_spv: &[u8]) -> anyhow::Result<Self> {
        let reflection: ComputeReflectionJson = serde_json::from_str(reflection_json)?;

        Ok(Self {
            compute_spv: read_spv(compute_spv, "compute")?,
            reflection,
        })
    }
}

impl ComputeShaderAtlasEntry for RuntimeComputeShader {
    fn source_file_name(&self) -> &str {
        &self.reflection.source_file_name
    }

    fn reflection_json(&self) -> &ComputeReflectionJson {
        &self.reflection
    }

    fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>> {
        layout_bindings_from_pipeline_layout(&self.reflection.pipeline_layout)
    }

    fn precompiled_compute_shader(&self) -> PrecompiledShader {
        PrecompiledShader {
            entry_point_name: entry_point_name(
                &self.reflection.compute_entry_point.entry_point_name,
            ),
            spv_bytes: self.compute_spv.clone(),
        }
    }

    fn pipeline_layout(&self) -> &ReflectedPipelineLayout {
        &self.reflection.pipeline_layout
    }

    fn workgroup_size(&self) -> [u32; 3] {
        self.reflection.workgroup_size
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
    const COMPUTE_REFLECTION: &str =
        include_str!("../../../../examples/recipes/shaders/compiled/recipes.comp.json");

    fn spv_module() -> Vec<u8> {
        // magic number, version, generator, bound, schema
        [0x0723_0203u32, 0x0001_0000, 0, 1, 0]
            .iter()
            .flat_map(|word| word.to_ne_bytes())
            .collect()
    }

    #[test]
    fn runtime_shader_describes_the_reflected_entry_points() {
        let module = spv_module();
        let shader = RuntimeShader::new(REFLECTION, &module, &module).unwrap();

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

    #[test]
    fn runtime_compute_shader_describes_the_reflected_entry_point() {
        let module = spv_module();
        let shader = RuntimeComputeShader::new(COMPUTE_REFLECTION, &module).unwrap();
        let reflection: ComputeReflectionJson = serde_json::from_str(COMPUTE_REFLECTION).unwrap();

        assert_eq!(shader.workgroup_size(), reflection.workgroup_size);
        assert_eq!(shader.source_file_name(), reflection.source_file_name);
        assert_eq!(
            shader.layout_bindings().len(),
            layout_bindings_from_pipeline_layout(&reflection.pipeline_layout).len()
        );
        let precompiled = shader.precompiled_compute_shader();
        assert_eq!(
            precompiled.entry_point_name.to_str().unwrap(),
            reflection.compute_entry_point.entry_point_name
        );
        assert_eq!(
            precompiled.spv_bytes,
            [0x0723_0203u32, 0x0001_0000, 0, 1, 0]
        );
    }

    #[test]
    fn runtime_compute_shader_rejects_misaligned_spirv() {
        let mut module = spv_module();
        module.push(0);
        let err = match RuntimeComputeShader::new(COMPUTE_REFLECTION, &module) {
            Ok(_) => panic!("misaligned SPIR-V was accepted"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("compute SPIR-V"), "{err}");
    }

    /// graphics reflection lacks the compute entry point and workgroup size
    #[test]
    fn runtime_compute_shader_rejects_graphics_reflection() {
        assert!(RuntimeComputeShader::new(REFLECTION, &spv_module()).is_err());
    }
}
