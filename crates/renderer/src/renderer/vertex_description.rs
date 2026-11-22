use std::marker::PhantomData;

use ash::vk;

pub use super::gpu_write::NoVertex;

/// The layout of a real vertex element type, as generated from a shader's
/// vertex input. Uploading a `Vec<V>` of these requires the renderer's GPU
/// write contract as well.
pub trait VertexDescription: super::GPUWrite {
    fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription>;
    fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription>;
}

/// The vertex interface of an indexed pipeline whose vertex input is the
/// generated vertex type `V`.
///
/// This is an interface marker, not a vertex type: it is never constructed,
/// uploaded, or described. It lets a pipeline config and its returned graphics
/// handle remember which vertex type the pipeline was built for.
pub struct TypedVertex<V>(PhantomData<fn() -> V>);

// not derived so we don't require `V: Debug`
impl<V> std::fmt::Debug for TypedVertex<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TypedVertex")
    }
}

/// The vertex interface of an indexed pipeline fed with vertex bytes already
/// packed to the shader entry's reflected vertex layout. There is no Rust
/// vertex type; the shader entry is the only source of the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeVertex;

mod sealed {
    pub trait Sealed {}
}

/// The vertex interfaces that indexed draws accept: [`TypedVertex`] and
/// [`RuntimeVertex`], but not [`NoVertex`]. Sealed, so the set of indexed
/// interfaces is exactly the set of indexed pipeline constructors.
pub trait IndexedVertexInterface: sealed::Sealed {}

impl<V: VertexDescription> sealed::Sealed for TypedVertex<V> {}

impl<V: VertexDescription> IndexedVertexInterface for TypedVertex<V> {}

impl sealed::Sealed for RuntimeVertex {}

impl IndexedVertexInterface for RuntimeVertex {}
