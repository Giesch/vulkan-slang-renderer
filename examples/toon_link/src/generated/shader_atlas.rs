pub mod mltrs;
pub mod skinning;
pub mod skinning_oracle_compute;
pub mod tev;
pub mod toon_link;
pub mod toon_link_modern;

use ::mltrs::shaders::atlas::ShaderAtlasRoot;

pub struct ShaderAtlas {
    pub toon_link: toon_link::Shader,
    pub toon_link_modern: toon_link_modern::Shader,
    pub skinning_oracle_compute: skinning_oracle_compute::Shader,
}

impl ShaderAtlasRoot for ShaderAtlas {
    const SHADERS_SOURCE_DIR: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source");

    fn init() -> Self {
        Self {
            toon_link: toon_link::Shader::init(),
            toon_link_modern: toon_link_modern::Shader::init(),
            skinning_oracle_compute: skinning_oracle_compute::Shader::init(),
        }
    }
}
