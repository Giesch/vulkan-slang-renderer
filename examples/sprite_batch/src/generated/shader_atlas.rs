pub mod mltrs;
pub mod sprite_batch;

use ::mltrs::shaders::atlas::ShaderAtlasRoot;

pub struct ShaderAtlas {
    pub sprite_batch: sprite_batch::Shader,
}

impl ShaderAtlasRoot for ShaderAtlas {
    const SHADERS_SOURCE_DIR: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source");

    fn init() -> Self {
        Self {
            sprite_batch: sprite_batch::Shader::init(),
        }
    }
}
