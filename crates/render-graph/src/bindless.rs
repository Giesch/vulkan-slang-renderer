use serde::{Serialize, Serializer};
use std::marker::PhantomData;

/// A texture descriptor slot as seen by the shader.
/// Slang lowers this to a uint2; only the low 32 bits carry the slot.
#[repr(transparent)]
pub struct BindlessHandle<T> {
    raw: u64,
    _shape: PhantomData<fn() -> T>,
}

impl<T> BindlessHandle<T> {
    /// Construct backend descriptor metadata without validating resource liveness.
    /// The backend must ensure the slot has the descriptor shape `T`.
    pub fn from_raw(raw: u64) -> Self {
        Self {
            raw,
            _shape: PhantomData,
        }
    }

    pub fn to_raw(self) -> u64 {
        self.raw
    }
}

/// Marker for Sampler2D.Handle.
pub enum Sampler2D {}

/// Marker for RWTexture2D.Handle.
pub enum RwTexture2D {}

impl<T> Clone for BindlessHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for BindlessHandle<T> {}

impl<T> std::fmt::Debug for BindlessHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BindlessHandle<{}>({})",
            std::any::type_name::<T>(),
            self.raw
        )
    }
}

impl<T> Serialize for BindlessHandle<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.raw)
    }
}

const _: () = assert!(std::mem::size_of::<BindlessHandle<()>>() == 8);
const _: () = assert!(std::mem::align_of::<BindlessHandle<()>>() == 8);
