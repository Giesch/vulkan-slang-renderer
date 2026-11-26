use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp_project(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("mltrs-{label}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn mltrs_from(cwd: &Path, project: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mltrs"))
        .args(["shaders", "compile", "--crate-dir"])
        .arg(project)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to invoke mltrs")
}

fn mltrs(project: &Path, args: &[&str]) -> Output {
    mltrs_from(project, project, args)
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_no_outputs(project: &Path) {
    assert!(!project.join("Generated").exists());
    assert!(!project.join("src").exists());
    assert!(!project.join("shaders/compiled").exists());
}

fn copy_shader_fixture(name: &str, destination: &Path) {
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/shaders")
            .join(name),
        destination.join(name),
    )
    .unwrap();
}

fn file_manifest(directory: &Path) -> Vec<String> {
    let mut names = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn file_contents(directory: &Path) -> Vec<(String, Vec<u8>)> {
    file_manifest(directory)
        .into_iter()
        .map(|name| {
            let bytes = fs::read(directory.join(&name)).unwrap();
            (name, bytes)
        })
        .collect()
}

#[test]
fn cli_language_marker_and_explicit_option_matrix() {
    for (label, markers, expected) in [
        ("neither", &[][..], "neither main.roc nor Cargo.toml"),
        (
            "both",
            &["main.roc", "Cargo.toml"][..],
            "both main.roc and Cargo.toml",
        ),
    ] {
        let project = temp_project(label);
        for marker in markers {
            fs::write(project.join(marker), "").unwrap();
        }
        let output = mltrs(&project, &[]);
        assert!(!output.status.success());
        let error = stderr(&output);
        assert!(error.contains(expected), "{error}");
        assert!(error.contains("--language rust|roc"), "{error}");
        assert_no_outputs(&project);
        fs::remove_dir_all(project).unwrap();
    }

    for (label, marker, args, option) in [
        (
            "roc-rust-dir",
            "main.roc",
            &["--language", "roc", "--rust-dir", "elsewhere"][..],
            "--rust-dir",
        ),
        (
            "roc-import-root",
            "main.roc",
            &["--language", "roc", "--import-root", "mltrs"][..],
            "--import-root",
        ),
        (
            "rust-roc-dir",
            "Cargo.toml",
            &["--language", "rust", "--roc-dir", "elsewhere"][..],
            "--roc-dir",
        ),
    ] {
        let project = temp_project(label);
        fs::write(project.join(marker), "").unwrap();
        fs::create_dir_all(project.join("Generated")).unwrap();
        fs::write(project.join("Generated/sentinel"), "keep").unwrap();
        let output = mltrs(&project, args);
        assert!(!output.status.success());
        let error = stderr(&output);
        assert!(error.contains(option), "{error}");
        assert_eq!(
            fs::read_to_string(project.join("Generated/sentinel")).unwrap(),
            "keep"
        );
        assert!(!project.join("shaders/compiled").exists());
        fs::remove_dir_all(project).unwrap();
    }

    // Explicit selection bypasses marker ambiguity, so these reach the source
    // directory check instead of reporting a marker error.
    for language in ["rust", "roc"] {
        let project = temp_project(&format!("explicit-{language}"));
        fs::write(project.join("main.roc"), "").unwrap();
        fs::write(project.join("Cargo.toml"), "").unwrap();
        let output = mltrs(&project, &["--language", language]);
        assert!(!output.status.success());
        let error = stderr(&output);
        assert!(!error.contains("both main.roc and Cargo.toml"), "{error}");
        assert_no_outputs(&project);
        fs::remove_dir_all(project).unwrap();
    }
}

#[test]
fn cli_marker_locality_explicit_success_and_relative_overrides() {
    // Directory markers and ancestor/descendant regular files do not select a language.
    for label in ["directory", "ancestor", "descendant"] {
        let root = temp_project(&format!("marker-{label}"));
        let project = if label == "ancestor" {
            fs::write(root.join("main.roc"), "").unwrap();
            fs::create_dir(root.join("project")).unwrap();
            root.join("project")
        } else {
            root.clone()
        };
        match label {
            "directory" => {
                fs::create_dir(project.join("main.roc")).unwrap();
                fs::create_dir(project.join("Cargo.toml")).unwrap();
            }
            "ancestor" => {}
            "descendant" => {
                fs::create_dir(project.join("nested")).unwrap();
                fs::write(project.join("nested/main.roc"), "").unwrap();
            }
            _ => unreachable!(),
        }
        let output = mltrs(&project, &[]);
        assert!(!output.status.success());
        assert!(stderr(&output).contains("neither main.roc nor Cargo.toml"));
        assert_no_outputs(&project);
        fs::remove_dir_all(root).unwrap();
    }

    // Explicit Roc selection succeeds without either marker and all relative
    // overrides stay process-CWD-relative rather than project-relative.
    let cwd = temp_project("override-cwd");
    let project = cwd.join("project");
    let source = cwd.join("relative source");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&source).unwrap();
    copy_shader_fixture("basic_triangle.shader.slang", &source);
    copy_shader_fixture("mltrs.slang", &source);
    let output = mltrs_from(
        &cwd,
        Path::new("project"),
        &[
            "--language",
            "roc",
            "--source-dir",
            "relative source",
            "--roc-dir",
            "relative roc",
            "--compiled-dir",
            "relative compiled",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(cwd.join("relative roc/ShaderAtlas.roc").is_file());
    assert!(
        cwd.join("relative compiled/basic_triangle.vert.spv")
            .is_file()
    );
    assert!(!project.join("Generated").exists());
    fs::remove_dir_all(cwd).unwrap();

    // Automatic Cargo.toml selection retains the existing Rust + JSON + SPIR-V contract.
    let rust_project = temp_project("auto-rust");
    fs::write(
        rust_project.join("Cargo.toml"),
        "[package]\nname='fixture'\n",
    )
    .unwrap();
    let rust_source = rust_project.join("shaders/source");
    fs::create_dir_all(&rust_source).unwrap();
    copy_shader_fixture("basic_triangle.shader.slang", &rust_source);
    copy_shader_fixture("mltrs.slang", &rust_source);
    let output = mltrs(&rust_project, &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(rust_project.join("src/generated.rs").is_file());
    assert!(rust_project.join("src/generated/shader_atlas.rs").is_file());
    assert!(
        rust_project
            .join("shaders/compiled/basic_triangle.json")
            .is_file()
    );
    assert!(
        rust_project
            .join("shaders/compiled/basic_triangle.vert.spv")
            .is_file()
    );
    assert!(!rust_project.join("Generated/ShaderAtlas.roc").exists());
    fs::remove_dir_all(rust_project).unwrap();
}

#[test]
fn roc_artifact_manifest_and_stale_regeneration() {
    let project = temp_project("roc-artifacts");
    let source = project.join("shaders/source");
    let compiled = project.join("compiled with space");
    let generated = project.join("Generated");
    fs::create_dir_all(&source).unwrap();
    fs::write(project.join("main.roc"), "package [] {}\n").unwrap();
    for name in [
        "basic_triangle.shader.slang",
        "particles.compute.slang",
        "particle.slang",
        "mltrs.slang",
    ] {
        copy_shader_fixture(name, &source);
    }

    let output = mltrs(&project, &["--compiled-dir", compiled.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        file_manifest(&generated),
        [
            "BasicTriangle.roc",
            "Mltrs.roc",
            "Particle.roc",
            "Particles.roc",
            "ShaderAtlas.roc",
        ]
    );
    assert_eq!(
        file_manifest(&compiled),
        [
            "basic_triangle.frag.spv",
            "basic_triangle.json",
            "basic_triangle.vert.spv",
            "particles.comp.json",
            "particles.comp.spv",
        ]
    );
    assert!(!project.join("src").exists());

    fs::write(generated.join("stale.roc"), "stale").unwrap();
    fs::write(generated.join("ShaderTypes.roc"), "old aggregate").unwrap();
    fs::write(compiled.join("stale.json"), "stale").unwrap();
    let output = mltrs(&project, &["--compiled-dir", compiled.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!generated.join("stale.roc").exists());
    assert!(!generated.join("ShaderTypes.roc").exists());
    assert!(!compiled.join("stale.json").exists());

    for name in [
        "BasicTriangle.roc",
        "Particles.roc",
        "ShaderAtlas.roc",
        "Mltrs.roc",
        "Particle.roc",
    ] {
        let text = fs::read_to_string(generated.join(name)).unwrap();
        // Snapshots ignore trailing whitespace; `roc fmt --check` does not.
        assert!(text.ends_with("}\n"), "{name} lacks a final newline");
        insta::assert_snapshot!(format!("roc_generated_{name}"), text);
    }

    let second = temp_project("roc-determinism");
    let second_source = second.join("shaders/source");
    fs::create_dir_all(&second_source).unwrap();
    fs::write(second.join("main.roc"), "package [] {}\n").unwrap();
    for name in [
        "mltrs.slang",
        "particle.slang",
        "particles.compute.slang",
        "basic_triangle.shader.slang",
    ] {
        copy_shader_fixture(name, &second_source);
    }
    let second_output = mltrs(
        &second,
        &[
            "--compiled-dir",
            second.join("compiled with space").to_str().unwrap(),
        ],
    );
    assert!(second_output.status.success(), "{}", stderr(&second_output));
    assert_eq!(
        file_contents(&generated),
        file_contents(&second.join("Generated"))
    );

    fs::remove_dir_all(second).unwrap();
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn cli_help_documents_language_and_output_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_mltrs"))
        .args(["shaders", "compile", "--help"])
        .output()
        .expect("failed to invoke mltrs help");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for text in [
        "--language <LANGUAGE>",
        "rust",
        "roc",
        "--roc-dir <ROC_DIR>",
        "<crate-dir>/Generated",
        "--compiled-dir <COMPILED_DIR>",
    ] {
        assert!(help.contains(text), "missing {text:?} in:\n{help}");
    }
}
