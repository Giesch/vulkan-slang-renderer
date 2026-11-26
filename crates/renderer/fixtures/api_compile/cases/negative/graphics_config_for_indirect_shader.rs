//! Negative control: the draw-call kind is inferred from the shader. A vertex
//! entry point that reads `SV_DrawIndex` generates a `GraphicsIndirect`
//! config, which cannot be used where a direct `Graphics` config is expected.

#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::{Graphics, NoPush, PipelineConfig};

use render_graph_api_checks::generated::shader_atlas::indirect_mesh::{
    self, IndirectMeshParams, Resources,
};

fn wrong(params: &UniformBufferHandle<IndirectMeshParams>) {
    let _config: PipelineConfig<'_, Graphics, NoPush> = indirect_mesh::Shader::init()
        .pipeline_config(Resources {
            indirect_mesh_params_buffer: params,
        });
}

fn main() {}
