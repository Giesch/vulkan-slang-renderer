pub mod projection;
pub mod ray_march_camera;
pub mod ray_marching;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub ray_marching: ray_marching::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            ray_marching: ray_marching::Shader::init(),
        }
    }
}
