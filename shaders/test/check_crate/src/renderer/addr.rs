use std::marker::PhantomData;

use serde::{Serialize, Serializer};

/// Stub of the real renderer::Addr (src/renderer/addr.rs): a typed
/// buffer device address, repr(transparent) over u64.
#[repr(transparent)]
pub struct Addr<T> {
    address: u64,
    _pointee: PhantomData<fn() -> T>,
}

impl<T> Clone for Addr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Addr<T> {}

impl<T> std::fmt::Debug for Addr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Addr({:#x})", self.address)
    }
}

impl<T> Serialize for Addr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.address)
    }
}
