//! JSON format for pipeline & descriptor set layouts
//!
//! These are based on what's needed for the vulkan builders

use serde::{Deserialize, Serialize};

/// reflected data for creating a vulkan PipelineLayout
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectedPipelineLayout {
    pub descriptor_set_layouts: Vec<ReflectedDescriptorSetLayout>,
    pub push_constant_ranges: Vec<ReflectedPushConstantRange>,
}

/// reflected data for creating a vulkan DescriptorSetLayout
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectedDescriptorSetLayout {
    pub binding_ranges: Vec<ReflectedDescriptorSetLayoutBinding>,
}

/// reflected data for creating a vulkan DescriptorSetLayoutBinding
/// samplers are deliberately excluded
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReflectedDescriptorSetLayoutBinding {
    pub binding: u32,
    pub descriptor_type: ReflectedBindingType,
    pub descriptor_count: u32,
    pub stage_flags: ReflectedStageFlags,
    pub size: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectedPushConstantRange {
    pub stage_flags: ReflectedStageFlags,
    pub offset: u32,
    pub size: u32,
}

// a slang BindingType or vulkan DescriptorType
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum ReflectedBindingType {
    Sampler,
    Texture,
    ConstantBuffer,
    CombinedTextureSampler,
    StorageBuffer,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum ReflectedStageFlags {
    Vertex,
    Fragment,
    Compute,
    All,
    Empty,
}
