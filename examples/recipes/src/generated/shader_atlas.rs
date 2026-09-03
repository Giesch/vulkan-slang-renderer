pub mod recipe_render;
pub mod recipes_compute;
pub mod shared;

use ::mltrs::shaders::atlas::ShaderAtlasRoot;

pub struct ShaderAtlas {
    pub recipe_render: recipe_render::Shader,
    pub recipes_compute: recipes_compute::Shader,
}

impl ShaderAtlasRoot for ShaderAtlas {
    const SHADERS_SOURCE_DIR: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source");

    fn init() -> Self {
        Self {
            recipe_render: recipe_render::Shader::init(),
            recipes_compute: recipes_compute::Shader::init(),
        }
    }
}
