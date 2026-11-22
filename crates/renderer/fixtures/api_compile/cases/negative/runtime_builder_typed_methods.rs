//! Negative control: a packed-bytes config has no typed vertex or typed
//! shared-mesh methods. `build_indexed_bytes` returns a complete
//! `RuntimeVertex` config; there is no vertex type to upload or match.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{MeshHandle, NoPush, PipelineConfigBuilder};

use render_graph_api_checks::generated::shader_atlas::mesh::MeshVertex;

fn wrong_mesh(
    builder: PipelineConfigBuilder<'_>,
    vertex_bytes: Vec<u8>,
    indices: Vec<u32>,
    mesh_handle: &MeshHandle<MeshVertex>,
) {
    let _ = builder
        .build_indexed_bytes::<NoPush>(vertex_bytes, indices)
        .with_shared_mesh(mesh_handle);
}

fn wrong_vertices(
    builder: PipelineConfigBuilder<'_>,
    vertex_bytes: Vec<u8>,
    indices: Vec<u32>,
    vertices: Vec<MeshVertex>,
) {
    let _ = builder
        .build_indexed_bytes::<NoPush>(vertex_bytes, indices.clone())
        .with_vertices(vertices, indices);
}

fn main() {}
