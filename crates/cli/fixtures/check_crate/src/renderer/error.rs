//! Stand-in for the renderer's `error` module: only what the production
//! `gpu_read.rs` (included by path in `mod.rs`) imports from `super::error`.
//! The real module also holds Vulkan- and slang-facing errors this fixture
//! cannot link. Keep this enum's variants in sync with
//! `crates/renderer/src/renderer/error.rs`.

/// A readback's element count and stride do not describe a readable range of its buffer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidReadbackRange {
    #[error("empty GPU readback ({count} elements of stride {stride})")]
    Empty { count: usize, stride: usize },
    #[error("GPU readback stride {gpu} differs from allocation stride {rust}")]
    StrideMismatch { gpu: usize, rust: usize },
    #[error("GPU readback byte size overflow ({count} elements of stride {stride})")]
    Overflow { count: usize, stride: usize },
    #[error("GPU readback of {len} bytes exceeds slice size limit")]
    ExceedsSliceLimit { len: usize },
    #[error("GPU readback of {len} bytes exceeds buffer capacity {capacity}")]
    ExceedsCapacity { len: usize, capacity: u64 },
}
