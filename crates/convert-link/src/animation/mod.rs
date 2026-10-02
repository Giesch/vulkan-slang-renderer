//! J3D animation decoding: BCK (body), BTP (face texture patterns), BTK
//! (face texture SRT). One module per format plus shared key-table machinery
//! in [`tracks`]; [`output`] emits the shared `gx::animation_manifest`
//! documents. The J3D binary conventions are documented per module and were
//! verified against the GZLE01 LkAnm/LkD00/LkD01 archives and the Wind Waker
//! decompilation (`J3DAnmLoader.cpp`, `J3DAnimation.cpp`).

pub mod bck;
pub mod btk;
pub mod btp;
pub mod output;
pub mod tracks;

use anyhow::{Result, anyhow, bail};

use gx::animation_manifest::{AnimationClip, AnimationFormat, ClipData, ClipIdentity};

/// The J3D1 file header of a single-chunk animation file.
pub struct ChunkHeader {
    /// Declared chunk size; vanilla BTKs can overrun the file by up to 0x20
    /// padding bytes, so use [`chunk_slice`] for data access.
    pub chunk_size: usize,
}

/// Validate the J3D1 header of a single-chunk animation file and return the
/// chunk header. `expected_type` is the 4CC after `J3D1`; `expected_chunk`
/// the chunk 4CC at 0x20.
pub fn j3d_single_chunk(
    data: &[u8],
    what: &str,
    expected_type: &str,
    expected_chunk: &[u8; 4],
) -> Result<ChunkHeader> {
    if data.len() < 0x28 {
        bail!(
            "{what}: {} bytes is smaller than a J3D1 header + chunk tag",
            data.len()
        );
    }
    if &data[0..4] != b"J3D1" {
        bail!("{what}: bad magic {:?}, expected J3D1", &data[0..4]);
    }
    let file_type = &data[4..8];
    if file_type != expected_type.as_bytes() {
        bail!(
            "{what}: file type {:?} does not match {expected_type:?}",
            file_type
        );
    }
    let length = u32_at(data, 8, "file length")? as usize;
    if length != data.len() {
        bail!(
            "{what}: header claims {length} bytes but file is {}",
            data.len()
        );
    }
    let nchunks = u32_at(data, 0xC, "chunk count")?;
    if nchunks != 1 {
        bail!("{what}: expected exactly 1 chunk, found {nchunks}");
    }
    if &data[0x20..0x24] != expected_chunk {
        bail!(
            "{what}: chunk {:?} does not match {:?}",
            &data[0x20..0x24],
            expected_chunk
        );
    }
    let chunk_size = u32_at(data, 0x24, "chunk size")? as u64;
    if chunk_size < 0x18 {
        bail!("{what}: chunk size {chunk_size} is smaller than its own header");
    }
    // u64 arithmetic: a hostile 0xFFFFFFFF size word must not wrap on 32-bit
    // hosts before the bound is applied.
    let chunk_end = 0x20u64 + chunk_size;
    if chunk_end > data.len() as u64 + 0x20 {
        bail!(
            "{what}: chunk claims {chunk_size} bytes, ending {} bytes past the {}-byte file",
            chunk_end - data.len() as u64,
            data.len()
        );
    }
    Ok(ChunkHeader {
        chunk_size: chunk_size as usize,
    })
}

/// The chunk-local slice: clamped to the file, since vanilla chunk-size words
/// can overrun EOF by up to one padding block (data structures are
/// bounds-checked individually when read).
pub fn chunk_slice<'a>(data: &'a [u8], header: &ChunkHeader) -> &'a [u8] {
    let end = (0x20 + header.chunk_size).min(data.len());
    &data[0x20..end.max(0x20)]
}

/// A bounds-checked sub-slice of `chunk` used as a pool/table, `len` bytes at
/// `off`. Pools may legitimately be empty (count 0, offset 0).
pub fn pool<'a>(
    chunk: &'a [u8],
    off: usize,
    len: usize,
    what: &str,
    clip: &str,
) -> Result<&'a [u8]> {
    let end = off
        .checked_add(len)
        .ok_or_else(|| anyhow!("{clip}: {what} range {off:#x}+{len} overflows"))?;
    chunk.get(off..end).ok_or_else(|| {
        anyhow!(
            "{clip}: {what} at {off:#x} spans {len} bytes, past the {:#x}-byte chunk",
            chunk.len()
        )
    })
}

pub fn u8_at(data: &[u8], off: usize, what: &str) -> Result<u8> {
    data.get(off)
        .copied()
        .ok_or_else(|| anyhow!("{what}: byte at {off:#x} out of bounds"))
}

pub fn u16_at(data: &[u8], off: usize, what: &str) -> Result<u16> {
    let b = data
        .get(off..off + 2)
        .ok_or_else(|| anyhow!("{what}: u16 at {off:#x} out of bounds"))?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}

pub fn u32_at(data: &[u8], off: usize, what: &str) -> Result<u32> {
    let b = data
        .get(off..off + 4)
        .ok_or_else(|| anyhow!("{what}: u32 at {off:#x} out of bounds"))?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Reachability check for one key descriptor against its pool (in words):
/// constant tracks need `index` in range, keyed tracks need the whole run.
pub fn check_descriptor(
    desc: tracks::KeyDescriptor,
    pool_words: usize,
    elem: usize,
    what: &str,
) -> Result<()> {
    let stride = if desc.tangent_type == 0 { 3 } else { 4 };
    let words_needed = match desc.count {
        0 => return Ok(()),
        1 => 1,
        n => n as usize * stride,
    };
    let _ = elem;
    let end = desc.index as usize + words_needed;
    if end > pool_words {
        bail!(
            "{what}: key data {index}..{end} exceeds the {pool_words}-word pool",
            index = desc.index
        );
    }
    Ok(())
}

/// Parse one decompressed raw clip by format tag into a schema document.
pub fn parse_clip(
    format: AnimationFormat,
    data: &[u8],
    identity: ClipIdentity,
) -> Result<AnimationClip> {
    let what = format!("{}/{}", identity.archive, identity.member);
    let data_result = match format {
        AnimationFormat::Bck => ClipData::Bck(bck::parse(data, &what)?),
        AnimationFormat::Btp => ClipData::Btp(btp::parse(data, &what)?),
        AnimationFormat::Btk => ClipData::Btk(btk::parse(data, &what)?),
    };
    Ok(AnimationClip {
        version: gx::animation_manifest::CLIP_VERSION,
        identity,
        data: data_result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_malformed_inputs_return_errors() {
        // Format-level malformed inputs; the per-format suites in bck/btp/btk
        // cover their own corruption cases.
        let bck = super::bck::fixtures::build_bck();
        let btp = super::btp::fixtures::build_btp();
        let btk = super::btk::fixtures::build_btk(false, 0).file;
        let identity = |member: &str| ClipIdentity {
            archive: "A".into(),
            member: member.into(),
            entry_index: 0,
            resource_id: 0,
            sha256: String::new(),
        };

        // Wrong chunk for the dispatched format.
        let mut bad = bck.clone();
        bad[0x20..0x24].copy_from_slice(b"TPT1");
        assert!(bck::parse(&bad, "m.bck").is_err());
        // Truncated file below the J3D header.
        assert!(bck::parse(&bck[..0x10], "m.bck").is_err());
        assert!(btp::parse(&btp[..0x10], "m.btp").is_err());
        assert!(btk::parse(&btk[..0x10], "m.btk").is_err());
        // Lying file length.
        let mut bad = bck.clone();
        bad[8..12].copy_from_slice(&7u32.to_be_bytes());
        assert!(parse_clip(AnimationFormat::Bck, &bad, identity("m.bck")).is_err());
        // Pool offset outside the chunk.
        let mut bad = btp.clone();
        bad[0x20 + 0x14..0x20 + 0x18].copy_from_slice(&0xFFFFu32.to_be_bytes());
        assert!(btp::parse(&bad, "m.btp").is_err());
        // Every error is an error value; nothing above panics.
    }

    #[test]
    fn dispatch_by_format() {
        let bck = super::bck::fixtures::build_bck();
        let identity = ClipIdentity {
            archive: "A".into(),
            member: "bcks/x.bck".into(),
            entry_index: 0,
            resource_id: 0,
            sha256: String::new(),
        };
        let clip = parse_clip(AnimationFormat::Bck, &bck, identity).unwrap();
        assert_eq!(clip.format(), AnimationFormat::Bck);
        assert!(matches!(clip.data, ClipData::Bck(_)));
        // wrong dispatch errors (TPT1 chunk through the BCK parser)
        let btp = super::btp::fixtures::build_btp();
        assert!(
            parse_clip(
                AnimationFormat::Bck,
                &btp,
                ClipIdentity {
                    archive: "A".into(),
                    member: "m".into(),
                    entry_index: 0,
                    resource_id: 0,
                    sha256: String::new(),
                }
            )
            .is_err()
        );
    }
}
