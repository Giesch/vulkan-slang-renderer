pub mod projection;
pub mod ray_march_camera;
pub mod ray_marching;

pub struct ShaderAtlas {
    pub ray_marching: ray_marching::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            ray_marching: ray_marching::Shader::init(),
        }
    }
}
