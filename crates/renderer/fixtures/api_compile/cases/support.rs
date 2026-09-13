#![allow(dead_code)]
use mltrs_render_graph::{backend::*, *};

pub struct OtherCommand([u64; 5]);
impl GPUWrite for OtherCommand {}
impl IndexedIndirectArgs for OtherCommand {
    fn index_count(&self) -> u32 {
        self.0[0] as u32
    }
    fn instance_count(&self) -> u32 {
        self.0[1] as u32
    }
    fn first_index(&self) -> u32 {
        self.0[2] as u32
    }
    fn vertex_offset(&self) -> i32 {
        self.0[3] as i32
    }
    fn first_instance(&self) -> u32 {
        self.0[4] as u32
    }
}
pub struct OtherBackend;
impl BackendTypes for OtherBackend {
    type IndirectCommand = OtherCommand;
    type Resource = ();
}
impl PreparationBackend for OtherBackend {
    fn max_image_dimension_2d(&self) -> u32 {
        4096
    }
    fn prepare_image(
        &mut self,
        _: u32,
        _: u32,
        _: GraphFormat,
    ) -> anyhow::Result<(PhysicalImage, ())> {
        unreachable!()
    }
}
pub struct Params;
impl GPUWrite for Params {}
impl GraphShaderParams for Params {
    type Data = ();
    type Bindings = ();
    type Input = ();
    fn input(_: &(), _: &()) {}
    fn assemble_input(_: &(), _: &BindingResolver<'_>) -> Self {
        Self
    }
}
pub fn node<I: IndexedIndirectArgs>() -> IndirectDrawNode<Params, (), I> {
    draw_indexed_indirect(
        DrawIndexedIndirectKey::<NoPush>::new(0),
        UniformSlot::<Params>::from_backend(0),
        ImmutableSlot::<I>::from_backend(0, 1),
        0,
        1,
        (),
    )
}
