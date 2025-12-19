use vulkan_slang_renderer::build_tasks::{self, Config};
use vulkan_slang_renderer::util::manifest_path;

pub fn main() {
    let arg = std::env::var("GENERATE_RUST_SOURCE").ok();

    let generate_rust_source = match arg {
        None => false,
        Some(s) if s.is_empty() => false,
        Some(s) if s.to_lowercase() == "false" => false,
        _ => true,
    };

    let config = Config {
        generate_rust_source,
        rust_source_dir: manifest_path(["src"]),
        shaders_source_dir: manifest_path(["shaders", "source"]),
        compiled_shaders_dir: manifest_path(["shaders", "compiled"]),
    };

    build_tasks::write_precompiled_shaders(config).unwrap();
}
