pub mod mltrs;
pub mod suzanne;

pub struct ShaderAtlas {
    pub suzanne: suzanne::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            suzanne: suzanne::Shader::init(),
        }
    }
}
