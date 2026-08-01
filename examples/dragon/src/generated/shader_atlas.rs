pub mod dragon;

pub struct ShaderAtlas {
    pub dragon: dragon::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            dragon: dragon::Shader::init(),
        }
    }
}
