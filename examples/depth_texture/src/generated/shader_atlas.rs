pub mod depth_texture;
pub mod mltrs;

pub struct ShaderAtlas {
    pub depth_texture: depth_texture::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            depth_texture: depth_texture::Shader::init(),
        }
    }
}
