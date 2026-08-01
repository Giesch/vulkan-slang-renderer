pub mod particle;
pub mod particle_render;
pub mod particles_compute;

pub struct ShaderAtlas {
    pub particle_render: particle_render::Shader,
    pub particles_compute: particles_compute::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            particle_render: particle_render::Shader::init(),
            particles_compute: particles_compute::Shader::init(),
        }
    }
}
