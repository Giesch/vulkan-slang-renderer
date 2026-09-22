#!/usr/bin/env -S cargo +nightly -Zscript
---
[package]
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
---


//! Selects the pre-commit checks for a set of staged paths.
//!
//! `MLTRS_WORKSPACE_GRAPH` contains the workspace graph: a JSON array of
//! `{"name", "dir", "dependencies"}` objects, one per workspace package,
//! as produced by `just _workspace-graph` from `cargo metadata`. `dir` is the
//! package directory relative to the workspace root and `dependencies` lists
//! the in-workspace dependencies by package name.
//!
//! Stdin carries NUL-delimited staged paths. Stdout is the `just` argument
//! list for the required checks, one argument per line. The `pre-commit`
//! recipe in the root justfile passes it to `just` as is.
//!
//! The output always names at least one recipe. The `test-crates` recipe
//! takes a variadic parameter, so it and its package names come last.
//!
//! An empty path list (for example `git commit --amend` with nothing staged)
//! selects every package so the amended tree is still tested.
//!
//! Test this script with:
//! ```sh
//! cargo +nightly -Zscript test --manifest-path scripts/pre-commit-checks.rs
//! ```

use std::collections::BTreeSet;
use std::io::{self, Read};
use std::process::ExitCode;

use serde::Deserialize;

/// Recipe for a commit that needs no check.
const SKIP: &str = "_pre-commit-skip";

/// Recipes that run before every other check.
const ALWAYS: &[&str] = &["_pre-commit-shaders", "lint"];

const TWW_ASSETS: &str = "toon_link::verify-assets";

const ROC: &str = "_roc-codegen-test-if-available";

/// Variadic recipe; must be the last recipe named.
const TEST_CRATES: &str = "test-crates";

/// Packages whose own sources feed the Wind Waker asset gates.
const TWW_PACKAGES: &[&str] = &["toon_link", "convert-link", "gx"];

/// Paths that affect no check.
const IGNORED_SUFFIXES: &[&[u8]] = &[b".md", b".org"];
const IGNORED_FILES: &[&[u8]] = &[b".gitignore"];
const IGNORED_DIRECTORIES: &[&[u8]] = &[b".github/"];

/// Paths that affect every package.
const WORKSPACE_FILES: &[&[u8]] = &[
    b"Cargo.toml",
    b"Cargo.lock",
    b"rust-toolchain.toml",
    b"justfile",
];
const WORKSPACE_DIRECTORIES: &[&[u8]] = &[b"scripts/", b".cargo/"];

/// Directories that hold packages; an unknown package under them affects
/// every package.
const PACKAGE_DIRECTORIES: &[&[u8]] = &[b"crates/", b"examples/"];

fn main() -> io::Result<ExitCode> {
    let Some(graph) = std::env::var_os("MLTRS_WORKSPACE_GRAPH") else {
        eprintln!("pre-commit-checks.rs: MLTRS_WORKSPACE_GRAPH is required");
        return Ok(ExitCode::FAILURE);
    };
    let Ok(graph) = graph.into_string() else {
        eprintln!("pre-commit-checks.rs: MLTRS_WORKSPACE_GRAPH must be UTF-8 JSON");
        return Ok(ExitCode::FAILURE);
    };

    let workspace = match Workspace::parse(&graph) {
        Ok(workspace) => workspace,
        Err(error) => {
            eprintln!("pre-commit-checks.rs: {error}");
            return Ok(ExitCode::FAILURE);
        }
    };

    let mut paths = Vec::new();
    io::stdin().read_to_end(&mut paths)?;

    for argument in arguments(&workspace, &paths) {
        println!("{argument}");
    }

    Ok(ExitCode::SUCCESS)
}

#[derive(Deserialize)]
struct Package {
    name: String,
    dir: String,
    dependencies: Vec<String>,
}

struct Workspace {
    packages: Vec<Package>,
}

impl Workspace {
    fn parse(graph: &str) -> Result<Self, String> {
        let packages: Vec<Package> =
            serde_json::from_str(graph).map_err(|error| format!("bad workspace graph: {error}"))?;
        let workspace = Workspace { packages };

        for package in TWW_PACKAGES {
            if !workspace.contains(package) {
                return Err(format!("workspace graph has no package named {package}"));
            }
        }

        Ok(workspace)
    }

    fn contains(&self, name: &str) -> bool {
        self.packages.iter().any(|package| package.name == name)
    }

    fn names(&self) -> BTreeSet<String> {
        self.packages
            .iter()
            .map(|package| package.name.clone())
            .collect()
    }

    fn owner(&self, path: &[u8]) -> Option<&str> {
        self.packages
            .iter()
            .find(|package| {
                path.starts_with(package.dir.as_bytes())
                    && path.get(package.dir.len()) == Some(&b'/')
            })
            .map(|package| package.name.as_str())
    }

    fn dependents<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.packages
            .iter()
            .filter(move |package| {
                package
                    .dependencies
                    .iter()
                    .any(|dependency| dependency == name)
            })
            .map(|package| package.name.as_str())
    }

    fn with_dependents(&self, packages: &mut BTreeSet<String>) {
        let mut pending: Vec<String> = packages.iter().cloned().collect();
        while let Some(package) = pending.pop() {
            for dependent in self.dependents(&package) {
                if packages.insert(dependent.to_string()) {
                    pending.push(dependent.to_string());
                }
            }
        }
    }
}

enum Scope {
    None,
    Package(String),
    Workspace,
}

fn scope(workspace: &Workspace, path: &[u8]) -> Scope {
    let starts_with_any = |directories: &[&[u8]]| {
        directories
            .iter()
            .any(|directory| path.starts_with(directory))
    };

    if IGNORED_SUFFIXES.iter().any(|suffix| path.ends_with(suffix))
        || IGNORED_FILES.contains(&path)
        || starts_with_any(IGNORED_DIRECTORIES)
    {
        return Scope::None;
    }

    if WORKSPACE_FILES.contains(&path) || starts_with_any(WORKSPACE_DIRECTORIES) {
        return Scope::Workspace;
    }

    if let Some(package) = workspace.owner(path) {
        return Scope::Package(package.to_string());
    }

    if starts_with_any(PACKAGE_DIRECTORIES) {
        return Scope::Workspace;
    }

    Scope::None
}

fn arguments(workspace: &Workspace, paths: &[u8]) -> Vec<String> {
    let mut packages = BTreeSet::new();
    let mut tww_assets = false;
    let mut everything = false;
    let mut staged = 0;

    for path in paths
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        staged += 1;
        match scope(workspace, path) {
            Scope::None => {}
            Scope::Package(package) => {
                tww_assets |= TWW_PACKAGES.contains(&package.as_str());
                packages.insert(package);
            }
            Scope::Workspace => {
                everything = true;
                tww_assets = true;
            }
        }
    }

    if everything || staged == 0 {
        packages = workspace.names();
    } else {
        workspace.with_dependents(&mut packages);
    }

    if packages.is_empty() && !tww_assets {
        return vec![SKIP.to_string()];
    }

    let mut arguments: Vec<String> = ALWAYS.iter().map(|recipe| recipe.to_string()).collect();
    if tww_assets {
        arguments.push(TWW_ASSETS.to_string());
        arguments.push("--quiet".to_string());
    }
    arguments.push(ROC.to_string());
    if !packages.is_empty() {
        arguments.push(TEST_CRATES.to_string());
        arguments.extend(packages);
    }

    arguments
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAPH: &str = r#"[
        {"name": "mltrs-slang-reflection", "dir": "crates/slang-reflection", "dependencies": []},
        {"name": "mltrs-cli", "dir": "crates/cli", "dependencies": ["mltrs-slang-reflection"]},
        {"name": "mltrs-render-graph", "dir": "crates/render-graph", "dependencies": []},
        {"name": "mltrs-renderer", "dir": "crates/renderer",
         "dependencies": ["mltrs-render-graph", "mltrs-slang-reflection", "mltrs-cli"]},
        {"name": "mltrs", "dir": "crates/mltrs", "dependencies": ["mltrs-renderer"]},
        {"name": "gx", "dir": "crates/gx", "dependencies": []},
        {"name": "convert-link", "dir": "crates/convert-link", "dependencies": ["gx"]},
        {"name": "sdf_2d", "dir": "examples/sdf_2d", "dependencies": ["mltrs"]},
        {"name": "toon_link", "dir": "examples/toon_link", "dependencies": ["gx", "mltrs"]}
    ]"#;

    fn workspace() -> Workspace {
        Workspace::parse(GRAPH).unwrap()
    }

    fn arguments_for(paths: &[&str]) -> Vec<String> {
        let mut input = paths.join("\0").into_bytes();
        input.push(0);
        arguments(&workspace(), &input)
    }

    fn expected(tww_assets: bool, packages: &[&str]) -> Vec<String> {
        let mut expected = ALWAYS.to_vec();
        if tww_assets {
            expected.push(TWW_ASSETS);
            expected.push("--quiet");
        }
        expected.push(ROC);
        if !packages.is_empty() {
            expected.push(TEST_CRATES);
            expected.extend(packages);
        }
        expected.into_iter().map(str::to_string).collect()
    }

    fn everything(tww_assets: bool) -> Vec<String> {
        let packages: Vec<String> = workspace().names().into_iter().collect();
        let packages: Vec<&str> = packages.iter().map(String::as_str).collect();
        expected(tww_assets, &packages)
    }

    #[test]
    fn graph_without_a_link_package_is_rejected() {
        let graph = r#"[{"name": "gx", "dir": "crates/gx", "dependencies": []}]"#;
        let Err(error) = Workspace::parse(graph) else {
            panic!("graph without toon_link was accepted");
        };
        assert!(error.contains("toon_link"), "{error}");
    }

    #[test]
    fn malformed_graph_is_rejected() {
        assert!(Workspace::parse("[{\"name\": 1}]").is_err());
    }

    #[test]
    fn documentation_and_unrelated_paths_skip() {
        let arguments = arguments_for(&[
            "examples/toon_link/README.md",
            "crates/gx/AGENTS.md",
            "notes.org",
            "docs/testing.md",
            ".gitignore",
            ".github/workflows/ci.yml",
            "llm_notes/plan.txt",
            "roc-platform/src/lib.rs",
        ]);
        assert_eq!(arguments, [SKIP]);
    }

    #[test]
    fn empty_index_tests_every_package() {
        assert_eq!(arguments(&workspace(), b""), everything(false));
    }

    #[test]
    fn workspace_files_test_everything_and_the_assets() {
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "justfile",
            "scripts/pre-commit-checks.rs",
            "scripts/pre-commit.sh",
            ".cargo/config.toml",
            "crates/unknown/src/lib.rs",
            "examples/unknown/src/main.rs",
        ] {
            assert_eq!(arguments_for(&[path]), everything(true), "{path}");
        }
    }

    #[test]
    fn package_directory_match_is_exact() {
        assert_eq!(
            arguments_for(&["crates/gx-extra/src/lib.rs"]),
            everything(true)
        );
    }

    #[test]
    fn leaf_example_tests_only_itself() {
        assert_eq!(
            arguments_for(&["examples/sdf_2d/shaders/source/file with spaces.slang"]),
            expected(false, &["sdf_2d"])
        );
    }

    #[test]
    fn link_packages_add_the_asset_gate() {
        assert_eq!(
            arguments_for(&["examples/toon_link/src/main.rs"]),
            expected(true, &["toon_link"])
        );
        assert_eq!(
            arguments_for(&["crates/convert-link/src/output.rs"]),
            expected(true, &["convert-link"])
        );
        assert_eq!(
            arguments_for(&["crates/gx/src/model_manifest.rs"]),
            expected(true, &["convert-link", "gx", "toon_link"])
        );
    }

    #[test]
    fn library_change_tests_transitive_dependents() {
        assert_eq!(
            arguments_for(&["crates/render-graph/src/lib.rs"]),
            expected(
                false,
                &[
                    "mltrs",
                    "mltrs-render-graph",
                    "mltrs-renderer",
                    "sdf_2d",
                    "toon_link"
                ]
            )
        );
    }

    #[test]
    fn dependents_do_not_add_the_asset_gate() {
        let arguments = arguments_for(&["crates/mltrs/src/lib.rs"]);
        assert!(arguments.contains(&"toon_link".to_string()));
        assert!(!arguments.contains(&TWW_ASSETS.to_string()));
    }

    #[test]
    fn mixed_paths_merge_and_name_test_crates_last() {
        let arguments = arguments_for(&[
            "docs/testing.md",
            "crates/gx/src/model_manifest.rs",
            "examples/sdf_2d/src/main.rs",
        ]);
        assert_eq!(
            arguments,
            expected(true, &["convert-link", "gx", "sdf_2d", "toon_link"])
        );
        let packages = workspace().names();
        let test_crates = arguments.iter().position(|a| a == TEST_CRATES).unwrap();
        assert!(
            arguments[test_crates + 1..]
                .iter()
                .all(|a| packages.contains(a))
        );
    }
}
