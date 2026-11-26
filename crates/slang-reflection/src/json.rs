use serde::{Deserialize, Serialize};

mod parameters;
pub use parameters::*;

mod pipeline_builders;
pub use pipeline_builders::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectionJson {
    pub source_file_name: String,
    pub global_parameters: Vec<GlobalParameter>,
    pub vertex_entry_point: EntryPoint,
    pub fragment_entry_point: EntryPoint,
    pub pipeline_layout: ReflectedPipelineLayout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeReflectionJson {
    pub source_file_name: String,
    pub global_parameters: Vec<GlobalParameter>,
    pub compute_entry_point: EntryPoint,
    pub workgroup_size: [u32; 3],
    pub pipeline_layout: ReflectedPipelineLayout,
}
