//! Stub of the real renderer::render_graph module
//! (src/renderer/render_graph.rs): the binding reference types, traits, and
//! resolver that generated split types compile against.

use std::marker::PhantomData;

use super::addr::{Addr, ImmutableAddr, ReadAddr};
use super::bindless::{BindlessHandle, RwTexture2D, Sampler2D};

// The graph's own GPU-data marker traits, mirroring the real
// renderer::render_graph definitions. Generated code implements these; it
// reaches the renderer-side traits only through the one-way blankets in
// super::gpu_write.
pub trait GPUWrite {}

pub trait PushConstantBlock: GPUWrite {}

impl GPUWrite for u8 {}
impl GPUWrite for f32 {}
impl GPUWrite for u32 {}

#[derive(Debug, Clone, Copy)]
pub struct SampledTexBinding;

#[derive(Debug, Clone, Copy)]
pub struct StorageTexBinding;

#[derive(Debug, Clone, Copy)]
pub struct RawBufferBinding;

pub struct BufferBinding<T>(PhantomData<fn() -> T>);

pub struct ReadBufferBinding<T>(PhantomData<fn() -> T>);

pub struct ImmutableBufferBinding<T>(PhantomData<fn() -> T>);

impl<T> Clone for BufferBinding<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for BufferBinding<T> {}
impl<T> Clone for ReadBufferBinding<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ReadBufferBinding<T> {}
impl<T> Clone for ImmutableBufferBinding<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ImmutableBufferBinding<T> {}

impl<T> std::fmt::Debug for BufferBinding<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BufferBinding")
    }
}
impl<T> std::fmt::Debug for ReadBufferBinding<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReadBufferBinding")
    }
}
impl<T> std::fmt::Debug for ImmutableBufferBinding<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImmutableBufferBinding")
    }
}

impl<T> BufferBinding<T> {
    pub fn erased(&self) -> RawBufferBinding {
        RawBufferBinding
    }
}

impl<T> ReadBufferBinding<T> {
    pub fn erased(&self) -> RawBufferBinding {
        RawBufferBinding
    }
}

impl<T> ImmutableBufferBinding<T> {
    pub fn erased(&self) -> RawBufferBinding {
        RawBufferBinding
    }
}

#[derive(Debug, Clone, Copy)]
pub enum GraphBinding {
    SampledTex(SampledTexBinding),
    StorageTex(StorageTexBinding),
    Buffer(RawBufferBinding),
}

pub trait GraphBindingSet {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding));
}

impl GraphBindingSet for () {
    fn visit(&self, _f: &mut dyn FnMut(GraphBinding)) {}
}

pub trait GraphParamBindingSet: GraphBindingSet {
    type Pending;
    fn pending() -> Self::Pending;
}

pub struct PendingParamBindings<B>(PhantomData<B>);

impl<B> PendingParamBindings<B> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

pub struct BindingResolver<'a>(PhantomData<&'a ()>);

impl BindingResolver<'_> {
    pub fn sampled_tex(&self, _binding: SampledTexBinding) -> BindlessHandle<Sampler2D> {
        unimplemented!("check_crate stub")
    }

    pub fn storage_tex(&self, _binding: StorageTexBinding) -> BindlessHandle<RwTexture2D> {
        unimplemented!("check_crate stub")
    }

    pub fn buf<T>(&self, _binding: BufferBinding<T>) -> Addr<T> {
        unimplemented!("check_crate stub")
    }

    pub fn read_buf<T>(&self, _binding: ReadBufferBinding<T>) -> ReadAddr<T> {
        unimplemented!("check_crate stub")
    }

    pub fn immutable_buf<T>(&self, _binding: ImmutableBufferBinding<T>) -> ImmutableAddr<T> {
        unimplemented!("check_crate stub")
    }
}

pub trait GraphShaderParams: Sized {
    type Data;
    type Bindings: GraphBindingSet;
    type Input: GraphBindingSet;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input;

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self;

    fn assemble(
        data: &Self::Data,
        bindings: &Self::Bindings,
        resolver: &BindingResolver<'_>,
    ) -> Self {
        Self::assemble_input(&Self::input(data, bindings), resolver)
    }
}
