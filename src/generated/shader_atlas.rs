pub mod basic_triangle;
pub mod depth_texture;
pub mod space_invaders;
pub mod sprite_batch;

pub struct ShaderAtlas {
    pub space_invaders: space_invaders::Shader,
    pub basic_triangle: basic_triangle::Shader,
    pub depth_texture: depth_texture::Shader,
    pub sprite_batch: sprite_batch::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            space_invaders: space_invaders::Shader::init(),
            basic_triangle: basic_triangle::Shader::init(),
            depth_texture: depth_texture::Shader::init(),
            sprite_batch: sprite_batch::Shader::init(),
        }
    }
}
