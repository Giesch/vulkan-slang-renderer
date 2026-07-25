pub mod serenity_crt;

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub serenity_crt: serenity_crt::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            serenity_crt: serenity_crt::Shader::init(),
        }
    }
}
