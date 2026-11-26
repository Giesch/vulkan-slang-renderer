//! Resolves the slang source dir a Roc app names in `Game.Init`.
//!
//! Generated Roc shader modules name it relative to the Roc project root
//! (`ShaderAtlas.shaders_source_dir` is `"shaders/source"`), but a Roc
//! executable starts from wherever it is launched: `roc main.roc` from the
//! project root, a built `main` from anywhere.

use std::path::{Path, PathBuf};

/// `dir` itself when absolute; otherwise `dir` joined to each of `bases` in
/// order. The first candidate that is a directory wins. `None` for an empty
/// `dir` or when no candidate is a directory.
pub fn resolve_shaders_source_dir(dir: &str, bases: &[PathBuf]) -> Option<PathBuf> {
    if dir.is_empty() {
        return None;
    }

    let dir = Path::new(dir);
    if dir.is_absolute() {
        return dir.is_dir().then(|| dir.to_path_buf());
    }

    bases
        .iter()
        .map(|base| base.join(dir))
        .find(|candidate| candidate.is_dir())
}

#[cfg(test)]
mod tests {
    use super::resolve_shaders_source_dir;
    use std::fs;
    use std::path::PathBuf;

    /// A fresh scratch dir per test, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "roc-platform-source-dir-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();

            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn empty_dir_names_no_sources() {
        assert_eq!(resolve_shaders_source_dir("", &[PathBuf::from("/")]), None);
    }

    #[test]
    fn absolute_dir_is_kept_only_when_it_exists() {
        let scratch = Scratch::new("absolute");
        let absolute = scratch.0.to_str().unwrap();

        assert_eq!(
            resolve_shaders_source_dir(absolute, &[]),
            Some(scratch.0.clone())
        );
        let missing = scratch.0.join("missing");
        assert_eq!(
            resolve_shaders_source_dir(missing.to_str().unwrap(), &[]),
            None
        );
    }

    #[test]
    fn relative_dir_resolves_against_the_first_base_holding_it() {
        let scratch = Scratch::new("relative");
        let cwd = scratch.0.join("cwd");
        let exe_dir = scratch.0.join("exe");
        fs::create_dir_all(cwd.join("other")).unwrap();
        fs::create_dir_all(exe_dir.join("shaders/source")).unwrap();
        let bases = [cwd.clone(), exe_dir.clone()];

        assert_eq!(
            resolve_shaders_source_dir("shaders/source", &bases),
            Some(exe_dir.join("shaders/source"))
        );

        fs::create_dir_all(cwd.join("shaders/source")).unwrap();
        assert_eq!(
            resolve_shaders_source_dir("shaders/source", &bases),
            Some(cwd.join("shaders/source"))
        );
        assert_eq!(resolve_shaders_source_dir("missing", &bases), None);
    }
}
