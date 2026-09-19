use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run(command: &mut Command, label: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{label}: {error}"));
    if !output.status.success() {
        panic!(
            "{label} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    output
}

fn bytes_literal(path: &Path) -> String {
    fs::read(path)
        .unwrap()
        .into_iter()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn copy_fixture_from(group: &str, name: &str, destination: &Path) {
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(group)
            .join(name),
        destination.join(name),
    )
    .unwrap();
}

fn write_app_main(generated: &Path) {
    let platform = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../roc-platform/platform/main.roc")
        .canonicalize()
        .unwrap();
    let example = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../roc-platform/examples/basic-triangle/main.roc"),
    )
    .unwrap();
    let body = example.split_once('\n').unwrap().1;
    fs::write(
        generated.join("main.roc"),
        format!(
            "app [game] {{ pf: platform \"{}\" }}\n{body}",
            platform.display()
        ),
    )
    .unwrap();
}

fn copy_fixture(name: &str, destination: &Path) {
    copy_fixture_from("shaders", name, destination);
}

fn roc_test(roc: &std::ffi::OsStr, generated: &Path, consumer: &Path) -> Output {
    Command::new(roc)
        .arg("test")
        .arg(format!("--main={}", generated.join("main.roc").display()))
        .arg(consumer)
        .current_dir(std::env::temp_dir())
        .output()
        .expect("failed to execute roc test")
}

#[test]
#[ignore = "explicit real-Roc gate; run with just roc-codegen-test"]
fn real_roc_codegen_gate() {
    let roc = std::ffi::OsStr::new("roc");
    let version = run(Command::new(roc).arg("version"), "roc version");
    println!("compiler executable: selected `roc` on PATH");
    println!(
        "compiler version: {}",
        String::from_utf8_lossy(&version.stdout).trim()
    );

    let project = std::env::temp_dir().join(format!("mltrs-roc-gate-{}", uuid::Uuid::new_v4()));
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
        copy_fixture(name, &source);
    }

    copy_fixture_from("roc_codegen", "roc_shared.shader.slang", &source);
    run(
        Command::new(env!("CARGO_BIN_EXE_mltrs"))
            .args(["shaders", "compile", "--crate-dir"])
            .arg(&project)
            .args(["--compiled-dir"])
            .arg(&compiled)
            .current_dir(std::env::temp_dir()),
        "fresh Roc generation",
    );
    for name in ["SyntheticReflection.roc", "SyntheticConsumer.roc"] {
        copy_fixture_from("roc_codegen", name, &generated);
    }
    write_app_main(&generated);
    run(
        Command::new(roc).args(["fmt", "--check"]).arg(&generated),
        "roc fmt --check generated tree",
    );
    for module in [
        "ShaderTypes.roc",
        "BasicTriangle.roc",
        "Particles.roc",
        "RocShared.roc",
        "ShaderAtlas.roc",
        "SyntheticReflection.roc",
    ] {
        run(
            Command::new(roc)
                .arg("check")
                .arg(format!("--main={}", generated.join("main.roc").display()))
                .arg(generated.join(module))
                .current_dir(std::env::temp_dir()),
            &format!("roc check {module}"),
        );
    }

    let vertex = bytes_literal(&compiled.join("basic_triangle.vert.spv"));
    let fragment = bytes_literal(&compiled.join("basic_triangle.frag.spv"));
    let compute = bytes_literal(&compiled.join("particles.comp.spv"));
    let particle_vertex = bytes_literal(&compiled.join("roc_shared.vert.spv"));
    let particle_fragment = bytes_literal(&compiled.join("roc_shared.frag.spv"));
    let consumer = generated.join("Consumer.roc");
    let consumer_source = format!(
        "import pf.ShaderReflection\nimport ShaderAtlas\nimport ShaderTypes\n\nConsumer := {{}}\n\nvertex : ShaderTypes.Vertex\nvertex = {{ position: {{ x: 1.0, y: 2.0, z: 3.0 }}, color: {{ x: 0.25, y: 0.5, z: 0.75 }} }}\nparticle : ShaderTypes.Particle\nparticle = {{ position: {{ x: 4.0, y: 5.0 }}, velocity: {{ x: 6.0, y: 7.0 }}, color: {{ x: 0.1, y: 0.2, z: 0.3, w: 0.4 }} }}\naddress : ShaderReflection.PointerAddress\naddress = PointerAddress(18446744073709551615)\nrow = {{ x: 1.0, y: 2.0, z: 3.0, w: 4.0 }}\nmatrices : ShaderTypes.MvpMatrices\nmatrices = {{ model: {{ row_0: row, row_1: row, row_2: row, row_3: row }}, view: {{ row_0: row, row_1: row, row_2: row, row_3: row }}, proj: {{ row_0: row, row_1: row, row_2: row, row_3: row }} }}\nparams : ShaderTypes.SimParams\nparams = {{ particles_in: address, particles_out: PointerAddress(0), delta_time: 0.016 }}\n\nexpect ShaderAtlas.basic_triangle.stages.vertex == [{vertex}]\nexpect ShaderAtlas.basic_triangle.stages.fragment == [{fragment}]\nexpect ShaderAtlas.particles.stages.compute == [{compute}]\nexpect ShaderAtlas.roc_shared.stages.vertex == [{particle_vertex}]\nexpect ShaderAtlas.roc_shared.stages.fragment == [{particle_fragment}]\nexpect ShaderAtlas.basic_triangle.reflection.source_file_name == \"basic_triangle.shader.slang\"\nexpect ShaderAtlas.basic_triangle.reflection.vertex_entry_point.entry_point_name == \"vertexMain\"\nexpect ShaderAtlas.basic_triangle.reflection.fragment_entry_point.entry_point_name == \"fragmentMain\"\nexpect ShaderAtlas.particles.reflection.compute_entry_point.entry_point_name == \"computeMain\"\nexpect ShaderAtlas.particles.reflection.workgroup_size == {{ x: 256, y: 1, z: 1 }}\nexpect vertex.position.z == 3.0\nexpect particle.velocity.y == 7.0\nexpect matrices.model.row_2.w == 4.0\nexpect match params.particles_in {{ PointerAddress(raw) => raw == 18446744073709551615 }}\n"
    );
    fs::write(&consumer, consumer_source).unwrap();
    run(
        Command::new(roc).arg("fmt").arg(&consumer),
        "format consumer",
    );
    run(
        &mut {
            let mut command = Command::new(roc);
            command
                .arg("fmt")
                .arg("--check")
                .arg(&consumer)
                .current_dir(std::env::temp_dir());
            command
        },
        "consumer format check",
    );
    let evaluated = roc_test(roc, &generated, &consumer);
    assert!(
        String::from_utf8_lossy(&evaluated.stdout).contains("All (14) tests passed"),
        "{}",
        String::from_utf8_lossy(&evaluated.stdout)
    );
    let synthetic = roc_test(roc, &generated, &generated.join("SyntheticConsumer.roc"));
    assert!(
        synthetic.status.success()
            && String::from_utf8_lossy(&synthetic.stdout).contains("All (22) tests passed"),
        "{}{}",
        String::from_utf8_lossy(&synthetic.stdout),
        String::from_utf8_lossy(&synthetic.stderr)
    );

    let missing = compiled.join("basic_triangle.vert.spv");
    let saved = fs::read(&missing).unwrap();
    fs::remove_file(&missing).unwrap();
    let missing_output = roc_test(roc, &generated, &consumer);
    assert!(!missing_output.status.success());
    let missing_error = format!(
        "{}{}",
        String::from_utf8_lossy(&missing_output.stdout),
        String::from_utf8_lossy(&missing_output.stderr)
    );
    assert!(
        missing_error.contains("file not found")
            && missing_error.contains("basic_triangle.vert.spv"),
        "{missing_error}"
    );
    fs::write(&missing, saved).unwrap();

    let false_consumer = generated.join("FalseBytes.roc");
    let wrong_first = fs::read(&missing).unwrap()[0].wrapping_add(1);
    fs::write(
        &false_consumer,
        format!(
            "import ShaderAtlas\n\nFalseBytes := {{}}\n\nexpect List.first(ShaderAtlas.basic_triangle.stages.vertex) == Ok({wrong_first})\n"
        ),
    )
    .unwrap();
    run(
        Command::new(roc)
            .arg("check")
            .arg(format!("--main={}", generated.join("main.roc").display()))
            .arg(&false_consumer)
            .current_dir(std::env::temp_dir()),
        "false-byte consumer typecheck",
    );
    let false_output = roc_test(roc, &generated, &false_consumer);
    assert!(!false_output.status.success());
    let false_text = format!(
        "{}{}",
        String::from_utf8_lossy(&false_output.stdout),
        String::from_utf8_lossy(&false_output.stderr)
    );
    assert!(
        false_text.contains("Ran 1 tests")
            && false_text.contains("1 failed")
            && false_text.contains("0 compiler errors"),
        "{false_text}"
    );

    let logical_project =
        std::env::temp_dir().join(format!("mltrs-roc-logical-{}", uuid::Uuid::new_v4()));
    let logical_source = logical_project.join("shaders/source");
    let logical_generated = logical_project.join("Generated");
    fs::create_dir_all(&logical_source).unwrap();
    fs::write(logical_project.join("main.roc"), "package [] {}\n").unwrap();
    copy_fixture("mltrs.slang", &logical_source);
    for name in [
        "handle_mixed.shader.slang",
        "std140_arrays.shader.slang",
        "std140_enums.shader.slang",
        "nested_pointer.shader.slang",
    ] {
        copy_fixture_from("alignment", name, &logical_source);
    }
    run(
        Command::new(env!("CARGO_BIN_EXE_mltrs"))
            .args(["shaders", "compile", "--crate-dir"])
            .arg(&logical_project)
            .current_dir(std::env::temp_dir()),
        "logical fixture generation",
    );
    run(
        Command::new(roc)
            .args(["fmt", "--check"])
            .arg(&logical_generated),
        "generated logical fixture format check",
    );
    copy_fixture_from("roc_codegen", "LogicalConsumer.roc", &logical_generated);
    write_app_main(&logical_generated);
    run(
        Command::new(roc)
            .args(["fmt", "--check"])
            .arg(logical_generated.join("LogicalConsumer.roc")),
        "logical consumer format check",
    );
    let logical = roc_test(
        roc,
        &logical_generated,
        &logical_generated.join("LogicalConsumer.roc"),
    );
    assert!(
        logical.status.success()
            && String::from_utf8_lossy(&logical.stdout).contains("All (8) tests passed"),
        "{}{}",
        String::from_utf8_lossy(&logical.stdout),
        String::from_utf8_lossy(&logical.stderr)
    );

    fs::remove_dir_all(logical_project).unwrap();
    fs::remove_dir_all(project).unwrap();
}
