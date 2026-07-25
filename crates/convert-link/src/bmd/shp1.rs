//! SHP1 chunk: shapes (GX primitive geometry). Structure (J3DShapeFactory.h,
//! confirmed against noclip's J3DLoader): u16 shape count at +8, then u32
//! offsets at +0x0C shape-init, +0x10 remap table, +0x14 name table, +0x18
//! vertex-descriptor pool, +0x1C matrix table, +0x20 display-list data, +0x24
//! matrix-init data, +0x28 draw-init data.
//!
//! Each shape (0x28-byte init record) has `mtxGroupNum` matrix groups. A group
//! carries a `useMtx` table (slices of the u16 matrix table; entries are DRW1
//! slot indices, `0xFFFF` = inherit from the previous packet) and a GX display
//! list. The display list is `u8 opcode, u16 vertexCount`, then per vertex one
//! value per enabled attribute (in descriptor order) at that attribute's
//! input-type width. PNMTXIDX (GX_DIRECT, 1 byte) encodes `slot×3`.
//!
//! cl.bdl is triangle strips only (573 of them). Billboards and non-triangle
//! primitives are hard errors — the subset gate.

use crate::be::BeReader;
use crate::bmd::BmdError;
use crate::bmd::vtx1::Vtx1;
use crate::gx::types::{Attr, AttrInputType, PrimitiveType, ShapeMatrixType};

const SHAPE_INIT_SIZE: usize = 0x28;
const MTX_INIT_SIZE: usize = 8;
const DRAW_INIT_SIZE: usize = 8;

/// Per-vertex attribute indices decoded from a display list.
#[derive(Debug, Clone, PartialEq)]
pub struct VertexIndices {
    /// PNMTXIDX byte / 3 (the shape's matrix-table slot), if the shape has
    /// per-vertex matrices.
    pub pnmtx_slot: Option<u8>,
    pub pos: u16,
    pub nrm: Option<u16>,
    pub uv: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Primitive {
    pub prim_type: PrimitiveType,
    pub verts: Vec<VertexIndices>,
}

pub struct MatrixGroup {
    /// DRW1 slot indices for this packet; `0xFFFF` means inherit.
    pub use_mtx: Vec<u16>,
    pub dl_size: u32,
    pub primitives: Vec<Primitive>,
}

pub struct Shape {
    pub mtx_type: ShapeMatrixType,
    pub attrs: Vec<(Attr, AttrInputType)>,
    pub groups: Vec<MatrixGroup>,
    pub radius: f32,
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
}

pub struct Shp1 {
    pub shapes: Vec<Shape>,
}

pub fn parse(chunk: &[u8], vtx1: &Vtx1, drw_slot_count: u16) -> Result<Shp1, BmdError> {
    let r = BeReader::new(chunk);
    let mut h = r.at(8);
    let count = h.u16()? as usize;
    h.skip(2)?;
    let init_off = h.u32()? as usize;
    let remap_off = h.u32()? as usize;
    let _name_off = h.u32()? as usize;
    let desc_off = h.u32()? as usize;
    let mtx_table_off = h.u32()? as usize;
    let dl_off = h.u32()? as usize;
    let mtx_init_off = h.u32()? as usize;
    let draw_init_off = h.u32()? as usize;

    let pos_count = vtx1.positions.len();
    let nrm_count = vtx1.normals.len();
    let uv_count = vtx1.uvs.len();

    let mut shapes = Vec::with_capacity(count);
    for s in 0..count {
        let remap = r.at(remap_off + s * 2).u16()? as usize;
        let mut init = r.at(init_off + remap * SHAPE_INIT_SIZE);
        let mtx_type = ShapeMatrixType::try_from(init.u8()?).map_err(|source| BmdError::Gx {
            context: format!("SHP1 shape {s} matrix type"),
            source,
        })?;
        init.skip(1)?; // pad
        let group_num = init.u16()? as usize;
        let desc_index = init.u16()? as usize;
        let mtx_init_index = init.u16()? as usize;
        let draw_init_index = init.u16()? as usize;
        init.skip(2)?; // pad
        let radius = init.f32()?;
        let bbox_min = [init.f32()?, init.f32()?, init.f32()?];
        let bbox_max = [init.f32()?, init.f32()?, init.f32()?];

        if matches!(
            mtx_type,
            ShapeMatrixType::Billboard | ShapeMatrixType::BillboardY
        ) {
            return Err(BmdError::Invariant(format!(
                "SHP1 shape {s} is a billboard ({mtx_type}); unsupported (cl.bdl has none)"
            )));
        }

        // Vertex descriptor list (byte-addressed by desc_index).
        let attrs = parse_desc_list(&r, desc_off + desc_index, s)?;

        let mut groups = Vec::with_capacity(group_num);
        for g in 0..group_num {
            let mut mi = r.at(mtx_init_off + (mtx_init_index + g) * MTX_INIT_SIZE);
            let _use_mtx_index = mi.u16()?; // unused: not the matrix-table head
            let use_mtx_count = mi.u16()? as usize;
            let first_use_mtx = mi.u32()? as usize;

            // The useMtx table (DRW1 slot indices, 0xFFFF = inherit) drives
            // both Single and Multi shapes; `mUseMtxIndex` is ignored.
            let mut use_mtx = Vec::with_capacity(use_mtx_count);
            let mut mt = r.at(mtx_table_off + first_use_mtx * 2);
            for _ in 0..use_mtx_count {
                let e = mt.u16()?;
                if e != 0xFFFF && e >= drw_slot_count {
                    return Err(BmdError::Invariant(format!(
                        "SHP1 shape {s} group {g} matrix slot {e} of {drw_slot_count} DRW1 slots"
                    )));
                }
                use_mtx.push(e);
            }

            let mut di = r.at(draw_init_off + (draw_init_index + g) * DRAW_INIT_SIZE);
            let dl_size = di.u32()?;
            let dl_data_off = di.u32()? as usize;

            let primitives = decode_display_list(
                chunk,
                dl_off + dl_data_off,
                dl_size as usize,
                &attrs,
                pos_count,
                nrm_count,
                uv_count,
                s,
            )?;
            groups.push(MatrixGroup {
                use_mtx,
                dl_size,
                primitives,
            });
        }

        shapes.push(Shape {
            mtx_type,
            attrs,
            groups,
            radius,
            bbox_min,
            bbox_max,
        });
    }

    Ok(Shp1 { shapes })
}

fn parse_desc_list(
    r: &BeReader,
    off: usize,
    shape: usize,
) -> Result<Vec<(Attr, AttrInputType)>, BmdError> {
    let mut d = r.at(off);
    let mut attrs = Vec::new();
    loop {
        let attr_raw = d.u32()?;
        if attr_raw == 0xFF {
            break;
        }
        let attr = Attr::try_from(attr_raw as u8).map_err(|source| BmdError::Gx {
            context: format!("SHP1 shape {shape} descriptor attr {attr_raw:#x}"),
            source,
        })?;
        let input_raw = d.u32()?;
        let input = AttrInputType::try_from(input_raw as u8).map_err(|source| BmdError::Gx {
            context: format!("SHP1 shape {shape} descriptor {attr} input type"),
            source,
        })?;
        // cl.bdl subset: PNMTXIDX is DIRECT; POS/NRM/TEX0 are INDEX16.
        match (attr, input) {
            (Attr::Pnmtxidx, AttrInputType::Direct) => {}
            (Attr::Pos | Attr::Nrm | Attr::Tex0, AttrInputType::Index16) => {}
            _ => {
                return Err(BmdError::Invariant(format!(
                    "SHP1 shape {shape} has unsupported descriptor {attr}/{input}"
                )));
            }
        }
        attrs.push((attr, input));
    }
    Ok(attrs)
}

#[allow(clippy::too_many_arguments)]
fn decode_display_list(
    chunk: &[u8],
    start: usize,
    size: usize,
    attrs: &[(Attr, AttrInputType)],
    pos_count: usize,
    nrm_count: usize,
    uv_count: usize,
    shape: usize,
) -> Result<Vec<Primitive>, BmdError> {
    let mut dl = BeReader::new(chunk);
    dl.seek(start)?;
    let end = start + size;
    let mut primitives = Vec::new();

    while dl.pos() < end {
        let opcode = dl.u8()?;
        if opcode == 0 {
            // Padding to the 32-byte-aligned dlSize: the rest must be zero.
            if chunk[dl.pos()..end].iter().any(|&b| b != 0) {
                return Err(BmdError::Invariant(format!(
                    "SHP1 shape {shape} display list has non-zero padding"
                )));
            }
            break;
        }
        let prim_type = PrimitiveType::try_from(opcode).map_err(|source| BmdError::Gx {
            context: format!("SHP1 shape {shape} display-list opcode"),
            source,
        })?;
        if !matches!(
            prim_type,
            PrimitiveType::Triangles | PrimitiveType::TriangleStrip | PrimitiveType::TriangleFan
        ) {
            return Err(BmdError::Invariant(format!(
                "SHP1 shape {shape} has unsupported primitive {prim_type}"
            )));
        }
        let vtx_count = dl.u16()? as usize;
        let mut verts = Vec::with_capacity(vtx_count);
        for _ in 0..vtx_count {
            let mut vi = VertexIndices {
                pnmtx_slot: None,
                pos: 0,
                nrm: None,
                uv: None,
            };
            for (attr, _input) in attrs {
                match attr {
                    Attr::Pnmtxidx => {
                        let byte = dl.u8()?;
                        if byte % 3 != 0 || (byte / 3) as usize >= 10 {
                            return Err(BmdError::Invariant(format!(
                                "SHP1 shape {shape} PNMTXIDX {byte} not a valid slot×3 (<10)"
                            )));
                        }
                        vi.pnmtx_slot = Some(byte / 3);
                    }
                    Attr::Pos => {
                        let i = dl.u16()?;
                        if i as usize >= pos_count {
                            return Err(BmdError::Invariant(format!(
                                "SHP1 shape {shape} POS index {i} of {pos_count}"
                            )));
                        }
                        vi.pos = i;
                    }
                    Attr::Nrm => {
                        let i = dl.u16()?;
                        if i as usize >= nrm_count {
                            return Err(BmdError::Invariant(format!(
                                "SHP1 shape {shape} NRM index {i} of {nrm_count}"
                            )));
                        }
                        vi.nrm = Some(i);
                    }
                    Attr::Tex0 => {
                        let i = dl.u16()?;
                        if i as usize >= uv_count {
                            return Err(BmdError::Invariant(format!(
                                "SHP1 shape {shape} TEX0 index {i} of {uv_count}"
                            )));
                        }
                        vi.uv = Some(i);
                    }
                    other => {
                        return Err(BmdError::Invariant(format!(
                            "SHP1 shape {shape} descriptor {other} unhandled in display list"
                        )));
                    }
                }
            }
            verts.push(vi);
        }
        primitives.push(Primitive { prim_type, verts });
    }

    Ok(primitives)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vtx1_stub(pos: usize, nrm: usize, uv: usize) -> Vtx1 {
        Vtx1 {
            formats: Vec::new(),
            positions: vec![[0.0; 3]; pos],
            normals: vec![[0.0; 3]; nrm],
            uvs: vec![[0.0; 2]; uv],
        }
    }

    /// A display list with one 4-vertex strip, attrs = PNMTXIDX(DIRECT) +
    /// POS(INDEX16) + TEX0(INDEX16), decoded directly.
    #[test]
    fn decodes_strip_with_pnmtx() {
        let attrs = vec![
            (Attr::Pnmtxidx, AttrInputType::Direct),
            (Attr::Pos, AttrInputType::Index16),
            (Attr::Tex0, AttrInputType::Index16),
        ];
        // per vertex: 1 (pnmtx) + 2 (pos) + 2 (uv) = 5 bytes
        let mut dl = Vec::new();
        dl.push(0x98); // strip
        dl.extend_from_slice(&4u16.to_be_bytes());
        for i in 0u16..4 {
            dl.push(0); // pnmtx slot 0 (byte 0 = 0*3)
            dl.extend_from_slice(&i.to_be_bytes()); // pos
            dl.extend_from_slice(&i.to_be_bytes()); // uv
        }
        // pad to a few zero bytes
        dl.extend_from_slice(&[0, 0, 0]);

        let prims = decode_display_list(&dl, 0, dl.len(), &attrs, 4, 0, 4, 0).unwrap();
        assert_eq!(prims.len(), 1);
        assert_eq!(prims[0].prim_type, PrimitiveType::TriangleStrip);
        assert_eq!(prims[0].verts.len(), 4);
        assert_eq!(prims[0].verts[2].pos, 2);
        assert_eq!(prims[0].verts[2].pnmtx_slot, Some(0));
        assert_eq!(prims[0].verts[2].uv, Some(2));
    }

    #[test]
    fn bad_pnmtx_is_error() {
        let attrs = vec![
            (Attr::Pnmtxidx, AttrInputType::Direct),
            (Attr::Pos, AttrInputType::Index16),
        ];
        let mut dl = Vec::new();
        dl.push(0x98);
        dl.extend_from_slice(&1u16.to_be_bytes());
        dl.push(1); // 1 % 3 != 0 -> error
        dl.extend_from_slice(&0u16.to_be_bytes());
        assert!(matches!(
            decode_display_list(&dl, 0, dl.len(), &attrs, 1, 0, 0, 0),
            Err(BmdError::Invariant(_))
        ));
    }

    #[test]
    fn nonzero_padding_is_error() {
        let attrs = vec![(Attr::Pos, AttrInputType::Index16)];
        let mut dl = Vec::new();
        dl.push(0x98);
        dl.extend_from_slice(&1u16.to_be_bytes());
        dl.extend_from_slice(&0u16.to_be_bytes());
        dl.push(0); // opcode 0 = padding
        dl.push(7); // ...but non-zero
        assert!(matches!(
            decode_display_list(&dl, 0, dl.len(), &attrs, 1, 0, 0, 0),
            Err(BmdError::Invariant(_))
        ));
        let _ = vtx1_stub(1, 1, 1); // silence unused in some cfgs
    }
}
