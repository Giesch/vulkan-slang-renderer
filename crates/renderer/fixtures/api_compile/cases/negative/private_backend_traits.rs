//! Negative control: downstream code cannot name the backend traits through
//! either renderer path. Game types must implement the render_graph traits.

#![allow(dead_code, unused_imports)]

use mltrs_renderer::renderer::GPUWrite as RootGPUWrite;
use mltrs_renderer::renderer::PushConstantBlock as RootPushConstantBlock;
use mltrs_renderer::renderer::gpu_write::GPUWrite as BackendGPUWrite;
use mltrs_renderer::renderer::gpu_write::PushConstantBlock as BackendPushConstantBlock;

fn main() {}
