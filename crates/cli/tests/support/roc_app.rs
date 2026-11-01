use std::fs;
use std::path::Path;

pub fn write_app_main(generated: &Path) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = repo.join("roc-platform/platform");
    let platform = generated.parent().unwrap().join("platform");
    fs::create_dir_all(&platform).unwrap();
    // Checking and testing Roc modules needs sources, not the platform's host binaries.
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let is_roc_source = path.extension().is_some_and(|extension| extension == "roc")
            && entry.file_type().unwrap().is_file();
        if is_roc_source {
            fs::copy(&path, platform.join(entry.file_name())).unwrap();
        }
    }

    let example =
        fs::read_to_string(repo.join("roc-platform/examples/basic-triangle/main.roc")).unwrap();
    let body = example.split_once('\n').unwrap().1;
    fs::write(
        generated.join("main.roc"),
        format!("app [game] {{ pf: platform \"../platform/main.roc\" }}\n{body}"),
    )
    .unwrap();
}
