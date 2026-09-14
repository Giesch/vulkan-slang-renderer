//! BCK (J3D1bck1 / ANK1) decoding: Link's body animation.
//!
//! BCK stores keyframed joint transforms (`J3DAnmTransformKey`); its full
//! acronym expansion is unverified (B likely means Binary, K means Key).
//! ANK1 can be read as Animation Key, version 1 (inferred expansion).
//!
//! Layout notes (verified against the GZLE01 data and the J3D loader):
//! - File header is 0x20 bytes; the u32 at 0x1C is the BAS sound-trailer
//!   offset or the 0xFFFFFFFF "absent" sentinel. The trailer lives *after*
//!   the declared chunk, so chunk-end != file-end here; its bytes are never
//!   interpreted, only located.
//! - ANK1 (single chunk at 0x20): loop u8@0x08, decimal shift u8@0x09,
//!   duration u16@0x0A, joint count u16@0x0C, pool element counts
//!   u16@0x0E/0x10/0x12, then chunk-relative offsets: tables u32@0x14,
//!   scale pool u32@0x18, rotation pool u32@0x1C, translation pool u32@0x20.
//! - The table is axis-major: joint *j* occupies entries `3j`, `3j+1`,
//!   `3j+2` (axes x, y, z), each entry 0x12 bytes holding that axis's
//!   {scale, rotation, translation} key descriptors. See `calcTransform`
//!   in J3DAnimation.cpp.

use anyhow::{Context, Result, anyhow, bail};

use gx::animation_manifest::{BasMetadata, BckClip, BckJoint};

use super::tracks::AxisDescriptors;

pub(crate) const SOUND_ABSENT: u32 = 0xFFFF_FFFF;

pub fn parse(data: &[u8], what: &str) -> Result<BckClip> {
    let header = super::j3d_single_chunk(data, what, "bck1", b"ANK1")
        .with_context(|| format!("{what}: not a readable BCK"))?;
    let chunk = super::chunk_slice(data, &header);

    let joint_count = super::u16_at(chunk, 0x0C, "ANK1 joint count")? as usize;
    let (scale_count, rot_count, trans_count) = (
        super::u16_at(chunk, 0x0E, "ANK1 scale pool count")? as usize,
        super::u16_at(chunk, 0x10, "ANK1 rotation pool count")? as usize,
        super::u16_at(chunk, 0x12, "ANK1 translation pool count")? as usize,
    );
    let table_off = super::u32_at(chunk, 0x14, "ANK1 table offset")? as usize;
    let scale_off = super::u32_at(chunk, 0x18, "ANK1 scale pool offset")? as usize;
    let rot_off = super::u32_at(chunk, 0x1C, "ANK1 rotation pool offset")? as usize;
    let trans_off = super::u32_at(chunk, 0x20, "ANK1 translation pool offset")? as usize;

    let scale_pool = super::pool(chunk, scale_off, scale_count * 4, "scale", what)?;
    let rot_pool = super::pool(chunk, rot_off, rot_count * 2, "rotation", what)?;
    let trans_pool = super::pool(chunk, trans_off, trans_count * 4, "translation", what)?;

    // 42 joints -> 126 table entries; reject counts that cannot be triplets.
    if joint_count == 0 || joint_count > 512 {
        bail!("{what}: implausible joint count {joint_count}");
    }
    let table_len = joint_count
        .checked_mul(3)
        .and_then(|n| n.checked_mul(0x12))
        .ok_or_else(|| anyhow!("{what}: joint count overflows"))?;
    let table = super::pool(chunk, table_off, table_len, "key table", what)?;

    let mut joints = Vec::with_capacity(joint_count);
    for j in 0..joint_count {
        let mut axes = Vec::with_capacity(3);
        for axis in 0..3 {
            let what_track = format!("{what}: joint {j} axis {axis}");
            let off = (j * 3 + axis) * 0x12;
            let desc = AxisDescriptors::read(table, off).with_context(|| what_track.clone())?;
            // Reachability check before decoding: each descriptor's index must
            // lie inside its pool so the error names the pool, not the word.
            for (desc, pool_len, elem, kind) in [
                (desc.scale, scale_count, 4usize, "scale"),
                (desc.rotation, rot_count, 2, "rotation"),
                (desc.translation, trans_count, 4, "translation"),
            ] {
                super::check_descriptor(desc, pool_len, elem, &format!("{what_track} {kind}"))?;
            }
            axes.push(desc.decode(scale_pool, rot_pool, trans_pool, &what_track)?);
        }
        let axes = <[_; 3]>::try_from(axes).expect("three axes per joint");
        joints.push(BckJoint {
            ordinal: j as u16,
            axes,
        });
    }

    let bas = bas_metadata(data, what)?;
    Ok(BckClip {
        duration_frames: super::u16_at(chunk, 0x0A, "ANK1 duration")?,
        loop_attribute: super::u8_at(chunk, 0x08, "ANK1 loop attribute")?,
        rotation_decimal_shift: super::u8_at(chunk, 0x09, "ANK1 decimal shift")?,
        joints,
        bas,
    })
}

/// BAS trailer location metadata. Validates the u16 entry count and the
/// `8 + count * 0x20` extent against the file when present, and that the
/// trailer starts after the declared chunk (matching the extraction-side
/// rule; a "trailer" inside the chunk is corruption).
pub(crate) fn bas_metadata(data: &[u8], what: &str) -> Result<BasMetadata> {
    let sound_off = u32::from_be_bytes([data[0x1C], data[0x1D], data[0x1E], data[0x1F]]);
    if sound_off == SOUND_ABSENT {
        return Ok(BasMetadata {
            present: false,
            offset: 0,
            length: 0,
        });
    }

    let off = sound_off as u64;
    if off + 2 > data.len() as u64 {
        bail!("{what}: BAS offset {off:#x} out of bounds");
    }

    let count = u16::from_be_bytes([data[sound_off as usize], data[sound_off as usize + 1]]) as u64;
    let length = 8u64
        .checked_add(
            count
                .checked_mul(0x20)
                .ok_or_else(|| anyhow!("{what}: BAS count overflows"))?,
        )
        .ok_or_else(|| anyhow!("{what}: BAS length overflows"))?;
    if off + length > data.len() as u64 {
        bail!(
            "{what}: BAS trailer at {off:#x} spans {length} bytes, past the {}-byte file",
            data.len()
        );
    }
    let declared_chunk_end =
        0x20u64 + u32::from_be_bytes([data[0x24], data[0x25], data[0x26], data[0x27]]) as u64;
    if off < declared_chunk_end {
        bail!(
            "{what}: BAS trailer at {off:#x} starts inside the declared chunk (ends {declared_chunk_end:#x})"
        );
    }

    Ok(BasMetadata {
        present: true,
        offset: sound_off,
        length: length as u32,
    })
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// Minimal ANK1 BCK: one joint, S=(default), R=constant, T=constant.
    pub(crate) fn build_bck() -> Vec<u8> {
        let mut chunk: Vec<u8> = vec![0; 0x24];
        chunk[0..4].copy_from_slice(b"ANK1");
        chunk.extend_from_slice(&[0u8; 0x1C]); // pad header to 0x40
        let table_off = chunk.len();
        // joint 0 axes x,y,z: each {S: default, R: constant idx 0, T: constant idx 0}
        for _ in 0..3 {
            chunk.extend_from_slice(&0u16.to_be_bytes()); // S count 0
            chunk.extend_from_slice(&0u16.to_be_bytes());
            chunk.extend_from_slice(&0u16.to_be_bytes());
            chunk.extend_from_slice(&1u16.to_be_bytes()); // R count 1 idx 0
            chunk.extend_from_slice(&0u16.to_be_bytes());
            chunk.extend_from_slice(&0u16.to_be_bytes());
            chunk.extend_from_slice(&1u16.to_be_bytes()); // T count 1 idx 0
            chunk.extend_from_slice(&0u16.to_be_bytes());
            chunk.extend_from_slice(&0u16.to_be_bytes());
        }
        let rot_off = chunk.len();
        chunk.extend_from_slice(&0x4000i16.to_be_bytes());
        let trans_off = chunk.len();
        chunk.extend_from_slice(&5.5f32.to_bits().to_be_bytes());
        // pad pools to 0x20 alignment
        while !chunk.len().is_multiple_of(0x20) {
            chunk.push(0);
        }
        let csize = chunk.len();
        // fill header
        chunk[4..8].copy_from_slice(&(csize as u32).to_be_bytes());
        chunk[8] = 2; // loop
        chunk[9] = 0; // dec shift
        chunk[0x0A..0x0C].copy_from_slice(&6u16.to_be_bytes()); // duration
        chunk[0x0C..0x0E].copy_from_slice(&1u16.to_be_bytes()); // joints
        chunk[0x0E..0x10].copy_from_slice(&0u16.to_be_bytes()); // scale count
        chunk[0x10..0x12].copy_from_slice(&1u16.to_be_bytes()); // rot count
        chunk[0x12..0x14].copy_from_slice(&1u16.to_be_bytes()); // trans count
        chunk[0x14..0x18].copy_from_slice(&(table_off as u32).to_be_bytes());
        chunk[0x18..0x1C].copy_from_slice(&0u32.to_be_bytes()); // scale pool (empty)
        chunk[0x1C..0x20].copy_from_slice(&(rot_off as u32).to_be_bytes());
        chunk[0x20..0x24].copy_from_slice(&(trans_off as u32).to_be_bytes());

        let mut file = vec![0u8; 0x20];
        file[0..4].copy_from_slice(b"J3D1");
        file[4..8].copy_from_slice(b"bck1");
        let file_len = 0x20 + csize;
        file[8..12].copy_from_slice(&(file_len as u32).to_be_bytes());
        file[12..16].copy_from_slice(&1u32.to_be_bytes());
        file[0x1C..0x20].copy_from_slice(&SOUND_ABSENT.to_be_bytes());
        file.extend_from_slice(&chunk);
        file
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::build_bck;
    use super::*;
    use gx::animation_manifest::{TrackF32, TrackI16};

    #[test]
    fn bck_synthetic_cases() {
        let data = build_bck();
        let clip = parse(&data, "synthetic.bck").unwrap();
        assert_eq!(clip.duration_frames, 6);
        assert_eq!(clip.loop_attribute, 2);
        assert_eq!(clip.joints.len(), 1);
        assert_eq!(clip.joints[0].ordinal, 0);
        for (axis, a) in clip.joints[0].axes.iter().enumerate() {
            assert!(matches!(a.scale, TrackF32::Default), "axis {axis}");
            assert_eq!(a.rotation, TrackI16::Constant { value: 0x4000 });
            assert_eq!(a.translation, TrackF32::Constant { value: 5.5 });
        }
        assert!(!clip.bas.present);
    }

    #[test]
    fn bck_bas_present_and_truncated() {
        let mut data = build_bck();
        let off = data.len();
        data[0x1C..0x20].copy_from_slice(&(off as u32).to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&[0u8; 6]); // rest of the 8-byte BAS header
        data.extend_from_slice(&[0u8; 0x20]); // one 0x20-byte entry
        // The J3D length field covers the BAS trailer too (gclib's writer
        // does the same), so update it after appending.
        let file_len = data.len();
        data[8..12].copy_from_slice(&(file_len as u32).to_be_bytes());
        let clip = parse(&data, "synthetic-bas.bck").unwrap();
        assert!(clip.bas.present);
        assert_eq!(clip.bas.offset, off as u32);
        assert_eq!(clip.bas.length, 8 + 0x20);

        // truncated trailer: count says 2 entries but only one is present
        let mut bad = data.clone();
        bad[off..off + 2].copy_from_slice(&2u16.to_be_bytes());
        let err = parse(&bad, "trunc.bck").unwrap_err();
        assert!(err.to_string().contains("BAS"), "{err}");
    }

    #[test]
    fn bck_malformed_inputs() {
        let good = build_bck();
        // wrong file type
        let mut bad = good.clone();
        bad[4..8].copy_from_slice(b"btp1");
        assert!(parse(&bad, "w.bck").is_err());
        // wrong chunk
        let mut bad = good.clone();
        bad[0x20..0x24].copy_from_slice(b"TPT1");
        assert!(parse(&bad, "w.bck").is_err());
        // header length lies
        let mut bad = good.clone();
        bad[8..12].copy_from_slice(&999u32.to_be_bytes());
        assert!(parse(&bad, "w.bck").is_err());
        // pool offset out of chunk
        let mut bad = good.clone();
        bad[0x20 + 0x20..0x20 + 0x24].copy_from_slice(&0xFF00u32.to_be_bytes());
        assert!(parse(&bad, "w.bck").is_err());
        // joint count claiming a table past the chunk
        let mut bad = good.clone();
        bad[0x20 + 0x0C..0x20 + 0x0E].copy_from_slice(&200u16.to_be_bytes());
        assert!(parse(&bad, "w.bck").is_err());
    }
}
