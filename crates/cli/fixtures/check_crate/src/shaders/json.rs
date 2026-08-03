use serde::Deserialize;

use crate::renderer::LayoutDescription;

#[derive(Clone, Deserialize)]
pub struct ReflectionJson {
    pub source_file_name: String,
    pub vertex_entry_point: EntryPoint,
    pub fragment_entry_point: EntryPoint,
    pub pipeline_layout: ReflectedPipelineLayout,
}

impl ReflectionJson {
    pub fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>> {
        vec![]
    }
}

#[derive(Clone, Deserialize)]
pub struct EntryPoint {
    pub entry_point_name: String,
}

#[derive(Clone, Deserialize)]
pub struct ComputeReflectionJson {
    pub source_file_name: String,
    pub compute_entry_point: EntryPoint,
    pub workgroup_size: [u32; 3],
    pub pipeline_layout: ReflectedPipelineLayout,
}

impl ComputeReflectionJson {
    pub fn layout_bindings(&self) -> Vec<Vec<LayoutDescription>> {
        vec![]
    }
}

#[derive(Clone, Deserialize)]
pub struct ReflectedPipelineLayout;
