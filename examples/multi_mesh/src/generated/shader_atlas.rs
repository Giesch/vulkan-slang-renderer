pub mod mltrs;
pub mod multi_mesh;

pub struct ShaderAtlas {
    pub multi_mesh: multi_mesh::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            multi_mesh: multi_mesh::Shader::init(),
        }
    }
}
