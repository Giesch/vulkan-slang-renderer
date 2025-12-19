pub mod basic_triangle;
pub mod depth_texture;
pub mod sdf_2d;

pub struct ShaderAtlas {
    pub sdf_2d: sdf_2d::Shader,
    pub basic_triangle: basic_triangle::Shader,
    pub depth_texture: depth_texture::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            sdf_2d: sdf_2d::Shader::init(),
            basic_triangle: basic_triangle::Shader::init(),
            depth_texture: depth_texture::Shader::init(),
        }
    }
}
