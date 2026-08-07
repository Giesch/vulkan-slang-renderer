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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_value_roundtrip_is_stable() {
        let raw = include_str!("fixtures/basic_triangle.json");
        let parsed: ReflectionJson = serde_json::from_str(raw).unwrap();
        let reparsed: ReflectionJson =
            serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            serde_json::to_value(&reparsed).unwrap()
        );
    }
}
