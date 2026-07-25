pub mod basic_triangle;
pub mod mvp;

pub struct ShaderAtlas {
    pub basic_triangle: basic_triangle::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            basic_triangle: basic_triangle::Shader::init(),
        }
    }
}
