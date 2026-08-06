//! Slang compilation and reflection, with no graphics API dependency.
//!
//! This is the only crate in the workspace that depends on `shader-slang`, and
//! no `shader_slang` type appears in its public API — the reflected data it
//! hands back (`json::…`) is plain serde structs. Turning that data into vulkan
//! objects is the renderer's job.

pub mod json;
