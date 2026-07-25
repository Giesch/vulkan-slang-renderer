pub mod mvp;
pub mod suzanne;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub suzanne: suzanne::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            suzanne: suzanne::Shader::init(),
        }
    }
}
