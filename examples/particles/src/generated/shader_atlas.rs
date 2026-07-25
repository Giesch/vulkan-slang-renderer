pub mod particle;
pub mod particle_render;
pub mod particles_compute;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub particle_render: particle_render::Shader,
    pub particles_compute: particles_compute::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            particle_render: particle_render::Shader::init(),
            particles_compute: particles_compute::Shader::init(),
        }
    }
}
