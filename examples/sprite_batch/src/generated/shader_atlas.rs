pub mod projection;
pub mod sprite_batch;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub sprite_batch: sprite_batch::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            sprite_batch: sprite_batch::Shader::init(),
        }
    }
}
