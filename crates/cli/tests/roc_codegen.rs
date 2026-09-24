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
    // The gate's app draws an empty graph: it depends on no generated shader,
    // so every fixture tree can host it.
    let body = "import pf.Game\nimport pf.RenderGraph\nimport pf.Graphs\n\ngame : Game\ngame = Game.new({ init!, graphs, draw })\n\ngraphs = Graphs.or_crash(Graphs.single(RenderGraph.empty))\n\ninit! : {} => Game.Init\ninit! = |_| { window_title: \"codegen gate\" }\n\ndraw = |_frame| graphs.draw({})\n";
    fs::write(
        generated.join("main.roc"),
        format!(
            "app [game] {{ pf: platform \"{}\" }}\n\n{body}",
            platform.display()
        ),
    )
    .unwrap();
}

fn copy_fixture(name: &str, destination: &Path) {
    copy_fixture_from("shaders", name, destination);
}

fn all_tests_passed(output: &Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);

    output.status.success() && stdout.contains("tests passed") && !stdout.contains("failed")
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
    for name in [
        "ownership_leaf.slang",
        "ownership_shared.slang",
        "ownership.shader.slang",
    ] {
        copy_fixture_from("roc_codegen", name, &source);
    }
    let other_triangle = fs::read_to_string(source.join("basic_triangle.shader.slang"))
        .unwrap()
        .replace("module basic_triangle;", "module other_triangle;");
    fs::write(source.join("other_triangle.shader.slang"), other_triangle).unwrap();
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
        "Mltrs.roc",
        "Particle.roc",
        "BasicTriangle.roc",
        "OtherTriangle.roc",
        "Particles.roc",
        "RocShared.roc",
        "Ownership.roc",
        "OwnershipLeaf.roc",
        "OwnershipShared.roc",
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
        "import pf.ShaderReflection\nimport ShaderAtlas\nimport Mltrs\nimport Particle\nimport Particles\nimport BasicTriangle\n\nConsumer := {{}}\n\nvertex : BasicTriangle.Vertex\nvertex = {{ position: {{ x: 1.0, y: 2.0, z: 3.0 }}, color: {{ x: 0.25, y: 0.5, z: 0.75 }} }}\nparticle : Particle.Particle\nparticle = {{ position: {{ x: 4.0, y: 5.0 }}, velocity: {{ x: 6.0, y: 7.0 }}, color: {{ x: 0.1, y: 0.2, z: 0.3, w: 0.4 }} }}\naddress : ShaderReflection.PointerAddress\naddress = ShaderReflection.PointerAddress.(18446744073709551615)\nrow = {{ x: 1.0, y: 2.0, z: 3.0, w: 4.0 }}\nmatrices : Mltrs.MvpMatrices\nmatrices = {{ model: {{ row_0: row, row_1: row, row_2: row, row_3: row }}, view: {{ row_0: row, row_1: row, row_2: row, row_3: row }}, proj: {{ row_0: row, row_1: row, row_2: row, row_3: row }} }}\nparams : Particles.SimParams\nparams = {{ particles_in: address, particles_out: ShaderReflection.PointerAddress.(0), delta_time: 0.016 }}\n\nexpect ShaderAtlas.basic_triangle.stages.vertex == [{vertex}]\nexpect ShaderAtlas.basic_triangle.stages.fragment == [{fragment}]\nexpect ShaderAtlas.particles.stages.compute == [{compute}]\nexpect ShaderAtlas.roc_shared.stages.vertex == [{particle_vertex}]\nexpect ShaderAtlas.roc_shared.stages.fragment == [{particle_fragment}]\nexpect ShaderAtlas.basic_triangle.reflection.source_file_name == \"basic_triangle.shader.slang\"\nexpect ShaderAtlas.basic_triangle.reflection.vertex_entry_point.entry_point_name == \"vertexMain\"\nexpect ShaderAtlas.basic_triangle.reflection.fragment_entry_point.entry_point_name == \"fragmentMain\"\nexpect ShaderAtlas.particles.reflection.compute_entry_point.entry_point_name == \"computeMain\"\nexpect ShaderAtlas.particles.reflection.workgroup_size == {{ x: 256, y: 1, z: 1 }}\nexpect vertex.position.z == 3.0\nexpect particle.velocity.y == 7.0\nexpect matrices.model.row_2.w == 4.0\nexpect params.particles_in == ShaderReflection.PointerAddress.(18446744073709551615)\nexpect ShaderReflection.f32_bytes(1.0) == [0, 0, 128, 63]\nexpect BasicTriangle.Vertex.to_bytes(vertex) == [0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 0, 0, 128, 62, 0, 0, 0, 63, 0, 0, 64, 63, 0, 0, 0, 0, 0, 0, 0, 0]\nexpect List.len(Mltrs.MvpMatrices.to_bytes(matrices)) == U32.to_u64(Mltrs.MvpMatrices.gpu_size)\nexpect BasicTriangle.shader.name == \"basic_triangle\"\nexpect Str.contains(BasicTriangle.shader.reflection_json, \"\\\"sourceFileName\\\": \\\"basic_triangle.shader.slang\\\"\")\n"
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
    // The platform's own modules carry expectations that `roc test` runs
    // alongside the consumer's, so the exact count is not asserted.
    let evaluated = roc_test(roc, &generated, &consumer);
    assert!(
        all_tests_passed(&evaluated),
        "{}{}",
        String::from_utf8_lossy(&evaluated.stdout),
        String::from_utf8_lossy(&evaluated.stderr)
    );
    let ownership = generated.join("OwnershipConsumer.roc");
    fs::write(
        &ownership,
        r#"import pf.RenderGraph
import Ownership
import OwnershipLeaf
import OwnershipShared

OwnershipConsumer := {}

leaf : OwnershipLeaf.LeafData
leaf = { color: { x: 1.0, y: 0.0, z: 0.0, w: 1.0 } }

shared : OwnershipShared.SharedPayload
shared = { data: leaf, mode: On }

local : Ownership.LocalParams
local = { payload: shared }

expect OwnershipLeaf.SharedMode.tag(shared.mode) == 7
expect OwnershipShared.SharedPayload.to_bytes(shared) == Ownership.LocalParams.to_bytes(local)
expect Ownership.LocalParams.to_bytes(local).len() == 32

pipeline = RenderGraph.vertex_count_pipeline({ name: "ownership", shader: Ownership.shader })
expect {
    pack = RenderGraph.draw_vertex_count(pipeline, 3).packer()
    pack(local) == [Ownership.LocalParams.to_bytes(local)]
}
uniform_pack = Ownership.params.to_bytes
expect uniform_pack(local) == Ownership.LocalParams.to_bytes(local)
"#,
    )
    .unwrap();
    let owned = roc_test(roc, &generated, &ownership);
    assert!(
        all_tests_passed(&owned),
        "{}{}",
        String::from_utf8_lossy(&owned.stdout),
        String::from_utf8_lossy(&owned.stderr)
    );

    // Same fields, different generated nominal types. First prove the consumer
    // and both shaders work, then change only the vertices at the constructor.
    let pairing = generated.join("VertexPairing.roc");
    let pairing_source = r#"import pf.RenderGraph
import pf.ShaderReflection
import BasicTriangle
import OtherTriangle

VertexPairing := {}

vertex : BasicTriangle.Vertex
vertex = { position: { x: 0.0, y: 0.0, z: 0.0 }, color: { x: 1.0, y: 0.0, z: 0.0 } }

other_vertex : OtherTriangle.Vertex
other_vertex = { position: { x: 0.0, y: 0.0, z: 0.0 }, color: { x: 1.0, y: 0.0, z: 0.0 } }

pipeline = RenderGraph.indexed_pipeline({
    name: "triangle",
    shader: BasicTriangle.shader,
    vertices: [vertex],
    indices: [0],
})

other_pipeline = RenderGraph.indexed_pipeline({
    name: "other triangle",
    shader: OtherTriangle.shader,
    vertices: [other_vertex],
    indices: [0],
})

expect pipeline.mesh.vertex_bytes.len() == 32
expect pipeline.shader.uniform.name == "matrices"
expect pipeline.shader.uniform.index == 0
expect pipeline.shader.uniform.size == 192
expect other_pipeline.mesh.vertex_bytes.len() == 32
"#;
    fs::write(&pairing, pairing_source).unwrap();
    let paired = roc_test(roc, &generated, &pairing);
    assert!(
        all_tests_passed(&paired),
        "{}{}",
        String::from_utf8_lossy(&paired.stdout),
        String::from_utf8_lossy(&paired.stderr)
    );
    fs::write(
        &pairing,
        pairing_source.replace("vertices: [vertex]", "vertices: [other_vertex]"),
    )
    .unwrap();
    let mismatched = Command::new(roc)
        .arg("check")
        .arg(format!("--main={}", generated.join("main.roc").display()))
        .arg(&pairing)
        .output()
        .unwrap();
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&mismatched.stdout),
        String::from_utf8_lossy(&mismatched.stderr)
    );
    assert!(!mismatched.status.success(), "wrong vertex type compiled");
    for expected in [
        "type mismatch",
        "VertexPairing.roc",
        "BasicTriangle.Vertex",
        "OtherTriangle.Vertex",
    ] {
        assert!(
            diagnostic.contains(expected),
            "missing {expected:?}: {diagnostic}"
        );
    }

    let synthetic = roc_test(roc, &generated, &generated.join("SyntheticConsumer.roc"));
    assert!(
        all_tests_passed(&synthetic),
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
        false_text.contains("1 failed") && false_text.contains("0 compiler errors"),
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
        all_tests_passed(&logical),
        "{}{}",
        String::from_utf8_lossy(&logical.stdout),
        String::from_utf8_lossy(&logical.stderr)
    );

    fs::remove_dir_all(logical_project).unwrap();
    fs::remove_dir_all(project).unwrap();
}
