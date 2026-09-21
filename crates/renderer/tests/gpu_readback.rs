//! Explicit real-Vulkan test; never substitutes a CPU decoder for GPU execution.
//! Run with SDL_VIDEODRIVER=offscreen and VK_ICD_FILENAMES pointing to lavapipe.
#![cfg(not(windows))]

use std::process::Command;

use mltrs_cli::build_tasks::{Config, OptimizationLevel, write_precompiled_shaders};

#[test]
#[ignore = "requires Vulkan 1.3, validation layers and an SDL Vulkan video driver"]
fn generated_readback_executes_on_vulkan() {
    let renderer = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = renderer.parent().unwrap().parent().unwrap();
    let fixture = renderer.join("fixtures/gpu_readback");
    let temporary = std::env::temp_dir().join(format!("mltrs-gpu-readback-{}", std::process::id()));
    std::fs::create_dir_all(temporary.join("src")).unwrap();
    std::fs::create_dir_all(temporary.join("shaders/source")).unwrap();
    std::fs::copy(fixture.join("src/main.rs"), temporary.join("src/main.rs")).unwrap();
    std::fs::copy(
        fixture.join("shaders/source/readback.compute.slang"),
        temporary.join("shaders/source/readback.compute.slang"),
    )
    .unwrap();
    std::fs::write(
        temporary.join("Cargo.toml"),
        format!(
            "[package]\nname = \"gpu-readback-check\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nmltrs-renderer = {{ path = {:?} }}\nfacet = {{ version = \"0.42\", features = [\"reflect\"] }}\nanyhow = \"1\"\nash = \"0.38\"\nglam = {{ version = \"0.33\", features = [\"serde\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\nsdl3 = {{ version = \"0.14.29\", features = [\"ash\", \"build-from-source-static\"] }}\npretty_env_logger = \"0.5\"\n[profile.dev.package.\"*\"]\nopt-level = 3\n",
            renderer
        ),
    )
    .unwrap();
    std::fs::copy(workspace.join("Cargo.lock"), temporary.join("Cargo.lock")).unwrap();
    write_precompiled_shaders(Config {
        generate_rust_source: true,
        rust_source_dir: temporary.join("src"),
        shaders_source_dir: temporary.join("shaders/source"),
        compiled_shaders_dir: temporary.join("shaders/compiled"),
        import_root: "mltrs_renderer".to_owned(),
        optimization: OptimizationLevel::High,
    })
    .expect("compile real Slang and generate the readback decoder");
    let output = Command::new(env!("CARGO"))
        .args(["run", "--offline", "--color=never", "--manifest-path"])
        .arg(temporary.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", workspace.join("target"))
        .current_dir(&temporary)
        .output()
        .expect("execute the Vulkan fixture");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("{stdout}\n{stderr}");
    assert!(
        output.status.success(),
        "Vulkan fixture failed: {}",
        output.status
    );
    assert!(stdout.contains(
        "GPU_READBACK_OK: 2 elements; invalid enum rejected; validation=0; teardown complete"
    ));
    std::fs::remove_dir_all(temporary).unwrap();
}
