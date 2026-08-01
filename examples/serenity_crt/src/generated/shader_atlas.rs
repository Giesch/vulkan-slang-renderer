pub mod serenity_crt;

pub struct ShaderAtlas {
    pub serenity_crt: serenity_crt::Shader,
}

impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            serenity_crt: serenity_crt::Shader::init(),
        }
    }
}
