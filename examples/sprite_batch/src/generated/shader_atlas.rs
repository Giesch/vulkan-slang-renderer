pub mod mltrs;
pub mod sprite_batch;

pub struct ShaderAtlas {
    pub sprite_batch: sprite_batch::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            sprite_batch: sprite_batch::Shader::init(),
        }
    }
}
