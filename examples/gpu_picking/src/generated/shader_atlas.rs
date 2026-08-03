pub mod gpu_picking;
pub mod gpu_picking_common;
pub mod gpu_picking_id;
pub mod mltrs;
pub mod ray_march_camera;

use ::mltrs::shaders::atlas::ShaderAtlasRoot;

pub struct ShaderAtlas {
    pub gpu_picking: gpu_picking::Shader,
    pub gpu_picking_id: gpu_picking_id::Shader,
}

impl ShaderAtlasRoot for ShaderAtlas {
    const SHADERS_SOURCE_DIR: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source");

    fn init() -> Self {
        Self {
            gpu_picking: gpu_picking::Shader::init(),
            gpu_picking_id: gpu_picking_id::Shader::init(),
        }
    }
}
