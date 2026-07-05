//! EVP1 chunk: skinning envelopes. Structure (J3DModelLoader.h
//! J3DEnvelopBlock, readEnvelop): u16 envelope count at +8, then u32 offsets
//! at +0x0C mix-count (u8 per envelope), +0x10 mix-index (u16, flat), +0x14
//! mix-weight (f32, flat), +0x18 inverse-bind matrices (3×4 f32, indexed by
//! joint). Envelope i has `count[i]` influences; the index/weight streams are
//! concatenated and walked with a running cursor.
//!
//! The inverse-bind matrices are the file's own FK answer key: at bind pose
//! `world(j)·invBind(j) = I` (checked in pose.rs).

use crate::be::BeReader;
use crate::bmd::BmdError;

/// One 3×4 (row-major) inverse-bind matrix; rows[r] = [m_r0, m_r1, m_r2, m_r3].
pub type Mtx3x4 = [[f32; 4]; 3];

const MTX_SIZE: usize = 0x30; // 12 f32

pub struct Evp1 {
    /// Per envelope: the (joint index, weight) influences.
    pub envelopes: Vec<Vec<(u16, f32)>>,
    /// Per joint: inverse bind matrix (3×4 rows).
    pub inv_bind: Vec<Mtx3x4>,
}

/// `joint_count` bounds both the influence joint indices and the inverse-bind
/// array length.
pub fn parse(chunk: &[u8], joint_count: u16) -> Result<Evp1, BmdError> {
    let r = BeReader::new(chunk);
    let mut h = r.at(8);
    let count = h.u16()? as usize;
    h.skip(2)?;
    let mix_count_off = h.u32()? as usize;
    let mix_index_off = h.u32()? as usize;
    let mix_weight_off = h.u32()? as usize;
    let inv_bind_off = h.u32()? as usize;

    let mut counts = r.at(mix_count_off);
    let mut idx = r.at(mix_index_off);
    let mut wgt = r.at(mix_weight_off);

    let mut envelopes = Vec::with_capacity(count);
    for e in 0..count {
        let n = counts.u8()? as usize;
        let mut influences = Vec::with_capacity(n);
        let mut sum = 0.0f32;
        for _ in 0..n {
            let joint = idx.u16()?;
            let weight = wgt.f32()?;
            if joint >= joint_count {
                return Err(BmdError::Invariant(format!(
                    "EVP1 envelope {e} references joint {joint} of {joint_count}"
                )));
            }
            sum += weight;
            influences.push((joint, weight));
        }
        if (sum - 1.0).abs() > 1e-3 {
            return Err(BmdError::Invariant(format!(
                "EVP1 envelope {e} weights sum to {sum:.6}, expected ~1.0"
            )));
        }
        envelopes.push(influences);
    }

    // Inverse binds span from their offset to the chunk end; one per joint.
    let inv_count = (chunk.len() - inv_bind_off) / MTX_SIZE;
    if inv_count < joint_count as usize {
        return Err(BmdError::Invariant(format!(
            "EVP1 has {inv_count} inverse-bind matrices but {joint_count} joints"
        )));
    }
    let mut inv_bind = Vec::with_capacity(joint_count as usize);
    let mut m = r.at(inv_bind_off);
    for _ in 0..joint_count {
        let mut rows = [[0.0f32; 4]; 3];
        for row in &mut rows {
            for c in row.iter_mut() {
                *c = m.f32()?;
            }
        }
        inv_bind.push(rows);
    }

    Ok(Evp1 {
        envelopes,
        inv_bind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_envelopes_and_binds() {
        // Layout: header(0x1C) then counts, indices, weights, invbinds.
        let hdr = 0x1Cusize;
        let count_off = hdr;
        let idx_off = count_off + 2; // 2 envelopes: counts [2,1]
        let wgt_off = idx_off + 3 * 2; // 3 total influences (u16)
        let inv_off = wgt_off + 3 * 4; // 3 weights (f32)
        let total = inv_off + 1 * MTX_SIZE; // one joint

        let mut d = vec![0u8; hdr];
        d[8..10].copy_from_slice(&2u16.to_be_bytes()); // count
        d[0x0C..0x10].copy_from_slice(&(count_off as u32).to_be_bytes());
        d[0x10..0x14].copy_from_slice(&(idx_off as u32).to_be_bytes());
        d[0x14..0x18].copy_from_slice(&(wgt_off as u32).to_be_bytes());
        d[0x18..0x1C].copy_from_slice(&(inv_off as u32).to_be_bytes());
        d.resize(total, 0);
        d[count_off] = 2;
        d[count_off + 1] = 1;
        // indices: env0 -> joints 0,0 ; env1 -> joint 0
        for (i, j) in [0u16, 0, 0].iter().enumerate() {
            let at = idx_off + i * 2;
            d[at..at + 2].copy_from_slice(&j.to_be_bytes());
        }
        for (i, w) in [0.5f32, 0.5, 1.0].iter().enumerate() {
            let at = wgt_off + i * 4;
            d[at..at + 4].copy_from_slice(&w.to_be_bytes());
        }
        // identity-ish 3x4
        let rows = [
            [1.0f32, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let mut o = inv_off;
        for row in rows {
            for c in row {
                d[o..o + 4].copy_from_slice(&c.to_be_bytes());
                o += 4;
            }
        }

        let e = parse(&d, 1).unwrap();
        assert_eq!(e.envelopes.len(), 2);
        assert_eq!(e.envelopes[0], vec![(0, 0.5), (0, 0.5)]);
        assert_eq!(e.envelopes[1], vec![(0, 1.0)]);
        assert_eq!(e.inv_bind.len(), 1);
        assert_eq!(e.inv_bind[0][1], [0.0, 1.0, 0.0, 0.0]);
    }
}
