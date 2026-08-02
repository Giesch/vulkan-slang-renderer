pub mod mltrs;
pub mod space_invaders;

pub struct ShaderAtlas {
    pub space_invaders: space_invaders::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            space_invaders: space_invaders::Shader::init(),
        }
    }
}
