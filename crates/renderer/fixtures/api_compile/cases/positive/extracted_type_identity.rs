#![allow(dead_code)]
use mltrs_render_graph as graph;
use mltrs_renderer::renderer::render_graph as facade;
fn identity(
    addr: graph::addr::Addr<u32>,
    read: graph::addr::ReadAddr<u32>,
    immutable: graph::addr::ImmutableAddr<u32>,
    sampled: graph::bindless::BindlessHandle<graph::bindless::Sampler2D>,
    planner: graph::ResourcePlanner,
) {
    let _: facade::addr::Addr<u32> = addr;
    let _: facade::addr::ReadAddr<u32> = read;
    let _: facade::addr::ImmutableAddr<u32> = immutable;
    let handle: facade::bindless::BindlessHandle<facade::bindless::Sampler2D> = sampled;
    let binding: facade::SampledTexBinding = handle.into();
    let _: graph::SampledTexBinding = binding;
    let _: facade::ResourcePlanner = planner;
}
fn main() {}
