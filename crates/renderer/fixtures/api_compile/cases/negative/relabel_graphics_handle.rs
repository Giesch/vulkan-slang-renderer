//! Negative control: only the renderer mints graphics handles. A caller
//! cannot rebuild a handle from its parts to give it a different vertex
//! interface, because the fields are private.

#![allow(dead_code)]

use std::marker::PhantomData;

use mltrs_renderer::renderer::pipeline::{
    DrawIndexed, GraphicsPipelineHandle, RuntimeVertex, TypedVertex,
};

use render_graph_api_checks::generated::shader_atlas::mesh::MeshVertex;

fn relabel(
    handle: GraphicsPipelineHandle<RuntimeVertex, DrawIndexed>,
) -> GraphicsPipelineHandle<TypedVertex<MeshVertex>, DrawIndexed> {
    let GraphicsPipelineHandle { raw, .. } = handle;

    GraphicsPipelineHandle {
        raw,
        _interface: PhantomData,
    }
}

fn main() {}
