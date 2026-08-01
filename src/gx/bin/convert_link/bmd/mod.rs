//! J3D2/bdl4 container: header validation and chunk-table walk.
//! Chunk-interior parsers (P2/P3) hang off the dispatch in `parse_model`.

pub mod drw1;
pub mod evp1;
pub mod geometry_dump;
pub mod inf1;
pub mod jnt1;
pub mod mat3;
pub mod mat3_dump;
pub mod shp1;
pub mod tex1;
pub mod vtx1;

use std::fmt;

use crate::be::{BeError, BeReader};
use crate::gx::types::GxEnumError;

pub const FILE_HEADER_SIZE: usize = 0x20;

const EXPECTED_FOURCCS: [[u8; 4]; 9] = [
    *b"INF1", *b"VTX1", *b"EVP1", *b"DRW1", *b"JNT1", *b"SHP1", *b"MAT3", *b"MDL3", *b"TEX1",
];

/// Chunks with a big-endian u16 element count at block offset +8, verified
/// against the tww decomp: J3DModelLoader.h (EVP1/DRW1/MAT3/TEX1),
/// J3DJointFactory.h (JNT1), J3DShapeFactory.h (SHP1). INF1 has flags there
/// and VTX1 a format-table offset — neither is a count.
const COUNT_AT_8: [[u8; 4]; 6] = [*b"EVP1", *b"DRW1", *b"JNT1", *b"SHP1", *b"MAT3", *b"TEX1"];

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FourCc(pub [u8; 4]);

impl fmt::Display for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match std::str::from_utf8(&self.0) {
            Ok(s) => f.write_str(s),
            Err(_) => write!(f, "{:02x?}", self.0),
        }
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChunkEntry {
    pub fourcc: FourCc,
    pub offset: usize,
    pub size: usize,
    pub count: Option<u16>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChunkTable {
    pub file_size: usize,
    pub block_num: u32,
    pub chunks: Vec<ChunkEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BmdError {
    BadMagic {
        found: [u8; 4],
    },
    BadType {
        found: [u8; 4],
    },
    SizeMismatch {
        header: usize,
        actual: usize,
    },
    BadBlockCount {
        found: u32,
    },
    UnknownFourCc {
        fourcc: [u8; 4],
        offset: usize,
    },
    BlockOverrun {
        fourcc: FourCc,
        offset: usize,
        size: usize,
        file_size: usize,
    },
    TrailingBytes {
        covered: usize,
        file_size: usize,
    },
    BadJointCount {
        found: Option<u16>,
    },
    /// An enum byte outside its known GX values; context names the chunk,
    /// entry, and field.
    Gx {
        context: String,
        source: GxEnumError,
    },
    /// Texture-specific structural problems (bad data size, mip count,
    /// palette range).
    Texture {
        name: String,
        what: String,
    },
    /// Structural invariant violations without a more specific variant.
    Invariant(String),
    Read(BeError),
}

impl fmt::Display for BmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BmdError::BadMagic { found } => {
                write!(f, "bad magic {found:02x?}, expected \"J3D2\"")
            }
            BmdError::BadType { found } => {
                write!(f, "bad file type {found:02x?}, expected \"bdl4\"")
            }
            BmdError::SizeMismatch { header, actual } => {
                write!(
                    f,
                    "header claims {header} bytes but file is {actual} (truncated?)"
                )
            }
            BmdError::BadBlockCount { found } => {
                write!(f, "expected 9 blocks, header claims {found}")
            }
            BmdError::UnknownFourCc { fourcc, offset } => {
                write!(f, "unknown chunk {fourcc:02x?} at offset {offset:#x}")
            }
            BmdError::BlockOverrun {
                fourcc,
                offset,
                size,
                file_size,
            } => write!(
                f,
                "block {fourcc} at {offset:#x} has size {size}, overrunning the {file_size}-byte file"
            ),
            BmdError::TrailingBytes { covered, file_size } => {
                write!(f, "blocks cover {covered} bytes of a {file_size}-byte file")
            }
            BmdError::BadJointCount { found: Some(n) } => {
                write!(f, "JNT1 reports {n} joints, expected 42")
            }
            BmdError::BadJointCount { found: None } => write!(f, "no JNT1 chunk found"),
            BmdError::Gx { context, source } => write!(f, "{context}: {source}"),
            BmdError::Texture { name, what } => write!(f, "texture {name}: {what}"),
            BmdError::Invariant(what) => f.write_str(what),
            BmdError::Read(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BmdError {}

impl From<BeError> for BmdError {
    fn from(e: BeError) -> Self {
        BmdError::Read(e)
    }
}

/// What a specific file is expected to contain. The public entry point pins
/// cl.bdl's exact shape; synthetic unit tests relax it.
struct Expectations {
    fourccs: &'static [[u8; 4]],
    block_num: u32,
    jnt1_count: Option<u16>,
}

const CL_BDL: Expectations = Expectations {
    fourccs: &EXPECTED_FOURCCS,
    block_num: 9,
    jnt1_count: Some(42),
};

pub fn parse_chunk_table(data: &[u8]) -> Result<ChunkTable, BmdError> {
    parse_chunk_table_with(data, &CL_BDL)
}

fn parse_chunk_table_with(data: &[u8], expect: &Expectations) -> Result<ChunkTable, BmdError> {
    let mut r = BeReader::new(data);

    let magic = four(&mut r)?;
    if &magic != b"J3D2" {
        return Err(BmdError::BadMagic { found: magic });
    }
    let file_type = four(&mut r)?;
    if &file_type != b"bdl4" {
        return Err(BmdError::BadType { found: file_type });
    }

    let file_size = r.u32()? as usize;
    if file_size != data.len() {
        return Err(BmdError::SizeMismatch {
            header: file_size,
            actual: data.len(),
        });
    }
    let block_num = r.u32()?;
    if block_num != expect.block_num {
        return Err(BmdError::BadBlockCount { found: block_num });
    }

    let mut chunks = Vec::with_capacity(block_num as usize);
    let mut offset = FILE_HEADER_SIZE;
    for _ in 0..block_num {
        r.seek(offset)?;
        let fourcc = four(&mut r)?;
        if !expect.fourccs.contains(&fourcc) {
            return Err(BmdError::UnknownFourCc { fourcc, offset });
        }
        let size = r.u32()? as usize;
        if size < 8 || offset.checked_add(size).is_none_or(|end| end > file_size) {
            return Err(BmdError::BlockOverrun {
                fourcc: FourCc(fourcc),
                offset,
                size,
                file_size,
            });
        }
        let count = if COUNT_AT_8.contains(&fourcc) {
            Some(r.at(offset + 8).u16()?)
        } else {
            None
        };
        chunks.push(ChunkEntry {
            fourcc: FourCc(fourcc),
            offset,
            size,
            count,
        });
        offset += size;
    }

    if offset != file_size {
        return Err(BmdError::TrailingBytes {
            covered: offset,
            file_size,
        });
    }

    if let Some(expected) = expect.jnt1_count {
        let found = chunks
            .iter()
            .find(|c| &c.fourcc.0 == b"JNT1")
            .and_then(|c| c.count);
        if found != Some(expected) {
            return Err(BmdError::BadJointCount { found });
        }
    }

    Ok(ChunkTable {
        file_size,
        block_num,
        chunks,
    })
}

fn four(r: &mut BeReader) -> Result<[u8; 4], BeError> {
    let b = r.bytes(4)?;
    Ok([b[0], b[1], b[2], b[3]])
}

/// The fully parsed model (chunks accumulate here as P2/P3 land).
pub struct Model<'a> {
    pub table: ChunkTable,
    pub tex1: tex1::Tex1<'a>,
    pub mat3: mat3::Mat3,
    pub inf1: inf1::Inf1,
    pub vtx1: vtx1::Vtx1,
    pub evp1: evp1::Evp1,
    pub drw1: drw1::Drw1,
    pub jnt1: jnt1::Jnt1,
    pub shp1: shp1::Shp1,
}

/// P2/P3 growth point: each chunk parser lands in `bmd/<chunk>.rs` and gets
/// a match arm here; nothing else in this module should need to change.
pub fn parse_model(data: &[u8]) -> Result<Model<'_>, BmdError> {
    let table = parse_chunk_table(data)?;
    let mut slices: std::collections::HashMap<[u8; 4], &[u8]> = std::collections::HashMap::new();
    for chunk in &table.chunks {
        let slice = &data[chunk.offset..chunk.offset + chunk.size];
        match &chunk.fourcc.0 {
            b"MDL3" => {} // skipped by design: MAT3 is authoritative
            b"INF1" | b"VTX1" | b"EVP1" | b"DRW1" | b"JNT1" | b"SHP1" | b"MAT3" | b"TEX1" => {
                slices.insert(chunk.fourcc.0, slice);
            }
            _ => unreachable!("validated by parse_chunk_table"),
        }
    }
    let need = |fourcc: &[u8; 4]| -> Result<&[u8], BmdError> {
        slices
            .get(fourcc)
            .copied()
            .ok_or_else(|| missing(std::str::from_utf8(fourcc).unwrap_or("?")))
    };

    // Parse in dependency order (mirrors the existing TEX1-before-MAT3 rule):
    // leaves first, then chunks that cross-check counts against them.
    let tex1 = tex1::parse(need(b"TEX1")?)?;
    let mat3 = mat3::parse(need(b"MAT3")?, tex1.entries.len() as u16)?;
    let vtx1 = vtx1::parse(need(b"VTX1")?)?;
    let jnt1 = jnt1::parse(need(b"JNT1")?)?;
    let joint_count = jnt1.joints.len();
    let inf1 = inf1::parse(need(b"INF1")?, joint_count)?;
    let evp1 = evp1::parse(need(b"EVP1")?, joint_count as u16)?;
    let drw1 = drw1::parse(
        need(b"DRW1")?,
        joint_count as u16,
        evp1.envelopes.len() as u16,
    )?;
    let shp1 = shp1::parse(need(b"SHP1")?, &vtx1, drw1.slots.len() as u16)?;

    Ok(Model {
        table,
        tex1,
        mat3,
        inf1,
        vtx1,
        evp1,
        drw1,
        jnt1,
        shp1,
    })
}

fn missing(fourcc: &str) -> BmdError {
    BmdError::Invariant(format!("no {fourcc} chunk found"))
}

/// JUTNameTab (JSystem string table): u16 count, u16 pad, then per entry a
/// u16 hash + u16 offset from the table start; strings are NUL-terminated.
pub fn read_name_table(r: &BeReader, pos: usize) -> Result<Vec<String>, BmdError> {
    let mut header = r.at(pos);
    let count = header.u16()?;
    let mut names = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let mut entry = r.at(pos + 4 + i * 4);
        let _hash = entry.u16()?;
        let str_offset = entry.u16()? as usize;
        names.push(r.at(pos + str_offset).cstr()?.to_string());
    }
    Ok(names)
}

/// The canonical `--info` format. `scripts/link_chunk_table.py` prints the
/// same spec (claude_notes/link_rendering/phase_01.md, step 4); verification
/// diffs the two byte-for-byte.
pub fn canonical_table(table: &ChunkTable) -> String {
    use std::fmt::Write;
    let mut out = format!(
        "J3D2 bdl4 size={} blocks={}\n",
        table.file_size, table.block_num
    );
    for c in &table.chunks {
        let count = c.count.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
        writeln!(out, "{} 0x{:06x} {} {}", c.fourcc, c.offset, c.size, count).unwrap();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_EXPECT: Expectations = Expectations {
        fourccs: &[*b"AAAA", *b"BBBB", *b"JNT1"],
        block_num: 2,
        jnt1_count: None,
    };

    /// Builds a valid file: 0x20 header + one block per (fourcc, body).
    fn synth(blocks: &[([u8; 4], &[u8])]) -> Vec<u8> {
        let mut out = vec![0u8; FILE_HEADER_SIZE];
        out[0..4].copy_from_slice(b"J3D2");
        out[4..8].copy_from_slice(b"bdl4");
        for (fourcc, body) in blocks {
            out.extend_from_slice(fourcc);
            out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
            out.extend_from_slice(body);
        }
        let file_size = out.len() as u32;
        out[8..12].copy_from_slice(&file_size.to_be_bytes());
        out[12..16].copy_from_slice(&(blocks.len() as u32).to_be_bytes());
        out
    }

    #[test]
    fn parses_minimal_two_block_file() {
        let data = synth(&[(*b"AAAA", &[0; 8]), (*b"BBBB", &[0; 4])]);
        let table = parse_chunk_table_with(&data, &TEST_EXPECT).unwrap();
        assert_eq!(table.file_size, data.len());
        assert_eq!(table.block_num, 2);
        let shape: Vec<_> = table
            .chunks
            .iter()
            .map(|c| (c.fourcc.0, c.offset, c.size, c.count))
            .collect();
        assert_eq!(
            shape,
            vec![(*b"AAAA", 0x20, 16, None), (*b"BBBB", 0x30, 12, None)]
        );
    }

    #[test]
    fn bad_magic() {
        let mut data = synth(&[(*b"AAAA", &[0; 8]), (*b"BBBB", &[0; 4])]);
        data[0..4].copy_from_slice(b"XXXX");
        assert_eq!(
            parse_chunk_table_with(&data, &TEST_EXPECT),
            Err(BmdError::BadMagic { found: *b"XXXX" })
        );
    }

    #[test]
    fn bad_type() {
        let mut data = synth(&[(*b"AAAA", &[0; 8]), (*b"BBBB", &[0; 4])]);
        data[4..8].copy_from_slice(b"bmd3");
        assert_eq!(
            parse_chunk_table_with(&data, &TEST_EXPECT),
            Err(BmdError::BadType { found: *b"bmd3" })
        );
    }

    #[test]
    fn file_size_mismatch() {
        let mut data = synth(&[(*b"AAAA", &[0; 8]), (*b"BBBB", &[0; 4])]);
        let wrong = data.len() as u32 + 1;
        data[8..12].copy_from_slice(&wrong.to_be_bytes());
        assert_eq!(
            parse_chunk_table_with(&data, &TEST_EXPECT),
            Err(BmdError::SizeMismatch {
                header: data.len() + 1,
                actual: data.len()
            })
        );
    }

    #[test]
    fn bad_block_count() {
        let data = synth(&[(*b"AAAA", &[0; 8])]); // one block, expectations say two
        assert_eq!(
            parse_chunk_table_with(&data, &TEST_EXPECT),
            Err(BmdError::BadBlockCount { found: 1 })
        );
    }

    #[test]
    fn unknown_fourcc() {
        let data = synth(&[(*b"ZZZZ", &[0; 8]), (*b"BBBB", &[0; 4])]);
        assert_eq!(
            parse_chunk_table_with(&data, &TEST_EXPECT),
            Err(BmdError::UnknownFourCc {
                fourcc: *b"ZZZZ",
                offset: 0x20
            })
        );
    }

    #[test]
    fn block_overrun_on_oversized_block() {
        let mut data = synth(&[(*b"AAAA", &[0; 8]), (*b"BBBB", &[0; 4])]);
        // first block's size field points past end of file
        data[0x24..0x28].copy_from_slice(&1000u32.to_be_bytes());
        assert_eq!(
            parse_chunk_table_with(&data, &TEST_EXPECT),
            Err(BmdError::BlockOverrun {
                fourcc: FourCc(*b"AAAA"),
                offset: 0x20,
                size: 1000,
                file_size: data.len(),
            })
        );
    }

    #[test]
    fn trailing_bytes() {
        let mut data = synth(&[(*b"AAAA", &[0; 8]), (*b"BBBB", &[0; 4])]);
        data.extend_from_slice(&[0; 8]);
        let file_size = data.len() as u32;
        data[8..12].copy_from_slice(&file_size.to_be_bytes());
        assert_eq!(
            parse_chunk_table_with(&data, &TEST_EXPECT),
            Err(BmdError::TrailingBytes {
                covered: data.len() - 8,
                file_size: data.len()
            })
        );
    }

    #[test]
    fn bad_joint_count() {
        let strict = Expectations {
            jnt1_count: Some(42),
            ..TEST_EXPECT
        };
        // JNT1 body starts at block offset +8: count 7
        let data = synth(&[(*b"JNT1", &[0x00, 0x07, 0, 0]), (*b"BBBB", &[0; 4])]);
        assert_eq!(
            parse_chunk_table_with(&data, &strict),
            Err(BmdError::BadJointCount { found: Some(7) })
        );
        // no JNT1 at all
        let data = synth(&[(*b"AAAA", &[0; 8]), (*b"BBBB", &[0; 4])]);
        assert_eq!(
            parse_chunk_table_with(&data, &strict),
            Err(BmdError::BadJointCount { found: None })
        );
    }

    #[test]
    fn count_peek_reads_u16_at_plus_8() {
        let data = synth(&[(*b"JNT1", &[0x00, 0x2A, 0, 0]), (*b"BBBB", &[0; 4])]);
        let table = parse_chunk_table_with(&data, &TEST_EXPECT).unwrap();
        assert_eq!(table.chunks[0].count, Some(42));
        assert_eq!(table.chunks[1].count, None); // BBBB is not a count-bearing chunk
    }

    #[test]
    fn truncated_header_is_read_error() {
        assert!(matches!(
            parse_chunk_table_with(b"J3D2bd", &TEST_EXPECT),
            Err(BmdError::Read(BeError::OutOfBounds { .. }))
        ));
    }

    #[test]
    #[ignore = "requires extracted assets (just extract-link); run via just link-verify-p1"]
    fn real_cl_bdl_invariants() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/link/raw/cl.bdl");
        let Ok(data) = std::fs::read(path) else {
            eprintln!("skipping: {path} not present");
            return;
        };
        let table = parse_chunk_table(&data).expect("cl.bdl must satisfy all invariants");
        // recorded facts: claude_notes/link_rendering/phase_01.md, verified
        // against the gclib oracle via `just link-verify-p1`
        assert_eq!(
            canonical_table(&table),
            "\
J3D2 bdl4 size=364544 blocks=9
INF1 0x000020 992 -
VTX1 0x000400 40576 -
EVP1 0x00a280 3744 120
DRW1 0x00b120 832 270
JNT1 0x00b460 3392 42
SHP1 0x00c1a0 31424 24
MAT3 0x013c60 12352 24
MDL3 0x016ca0 13344 -
TEX1 0x01a0c0 257856 41
"
        );
    }
}
