//! DRW1 chunk: the draw-matrix table. Structure (J3DModelLoader.h
//! J3DDrawBlock, readDraw): u16 slot count at +8, then u32 offsets at +0x0C
//! isWeighted flags (u8 per slot) and +0x10 indices (u16 per slot). Flag 0 →
//! index is a JNT1 joint (rigid); flag 1 → index is an EVP1 envelope
//! (weighted). In cl.bdl the rigid slots are packed before the weighted ones.
//!
//! SHP1 matrix tables index these slots; pose.rs resolves each slot to a
//! skinning matrix (joint world, or the envelope's weighted blend).

use crate::be::BeReader;
use crate::bmd::BmdError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrwSlot {
    Joint(u16),
    Envelope(u16),
}

pub struct Drw1 {
    pub slots: Vec<DrwSlot>,
}

pub fn parse(chunk: &[u8], joint_count: u16, envelope_count: u16) -> Result<Drw1, BmdError> {
    let r = BeReader::new(chunk);
    let mut h = r.at(8);
    let count = h.u16()? as usize;
    h.skip(2)?;
    let flags_off = h.u32()? as usize;
    let index_off = h.u32()? as usize;

    let mut flags = r.at(flags_off);
    let mut indices = r.at(index_off);
    let mut slots = Vec::with_capacity(count);
    for i in 0..count {
        let flag = flags.u8()?;
        let index = indices.u16()?;
        let slot = match flag {
            0 => {
                if index >= joint_count {
                    return Err(BmdError::Invariant(format!(
                        "DRW1 slot {i} references joint {index} of {joint_count}"
                    )));
                }
                DrwSlot::Joint(index)
            }
            1 => {
                if index >= envelope_count {
                    return Err(BmdError::Invariant(format!(
                        "DRW1 slot {i} references envelope {index} of {envelope_count}"
                    )));
                }
                DrwSlot::Envelope(index)
            }
            other => {
                return Err(BmdError::Invariant(format!(
                    "DRW1 slot {i} has flag {other}, expected 0 (joint) or 1 (envelope)"
                )));
            }
        };
        slots.push(slot);
    }

    Ok(Drw1 { slots })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_rigid_and_weighted() {
        let hdr = 0x14usize;
        let flags_off = hdr;
        let index_off = flags_off + 3; // 3 slots
        let total = index_off + 3 * 2;
        let mut d = vec![0u8; hdr];
        d[8..10].copy_from_slice(&3u16.to_be_bytes());
        d[0x0C..0x10].copy_from_slice(&(flags_off as u32).to_be_bytes());
        d[0x10..0x14].copy_from_slice(&(index_off as u32).to_be_bytes());
        d.resize(total, 0);
        d[flags_off] = 0; // joint
        d[flags_off + 1] = 0; // joint
        d[flags_off + 2] = 1; // envelope
        for (i, v) in [5u16, 6, 2].iter().enumerate() {
            let at = index_off + i * 2;
            d[at..at + 2].copy_from_slice(&v.to_be_bytes());
        }
        let drw = parse(&d, 42, 3).unwrap();
        assert_eq!(
            drw.slots,
            vec![DrwSlot::Joint(5), DrwSlot::Joint(6), DrwSlot::Envelope(2)]
        );
    }

    #[test]
    fn out_of_range_joint_is_error() {
        let hdr = 0x14usize;
        let flags_off = hdr;
        let index_off = flags_off + 1;
        let total = index_off + 2;
        let mut d = vec![0u8; hdr];
        d[8..10].copy_from_slice(&1u16.to_be_bytes());
        d[0x0C..0x10].copy_from_slice(&(flags_off as u32).to_be_bytes());
        d[0x10..0x14].copy_from_slice(&(index_off as u32).to_be_bytes());
        d.resize(total, 0);
        d[flags_off] = 0;
        d[index_off..index_off + 2].copy_from_slice(&99u16.to_be_bytes());
        assert!(matches!(parse(&d, 42, 3), Err(BmdError::Invariant(_))));
    }
}
