use std::marker::PhantomData;

use serde::{Serialize, Serializer};

/// A GPU buffer device address pointing at std430-laid-out `T`s.
/// Rust-side counterpart of the Slang `Addr<T>` alias
/// (= `LayoutPtr<T, Std430DataLayout>`, shaders/source/addr.slang).
/// Exactly 8 bytes; written into uniform structs as a raw address.
#[repr(transparent)]
pub struct Addr<T> {
    address: u64,
    // fn() -> T keeps Addr<T> Send/Sync/Copy regardless of T
    _pointee: PhantomData<fn() -> T>,
}

impl<T> Addr<T> {
    pub(super) fn from_raw(address: u64) -> Self {
        Self {
            address,
            _pointee: PhantomData,
        }
    }

    pub fn to_raw(self) -> u64 {
        self.address
    }
}

// manual impls: derives would add spurious `T: ...` bounds
impl<T> Clone for Addr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Addr<T> {}

impl<T> std::fmt::Debug for Addr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Addr<{}>({:#x})",
            std::any::type_name::<T>(),
            self.address
        )
    }
}

impl<T> Serialize for Addr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.address)
    }
}

const _: () = assert!(std::mem::size_of::<Addr<()>>() == 8);
const _: () = assert!(std::mem::align_of::<Addr<()>>() == 8);

/// Read-only counterpart of Addr<T>
///
/// This is distinct from ImmutableAddr<T>;
/// ReadAddr only promises that *this shader* cannot write through *this pointer*
///
/// matches Slang's ReadAddr<T> (shaders/source/addr.slang).
#[repr(transparent)]
pub struct ReadAddr<T> {
    address: u64,
    // fn() -> T keeps ReadAddr<T> Send/Sync/Copy regardless of T
    _pointee: PhantomData<fn() -> T>,
}

impl<T> ReadAddr<T> {
    pub(super) fn from_raw(address: u64) -> Self {
        Self {
            address,
            _pointee: PhantomData,
        }
    }

    pub fn to_raw(self) -> u64 {
        self.address
    }
}

impl<T> From<Addr<T>> for ReadAddr<T> {
    fn from(addr: Addr<T>) -> Self {
        Self {
            address: addr.address,
            _pointee: PhantomData,
        }
    }
}

// manual impls: derives would add spurious `T: ...` bounds
impl<T> Clone for ReadAddr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ReadAddr<T> {}

impl<T> std::fmt::Debug for ReadAddr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ReadAddr<{}>({:#x})",
            std::any::type_name::<T>(),
            self.address
        )
    }
}

impl<T> Serialize for ReadAddr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.address)
    }
}

const _: () = assert!(std::mem::size_of::<ReadAddr<()>>() == 8);
const _: () = assert!(std::mem::align_of::<ReadAddr<()>>() == 8);

/// A pointer to a buffer that the GPU never writes to
///
/// The CPU may still update the buffer between frames
/// emits SPIR-V Restrict, so it must not change during a shader execution (UB otherwise)
///
/// matches Slang's Addr<T> (shaders/source/addr.slang)
#[repr(transparent)]
pub struct ImmutableAddr<T> {
    address: u64,
    // fn() -> T keeps ImmutableAddr<T> Send/Sync/Copy regardless of T
    _pointee: PhantomData<fn() -> T>,
}

impl<T> ImmutableAddr<T> {
    // pub(crate): minting is restricted to Renderer/Gpu accessors that take
    // an ImmutableBufferHandle, which upholds the never-GPU-written invariant
    // Access.Immutable requires.
    pub(super) fn from_raw(address: u64) -> Self {
        Self {
            address,
            _pointee: PhantomData,
        }
    }

    pub fn to_raw(self) -> u64 {
        self.address
    }
}

// a buffer the GPU never writes to is safe to read via Access.Read
impl<T> From<ImmutableAddr<T>> for ReadAddr<T> {
    fn from(addr: ImmutableAddr<T>) -> Self {
        Self {
            address: addr.address,
            _pointee: PhantomData,
        }
    }
}

// manual impls: derives would add spurious `T: ...` bounds
impl<T> Clone for ImmutableAddr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ImmutableAddr<T> {}

impl<T> std::fmt::Debug for ImmutableAddr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ImmutableAddr<{}>({:#x})",
            std::any::type_name::<T>(),
            self.address
        )
    }
}

impl<T> Serialize for ImmutableAddr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.address)
    }
}

const _: () = assert!(std::mem::size_of::<ImmutableAddr<()>>() == 8);
const _: () = assert!(std::mem::align_of::<ImmutableAddr<()>>() == 8);
