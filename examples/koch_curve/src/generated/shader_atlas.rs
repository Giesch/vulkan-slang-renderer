pub mod koch_curve;

use ::mltrs::shaders::atlas::ShaderAtlasRoot;

pub struct ShaderAtlas {
    pub koch_curve: koch_curve::Shader,
}

impl ShaderAtlasRoot for ShaderAtlas {
    const SHADERS_SOURCE_DIR: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source");

    fn init() -> Self {
        Self {
            koch_curve: koch_curve::Shader::init(),
        }
    }
}
