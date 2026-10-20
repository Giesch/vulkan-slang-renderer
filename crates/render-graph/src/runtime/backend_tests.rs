//! Tests the render graph's interaction with a fake backend that records events
//! instead of calling Vulkan.
//!
//! Covers:
//!
//! - Resource preparation: extent validation before allocation, physical image
//!   allocation, resource lifetimes, and partial allocation failures.
//! - Execution validation: rejection of dropped buffers and oversized uploads.
//! - Submission callbacks: texture history advancement only after successful
//!   submission, advancement despite presentation failure,
//!   and exactly one commit per frame.
//! - Command generation: pipelines, dispatch sizes, draw arguments, push
//!   constants, and picking coordinates passed to the backend.
//! - Address resolution: current-frame, previous-frame, and singleton buffers.

use super::*;
use crate::backend::{BufferAddressKind, PhysicalImage};
use std::{cell::RefCell, rc::Rc};

type Events = Rc<RefCell<Vec<Event>>>;

#[derive(Debug, PartialEq)]
enum Event {
    Allocate {
        id: usize,
        width: u32,
        height: u32,
        format: GraphFormat,
    },
    Clear(usize),
    Alias(usize),
    Drop(usize),
    Address {
        kind: BufferAddressKind,
        index: usize,
    },
    Wait,
    Write {
        uniform: bool,
        index: usize,
        byte_len: usize,
    },
    Compute {
        pipeline: usize,
        groups: [u32; 3],
        push: Option<Vec<u8>>,
    },
    Draw {
        pipeline: usize,
        call: DrawEvent,
        push: Option<Vec<u8>>,
    },
    Picking {
        pipeline: usize,
        position: [f32; 2],
    },
    Submit,
    Commit,
    Present,
}

#[derive(Debug, PartialEq, Eq)]
enum DrawEvent {
    VertexCount(u32),
    IndexCount(u32),
    IndexRange {
        first_index: u32,
        index_count: u32,
    },
    IndexedIndirect {
        buffer: usize,
        offset: u64,
        draw_count: u32,
        stride: usize,
        alignment: usize,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FailPoint {
    Wait,
    Submit,
    Present,
}

struct Resource {
    id: usize,
    events: Events,
}

impl Drop for Resource {
    fn drop(&mut self) {
        self.events.borrow_mut().push(Event::Drop(self.id));
    }
}

struct Fake {
    events: Events,
    max: u32,
    allocated: usize,
    fail_allocation: Option<usize>,
    fail: Option<FailPoint>,
    live: bool,
    registered: Vec<Rc<Resource>>,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            events: Default::default(),
            max: 64,
            allocated: 0,
            fail_allocation: None,
            fail: None,
            live: true,
            registered: vec![],
        }
    }
}

impl BackendTypes for Fake {
    type IndirectCommand = super::tests::DrawIndexedIndirectCommand;
    type Resource = Rc<Resource>;
}

impl PreparationBackend for Fake {
    fn max_image_dimension_2d(&self) -> u32 {
        self.max
    }
    fn prepare_image(
        &mut self,
        width: u32,
        height: u32,
        format: GraphFormat,
    ) -> anyhow::Result<(PhysicalImage, Self::Resource)> {
        let id = self.allocated;
        self.events.borrow_mut().push(Event::Allocate {
            id,
            width,
            height,
            format,
        });
        anyhow::ensure!(self.fail_allocation != Some(id), "allocation failed");
        self.allocated += 1;
        let resource = Rc::new(Resource {
            id,
            events: self.events.clone(),
        });
        self.registered.push(resource.clone());
        self.events
            .borrow_mut()
            .extend([Event::Clear(id), Event::Alias(id)]);

        Ok((
            PhysicalImage {
                storage: BindlessHandle::from_raw(id as u64 + 10),
                sampled: BindlessHandle::from_raw(id as u64 + 20),
            },
            resource,
        ))
    }
}

impl BindingLookup for Fake {
    fn buffer_address(&self, kind: BufferAddressKind, index: usize) -> u64 {
        self.events
            .borrow_mut()
            .push(Event::Address { kind, index });

        (match kind {
            BufferAddressKind::Current => 1000,
            BufferAddressKind::Previous => 2000,
            BufferAddressKind::Singleton => 3000,
        }) + index as u64 * 100
    }
}

impl FrameLookup for Fake {
    fn uniform_live(&self, _: usize) -> bool {
        self.live
    }

    fn storage_live(&self, _: usize) -> bool {
        self.live
    }

    fn singleton_live(&self, _: usize) -> bool {
        self.live
    }

    fn whole_index_count(&self, _: usize) -> u32 {
        12
    }
}

struct Frame<'a>(&'a Fake);

impl FrameBackend for Frame<'_> {
    type Backend = Fake;
    type Error = anyhow::Error;

    fn lookup(&self) -> &dyn FrameLookup {
        self.0
    }

    fn submit(
        self,
        batch: crate::commands::CommandBatch,
        on_submitted: impl FnOnce(),
    ) -> anyhow::Result<()> {
        let event = |event| self.0.events.borrow_mut().push(event);
        event(Event::Wait);
        anyhow::ensure!(self.0.fail != Some(FailPoint::Wait), "wait failed");
        batch.visit_writes(|uniform, index, bytes| {
            event(Event::Write {
                uniform,
                index,
                byte_len: bytes.len(),
            });
        });
        for (pipeline, groups, push) in batch.dispatches() {
            event(Event::Compute {
                pipeline: *pipeline,
                groups: *groups,
                push: push.as_ref().map(|push| push.as_slice().to_vec()),
            });
        }

        for draw in batch.draws() {
            let call = match draw.draw_call() {
                DrawCallConfig::VertexCount(count) => DrawEvent::VertexCount(count),
                DrawCallConfig::IndexCount(count) => DrawEvent::IndexCount(count),
                DrawCallConfig::IndexRange {
                    first_index,
                    index_count,
                } => DrawEvent::IndexRange {
                    first_index,
                    index_count,
                },
                DrawCallConfig::IndexedIndirect(request) => DrawEvent::IndexedIndirect {
                    buffer: request.buffer(),
                    offset: request.offset(),
                    draw_count: request.draw_count(),
                    stride: request.stride(),
                    alignment: request.alignment(),
                },
            };
            event(Event::Draw {
                pipeline: draw.pipeline_index(),
                call,
                push: draw.push_constants().map(|push| push.as_slice().to_vec()),
            });
        }

        if let Some(picking) = batch.picking() {
            event(Event::Picking {
                pipeline: picking.pipeline_index(),
                position: picking.position(),
            });
        }

        event(Event::Submit);
        anyhow::ensure!(self.0.fail != Some(FailPoint::Submit), "submit failed");
        on_submitted();
        event(Event::Commit);
        event(Event::Present);
        anyhow::ensure!(self.0.fail != Some(FailPoint::Present), "present failed");

        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Bindings {
    previous: SampledTexBinding,
    next: StorageTexBinding,
}

impl GraphBindingSet for Bindings {
    fn visit(&self, visit: &mut dyn FnMut(GraphBinding)) {
        visit(GraphBinding::SampledTex(self.previous));
        visit(GraphBinding::StorageTex(self.next));
    }
}

#[repr(C)]
struct Params([u64; 2]);

impl GPUWrite for Params {}

impl GraphShaderParams for Params {
    type Data = ();
    type Bindings = Bindings;
    type Input = Bindings;

    fn input(_: &(), bindings: &Bindings) -> Bindings {
        *bindings
    }

    fn assemble_input(input: &Bindings, resolver: &BindingResolver<'_>) -> Self {
        Self([
            resolver.sampled_tex(input.previous).to_raw(),
            resolver.storage_tex(input.next).to_raw(),
        ])
    }
}

fn graph(extent: u32) -> RenderGraph<ComputeNode<Params>> {
    let mut resources = ResourcePlanner::new();
    let tex = resources.texture("history", extent, extent, GraphFormat::R32Float);
    RenderGraph::new(
        resources,
        ComputeNode {
            pipeline_index: 3,
            uniform: UniformSlot::from_backend(2),
            group_count: [2, 3, 1],
            bindings: Bindings {
                previous: tex.read(),
                next: tex.write(),
            },
            push: (),
        },
    )
    .unwrap()
}

#[test]
fn backend_prepare_extent_before_allocation() {
    let mut backend = Fake::default();
    assert!(graph(65).prepare(&mut backend).is_err());
    assert!(backend.events.borrow().is_empty());
}

#[test]
fn backend_prepare_physical_images() {
    let mut backend = Fake::default();
    let prepared = graph(8).prepare(&mut backend).unwrap();
    assert_eq!(backend.allocated, 2);
    assert_eq!(prepared.phys[0].images.len(), 2);
    assert_eq!(prepared.phys[0].images[1].sampled.to_raw(), 21);
    assert_eq!(
        *backend.events.borrow(),
        [
            Event::Allocate {
                id: 0,
                width: 8,
                height: 8,
                format: GraphFormat::R32Float
            },
            Event::Clear(0),
            Event::Alias(0),
            Event::Allocate {
                id: 1,
                width: 8,
                height: 8,
                format: GraphFormat::R32Float
            },
            Event::Clear(1),
            Event::Alias(1)
        ]
    );
}

#[test]
fn backend_prepared_resources_kept_alive() {
    let mut backend = Fake::default();
    let prepared = graph(8).prepare(&mut backend).unwrap();
    backend.registered.clear();
    assert!(
        !backend
            .events
            .borrow()
            .iter()
            .any(|event| matches!(event, Event::Drop(_)))
    );
    drop(prepared);
    let dropped: Vec<_> = backend
        .events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            Event::Drop(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(dropped, [0, 1]);
}

#[test]
fn backend_partial_prepare_failure() {
    let mut backend = Fake {
        fail_allocation: Some(1),
        ..Default::default()
    };
    assert!(graph(8).prepare(&mut backend).is_err());
    assert_eq!(backend.registered.len(), 1);
    assert!(
        !backend
            .events
            .borrow()
            .iter()
            .any(|event| matches!(event, Event::Drop(_)))
    );
    backend.registered.clear();
    assert!(backend.events.borrow().contains(&Event::Drop(0)));
}

#[test]
fn backend_dropped_buffer_rejected() {
    let mut backend = Fake::default();
    let mut prepared = graph(8).prepare(&mut backend).unwrap();
    backend.live = false;
    backend.events.borrow_mut().clear();
    assert!(
        prepared
            .execute(Frame(&backend), &())
            .unwrap_err()
            .to_string()
            .contains("dropped")
    );
    assert!(backend.events.borrow().is_empty());
}

#[test]
fn backend_upload_capacity_rejected() {
    let mut backend = Fake::default();
    let mut prepared = RenderGraph::new(
        ResourcePlanner::new(),
        upload(CpuUpdatedSlot::<u32>::from_backend(0, 1)),
    )
    .unwrap()
    .prepare(&mut backend)
    .unwrap();
    assert!(
        prepared
            .execute(Frame(&backend), &vec![1, 2])
            .unwrap_err()
            .to_string()
            .contains("capacity")
    );
    assert!(backend.events.borrow().is_empty());
}

#[test]
fn backend_cursor_unchanged_on_submission_failure() {
    for fail in [FailPoint::Wait, FailPoint::Submit] {
        let mut backend = Fake {
            fail: Some(fail),
            ..Default::default()
        };
        let mut prepared = graph(8).prepare(&mut backend).unwrap();
        let before = prepared.tex[0].cursor;
        assert!(prepared.execute(Frame(&backend), &()).is_err());
        assert_eq!(prepared.tex[0].cursor, before);
    }
}

#[test]
fn backend_cursor_advances_despite_presentation_failure() {
    let mut backend = Fake {
        fail: Some(FailPoint::Present),
        ..Default::default()
    };
    let mut prepared = graph(8).prepare(&mut backend).unwrap();
    let before = prepared.tex[0].cursor;
    assert!(prepared.execute(Frame(&backend), &()).is_err());
    assert_ne!(prepared.tex[0].cursor, before);
}

#[repr(C)]
struct Plain(u32);

impl GPUWrite for Plain {}

impl PushConstantBlock for Plain {}

impl GraphShaderParams for Plain {
    type Data = u32;
    type Bindings = ();
    type Input = Plain;

    fn input(data: &u32, _: &()) -> Plain {
        Self(*data)
    }

    fn assemble_input(input: &Plain, _: &BindingResolver<'_>) -> Self {
        Self(input.0)
    }
}

impl GraphBindingSet for Plain {
    fn visit(&self, _: &mut dyn FnMut(GraphBinding)) {}
}

#[derive(Clone, Copy)]
struct Cursor;

impl PickingCursor for Cursor {
    fn position(&self) -> [f32; 2] {
        [12.5, 7.0]
    }
}

#[test]
fn backend_command_plan() {
    let mut backend = Fake::default();
    let nodes = (
        dispatch(
            ComputePipelineKey::<NoPush>::new(1),
            UniformSlot::<Plain>::from_backend(1),
            [4, 2, 1],
        ),
        draw_vertex_count(
            DrawVertexCountKey::<PushBlock<Plain>>::new(2),
            UniformSlot::<Plain>::from_backend(2),
            3,
            (),
        )
        .with_push_constant(Plain(0x01010101)),
        draw_indexed(
            DrawIndexedKey::<NoPush>::new(3),
            UniformSlot::<Plain>::from_backend(3),
            (),
        ),
        draw_indexed_indirect(
            DrawIndexedIndirectKey::<NoPush>::new(4),
            UniformSlot::<Plain>::from_backend(4),
            ImmutableSlot::<super::tests::DrawIndexedIndirectCommand>::from_backend(5, 3),
            1,
            2,
            (),
        ),
        picking::<Cursor>(PickingPipelineKey::new(6)),
    );
    let mut prepared = RenderGraph::new(ResourcePlanner::new(), nodes)
        .unwrap()
        .prepare(&mut backend)
        .unwrap();
    prepared
        .execute(Frame(&backend), &(7, 8, 9, 10, Cursor))
        .unwrap();
    let events = backend.events.borrow();
    for expected in [
        Event::Compute {
            pipeline: 1,
            groups: [4, 2, 1],
            push: None,
        },
        Event::Draw {
            pipeline: 2,
            call: DrawEvent::VertexCount(3),
            push: Some(vec![1, 1, 1, 1]),
        },
        Event::Draw {
            pipeline: 3,
            call: DrawEvent::IndexCount(12),
            push: None,
        },
        Event::Draw {
            pipeline: 4,
            call: DrawEvent::IndexedIndirect {
                buffer: 5,
                offset: 20,
                draw_count: 2,
                stride: 20,
                alignment: 4,
            },
            push: None,
        },
        Event::Picking {
            pipeline: 6,
            position: [12.5, 7.0],
        },
    ] {
        assert!(
            events.contains(&expected),
            "missing {expected:?}: {events:?}"
        );
    }
}

#[derive(Clone, Copy)]
struct Addresses;

impl GraphBindingSet for Addresses {
    fn visit(&self, visit: &mut dyn FnMut(GraphBinding)) {
        visit(GraphBinding::Buffer(
            GpuUpdatedSlot::<u32>::from_backend(2).current().erased(),
        ));
        visit(GraphBinding::Buffer(
            GpuUpdatedSlot::<u32>::from_backend(2).previous().erased(),
        ));
        visit(GraphBinding::Buffer(
            SingletonSlot::<u32>::from_backend(3, 4).addr_at(2).erased(),
        ));
    }
}

struct AddressParams;

impl GPUWrite for AddressParams {}

impl GraphShaderParams for AddressParams {
    type Data = ();
    type Bindings = Addresses;
    type Input = Addresses;

    fn input(_: &(), bindings: &Addresses) -> Addresses {
        *bindings
    }

    fn assemble_input(_: &Addresses, resolver: &BindingResolver<'_>) -> Self {
        assert_eq!(
            resolver
                .buf(GpuUpdatedSlot::<u32>::from_backend(2).current())
                .to_raw(),
            1200
        );
        assert_eq!(
            resolver
                .read_buf(GpuUpdatedSlot::<u32>::from_backend(2).previous())
                .to_raw(),
            2200
        );
        assert_eq!(
            resolver
                .immutable_buf(SingletonSlot::<u32>::from_backend(3, 4).addr_at(2))
                .to_raw(),
            3308
        );

        Self
    }
}

#[test]
fn backend_current_previous_addresses() {
    let mut backend = Fake::default();
    let node = ComputeNode {
        pipeline_index: 1,
        uniform: UniformSlot::<AddressParams>::from_backend(1),
        group_count: [1; 3],
        bindings: Addresses,
        push: (),
    };
    let mut prepared = RenderGraph::new(ResourcePlanner::new(), node)
        .unwrap()
        .prepare(&mut backend)
        .unwrap();
    prepared.execute(Frame(&backend), &()).unwrap();
    let events = backend.events.borrow();
    for expected in [
        Event::Address {
            kind: BufferAddressKind::Current,
            index: 2,
        },
        Event::Address {
            kind: BufferAddressKind::Previous,
            index: 2,
        },
        Event::Address {
            kind: BufferAddressKind::Singleton,
            index: 3,
        },
    ] {
        assert!(
            events.contains(&expected),
            "missing {expected:?}: {events:?}"
        );
    }
}

#[test]
fn backend_commit_once() {
    let mut backend = Fake::default();
    let mut prepared = graph(8).prepare(&mut backend).unwrap();
    for expected in [1, 0, 1] {
        prepared.execute(Frame(&backend), &()).unwrap();
        assert_eq!(prepared.tex[0].cursor, expected);
    }
}

#[test]
fn backend_erased_draw_plan() {
    let mut backend = Fake::default();
    let nodes = vec![
        DynDrawNode::indexed(
            DrawIndexedKey::<NoPush>::new(3),
            UniformSlot::<()>::from_backend(3),
            192,
        ),
        DynDrawNode::vertex_count(
            DrawVertexCountKey::<NoPush>::new(4),
            UniformSlot::<()>::from_backend(4),
            16,
            6,
        ),
    ];
    let mut prepared = RenderGraph::new(ResourcePlanner::new(), nodes)
        .unwrap()
        .prepare(&mut backend)
        .unwrap();

    let short = prepared.execute(Frame(&backend), &vec![vec![0u8; 100], vec![0u8; 16]]);
    assert!(
        short
            .unwrap_err()
            .to_string()
            .contains("received 100 bytes")
    );
    let missing = prepared.execute(Frame(&backend), &vec![vec![0u8; 192]]);
    assert!(
        missing
            .unwrap_err()
            .to_string()
            .contains("2 nodes received 1")
    );
    assert!(backend.events.borrow().is_empty(), "nothing submitted");

    prepared
        .execute(Frame(&backend), &vec![vec![0u8; 192], vec![0u8; 16]])
        .unwrap();
    let events = backend.events.borrow();
    for expected in [
        Event::Write {
            uniform: true,
            index: 3,
            byte_len: 192,
        },
        Event::Write {
            uniform: true,
            index: 4,
            byte_len: 16,
        },
        Event::Draw {
            pipeline: 3,
            call: DrawEvent::IndexCount(12),
            push: None,
        },
        Event::Draw {
            pipeline: 4,
            call: DrawEvent::VertexCount(6),
            push: None,
        },
    ] {
        assert!(
            events.contains(&expected),
            "missing {expected:?}: {events:?}"
        );
    }
}
