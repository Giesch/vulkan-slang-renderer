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

/// Stub of the real renderer::ReadAddr (src/renderer/addr.rs): a typed
/// read-only buffer device address, repr(transparent) over u64.
#[repr(transparent)]
pub struct ReadAddr<T> {
    address: u64,
    _pointee: PhantomData<fn() -> T>,
}

impl<T> From<Addr<T>> for ReadAddr<T> {
    fn from(addr: Addr<T>) -> Self {
        Self {
            address: addr.address,
            _pointee: PhantomData,
        }
    }
}

impl<T> Clone for ReadAddr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ReadAddr<T> {}

impl<T> std::fmt::Debug for ReadAddr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReadAddr({:#x})", self.address)
    }
}

impl<T> Serialize for ReadAddr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.address)
    }
}

/// Stub of the real renderer::ImmutableAddr (src/renderer/addr.rs): a typed
/// device address for a buffer nothing on the GPU ever writes.
#[repr(transparent)]
pub struct ImmutableAddr<T> {
    address: u64,
    _pointee: PhantomData<fn() -> T>,
}

impl<T> From<ImmutableAddr<T>> for ReadAddr<T> {
    fn from(addr: ImmutableAddr<T>) -> Self {
        Self {
            address: addr.address,
            _pointee: PhantomData,
        }
    }
}

impl<T> Clone for ImmutableAddr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ImmutableAddr<T> {}

impl<T> std::fmt::Debug for ImmutableAddr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImmutableAddr({:#x})", self.address)
    }
}

impl<T> Serialize for ImmutableAddr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.address)
    }
}
