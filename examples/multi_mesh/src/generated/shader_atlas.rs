pub mod multi_mesh;
pub mod mvp;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub multi_mesh: multi_mesh::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            multi_mesh: multi_mesh::Shader::init(),
        }
    }
}
