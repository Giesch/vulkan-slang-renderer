//! Checked decoding of owned values from little-endian shader storage bytes.
//!
//! Matrix decoding preserves the engine's upload byte representation, not the
//! same mathematical row/column indices: Slang is compiled row-major, so each
//! shader matrix row becomes a glam column. This reverses the existing glam
//! upload representation. Use an explicit transpose when same-index mathematical
//! reconstruction is required. Packed matrix/vector codecs do not infer padding;
//! generated field layouts must match their declared byte sizes.

use eros::IntoUnion as _;

/// A readback buffer's byte length did not match the decoded type's GPU size.
#[derive(Debug, thiserror::Error)]
#[error("invalid GPU readback byte length for {type_name}: expected {expected}, got {actual}")]
pub struct GpuByteLengthMismatch {
    pub type_name: &'static str,
    pub expected: usize,
    pub actual: usize,
}

/// A GPU enum discriminant did not name any variant.
#[derive(Debug, thiserror::Error)]
#[error("invalid {type_name} tag: {tag}")]
pub struct InvalidGpuEnumTag {
    pub type_name: &'static str,
    pub tag: i128,
}

pub type GpuReadErrors = (GpuByteLengthMismatch, InvalidGpuEnumTag);

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

/// Decode one shader value without interpreting arbitrary bytes as a Rust value.
///
/// `GPU_SIZE` is the storage array stride, including shader padding. Implementors
/// must validate field values (especially enum discriminants) before constructing
/// Rust values. Generated implementations support pure data, not GPU addresses.
pub trait GPURead: Sized {
    const GPU_SIZE: usize;
    fn read_gpu(bytes: &[u8]) -> eros::Result<Self, GpuReadErrors>;
}

/// The length check every decoder starts with.
fn check_len<T: GPURead>(bytes: &[u8]) -> eros::Result<(), GpuReadErrors> {
    if bytes.len() == T::GPU_SIZE {
        Ok(())
    } else {
        Err(GpuByteLengthMismatch {
            type_name: std::any::type_name::<T>(),
            expected: T::GPU_SIZE,
            actual: bytes.len(),
        })
        .union()
    }
}

macro_rules! scalar {
    ($($ty:ty),* $(,)?) => {$ (
        impl GPURead for $ty {
            const GPU_SIZE: usize = size_of::<Self>();

            fn read_gpu(bytes: &[u8]) -> eros::Result<Self, GpuReadErrors> {
                check_len::<Self>(bytes)?;
                let array = bytes.try_into().map_err(|_| GpuByteLengthMismatch {
                    type_name: std::any::type_name::<Self>(),
                    expected: Self::GPU_SIZE,
                    actual: bytes.len(),
                });

                Ok(Self::from_le_bytes(array.union()?))
            }
        }
    )* };
}
// including only primitives that implement GPUWrite or are allowed scalar fields
scalar!(u8, u32, i32, u64, f32);

impl<T: GPURead, const N: usize> GPURead for [T; N] {
    const GPU_SIZE: usize = match T::GPU_SIZE.checked_mul(N) {
        Some(size) => size,
        None => panic!("GPU array size overflow"),
    };

    fn read_gpu(bytes: &[u8]) -> eros::Result<Self, GpuReadErrors> {
        check_len::<Self>(bytes)?;
        const { assert!(T::GPU_SIZE != 0, "zero GPU element stride") };

        // `as_chunks` needs a const generic argument,
        // and an associated const of a type parameter cannot be one on stable.
        let values = bytes
            .chunks_exact(T::GPU_SIZE)
            .map(T::read_gpu)
            .collect::<eros::Result<Vec<_>, GpuReadErrors>>()?;

        // unreachable after the length check, but reported rather than panicking
        values
            .try_into()
            .map_err(|values: Vec<T>| GpuByteLengthMismatch {
                type_name: std::any::type_name::<Self>(),
                expected: Self::GPU_SIZE,
                actual: values.len() * T::GPU_SIZE,
            })
            .union()
    }
}

macro_rules! vector {
    ($ty:ty, $scalar:ty, $n:expr, $stride:expr) => {
        impl GPURead for $ty {
            const GPU_SIZE: usize = $stride;

            fn read_gpu(bytes: &[u8]) -> eros::Result<Self, GpuReadErrors> {
                check_len::<Self>(bytes)?;
                let values = <[$scalar; $n]>::read_gpu(&bytes[..size_of::<$scalar>() * $n])?;

                Ok(Self::from_array(values))
            }
        }
    };
}
// 3-component vectors are deliberately unsupported: std430 stride 16 != size 12
vector!(glam::Vec2, f32, 2, 8);
vector!(glam::Vec4, f32, 4, 16);
vector!(glam::IVec2, i32, 2, 8);
vector!(glam::IVec4, i32, 4, 16);
vector!(glam::UVec2, u32, 2, 8);
vector!(glam::UVec4, u32, 4, 16);

macro_rules! matrix {
    ($ty:ty, $n:expr) => {
        impl GPURead for $ty {
            const GPU_SIZE: usize = $n * 4;

            fn read_gpu(bytes: &[u8]) -> eros::Result<Self, GpuReadErrors> {
                Ok(Self::from_cols_array(&<[f32; $n]>::read_gpu(bytes)?))
            }
        }
    };
}
// smaller matrices are deliberately unsupported
matrix!(glam::Mat4, 16);

pub(super) fn readback_byte_len(
    count: usize,
    stride: usize,
    rust_stride: usize,
    capacity: u64,
) -> Result<usize, InvalidReadbackRange> {
    if count == 0 || stride == 0 {
        return Err(InvalidReadbackRange::Empty { count, stride });
    }
    if stride != rust_stride {
        return Err(InvalidReadbackRange::StrideMismatch {
            gpu: stride,
            rust: rust_stride,
        });
    }
    let len = count
        .checked_mul(stride)
        .ok_or(InvalidReadbackRange::Overflow { count, stride })?;
    if len > isize::MAX as usize {
        return Err(InvalidReadbackRange::ExceedsSliceLimit { len });
    }
    if u64::try_from(len).map_or(true, |len| len > capacity) {
        return Err(InvalidReadbackRange::ExceedsCapacity { len, capacity });
    }

    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_scalar_array_and_vector_decode() {
        assert_eq!(u32::read_gpu(&[4, 3, 2, 1]).unwrap(), 0x01020304);
        assert!(u32::read_gpu(&[0; 3]).is_err());
        assert!(u32::read_gpu(&[0; 5]).is_err());
        assert_eq!(
            <[u32; 2]>::read_gpu(&[1, 0, 0, 0, 2, 0, 0, 0]).unwrap(),
            [1, 2]
        );
        assert!(<[u32; 2]>::read_gpu(&[0; 7]).is_err());
        assert_eq!(glam::Vec4::read_gpu(&[0; 16]).unwrap(), glam::Vec4::ZERO);
        assert_eq!(glam::Mat4::read_gpu(&[0; 64]).unwrap(), glam::Mat4::ZERO);
        assert_eq!(<[u32; 0]>::read_gpu(&[]).unwrap(), [0_u32; 0]);
    }

    #[test]
    fn length_mismatch_is_typed_and_names_the_type() {
        let error = <[u32; 2]>::read_gpu(&[0; 7]).unwrap_err();
        let mismatch = error
            .downcast_inner_ref::<GpuByteLengthMismatch>()
            .expect("a byte length mismatch");
        assert_eq!(mismatch.type_name, "[u32; 2]");
        assert_eq!((mismatch.expected, mismatch.actual), (8, 7));

        // a vector's stride is checked, not only its component bytes
        let error = glam::Vec4::read_gpu(&[0; 12]).unwrap_err();
        assert!(error.is_inner::<GpuByteLengthMismatch>());
    }

    #[test]
    fn readback_range_errors_are_typed() {
        assert_eq!(
            readback_byte_len(3, 4, 4, 8),
            Err(InvalidReadbackRange::ExceedsCapacity {
                len: 12,
                capacity: 8
            })
        );
        assert_eq!(
            readback_byte_len(1, 4, 8, 8),
            Err(InvalidReadbackRange::StrideMismatch { gpu: 4, rust: 8 })
        );
    }

    #[test]
    fn readback_rejects_invalid_ranges() {
        assert_eq!(readback_byte_len(2, 4, 4, 8).unwrap(), 8);
        for (count, stride, rust_stride, capacity) in [
            (0, 4, 4, 8),
            (1, 0, 0, 8),
            (1, 4, 8, 8),
            (3, 4, 4, 8),
            (usize::MAX, 4, 4, u64::MAX),
            (isize::MAX as usize + 1, 1, 1, u64::MAX),
        ] {
            assert!(readback_byte_len(count, stride, rust_stride, capacity).is_err());
        }
    }
}
