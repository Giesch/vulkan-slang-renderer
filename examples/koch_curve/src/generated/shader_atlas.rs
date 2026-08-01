pub mod koch_curve;

pub struct ShaderAtlas {
    pub koch_curve: koch_curve::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            koch_curve: koch_curve::Shader::init(),
        }
    }
}
