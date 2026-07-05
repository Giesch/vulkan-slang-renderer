//! VTX1 chunk: vertex attribute arrays. Structure (J3DModelLoader.h
//! J3DVertexBlock, gclib vtx1.py): u32 format-table offset at +8, then 13
//! u32 array offsets from +0x0C (pos, nrm, NBT, color0/1, tex0..7). The
//! format table is 0x10-byte entries `{u32 attr, u32 compCount, u32 compType,
//! u8 shift, pad}` terminated by attr 0xFF. Fixed-point integer components
//! scale by 1/2^shift.
//!
//! cl.bdl uses exactly three arrays — positions f32 XYZ, normals f32 XYZ,
//! tex0 s16 ST (shift 8, ÷256). Anything else (color/NBT arrays, non-f32
//! pos/nrm, a second UV) is a hard error, matching P2's Expectations-style
//! strictness. Element counts are derived from the gaps between consecutive
//! array offsets (the method gclib uses; padding at the tail is harmless — it
//! only loosens the index bound).

use crate::be::BeReader;
use crate::bmd::BmdError;
use crate::gx::types::{Attr, ComponentType};

// Array-offset field indices, relative to the 13 u32 offsets at +0x0C
// (pos, nrm, NBT, color0, color1, tex0..tex7).
const POS: usize = 0;
const NRM: usize = 1;
const TEX0: usize = 5;

/// A parsed VTX1 format-table entry, kept raw for the canonical dump.
#[derive(Debug, Clone, PartialEq)]
pub struct VtxFormat {
    pub attr: Attr,
    /// Raw GXCompCnt value (attr-relative: POS_XYZ=1, NRM_XYZ=0, TEX_ST=1).
    pub comp_count: u32,
    pub comp_type: ComponentType,
    pub shift: u8,
}

pub struct Vtx1 {
    pub formats: Vec<VtxFormat>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
}

impl Vtx1 {
    pub fn pos(&self, i: u16) -> Result<[f32; 3], BmdError> {
        self.positions
            .get(i as usize)
            .copied()
            .ok_or_else(|| BmdError::Invariant(format!("VTX1 position index {i} out of range")))
    }

    pub fn nrm(&self, i: u16) -> Result<[f32; 3], BmdError> {
        self.normals
            .get(i as usize)
            .copied()
            .ok_or_else(|| BmdError::Invariant(format!("VTX1 normal index {i} out of range")))
    }

    pub fn uv0(&self, i: u16) -> Result<[f32; 2], BmdError> {
        self.uvs
            .get(i as usize)
            .copied()
            .ok_or_else(|| BmdError::Invariant(format!("VTX1 tex0 index {i} out of range")))
    }
}

pub fn parse(chunk: &[u8]) -> Result<Vtx1, BmdError> {
    let r = BeReader::new(chunk);
    let mut h = r.at(8);
    let fmt_off = h.u32()? as usize;
    let mut array_offsets = [0usize; 13];
    for slot in &mut array_offsets {
        *slot = h.u32()? as usize;
    }

    // Format table, attr 0xFF terminated.
    let mut formats = Vec::new();
    let mut f = r.at(fmt_off);
    loop {
        let attr_raw = f.u32()?;
        if attr_raw == 0xFF {
            break;
        }
        let attr = Attr::try_from(attr_raw as u8).map_err(|source| BmdError::Gx {
            context: format!("VTX1 format attr {attr_raw:#x}"),
            source,
        })?;
        let comp_count = f.u32()?;
        let comp_type_raw = f.u32()?;
        let comp_type =
            ComponentType::try_from(comp_type_raw as u8).map_err(|source| BmdError::Gx {
                context: format!("VTX1 format {attr} compType"),
                source,
            })?;
        let shift = f.u8()?;
        f.skip(3)?; // pad to 0x10
        formats.push(VtxFormat {
            attr,
            comp_count,
            comp_type,
            shift,
        });
    }

    // cl.bdl's exact attribute set — reject anything else.
    for fmt in &formats {
        match fmt.attr {
            Attr::Pos => reject_unless(
                fmt.comp_type == ComponentType::F32 && fmt.comp_count == 1,
                "VTX1 POS must be F32 XYZ",
            )?,
            Attr::Nrm => reject_unless(
                fmt.comp_type == ComponentType::F32 && fmt.comp_count == 0,
                "VTX1 NRM must be F32 XYZ",
            )?,
            Attr::Tex0 => reject_unless(
                fmt.comp_type == ComponentType::S16 && fmt.comp_count == 1,
                "VTX1 TEX0 must be S16 ST",
            )?,
            other => {
                return Err(BmdError::Invariant(format!(
                    "VTX1 has unsupported attribute {other} (cl.bdl uses only POS/NRM/TEX0)"
                )));
            }
        }
    }

    // Element counts from offset deltas: for each present array, the next
    // boundary is the smallest present offset strictly greater than it, else
    // the chunk end.
    let present: Vec<usize> = array_offsets.iter().copied().filter(|&o| o != 0).collect();
    let boundary = |off: usize| -> usize {
        present
            .iter()
            .copied()
            .filter(|&o| o > off)
            .min()
            .unwrap_or(chunk.len())
    };

    let positions = decode_vec3(&r, array_offsets[POS], boundary(array_offsets[POS]), 12)?;
    let normals = decode_vec3(&r, array_offsets[NRM], boundary(array_offsets[NRM]), 12)?;
    let uv_shift = formats
        .iter()
        .find(|f| f.attr == Attr::Tex0)
        .map(|f| f.shift)
        .unwrap_or(0);
    let uvs = decode_uv(
        &r,
        array_offsets[TEX0],
        boundary(array_offsets[TEX0]),
        uv_shift,
    )?;

    Ok(Vtx1 {
        formats,
        positions,
        normals,
        uvs,
    })
}

fn reject_unless(ok: bool, msg: &str) -> Result<(), BmdError> {
    if ok {
        Ok(())
    } else {
        Err(BmdError::Invariant(msg.to_string()))
    }
}

fn decode_vec3(
    r: &BeReader,
    off: usize,
    end: usize,
    stride: usize,
) -> Result<Vec<[f32; 3]>, BmdError> {
    let count = (end - off) / stride;
    let mut out = Vec::with_capacity(count);
    let mut cur = r.at(off);
    for _ in 0..count {
        out.push([cur.f32()?, cur.f32()?, cur.f32()?]);
    }
    Ok(out)
}

fn decode_uv(r: &BeReader, off: usize, end: usize, shift: u8) -> Result<Vec<[f32; 2]>, BmdError> {
    let scale = 1.0 / (1u32 << shift) as f32;
    let count = (end - off) / 4; // s16 ST = 4 bytes
    let mut out = Vec::with_capacity(count);
    let mut cur = r.at(off);
    for _ in 0..count {
        let s = cur.i16()? as f32 * scale;
        let t = cur.i16()? as f32 * scale;
        out.push([s, t]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT_ENTRY_SIZE: usize = 0x10;

    #[test]
    fn s16_shift8_uv_decodes() {
        // 0x0180 with shift 8 = 384/256 = 1.5
        assert_eq!(384i16 as f32 / 256.0, 1.5);
    }

    /// A minimal VTX1 with pos (2 verts), nrm (2), tex0 s16 shift-8 (2).
    #[test]
    fn parses_three_arrays() {
        let header_len = 8 + 4 + 13 * 4; // magic/size + fmt + 13 arrays
        let fmt_off = header_len;
        // 3 format entries + terminator, 0x10 each
        let fmt_len = 4 * FORMAT_ENTRY_SIZE;
        let pos_off = fmt_off + fmt_len;
        let nrm_off = pos_off + 2 * 12;
        let tex_off = nrm_off + 2 * 12;
        let total = tex_off + 2 * 4;

        let mut d = vec![0u8; header_len];
        // fake magic+size (unused by parse beyond +8)
        d[8..12].copy_from_slice(&(fmt_off as u32).to_be_bytes());
        let put = |d: &mut Vec<u8>, idx: usize, v: u32| {
            let at = 12 + idx * 4;
            d[at..at + 4].copy_from_slice(&v.to_be_bytes());
        };
        put(&mut d, POS, pos_off as u32);
        put(&mut d, NRM, nrm_off as u32);
        put(&mut d, TEX0, tex_off as u32);
        d.resize(total, 0);

        let write_fmt = |d: &mut Vec<u8>, base: usize, attr: u32, cnt: u32, ty: u32, sh: u8| {
            d[base..base + 4].copy_from_slice(&attr.to_be_bytes());
            d[base + 4..base + 8].copy_from_slice(&cnt.to_be_bytes());
            d[base + 8..base + 12].copy_from_slice(&ty.to_be_bytes());
            d[base + 12] = sh;
        };
        write_fmt(&mut d, fmt_off, 0x09, 1, 4, 0); // POS F32 XYZ
        write_fmt(&mut d, fmt_off + 0x10, 0x0A, 0, 4, 0); // NRM F32 XYZ
        write_fmt(&mut d, fmt_off + 0x20, 0x0D, 1, 3, 8); // TEX0 S16 ST shift8
        d[fmt_off + 0x30..fmt_off + 0x34].copy_from_slice(&0xFFu32.to_be_bytes()); // NULL

        // positions
        let w = |d: &mut Vec<u8>, at: usize, vals: &[f32]| {
            let mut o = at;
            for v in vals {
                d[o..o + 4].copy_from_slice(&v.to_be_bytes());
                o += 4;
            }
        };
        w(&mut d, pos_off, &[1.0, 2.0, 3.0]);
        w(&mut d, pos_off + 12, &[4.0, 5.0, 6.0]);
        w(&mut d, nrm_off, &[0.0, 1.0, 0.0]);
        w(&mut d, nrm_off + 12, &[1.0, 0.0, 0.0]);
        // tex0 s16: (0x0180, 0x0080) -> (1.5, 0.5), then (256, 512)->(1.0,2.0)
        d[tex_off..tex_off + 2].copy_from_slice(&384i16.to_be_bytes());
        d[tex_off + 2..tex_off + 4].copy_from_slice(&128i16.to_be_bytes());
        d[tex_off + 4..tex_off + 6].copy_from_slice(&256i16.to_be_bytes());
        d[tex_off + 6..tex_off + 8].copy_from_slice(&512i16.to_be_bytes());

        let v = parse(&d).unwrap();
        assert_eq!(v.positions, vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
        assert_eq!(v.normals, vec![[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]);
        assert_eq!(v.uvs, vec![[1.5, 0.5], [1.0, 2.0]]);
        assert_eq!(v.formats.len(), 3);
    }

    #[test]
    fn color_array_is_rejected() {
        // A format table with a CLR0 attr must fail.
        let header_len = 8 + 4 + 13 * 4;
        let fmt_off = header_len;
        let mut d = vec![0u8; header_len + 2 * FORMAT_ENTRY_SIZE];
        d[8..12].copy_from_slice(&(fmt_off as u32).to_be_bytes());
        d[fmt_off..fmt_off + 4].copy_from_slice(&0x0Bu32.to_be_bytes()); // CLR0
        d[fmt_off + 4..fmt_off + 8].copy_from_slice(&1u32.to_be_bytes());
        d[fmt_off + 8..fmt_off + 12].copy_from_slice(&4u32.to_be_bytes()); // F32 (valid type)
        d[fmt_off + 0x10..fmt_off + 0x14].copy_from_slice(&0xFFu32.to_be_bytes());
        assert!(matches!(parse(&d), Err(BmdError::Invariant(_))));
    }
}
