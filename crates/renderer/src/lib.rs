//! The Vulkan rendering engine behind `mltrs`, plus the slang reflection types
//! its generated bindings are built from.
//!
//! Consumers normally reach these through the `mltrs` facade rather than
//! depending on this crate directly.

pub mod editor;
pub mod renderer;
pub mod shaders;

#[cfg(debug_assertions)]
mod shader_watcher;
