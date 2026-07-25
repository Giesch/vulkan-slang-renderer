pub mod basic_triangle;
pub mod mvp;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub basic_triangle: basic_triangle::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            basic_triangle: basic_triangle::Shader::init(),
        }
    }
}
