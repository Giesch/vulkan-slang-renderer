//! JNT1 chunk: joint (bone) transforms. Structure (J3DJointFactory.h,
//! confirmed against noclip's J3DLoader and gclib jnt1.py): u16 count at +8,
//! u32 offsets at +0x0C init-data, +0x10 remap (index) table, +0x14 name
//! table. Each on-disk joint record is 0x40 bytes; `joints[i]` resolves
//! through the remap table to `initData[remap[i]]` (the tww factory does the
//! same). Parentage is NOT here — it comes from INF1.
//!
//! Every scale in cl.bdl is exactly (1,1,1), which the parser hard-asserts:
//! it collapses the Maya scaling-rule / no-inherit-scale subtleties so FK
//! reduces to `world = parent · T · R` (see pose.rs). Rotations are kept as
//! raw s16 (0x8000 = -π); radians are derived in pose.rs.

use crate::be::BeReader;
use crate::bmd::{BmdError, read_name_table};

/// On-disk joint record size (J3DJointInitData + bounds), verified against
/// noclip / BMDView: u16 kind, u8 calc-flag, pad, scale f32×3, rot s16×3,
/// pad, translate f32×3, radius f32, bbox min/max f32×3 each.
const JOINT_RECORD_SIZE: usize = 0x40;

#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    pub name: String,
    pub matrix_type: u16,
    pub no_inherit_scale: u8,
    pub scale: [f32; 3],
    pub rotation_s16: [i16; 3],
    pub translation: [f32; 3],
    pub radius: f32,
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
}

pub struct Jnt1 {
    pub joints: Vec<Joint>,
}

pub fn parse(chunk: &[u8]) -> Result<Jnt1, BmdError> {
    let r = BeReader::new(chunk);
    let mut h = r.at(8);
    let count = h.u16()? as usize;
    h.skip(2)?;
    let init_off = h.u32()? as usize;
    let remap_off = h.u32()? as usize;
    let name_off = h.u32()? as usize;

    let names = read_name_table(&r, name_off)?;
    if names.len() != count {
        return Err(BmdError::Invariant(format!(
            "JNT1 has {count} joints but {} names",
            names.len()
        )));
    }

    let mut joints = Vec::with_capacity(count);
    for (i, name) in names.iter().enumerate() {
        let remap = r.at(remap_off + i * 2).u16()? as usize;
        let mut rec = r.at(init_off + remap * JOINT_RECORD_SIZE);
        let matrix_type = rec.u16()?;
        let no_inherit_scale = rec.u8()?;
        rec.skip(1)?; // pad
        let scale = [rec.f32()?, rec.f32()?, rec.f32()?];
        let rotation_s16 = [rec.i16()?, rec.i16()?, rec.i16()?];
        rec.skip(2)?; // pad
        let translation = [rec.f32()?, rec.f32()?, rec.f32()?];
        let radius = rec.f32()?;
        let bbox_min = [rec.f32()?, rec.f32()?, rec.f32()?];
        let bbox_max = [rec.f32()?, rec.f32()?, rec.f32()?];

        if scale != [1.0, 1.0, 1.0] {
            return Err(BmdError::Invariant(format!(
                "JNT1 joint {i} ({name}) has non-unit scale {scale:?}; \
                 pose.rs assumes unit scale (cl.bdl invariant)"
            )));
        }

        joints.push(Joint {
            name: name.clone(),
            matrix_type,
            no_inherit_scale,
            scale,
            rotation_s16,
            translation,
            radius,
            bbox_min,
            bbox_max,
        });
    }

    Ok(Jnt1 { joints })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One joint record with distinct field values (identity remap, one name).
    fn synth_one_joint() -> Vec<u8> {
        // header (0x18) + name table + remap table + init data, all packed.
        let mut data = vec![0u8; 8];
        let name_tab_off = 0x18usize;
        // header: count, pad, init_off, remap_off, name_off
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&0u16.to_be_bytes());
        // offsets filled after we know the layout
        let name_bytes = {
            // JUTNameTab: u16 count, u16 pad, [u16 hash,u16 offset], then strings
            let mut t = Vec::new();
            t.extend_from_slice(&1u16.to_be_bytes());
            t.extend_from_slice(&0u16.to_be_bytes());
            t.extend_from_slice(&0u16.to_be_bytes()); // hash
            t.extend_from_slice(&8u16.to_be_bytes()); // str offset from table start
            t.extend_from_slice(b"j\0");
            t
        };
        let remap_off = name_tab_off + name_bytes.len();
        let init_off = remap_off + 2;
        data.extend_from_slice(&(init_off as u32).to_be_bytes());
        data.extend_from_slice(&(remap_off as u32).to_be_bytes());
        data.extend_from_slice(&(name_tab_off as u32).to_be_bytes());
        assert_eq!(data.len(), name_tab_off);
        data.extend_from_slice(&name_bytes);
        data.extend_from_slice(&0u16.to_be_bytes()); // remap[0] = 0
        // init record 0x40
        let mut rec = Vec::new();
        rec.extend_from_slice(&7u16.to_be_bytes()); // matrix_type
        rec.push(1); // no_inherit_scale
        rec.push(0xFF); // pad
        for v in [1.0f32, 1.0, 1.0] {
            rec.extend_from_slice(&v.to_be_bytes());
        }
        for v in [0i16, -32768, 16384] {
            rec.extend_from_slice(&v.to_be_bytes());
        }
        rec.extend_from_slice(&0u16.to_be_bytes()); // pad
        for v in [10.0f32, 20.0, 30.0] {
            rec.extend_from_slice(&v.to_be_bytes());
        }
        rec.extend_from_slice(&99.0f32.to_be_bytes()); // radius
        for v in [-1.0f32, -2.0, -3.0] {
            rec.extend_from_slice(&v.to_be_bytes());
        }
        for v in [1.0f32, 2.0, 3.0] {
            rec.extend_from_slice(&v.to_be_bytes());
        }
        assert_eq!(rec.len(), JOINT_RECORD_SIZE);
        data.extend_from_slice(&rec);
        data
    }

    #[test]
    fn parses_one_joint() {
        let data = synth_one_joint();
        let jnt1 = parse(&data).unwrap();
        assert_eq!(jnt1.joints.len(), 1);
        let j = &jnt1.joints[0];
        assert_eq!(j.name, "j");
        assert_eq!(j.matrix_type, 7);
        assert_eq!(j.no_inherit_scale, 1);
        assert_eq!(j.rotation_s16, [0, -32768, 16384]);
        assert_eq!(j.translation, [10.0, 20.0, 30.0]);
        assert_eq!(j.radius, 99.0);
        assert_eq!(j.bbox_min, [-1.0, -2.0, -3.0]);
        assert_eq!(j.bbox_max, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn non_unit_scale_is_error() {
        let mut data = synth_one_joint();
        // scale.x lives at init record + 0x04; init record is the last 0x40
        // bytes. Overwrite it with 2.0.
        let init = data.len() - JOINT_RECORD_SIZE;
        data[init + 4..init + 8].copy_from_slice(&2.0f32.to_be_bytes());
        assert!(matches!(parse(&data), Err(BmdError::Invariant(_))));
    }
}
