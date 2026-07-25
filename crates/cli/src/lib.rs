//! Slang -> Rust codegen for `mltrs`.
//!
//! Exposed as a library so a consumer can drive codegen from a `build.rs`; the
//! `mltrs` binary is a thin clap wrapper over [`build_tasks`].

pub mod build_tasks;
pub mod util;
