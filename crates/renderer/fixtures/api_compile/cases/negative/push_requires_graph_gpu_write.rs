//! Negative control: the graph `PushConstantBlock` requires the graph
//! `GPUWrite` supertrait. Implementing the push marker without the data
//! marker is a compile error.

#![allow(dead_code)]

use mltrs_renderer::renderer::render_graph;

struct NotGpuWrite;

impl render_graph::PushConstantBlock for NotGpuWrite {}

fn main() {}
