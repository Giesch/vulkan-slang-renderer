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
    pub fn from_raw(address: u64) -> Self {
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
