pub mod mltrs;
pub mod ray_march_camera;
pub mod ray_marching;

use ::mltrs::shaders::atlas::ShaderAtlasRoot;

pub struct ShaderAtlas {
    pub ray_marching: ray_marching::Shader,
}

impl ShaderAtlasRoot for ShaderAtlas {
    const SHADERS_SOURCE_DIR: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source");

    fn init() -> Self {
        Self {
            ray_marching: ray_marching::Shader::init(),
        }
    }
}
