pub use mltrs_render_graph::bindless::{BindlessHandle, RwTexture2D, Sampler2D};

use super::descriptor_heap::BindlessIndex;

pub(super) fn from_slot<T>(slot: BindlessIndex) -> BindlessHandle<T> {
    BindlessHandle::from_raw(u64::from(slot.to_raw()))
}
