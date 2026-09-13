//! Compile harness for the public render-graph API — phase-2 step P2.1 of
//! `llm_notes/render-graph/08_plain_data_graph/02_schemas_and_compile_checks.md`.
//!
//! The harness generates the fixture's shader bindings, checks every case
//! against them, and deletes the generated output again. This is the shape the
//! cli's own codegen tests use (`alignment_tests` in
//! `crates/cli/src/build_tasks.rs`), so nothing generated is committed and no
//! justfile recipe maintains it.
//!
//! Each case in `fixtures/api_compile/cases` is one bin target of the fixture
//! crate `crates/renderer/fixtures/api_compile`. The harness checks every bin
//! independently with `cargo check` against the real `mltrs-renderer` crate.
//! Positive cases must compile; each negative case must fail at its intended
//! operation, proved by the expected error code, type names and case file name
//! in [`CASES`]. The harness rejects a case that fails for any other reason: an
//! unresolved import or a broken dependency is not API-safety evidence.
//!
//! The case types come from the generated modules, so they cannot drift from
//! what `crates/cli/templates/` emits. The cli stub fixture
//! (`crates/cli/fixtures/check_crate`) compiles the same generated code against
//! stub renderer types; this crate compiles it against the real renderer.
//!
//! No fixture allocates a GPU. Every case type-checks functions with renderer
//! handles as parameters; no case constructs a `Renderer`, so `cargo check`
//! is the whole job.
//!
//! The fixture crate sits outside the workspace (root `Cargo.toml` `exclude`)
//! because its negative bins never compile and its `src/generated` exists only
//! while this test runs. `CARGO_TARGET_DIR` points at the workspace `target/`,
//! so the checks reuse the workspace build of `mltrs-renderer`.
//!
//! The harness runs `cargo`, which takes the build-directory lock. A cargo
//! build running at the same time (bacon, a second shell) makes the harness
//! wait for that build to finish.

#![cfg(not(windows))]

use std::path::{Path, PathBuf};
use std::process::Command;

use mltrs_cli::build_tasks::{
    Config, OptimizationLevel, VENDORED_MODULES, write_precompiled_shaders,
};

/// What `cargo check --bin <case>` must produce.
enum Expectation {
    /// A positive control: the case must compile.
    Compiles,
    /// A negative case: the check must fail with this error code, and every
    /// snippet must appear in the diagnostics of the intended operation.
    Fails {
        code: &'static str,
        snippets: &'static [&'static str],
    },
}

struct Case {
    bin: &'static str,
    /// The case source file name. A negative case must name it in the
    /// diagnostics, which pins the failure to the case rather than to a
    /// generated module or the renderer.
    file: &'static str,
    expectation: Expectation,
}

/// Error codes that mean a case failed on its imports or dependencies rather
/// than on the API restriction under test.
const UNRELATED_FAILURES: [&str; 3] = ["error[E0432]", "error[E0433]", "error[E0463]"];

const CASES: &[Case] = &[
    Case {
        bin: "positive_param_bindings",
        file: "param_bindings.rs",
        expectation: Expectation::Compiles,
    },
    Case {
        bin: "negative_missing_param_bindings",
        file: "missing_param_bindings.rs",
        expectation: Expectation::Fails {
            code: "E0277",
            snippets: &["PendingParamBindings", "GraphNode"],
        },
    },
    Case {
        bin: "negative_push_without_param_bindings",
        file: "push_without_param_bindings.rs",
        expectation: Expectation::Fails {
            code: "E0277",
            snippets: &["PendingParamBindings", "GraphNode"],
        },
    },
    Case {
        bin: "negative_wrong_param_bindings",
        file: "wrong_param_bindings.rs",
        expectation: Expectation::Fails {
            code: "E0308",
            snippets: &["ScaleParamsBindings", "TexParamsBindings"],
        },
    },
    Case {
        bin: "negative_duplicate_param_bindings",
        file: "duplicate_param_bindings.rs",
        expectation: Expectation::Fails {
            code: "E0599",
            snippets: &["with_param_bindings"],
        },
    },
    // The complete frame tuple reaches `execute` unchanged.
    Case {
        bin: "positive_complete_tuple",
        file: "complete_tuple.rs",
        expectation: Expectation::Compiles,
    },
    Case {
        bin: "negative_omitted_tuple_element",
        file: "omitted_tuple_element.rs",
        // `RenderParamsData` appears only in the expected frame type, so the
        // failure is the omitted draw element.
        expectation: Expectation::Fails {
            code: "E0308",
            snippets: &["RenderParamsData"],
        },
    },
    // Binding kinds match the generated field types.
    Case {
        bin: "positive_right_binding_kind",
        file: "right_binding_kind.rs",
        expectation: Expectation::Compiles,
    },
    Case {
        bin: "negative_wrong_binding_kind",
        file: "wrong_binding_kind.rs",
        expectation: Expectation::Fails {
            code: "E0308",
            snippets: &["SampledTexBinding", "StorageTexBinding"],
        },
    },
    // Buffer bindings carry the generated element type.
    Case {
        bin: "positive_right_buffer_element_type",
        file: "right_buffer_element_type.rs",
        expectation: Expectation::Compiles,
    },
    Case {
        bin: "negative_wrong_buffer_element_type",
        file: "wrong_buffer_element_type.rs",
        expectation: Expectation::Fails {
            code: "E0308",
            snippets: &["BufferBinding", "OtherElement"],
        },
    },
    // A repeat node's frame is `(LoopCount, BodyFrame)`.
    Case {
        bin: "positive_repeat_frame",
        file: "repeat_frame.rs",
        expectation: Expectation::Compiles,
    },
    Case {
        bin: "negative_repeat_frame_without_loop_count",
        file: "repeat_frame_without_loop_count.rs",
        expectation: Expectation::Fails {
            code: "E0308",
            snippets: &["LoopCount"],
        },
    },
    // An optional node's frame is `Option<BodyFrame>`.
    Case {
        bin: "positive_optional_frame",
        file: "optional_frame.rs",
        expectation: Expectation::Compiles,
    },
    Case {
        bin: "negative_optional_frame_without_option",
        file: "optional_frame_without_option.rs",
        expectation: Expectation::Fails {
            code: "E0308",
            snippets: &["Option<"],
        },
    },
    // Twelve-element and nested tuples compile.
    Case {
        bin: "positive_tuple_twelve_elements",
        file: "tuple_twelve_elements.rs",
        expectation: Expectation::Compiles,
    },
    Case {
        bin: "positive_nested_tuples",
        file: "nested_tuples.rs",
        expectation: Expectation::Compiles,
    },
    // Every constructor family is reachable through the graph-owned
    // vocabulary: keys/slots minted from renderer handles, push variants,
    // picking, upload, and GPU-free logical construction.
    Case {
        bin: "positive_construction_families",
        file: "construction_families.rs",
        expectation: Expectation::Compiles,
    },
    // The logical/prepared lifecycle: GPU-free construction, consuming
    // preparation, execute only on the prepared type.
    Case {
        bin: "positive_prepared_lifecycle",
        file: "prepared_lifecycle.rs",
        expectation: Expectation::Compiles,
    },
    // Graph traits reach renderer traits one-way through the blankets;
    // renderer-only direct impls stay usable on renderer paths.
    Case {
        bin: "positive_trait_bridge",
        file: "trait_bridge.rs",
        expectation: Expectation::Compiles,
    },
    // Pipeline families are distinct key types, so a handle or key of one
    // family has no `Into` conversion to another family's key.
    Case {
        bin: "negative_wrong_pipeline_kind",
        file: "wrong_pipeline_kind.rs",
        expectation: Expectation::Fails {
            code: "E0277",
            snippets: &["DrawVertexCountKey", "DrawIndexedKey"],
        },
    },
    // The push interface is part of the pipeline's type: a no-push command
    // has no push completion method.
    Case {
        bin: "negative_wrong_push_block",
        file: "wrong_push_block.rs",
        expectation: Expectation::Fails {
            code: "E0599",
            snippets: &["with_push_constant", "ComputeNode"],
        },
    },
    // A push pipeline accepts only its own push block, not any push type.
    Case {
        bin: "negative_wrong_push_block_type",
        file: "wrong_push_block_type.rs",
        expectation: Expectation::Fails {
            code: "E0308",
            snippets: &["ScalePushInput", "OtherPushInput"],
        },
    },
    // A pending-push command cannot enter a graph.
    Case {
        bin: "negative_missing_push",
        file: "missing_push.rs",
        expectation: Expectation::Fails {
            code: "E0277",
            snippets: &["PendingPush", "GraphPush"],
        },
    },
    // Indirect arguments carry the command element type.
    Case {
        bin: "negative_wrong_indirect_element",
        file: "wrong_indirect_element.rs",
        expectation: Expectation::Fails {
            code: "E0277",
            snippets: &["ImmutableSlot", "DrawIndexedIndirectCommand"],
        },
    },
    // Only the prepared type executes.
    Case {
        bin: "negative_logical_no_execute",
        file: "logical_no_execute.rs",
        expectation: Expectation::Fails {
            code: "E0599",
            snippets: &["execute", "RenderGraph"],
        },
    },
    // Preparation consumes the logical graph.
    Case {
        bin: "negative_consumed_after_prepare",
        file: "consumed_after_prepare.rs",
        expectation: Expectation::Fails {
            code: "E0382",
            snippets: &["prepare", "moved"],
        },
    },
    // Backend traits cannot be named or implemented outside the renderer crate.
    Case {
        bin: "negative_private_backend_traits",
        file: "private_backend_traits.rs",
        expectation: Expectation::Fails {
            code: "E0603",
            snippets: &[
                "trait `GPUWrite` is private",
                "trait `PushConstantBlock` is private",
                "BackendGPUWrite",
                "BackendPushConstantBlock",
                "RootGPUWrite",
                "RootPushConstantBlock",
            ],
        },
    },
    // The graph push marker requires the graph GPUWrite supertrait.
    Case {
        bin: "negative_push_requires_graph_gpu_write",
        file: "push_requires_graph_gpu_write.rs",
        expectation: Expectation::Fails {
            code: "E0277",
            snippets: &["PushConstantBlock", "GPUWrite"],
        },
    },
];

/// Write the vendored engine slang module the fixture's shaders import, then
/// generate the fixture's bindings against `mltrs_renderer` rather than the
/// default `mltrs`, so the cases reach the renderer API directly.
fn generate(fixture: &Path) {
    let source_dir = fixture.join("shaders/source");
    std::fs::create_dir_all(&source_dir).unwrap();
    for (file_name, content) in VENDORED_MODULES {
        std::fs::write(source_dir.join(file_name), content).unwrap();
    }

    write_precompiled_shaders(Config {
        generate_rust_source: true,
        rust_source_dir: fixture.join("src"),
        shaders_source_dir: source_dir,
        compiled_shaders_dir: fixture.join("shaders/compiled"),
        import_root: "mltrs_renderer".to_string(),
        optimization: OptimizationLevel::High,
    })
    .expect("the fixture's shaders must compile and generate bindings");
}

/// Remove everything [`generate`] wrote. The committed tree holds the slang
/// sources and the cases, nothing built from them.
fn clean(fixture: &Path) {
    for (file_name, _) in VENDORED_MODULES {
        let _ = std::fs::remove_file(fixture.join("shaders/source").join(file_name));
    }
    let _ = std::fs::remove_dir_all(fixture.join("src/generated"));
    let _ = std::fs::remove_file(fixture.join("src/generated.rs"));
    let _ = std::fs::remove_dir_all(fixture.join("shaders/compiled"));
}

/// Check one case. Returns the failure report, or `None` when the case met its
/// expectation.
fn check(case: &Case, manifest: &Path, target_dir: &Path) -> Option<String> {
    let output = Command::new(env!("CARGO"))
        .args(["check", "--locked", "--color=never", "--manifest-path"])
        .arg(manifest)
        .arg("--bin")
        .arg(case.bin)
        .env("CARGO_TARGET_DIR", target_dir)
        .output()
        .expect("cargo must be available to run the compile harness");

    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let report = |reason: String| Some(format!("{}: {reason}\n{diagnostics}", case.bin));

    match case.expectation {
        Expectation::Compiles => {
            if output.status.success() {
                return None;
            }
            report("positive control must compile against mltrs-renderer".to_string())
        }
        Expectation::Fails { code, snippets } => {
            if output.status.success() {
                return report("negative case must not compile".to_string());
            }
            if UNRELATED_FAILURES
                .iter()
                .any(|unrelated| diagnostics.contains(unrelated))
            {
                return report(
                    "negative case failed on an import or dependency, \
                     not on the intended operation"
                        .to_string(),
                );
            }
            if !diagnostics.contains(case.file) {
                return report(format!("negative case failed outside `{}`", case.file));
            }
            let expected_code = format!("error[{code}]");
            if !diagnostics.contains(&expected_code) {
                return report(format!(
                    "negative case failed, but not with `{expected_code}`"
                ));
            }
            let missing = snippets
                .iter()
                .find(|snippet| !diagnostics.contains(*snippet));
            match missing {
                Some(snippet) => report(format!(
                    "negative case failed, but the diagnostics do not contain `{snippet}`"
                )),
                None => None,
            }
        }
    }
}

#[test]
fn render_graph_api_compile_checks() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/api_compile");
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| fixture.join("../../../../target"), PathBuf::from);

    clean(&fixture);
    generate(&fixture);

    let manifest = fixture.join("Cargo.toml");
    let failures: Vec<String> = CASES
        .iter()
        .filter_map(|case| check(case, &manifest, &target_dir))
        .collect();

    // clean up before asserting, so a failure leaves no generated files behind
    clean(&fixture);

    assert!(
        failures.is_empty(),
        "{} of {} render-graph API compile cases failed:\n\n{}",
        failures.len(),
        CASES.len(),
        failures.join("\n\n"),
    );
}
