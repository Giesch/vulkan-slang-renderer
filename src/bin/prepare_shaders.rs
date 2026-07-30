use vulkan_slang_renderer::build_tasks::{self, Config};
use vulkan_slang_renderer::env_config::EnvConfig;
use vulkan_slang_renderer::util::manifest_path;

pub fn main() {
    let env = EnvConfig::from_env();

    let config = Config {
        generate_rust_source: env.generate_rust_source,
        rust_source_dir: manifest_path(["src"]),
        shaders_source_dir: manifest_path(["shaders", "source"]),
        compiled_shaders_dir: manifest_path(["shaders", "compiled"]),
    };

    build_tasks::write_precompiled_shaders(config).unwrap();
}
