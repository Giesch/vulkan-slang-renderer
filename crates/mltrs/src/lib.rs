//! A Vulkan renderer with type-safe, reflection-based Slang shader bindings.
//!
//! Generated shader bindings import from this crate's paths (`mltrs::renderer`,
//! `mltrs::shaders::atlas`, ...), so the modules re-exported here are the
//! public contract that `mltrs shaders compile` emits against.

pub use mltrs_renderer::{editor, renderer, shaders};

pub mod app;
pub mod game;
pub mod ktx;
pub mod model_manifest;
pub mod util;

pub use game::*;
