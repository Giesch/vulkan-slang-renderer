//! Positive control for every graph constructor family.
//!
//! Each function type-checks one family against real renderer handles and
//! real generated binding types. No case constructs a `Renderer`: the
//! functions take handles as parameters and are never run. The push
//! families use the `push.compute.slang` push block; the no-push families
//! use the plain compute and draw shaders.

#![allow(dead_code)]

use mltrs_renderer::renderer::pipeline::{
    Compute, DrawIndexed, DrawIndexedIndirect, DrawVertexCount, NoPush, PickingPipelineHandle,
    PipelineHandle, PushBlock,
};
use mltrs_renderer::renderer::render_graph::{
    self, ComputeNode, ComputePipelineKey, DrawIndexedIndirectKey, DrawIndexedKey,
    DrawVertexCountKey, DrawVertexCountNode, GraphShaderParams, StorageSlot, UniformSlot, dispatch,
    draw_index_range, draw_indexed, draw_indexed_indirect, draw_vertex_count,
};
use mltrs_renderer::renderer::{
    DrawIndexedIndirectCommand, GpuOnlyBufferHandle, GpuOnlySlot, ImmutableBufferHandle,
    ImmutableSlot, PickingCursor, Renderer, StorageBufferHandle, UniformBufferHandle,
};

fn upload_indirect_commands(
    renderer: &mut Renderer,
    commands: Vec<DrawIndexedIndirectCommand>,
) -> ImmutableBufferHandle<DrawIndexedIndirectCommand> {
    let mut buffer = renderer.create_indirect_buffer(&commands).unwrap();
    renderer.write_immutable_all_frames(&mut buffer, &commands);
    let _: ImmutableSlot<DrawIndexedIndirectCommand> = (&buffer).into();

    buffer
}

fn update_indirect_commands(
    gpu: &mut mltrs_renderer::renderer::Gpu,
    buffer: &mut ImmutableBufferHandle<DrawIndexedIndirectCommand>,
    commands: &[DrawIndexedIndirectCommand],
) {
    gpu.write_immutable(buffer, commands);
}

use render_graph_api_checks::generated::shader_atlas::particle::{OtherElement, Particle};
use render_graph_api_checks::generated::shader_atlas::push_compute::{
    ScaleParams, ScaleParamsBindings, ScalePush, ScalePushBindings, ScalePushData,
};
use render_graph_api_checks::generated::shader_atlas::render::{
    RenderParams, RenderParamsBindings,
};
use render_graph_api_checks::generated::shader_atlas::sim_compute::{SimParams, SimParamsBindings};

#[derive(Clone, Copy)]
pub struct Cursor {
    pub x: f32,
    pub y: f32,
}

impl PickingCursor for Cursor {
    fn position(&self) -> [f32; 2] {
        [self.x, self.y]
    }
}

fn sim_bindings(particles: &GpuOnlyBufferHandle<Particle>) -> SimParamsBindings {
    let slots = GpuOnlySlot::from(particles);
    SimParamsBindings {
        particles_in: slots.previous(),
        particles_out: slots.current(),
    }
}

/// The no-push dispatch and draw families, plus picking and upload. Each
/// constructor takes `impl Into<Key>`, so a handle reference passes
/// directly; the explicit-key form stays covered elsewhere in this case.
fn no_push_families(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
    render_pipeline: &PipelineHandle<DrawVertexCount, NoPush>,
    render_params: &UniformBufferHandle<RenderParams>,
    picking_pipeline: &PickingPipelineHandle,
    storage: &StorageBufferHandle<Particle>,
) {
    let _ =
        dispatch(sim_pipeline, sim_params, [4, 1, 1]).with_param_bindings(sim_bindings(particles));
    let _ = draw_vertex_count(
        render_pipeline,
        render_params,
        6,
        RenderParamsBindings {
            particles: GpuOnlySlot::from(particles).current().into(),
        },
    );
    let _ = render_graph::upload(StorageSlot::from(storage));
    let _ = render_graph::picking::<Cursor>(picking_pipeline);
}

/// A valid logical graph, built GPU-free, then consumed by preparation.
fn logical_then_prepared(
    sim_pipeline: &PipelineHandle<Compute, NoPush>,
    sim_params: &UniformBufferHandle<SimParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
    render_pipeline: &PipelineHandle<DrawVertexCount, NoPush>,
    render_params: &UniformBufferHandle<RenderParams>,
    renderer: &mut Renderer,
) -> render_graph::PreparedRenderGraph<(ComputeNode<SimParams>, DrawVertexCountNode<RenderParams>)>
{
    let graph = render_graph::RenderGraph::new(
        render_graph::ResourcePlanner::new(),
        (
            dispatch(
                ComputePipelineKey::from(sim_pipeline),
                sim_params,
                [4, 1, 1],
            )
            .with_param_bindings(sim_bindings(particles)),
            draw_vertex_count(
                DrawVertexCountKey::from(render_pipeline),
                render_params,
                6,
                RenderParamsBindings {
                    particles: GpuOnlySlot::from(particles).current().into(),
                },
            ),
        ),
    )
    .unwrap();

    graph.prepare(renderer).unwrap()
}

/// The indexed draw families with an indirect argument buffer.
fn indexed_families(
    render_pipeline: &PipelineHandle<DrawIndexed, NoPush>,
    render_params: &UniformBufferHandle<RenderParams>,
    particles: &GpuOnlyBufferHandle<Particle>,
    indirect_pipeline: &PipelineHandle<DrawIndexedIndirect, NoPush>,
    indirect_params: &UniformBufferHandle<RenderParams>,
    args: &ImmutableBufferHandle<DrawIndexedIndirectCommand>,
) {
    let bindings = RenderParamsBindings {
        particles: GpuOnlySlot::from(particles).current().into(),
    };
    let _ = draw_indexed(render_pipeline, render_params, bindings);
    let _ = draw_index_range(
        DrawIndexedKey::from(render_pipeline),
        render_params,
        0,
        6,
        bindings,
    );
    let _ = draw_indexed_indirect(
        DrawIndexedIndirectKey::from(indirect_pipeline),
        indirect_params,
        args,
        0,
        1,
        bindings,
    );
}

/// Every draw family built from a push-constant pipeline, plus the compute
/// form. Each command completes with `.with_push_constant`. Each call mints a
/// fresh push input (the value moves into the node).
#[allow(clippy::too_many_arguments)]
fn push_families(
    push_pipeline: &PipelineHandle<Compute, PushBlock<ScalePush>>,
    push_params: &UniformBufferHandle<ScaleParams>,
    items: &StorageBufferHandle<OtherElement>,
    vertex_push_pipeline: &PipelineHandle<DrawVertexCount, PushBlock<ScalePush>>,
    vertex_push_params: &UniformBufferHandle<ScaleParams>,
    indexed_push_pipeline: &PipelineHandle<DrawIndexed, PushBlock<ScalePush>>,
    indexed_push_params: &UniformBufferHandle<ScaleParams>,
    indirect_push_pipeline: &PipelineHandle<DrawIndexedIndirect, PushBlock<ScalePush>>,
    indirect_push_params: &UniformBufferHandle<ScaleParams>,
    args: &ImmutableBufferHandle<DrawIndexedIndirectCommand>,
) {
    let items_addr = StorageSlot::from(items).addr();
    let push_input = || {
        ScalePush::input(
            &ScalePushData { factor: 2.0 },
            &ScalePushBindings { items: items_addr },
        )
    };
    let params = ScaleParamsBindings { items: items_addr };

    let _ = draw_vertex_count(vertex_push_pipeline, vertex_push_params, 6, params)
        .with_push_constant(push_input());
    let _ = draw_indexed(
        DrawIndexedKey::from(indexed_push_pipeline),
        indexed_push_params,
        params,
    )
    .with_push_constant(push_input());
    let _ = draw_index_range(
        DrawIndexedKey::from(indexed_push_pipeline),
        indexed_push_params,
        0,
        6,
        params,
    )
    .with_push_constant(push_input());
    let _ = draw_indexed_indirect(
        indirect_push_pipeline,
        indirect_push_params,
        ImmutableSlot::from(args),
        0,
        1,
        params,
    )
    .with_push_constant(push_input());

    // the pending-push form completes before entering a graph
    let _ = render_graph::repeat((dispatch(
        ComputePipelineKey::from(push_pipeline),
        push_params,
        [4, 1, 1],
    )
    .with_param_bindings(params)
    .with_push_constant(push_input()),));
}

/// The indirect argument slot can also be minted from the handle at the
/// call site; the element type is part of the slot's type.
fn indirect_slot_element(args: &ImmutableBufferHandle<OtherElement>) {
    let slot: ImmutableSlot<OtherElement> = args.into();
    let _ = slot;
}

fn main() {}
