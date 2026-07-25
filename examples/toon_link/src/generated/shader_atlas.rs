pub mod mvp;
pub mod toon_link;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub toon_link: toon_link::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            toon_link: toon_link::Shader::init(),
        }
    }
}
