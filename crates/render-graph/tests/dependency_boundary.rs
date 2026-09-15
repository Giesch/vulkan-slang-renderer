use std::{collections::HashSet, process::Command};

#[test]
fn graph_dependency_boundary() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata must run");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let root = packages
        .iter()
        .find(|p| p["name"] == "mltrs-render-graph")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let nodes = metadata["resolve"]["nodes"].as_array().unwrap();
    let mut pending = vec![root];
    let mut visited = HashSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let package = packages.iter().find(|p| p["id"] == id).unwrap();
        let name = package["name"].as_str().unwrap();
        assert!(
            ![
                "mltrs-renderer",
                "ash",
                "vk-mem",
                "sdl3",
                "sdl3-sys",
                "shader-slang",
                "shader-slang-sys"
            ]
            .contains(&name),
            "forbidden graph dependency: {name}"
        );
        let node = nodes.iter().find(|node| node["id"] == id).unwrap();
        pending.extend(
            node["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|dependency| dependency.as_str().unwrap()),
        );
    }
    assert!(
        visited.len() > 1,
        "dependency traversal must include transitive packages"
    );
}
