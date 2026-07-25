pub mod projection;
pub mod space_invaders;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub space_invaders: space_invaders::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            space_invaders: space_invaders::Shader::init(),
        }
    }
}
