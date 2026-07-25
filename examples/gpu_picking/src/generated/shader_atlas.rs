pub mod gpu_picking;
pub mod gpu_picking_common;
pub mod gpu_picking_id;
pub mod projection;
pub mod ray_march_camera;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub gpu_picking: gpu_picking::Shader,
    pub gpu_picking_id: gpu_picking_id::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            gpu_picking: gpu_picking::Shader::init(),
            gpu_picking_id: gpu_picking_id::Shader::init(),
        }
    }
}
