//! slang-reflection-based vulkan builders
//! using automatically generated bindings from slang ParameterBlocks
//!
//! based on the example in the slang docs here:
//! https://docs.shader-slang.org/en/latest/parameter-blocks.html#using-parameter-blocks-with-reflection

use shader_slang as slang;

use crate::shaders::json::*;

pub fn reflect_pipeline_layout(
    program_layout: &slang::reflection::Shader,
) -> ReflectedPipelineLayout {
    let mut pipeline_layout_builder = PipelineLayoutBuilder::new();

    let mut default_descriptor_set_layout_builder =
        DescriptorSetLayoutBuilder::reserve_slot(&mut pipeline_layout_builder);

    default_descriptor_set_layout_builder
        .add_global_scope_parameters(program_layout, &mut pipeline_layout_builder);
    default_descriptor_set_layout_builder
        .add_entry_point_parameters(program_layout, &mut pipeline_layout_builder);

    default_descriptor_set_layout_builder.build_and_add(&mut pipeline_layout_builder);

    pipeline_layout_builder.build()
}

pub struct PipelineLayoutBuilder {
    descriptor_set_layouts: Vec<Option<ReflectedDescriptorSetLayout>>,
    push_constant_ranges: Vec<ReflectedPushConstantRange>,
    current_stage_flags: ReflectedStageFlags,
}

impl PipelineLayoutBuilder {
    pub fn new() -> Self {
        Self {
            descriptor_set_layouts: vec![],
            push_constant_ranges: vec![],
            current_stage_flags: ReflectedStageFlags::All,
        }
    }

    pub fn add_push_constatant_range_for_constant_buffer(
        &mut self,
        constant_buffer_type_layout: &slang::reflection::TypeLayout,
    ) {
        let element_type_layout = constant_buffer_type_layout.element_type_layout();
        let element_size = element_type_layout.size(slang::ParameterCategory::Uniform);

        if element_size == 0 {
            return;
        }

        // NOTE this relies on the way the slang compiler
        // only ever uses one push constant range per entry point
        let offset = 0;

        let push_constant_range = ReflectedPushConstantRange {
            stage_flags: self.current_stage_flags,
            offset,
            size: element_size as u32,
        };

        self.push_constant_ranges.push(push_constant_range);
    }

    fn add_sub_object_ranges(&mut self, type_layout: &slang::reflection::TypeLayout) {
        for sub_object_range_index in 0..type_layout.sub_object_range_count() {
            self.add_sub_object_range(type_layout, sub_object_range_index);
        }
    }

    fn add_sub_object_range(
        &mut self,
        type_layout: &slang::reflection::TypeLayout,
        sub_object_range_index: i64,
    ) {
        let binding_range_index =
            type_layout.sub_object_range_binding_range_index(sub_object_range_index);
        let binding_type = type_layout.binding_range_type(binding_range_index);

        match binding_type {
            slang::BindingType::ParameterBlock => {
                let parameter_block_type_layout =
                    type_layout.binding_range_leaf_type_layout(binding_range_index);
                self.add_descriptor_set_for_parameter_block(parameter_block_type_layout);
            }

            slang::BindingType::PushConstant => {
                let constant_buffer_type_layout =
                    type_layout.binding_range_leaf_type_layout(binding_range_index);
                self.add_push_constatant_range_for_constant_buffer(constant_buffer_type_layout);
            }

            // slang::BindingType::Unknown => todo!(),
            // slang::BindingType::Sampler => todo!(),
            // slang::BindingType::Texture => todo!(),
            // slang::BindingType::ConstantBuffer => todo!(),
            // slang::BindingType::TypedBuffer => todo!(),
            // slang::BindingType::RawBuffer => todo!(),
            // slang::BindingType::CombinedTextureSampler => todo!(),
            // slang::BindingType::InputRenderTarget => todo!(),
            // slang::BindingType::InlineUniformData => todo!(),
            // slang::BindingType::RayTracingAccelerationStructure => todo!(),
            // slang::BindingType::VaryingInput => todo!(),
            // slang::BindingType::VaryingOutput => todo!(),
            // slang::BindingType::ExistentialValue => todo!(),
            // slang::BindingType::MutableFlag => todo!(),
            // slang::BindingType::MutableTeture => todo!(),
            // slang::BindingType::MutableTypedBuffer => todo!(),
            // slang::BindingType::MutableRawBuffer => todo!(),
            // slang::BindingType::BaseMask => todo!(),
            // slang::BindingType::ExtMask => todo!(),
            _ => {}
        }
    }

    pub fn add_descriptor_set_for_parameter_block(
        &mut self,
        parameter_block_type_layout: &slang::reflection::TypeLayout,
    ) {
        let mut descriptor_set_layout_builder = DescriptorSetLayoutBuilder::reserve_slot(self);
        descriptor_set_layout_builder.add_descriptor_ranges_for_parameter_block_element(
            parameter_block_type_layout.element_type_layout(),
            self,
        );

        descriptor_set_layout_builder.build_and_add(self);
    }

    // aka 'finishBuilding' in the docs
    pub fn build(self) -> ReflectedPipelineLayout {
        // a null here represents an unused reserved slot for a
        // ParameterBlock that ended up only containing other ParameterBlocks
        // https://docs.shader-slang.org/en/latest/parameter-blocks.html#empty-parameter-blocks
        let descriptor_set_layouts: Vec<ReflectedDescriptorSetLayout> =
            self.descriptor_set_layouts.into_iter().flatten().collect();

        ReflectedPipelineLayout {
            descriptor_set_layouts,
            push_constant_ranges: self.push_constant_ranges,
        }
    }
}

pub struct DescriptorSetLayoutBuilder {
    set_index: usize,
    binding_ranges: Vec<ReflectedDescriptorSetLayoutBinding>,
}

impl DescriptorSetLayoutBuilder {
    pub fn reserve_slot(pipeline_layout_builder: &mut PipelineLayoutBuilder) -> Self {
        // reserve a layout slot to be filled in later
        // this preserves the correct index order for nested ParameterBlocks
        // https://docs.shader-slang.org/en/latest/parameter-blocks.html#ordering-of-nested-parameter-blocks
        let set_index = pipeline_layout_builder.descriptor_set_layouts.len();
        pipeline_layout_builder.descriptor_set_layouts.push(None);

        Self {
            set_index,
            binding_ranges: vec![],
        }
    }

    /// https://docs.shader-slang.org/en/latest/parameter-blocks.html#automatically-introduced-uniform-buffer
    pub fn add_descriptor_ranges_for_parameter_block_element(
        &mut self,
        element_layout: &slang::reflection::TypeLayout,
        pipeline_layout_builder: &mut PipelineLayoutBuilder,
    ) {
        // in the cpp header there's a default argument overload for Uniform
        let default_uniform_buffer_size = element_layout.size(slang::ParameterCategory::Uniform);
        if default_uniform_buffer_size > 0 {
            self.add_automatically_introduced_uniform_buffer(
                pipeline_layout_builder,
                default_uniform_buffer_size,
            );
        }

        self.add_descriptor_ranges(pipeline_layout_builder, element_layout);
        pipeline_layout_builder.add_sub_object_ranges(element_layout);
    }

    fn add_automatically_introduced_uniform_buffer(
        &mut self,
        pipeline_layout_builder: &mut PipelineLayoutBuilder,
        size: usize,
    ) {
        // this relies on using no manual binding annotations
        let vk_binding_index = self.binding_ranges.len() as u32;

        let binding = ReflectedDescriptorSetLayoutBinding {
            binding: vk_binding_index,
            descriptor_type: ReflectedBindingType::ConstantBuffer,
            descriptor_count: 1,
            stage_flags: pipeline_layout_builder.current_stage_flags,
            size,
        };

        self.binding_ranges.push(binding)
    }

    fn add_descriptor_ranges(
        &mut self,
        pipeline_layout_builder: &mut PipelineLayoutBuilder,
        type_layout: &slang::reflection::TypeLayout,
    ) {
        // NOTE this means we are only querying the first descriptor set
        // doing this is vulkan-specific
        let relative_set_index = 0;

        let range_count = type_layout.descriptor_set_descriptor_range_count(relative_set_index);

        for range_index in 0..range_count {
            self.add_descriptor_range(
                pipeline_layout_builder,
                type_layout,
                relative_set_index,
                range_index,
            );
        }
    }

    fn add_descriptor_range(
        &mut self,
        pipeline_layout_builder: &mut PipelineLayoutBuilder,
        type_layout: &slang::reflection::TypeLayout,
        relative_set_index: i64,
        range_index: i64,
    ) {
        let binding_type =
            type_layout.descriptor_set_descriptor_range_type(relative_set_index, range_index);
        if binding_type == slang::BindingType::PushConstant {
            // this is accounted for in add_sub_object_range
            return;
        }

        let descriptor_count = type_layout
            .descriptor_set_descriptor_range_descriptor_count(relative_set_index, range_index);

        // this relies on using no manual binding annotations
        let vk_binding_index = self.binding_ranges.len() as u32;
        let descriptor_type = ReflectedBindingType::from_slang(binding_type);

        let size = type_layout.size(type_layout.parameter_category());
        let descriptor_set_layout_binding = ReflectedDescriptorSetLayoutBinding {
            binding: vk_binding_index,
            descriptor_type,
            descriptor_count: descriptor_count as u32,
            stage_flags: pipeline_layout_builder.current_stage_flags,
            size,
        };

        self.binding_ranges.push(descriptor_set_layout_binding);
    }

    pub fn add_global_scope_parameters(
        &mut self,
        program_layout: &slang::reflection::Shader,
        pipeline_layout_builder: &mut PipelineLayoutBuilder,
    ) {
        pipeline_layout_builder.current_stage_flags = ReflectedStageFlags::All;
        self.add_descriptor_ranges_for_parameter_block_element(
            program_layout.global_params_type_layout(),
            pipeline_layout_builder,
        );
    }

    pub fn add_entry_point_parameters(
        &mut self,
        program_layout: &slang::reflection::Shader,
        pipeline_layout_builder: &mut PipelineLayoutBuilder,
    ) {
        for entry_point in program_layout.entry_points() {
            pipeline_layout_builder.current_stage_flags =
                ReflectedStageFlags::from_slang(entry_point.stage());
            self.add_descriptor_ranges_for_parameter_block_element(
                entry_point.type_layout(),
                pipeline_layout_builder,
            );
        }
    }

    // aka 'finishBuilding' in the docs
    // creates a vulkan DescriptorSetLayout and adds it to the PipelineLayoutBuilder
    pub fn build_and_add(&self, pipeline_layout_builder: &mut PipelineLayoutBuilder) {
        if self.binding_ranges.is_empty() {
            return;
        }

        let layout = ReflectedDescriptorSetLayout {
            binding_ranges: self.binding_ranges.clone(),
        };

        pipeline_layout_builder.descriptor_set_layouts[self.set_index] = Some(layout);
    }
}

impl ReflectedStageFlags {
    // cpp getShaderStageFlags
    pub fn from_slang(stage: slang::Stage) -> Self {
        match stage {
            slang::Stage::Vertex => Self::Vertex,
            slang::Stage::Fragment => Self::Fragment,
            slang::Stage::Compute => Self::Compute,
            slang::Stage::None => Self::Empty,

            // raytracing, mesh, tesselation, dispatch, & count
            _ => unimplemented!(),
        }
    }
}

impl ReflectedBindingType {
    // cpp mapSlangBindingTypeToVulkanDescriptorType
    pub fn from_slang(binding_type: slang::BindingType) -> Self {
        match binding_type {
            slang::BindingType::Sampler => Self::Sampler,
            slang::BindingType::Texture => Self::Texture,
            slang::BindingType::ConstantBuffer => Self::ConstantBuffer,
            slang::BindingType::CombinedTextureSampler => Self::CombinedTextureSampler,
            slang::BindingType::RawBuffer => Self::StorageBuffer,

            slang::BindingType::PushConstant => todo!(),
            slang::BindingType::ParameterBlock => todo!(),

            slang::BindingType::VaryingInput => todo!(),
            slang::BindingType::VaryingOutput => todo!(),
            slang::BindingType::TypedBuffer => todo!(),
            slang::BindingType::InputRenderTarget => todo!(),
            slang::BindingType::InlineUniformData => todo!(),
            slang::BindingType::RayTracingAccelerationStructure => todo!(),
            slang::BindingType::ExistentialValue => todo!(),
            slang::BindingType::MutableFlag => todo!(),
            slang::BindingType::MutableTeture => todo!(),
            slang::BindingType::MutableTypedBuffer => todo!(),
            slang::BindingType::MutableRawBuffer => todo!(),
            slang::BindingType::BaseMask => todo!(),
            slang::BindingType::ExtMask => todo!(),
            slang::BindingType::Unknown => todo!(),
        }
    }
}
