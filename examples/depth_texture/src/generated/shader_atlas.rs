pub mod depth_texture;
pub mod mvp;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub depth_texture: depth_texture::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            depth_texture: depth_texture::Shader::init(),
        }
    }
}
