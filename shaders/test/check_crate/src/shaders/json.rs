use serde::Deserialize;

use crate::renderer::LayoutDescription;

#[derive(Deserialize)]
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

#[derive(Deserialize)]
pub struct EntryPoint {
    pub entry_point_name: String,
}

#[derive(Deserialize)]
pub struct ReflectedPipelineLayout;
