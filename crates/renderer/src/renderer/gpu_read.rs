//! Checked decoding of owned values from little-endian shader storage bytes.
//!
//! Matrix decoding preserves the engine's upload byte representation, not the
//! same mathematical row/column indices: Slang is compiled row-major, so each
//! shader matrix row becomes a glam column. This reverses the existing glam
//! upload representation. Use an explicit transpose when same-index mathematical
//! reconstruction is required. Packed matrix/vector codecs do not infer padding;
//! generated field layouts must match their declared byte sizes.

/// Decode one shader value without interpreting arbitrary bytes as a Rust value.
///
/// `GPU_SIZE` is the storage array stride, including shader padding. Implementors
/// must validate field values (especially enum discriminants) before constructing
/// Rust values. Generated implementations support pure data, not GPU addresses.
pub trait GPURead: Sized {
    const GPU_SIZE: usize;
    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self>;
}

macro_rules! scalar {
    ($($ty:ty),* $(,)?) => {$ (
        impl GPURead for $ty {
            const GPU_SIZE: usize = size_of::<Self>();

            fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
                anyhow::ensure!(bytes.len() == Self::GPU_SIZE, "incorrect GPU scalar byte length");

                Ok(Self::from_le_bytes(bytes.try_into()?))
            }
        }
    )* };
}
scalar!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

impl<T: GPURead, const N: usize> GPURead for [T; N] {
    const GPU_SIZE: usize = match T::GPU_SIZE.checked_mul(N) {
        Some(size) => size,
        None => panic!("GPU array size overflow"),
    };

    fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == Self::GPU_SIZE,
            "incorrect GPU array byte length"
        );
        anyhow::ensure!(T::GPU_SIZE != 0, "zero GPU element stride");
        // `as_chunks` needs a const generic argument, and an associated const
        // of a type parameter cannot be one on stable.
        #[allow(clippy::chunks_exact_to_as_chunks)]
        let values = bytes
            .chunks_exact(T::GPU_SIZE)
            .map(T::read_gpu)
            .collect::<anyhow::Result<Vec<_>>>()?;

        values
            .try_into()
            .map_err(|_| anyhow::anyhow!("incorrect GPU array element count"))
    }
}

macro_rules! vector {
    ($ty:ty, $scalar:ty, $n:expr, $stride:expr) => {
        impl GPURead for $ty {
            const GPU_SIZE: usize = $stride;

            fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
                anyhow::ensure!(
                    bytes.len() == Self::GPU_SIZE,
                    "incorrect GPU vector byte length"
                );
                let values = <[$scalar; $n]>::read_gpu(&bytes[..size_of::<$scalar>() * $n])?;

                Ok(Self::from_array(values))
            }
        }
    };
}
vector!(glam::Vec2, f32, 2, 8);
vector!(glam::Vec3, f32, 3, 12);
vector!(glam::Vec3A, f32, 3, 16);
vector!(glam::Vec4, f32, 4, 16);
vector!(glam::IVec2, i32, 2, 8);
vector!(glam::IVec3, i32, 3, 12);
vector!(glam::IVec4, i32, 4, 16);
vector!(glam::UVec2, u32, 2, 8);
vector!(glam::UVec3, u32, 3, 12);
vector!(glam::UVec4, u32, 4, 16);

macro_rules! matrix {
    ($ty:ty, $n:expr) => {
        impl GPURead for $ty {
            const GPU_SIZE: usize = $n * 4;

            fn read_gpu(bytes: &[u8]) -> anyhow::Result<Self> {
                Ok(Self::from_cols_array(&<[f32; $n]>::read_gpu(bytes)?))
            }
        }
    };
}
matrix!(glam::Mat2, 4);
matrix!(glam::Mat3, 9);
matrix!(glam::Mat4, 16);

pub(super) fn readback_byte_len(
    count: usize,
    stride: usize,
    rust_stride: usize,
    capacity: u64,
) -> anyhow::Result<usize> {
    anyhow::ensure!(count != 0 && stride != 0, "empty GPU readback");
    anyhow::ensure!(
        stride == rust_stride,
        "GPU readback stride differs from allocation stride"
    );
    let len = count
        .checked_mul(stride)
        .ok_or_else(|| anyhow::anyhow!("GPU readback byte size overflow"))?;
    anyhow::ensure!(
        len <= isize::MAX as usize,
        "GPU readback exceeds slice size limit"
    );
    anyhow::ensure!(
        u64::try_from(len)? <= capacity,
        "GPU readback exceeds buffer capacity"
    );

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
        assert_eq!(<[u16; 2]>::read_gpu(&[1, 0, 2, 0]).unwrap(), [1, 2]);
        assert!(<[u16; 2]>::read_gpu(&[0; 3]).is_err());
        assert_eq!(glam::Vec3::read_gpu(&[0; 12]).unwrap(), glam::Vec3::ZERO);
        assert_eq!(glam::Mat4::read_gpu(&[0; 64]).unwrap(), glam::Mat4::ZERO);
        assert_eq!(<[u32; 0]>::read_gpu(&[]).unwrap(), [0_u32; 0]);
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
