//! JSON format for pipeline & descriptor set layouts
//!
//! These are based on what's needed for the vulkan builders

use serde::{Deserialize, Serialize};

/// reflected data for creating a vulkan PipelineLayout
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectedPipelineLayout {
    pub descriptor_set_layouts: Vec<ReflectedDescriptorSetLayout>,
    pub push_constant_ranges: Vec<ReflectedPushConstantRange>,

    /// The descriptor set index slang reserved for its bindless texture heap,
    /// or None when this shader declares no `DescriptorHandle` field.
    pub bindless_heap_set: Option<u32>,
}

/// reflected data for creating a vulkan DescriptorSetLayout
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectedPushConstantRange {
    pub stage_flags: ReflectedStageFlags,
    pub offset: u32,
    pub size: u32,
}

/// The vulkan-guaranteed push constant budget: `maxPushConstantsSize` is at least
/// 128 bytes on every conformant implementation, so a block within it is portable
/// without querying the device.
///
/// Lives here rather than in either consumer because there are three places the
/// number has to agree: codegen's own budget check, the `<= N` assert codegen emits,
/// and the renderer's inline payload buffer.
pub const MAX_PUSH_CONSTANT_BYTES: usize = 128;

// a slang BindingType or vulkan DescriptorType
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReflectedBindingType {
    Sampler,
    Texture,
    ConstantBuffer,
    CombinedTextureSampler,
    StorageImage,
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
