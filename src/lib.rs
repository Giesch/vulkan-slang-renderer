#![feature(never_type)]

pub mod app;
pub mod game;
pub mod generated;
pub mod renderer;
pub mod shaders;
pub mod util;

#[cfg(debug_assertions)]
mod shader_watcher;

pub use game::*;
pub use shaders::build_tasks;
