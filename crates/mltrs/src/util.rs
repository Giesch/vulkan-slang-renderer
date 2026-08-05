/// Builds a path from the *calling* crate's manifest dir. A macro because
/// `env!("CARGO_MANIFEST_DIR")` in a library fn would bake in the library's
/// own dir; expanding at the call site yields the consumer's crate dir.
#[macro_export]
macro_rules! manifest_path {
    ($($seg:expr),* $(,)?) => {{
        let p: ::std::path::PathBuf =
            [env!("CARGO_MANIFEST_DIR"), $($seg),*].into_iter().collect();
        p
    }};
}
