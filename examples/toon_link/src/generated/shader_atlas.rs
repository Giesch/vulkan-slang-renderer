pub mod mltrs;
pub mod tev;
pub mod toon_link;

pub struct ShaderAtlas {
    pub toon_link: toon_link::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            toon_link: toon_link::Shader::init(),
        }
    }
}
