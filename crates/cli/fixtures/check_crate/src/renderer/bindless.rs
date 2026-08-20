use std::marker::PhantomData;

use serde::{Serialize, Serializer};

/// Stub of the real renderer::BindlessHandle (src/renderer/bindless.rs): a
/// typed bindless heap slot, repr(transparent) over u64.
#[repr(transparent)]
pub struct BindlessHandle<T> {
    raw: u64,
    _shape: PhantomData<fn() -> T>,
}

impl<T> Clone for BindlessHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for BindlessHandle<T> {}

impl<T> std::fmt::Debug for BindlessHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BindlessHandle({})", self.raw)
    }
}

impl<T> Serialize for BindlessHandle<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.raw)
    }
}

/// Stub of the real renderer::Sampler2D marker.
pub enum Sampler2D {}

/// Stub of the real renderer::RwTexture2D marker.
pub enum RwTexture2D {}
