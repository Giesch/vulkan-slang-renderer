pub mod sdf_2d;

pub struct ShaderAtlas {
    pub sdf_2d: sdf_2d::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            sdf_2d: sdf_2d::Shader::init(),
        }
    }
}
