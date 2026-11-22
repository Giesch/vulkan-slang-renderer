//! Negative control: a typed indexed config accepts only a mesh of its own
//! vertex type. A `MeshHandle<OtherVertex>` cannot complete a config whose
//! shader's vertex input is `MeshVertex`.

#![allow(dead_code)]

use mltrs_renderer::renderer::UniformBufferHandle;
use mltrs_renderer::renderer::pipeline::MeshHandle;

use render_graph_api_checks::generated::shader_atlas::mesh::{self, MeshParams, Resources};
use render_graph_api_checks::generated::shader_atlas::other_mesh::OtherVertex;

fn wrong(params: &UniformBufferHandle<MeshParams>, other_mesh: &MeshHandle<OtherVertex>) {
    let _ = mesh::Shader::init()
        .pipeline_config(Resources {
            mesh_params_buffer: params,
        })
        .with_shared_mesh(other_mesh);
}

fn main() {}
