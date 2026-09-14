use super::*;
use mltrs_render_graph::{backend::*, *};
use std::cell::RefCell;

struct Backend {
    targets: Vec<Option<UploadTarget>>,
    events: RefCell<Vec<&'static str>>,
}
impl BackendTypes for Backend {
    type IndirectCommand = DrawIndexedIndirectCommand;
    type Resource = ();
}
impl PreparationBackend for Backend {
    fn max_image_dimension_2d(&self) -> u32 {
        64
    }
    fn prepare_image(
        &mut self,
        _: u32,
        _: u32,
        _: GraphFormat,
    ) -> anyhow::Result<(PhysicalImage, ())> {
        unreachable!("upload tests allocate no textures")
    }
}
impl BindingLookup for Backend {
    fn buffer_address(&self, _: BufferAddressKind, _: usize) -> u64 {
        0
    }
}
impl FrameLookup for Backend {
    fn uniform_live(&self, _: usize) -> bool {
        true
    }
    fn storage_live(&self, _: usize) -> bool {
        true
    }
    fn singleton_live(&self, _: usize) -> bool {
        true
    }
    fn whole_index_count(&self, _: usize) -> u32 {
        0
    }
}
struct Frame<'a>(&'a Backend);
impl FrameBackend for Frame<'_> {
    type Backend = Backend;
    type Error = crate::renderer::DrawError;
    fn lookup(&self) -> &dyn FrameLookup {
        self.0
    }
    fn submit(
        self,
        batch: mltrs_render_graph::commands::CommandBatch,
        commit: impl FnOnce(),
    ) -> Result<(), Self::Error> {
        let writes = validate_uploads(&batch, |_, index| {
            self.0.targets.get(index).copied().flatten()
        })?;
        self.0.events.borrow_mut().push("wait");
        for _ in writes {
            self.0.events.borrow_mut().push("write");
        }
        self.0.events.borrow_mut().push("submit");
        commit();
        self.0.events.borrow_mut().push("commit");
        Ok(())
    }
}
fn target(bytes: &mut [u8], kind: UploadKind, byte_size: u64) -> UploadTarget {
    UploadTarget {
        byte_size,
        mapped_mem: bytes.as_mut_ptr().cast(),
        kind,
    }
}
fn storage_case(target: Option<UploadTarget>, succeeds: bool) {
    let mut backend = Backend {
        targets: vec![target, target],
        events: RefCell::new(vec![]),
    };
    let graph = RenderGraph::new(
        ResourcePlanner::new(),
        (
            upload(StorageSlot::<u32>::from_backend(0, 1)),
            upload(StorageSlot::<u32>::from_backend(1, 1000)),
        ),
    )
    .unwrap();
    let mut graph = graph.prepare(&mut backend).unwrap();
    let result = graph.execute(Frame(&backend), &(vec![1], vec![2, 3]));
    assert_eq!(result.is_ok(), succeeds);
    if !succeeds {
        assert!(matches!(
            result,
            Err(crate::renderer::DrawError::DrawError(_))
        ));
        assert!(
            backend.events.borrow().is_empty(),
            "no partial write, submit, or commit"
        );
    } else {
        assert_eq!(
            *backend.events.borrow(),
            ["wait", "write", "write", "submit", "commit"]
        );
    }
}
#[test]
fn forged_storage_capacity_rejected_before_any_write() {
    let mut bytes = [0xa5; 8];
    storage_case(Some(target(&mut bytes, UploadKind::Storage, 4)), false);
    assert_eq!(bytes, [0xa5; 8]);
    storage_case(Some(target(&mut bytes, UploadKind::Storage, 8)), true);
}
#[test]
fn unmapped_wrong_kind_and_missing_storage_rejected() {
    let mut bytes = [0xa5; 8];
    storage_case(Some(target(&mut bytes, UploadKind::ReadOnly, 8)), false);
    storage_case(Some(target(&mut bytes, UploadKind::Uniform, 8)), false);
    storage_case(
        Some(UploadTarget {
            byte_size: 8,
            mapped_mem: std::ptr::null_mut(),
            kind: UploadKind::Storage,
        }),
        false,
    );
    storage_case(None, false);
    assert_eq!(bytes, [0xa5; 8]);
}
struct Params {
    _values: [u32; 2],
}
impl mltrs_render_graph::GPUWrite for Params {}
impl GraphShaderParams for Params {
    type Data = ();
    type Bindings = ();
    type Input = ();
    fn input(_: &(), _: &()) {}
    fn assemble_input(_: &(), _: &BindingResolver<'_>) -> Self {
        Self { _values: [1, 2] }
    }
}
#[test]
fn forged_uniform_type_uses_logical_not_padded_capacity() {
    // Backing memory may be padded, but only the registered payload is writable.
    let mut bytes = [0xa5; 256];
    for (kind, capacity, mapped, succeeds) in [
        (UploadKind::Uniform, 4, true, false),
        (UploadKind::Uniform, 16, true, false),
        (UploadKind::Storage, 8, true, false),
        (UploadKind::Uniform, 8, false, false),
        (UploadKind::Uniform, 8, true, true),
    ] {
        let mut destination = target(&mut bytes, kind, capacity);
        if !mapped {
            destination.mapped_mem = std::ptr::null_mut();
        }
        let mut backend = Backend {
            targets: vec![Some(destination)],
            events: RefCell::new(vec![]),
        };
        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            dispatch(
                ComputePipelineKey::<NoPush>::new(0),
                UniformSlot::<Params>::from_backend(0),
                [1; 3],
            ),
        )
        .unwrap();
        let mut graph = graph.prepare(&mut backend).unwrap();
        let result = graph.execute(Frame(&backend), &());
        assert_eq!(result.is_ok(), succeeds);
        if !succeeds {
            assert!(backend.events.borrow().is_empty());
        }
    }
    assert_eq!(bytes, [0xa5; 256]);
}
