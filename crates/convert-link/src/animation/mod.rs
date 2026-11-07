//! J3D animation decoding: BCK (body), BTP (face texture patterns), BTK
//! (face texture SRT). One module per format plus shared key-table machinery
//! in [`tracks`]; [`output`] emits the shared `gx::animation_manifest`
//! documents. The J3D binary conventions are documented per module and were
//! verified against the GZLE01 LkAnm/LkD00/LkD01 archives and the Wind Waker
//! decompilation (`J3DAnmLoader.cpp`, `J3DAnimation.cpp`).

pub mod bck;
pub mod btk;
pub mod btp;
pub mod error;
pub mod output;
pub mod tracks;

use eros::{IntoUnion, ReshapeUnion};

use gx::animation_manifest::{AnimationClip, AnimationFormat, ClipData, ClipIdentity};

use error::{
    AnimationParseErrors, FourCcMismatch, HeaderErrors, InvalidJ3dHeader, KeyDataOutOfPool,
    OutOfBounds, PoolErrors, RangeOverflow, ReadErrors,
};

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
    expected_type: &[u8; 4],
    expected_chunk: &[u8; 4],
) -> eros::Result<ChunkHeader, HeaderErrors> {
    if data.len() < 0x28 {
        return Err(InvalidJ3dHeader::TooShort { len: data.len() }).union();
    }
    let fourcc = |off: usize| -> [u8; 4] { data[off..off + 4].try_into().expect("4 bytes") };
    let check = |field: &'static str, off: usize, expected: &[u8; 4]| {
        let found = fourcc(off);
        if &found == expected {
            Ok(())
        } else {
            Err(FourCcMismatch {
                field,
                found,
                expected: *expected,
            })
            .union()
        }
    };
    check("magic", 0, b"J3D1")?;
    check("file type", 4, expected_type)?;
    let length = u32_at(data, 8, "file length").widen()? as usize;
    if length != data.len() {
        return Err(InvalidJ3dHeader::FileLength {
            declared: length,
            actual: data.len(),
        })
        .union();
    }
    let nchunks = u32_at(data, 0xC, "chunk count").widen()?;
    if nchunks != 1 {
        return Err(InvalidJ3dHeader::ChunkCount { found: nchunks }).union();
    }
    check("chunk", 0x20, expected_chunk)?;
    let chunk_size = u32_at(data, 0x24, "chunk size").widen()? as u64;
    if chunk_size < 0x18 {
        return Err(InvalidJ3dHeader::ChunkTooSmall { size: chunk_size }).union();
    }
    // u64 arithmetic: a hostile 0xFFFFFFFF size word must not wrap on 32-bit
    // hosts before the bound is applied.
    let chunk_end = 0x20u64 + chunk_size;
    if chunk_end > data.len() as u64 + 0x20 {
        return Err(InvalidJ3dHeader::ChunkOverrun {
            size: chunk_size,
            past: chunk_end - data.len() as u64,
            file_len: data.len(),
        })
        .union();
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
) -> eros::Result<&'a [u8], PoolErrors> {
    let Some(end) = off.checked_add(len) else {
        return Err(RangeOverflow {
            what: what.to_string(),
        })
        .union();
    };
    chunk
        .get(off..end)
        .ok_or_else(|| out_of_bounds(chunk, off, len, what))
        .union()
}

/// The [`OutOfBounds`] error for reading `len` bytes at `off` in `data`.
pub fn out_of_bounds(data: &[u8], off: usize, len: usize, what: &str) -> OutOfBounds {
    OutOfBounds {
        what: what.to_string(),
        offset: off,
        len,
        available: data.len(),
    }
}

fn bytes_at<const N: usize>(
    data: &[u8],
    off: usize,
    what: &str,
) -> eros::Result<[u8; N], ReadErrors> {
    let bytes = off
        .checked_add(N)
        .and_then(|end| data.get(off..end))
        .ok_or_else(|| out_of_bounds(data, off, N, what))?;
    Ok(bytes.try_into().expect("N bytes"))
}

pub fn u8_at(data: &[u8], off: usize, what: &str) -> eros::Result<u8, ReadErrors> {
    bytes_at::<1>(data, off, what).map(|[b]| b)
}

pub fn u16_at(data: &[u8], off: usize, what: &str) -> eros::Result<u16, ReadErrors> {
    bytes_at(data, off, what).map(u16::from_be_bytes)
}

pub fn u32_at(data: &[u8], off: usize, what: &str) -> eros::Result<u32, ReadErrors> {
    bytes_at(data, off, what).map(u32::from_be_bytes)
}

/// Reachability check for one key descriptor against its pool (in words):
/// constant tracks need `index` in range, keyed tracks need the whole run.
pub fn check_descriptor(
    desc: tracks::KeyDescriptor,
    pool_words: usize,
    what: &str,
) -> eros::Result<(), (KeyDataOutOfPool,)> {
    let stride = if desc.tangent_type == 0 { 3 } else { 4 };
    let words_needed = match desc.count {
        0 => return Ok(()),
        1 => 1,
        n => n as usize * stride,
    };
    let start = desc.index as usize;
    let end = start + words_needed;
    if end > pool_words {
        return Err(KeyDataOutOfPool {
            what: what.to_string(),
            start,
            end,
            pool_words,
        })
        .union();
    }
    Ok(())
}

/// Parse one decompressed raw clip by format tag into a schema document.
/// The per-format parser attaches the `archive/member` name as context.
pub fn parse_clip(
    format: AnimationFormat,
    data: &[u8],
    identity: ClipIdentity,
) -> eros::Result<AnimationClip, AnimationParseErrors> {
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
    fn errors_are_typed_and_carry_clip_context() {
        let mut bad = super::bck::fixtures::build_bck();
        bad[0x20..0x24].copy_from_slice(b"TPT1");
        let identity = ClipIdentity {
            archive: "LkAnm".into(),
            member: "bcks/x.bck".into(),
            entry_index: 0,
            resource_id: 0,
            sha256: String::new(),
        };
        let err = parse_clip(AnimationFormat::Bck, &bad, identity).unwrap_err();
        // Display is the specific root error; Debug adds the clip context.
        assert_eq!(
            err.to_string(),
            "chunk \"TPT1\" does not match expected \"ANK1\""
        );
        let report = format!("{err:#?}");
        assert!(report.contains("Parse BCK LkAnm/bcks/x.bck"), "{report}");
        let mismatch = err.narrow::<FourCcMismatch, _>().expect("FourCcMismatch");
        assert_eq!(mismatch.field, "chunk");
        assert_eq!(&mismatch.found, b"TPT1");

        // A truncated file is a structural header error, not a 4CC one.
        let short = super::btp::fixtures::build_btp();
        let err = super::btp::parse(&short[..0x10], "m.btp").unwrap_err();
        assert!(matches!(
            err.narrow::<InvalidJ3dHeader, _>(),
            Ok(InvalidJ3dHeader::TooShort { len: 0x10 })
        ));
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
