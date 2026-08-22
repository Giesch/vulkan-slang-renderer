use std::marker::PhantomData;

use serde::{Serialize, Serializer};

use super::descriptor_heap::BindlessIndex;

/// A texture's slot in the bindless descriptor heap, as the shader sees it.
/// Rust-side counterpart of Slang's `DescriptorHandle<T>` (eg. `Sampler2D.Handle`).
///
/// Slang lowers a handle to a `uint2`. Only the low 32 bits carry the slot.
///
/// The compiler decorates every heap access `NonUniform`, so a handle may
/// vary within a draw. A uniform handle avoids the waterfall loop.
#[repr(transparent)]
pub struct BindlessHandle<T> {
    raw: u64,
    // fn() -> T keeps BindlessHandle<T> Send/Sync/Copy regardless of T
    _shape: PhantomData<fn() -> T>,
}

impl<T> BindlessHandle<T> {
    pub(super) fn from_slot(slot: BindlessIndex) -> Self {
        Self {
            raw: u64::from(slot.to_raw()),
            _shape: PhantomData,
        }
    }

    pub fn to_raw(self) -> u64 {
        self.raw
    }
}

/// Marker for `Sampler2D.Handle`
///
/// In the future, there may be other markers for, eg, 3D textures
pub enum Sampler2D {}

/// Marker for `RWTexture2D.Handle`
pub enum RwTexture2D {}

// manual impls: derives would add spurious `T: ...` bounds
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
