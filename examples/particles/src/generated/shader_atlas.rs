pub mod particle;
pub mod particle_render;
pub mod particles_compute;

use ::mltrs::shaders::atlas::ShaderAtlasRoot;

pub struct ShaderAtlas {
    pub particle_render: particle_render::Shader,
    pub particles_compute: particles_compute::Shader,
}

impl ShaderAtlasRoot for ShaderAtlas {
    const SHADERS_SOURCE_DIR: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source");

    fn init() -> Self {
        Self {
            particle_render: particle_render::Shader::init(),
            particles_compute: particles_compute::Shader::init(),
        }
    }
}
