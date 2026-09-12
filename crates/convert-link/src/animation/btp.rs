//! BTP (J3D1btp1 / TPT1) decoding: stepped face texture-pattern animation.
//!
//! TPT1 (single chunk at 0x20): loop u8@0x08, pad u8@0x09, duration u16@0x0A,
//! anim count u16@0x0C, texture-index pool count u16@0x0E, then chunk-relative
//! offsets: anim table u32@0x10, u16 value pool u32@0x14, material remap
//! u32@0x18, material names u32@0x1C.
//!
//! Each 8-byte table row is `{u16 count, u16 index, u8 tex-map-slot, u8 pad,
//! u16 pad}`; its samples are `pool[index .. index+count]` stepped u16
//! texture indices. The remap array is one author-time u16 per row; the J3D
//! runtime replaces it by material-name lookup
//! (`J3DAnmTexPattern::searchUpdateMaterialID`), so rows keep both the name
//! and the raw u16.

use anyhow::{Context, Result};

use gx::animation_manifest::{BtpClip, BtpTarget};

pub fn parse(data: &[u8], what: &str) -> Result<BtpClip> {
    let header = super::j3d_single_chunk(data, what, "btp1", b"TPT1")
        .with_context(|| format!("{what}: not a readable BTP"))?;
    let chunk = super::chunk_slice(data, &header);

    let anim_count = super::u16_at(chunk, 0x0C, "TPT1 anim count")? as usize;
    let value_count = super::u16_at(chunk, 0x0E, "TPT1 value count")? as usize;
    let table_off = super::u32_at(chunk, 0x10, "TPT1 table offset")? as usize;
    let values_off = super::u32_at(chunk, 0x14, "TPT1 values offset")? as usize;
    let remap_off = super::u32_at(chunk, 0x18, "TPT1 remap offset")? as usize;
    let names_off = super::u32_at(chunk, 0x1C, "TPT1 names offset")? as usize;

    let table = super::pool(chunk, table_off, anim_count * 8, "anim table", what)?;
    let values = super::pool(chunk, values_off, value_count * 2, "value pool", what)?;
    let remap = super::pool(chunk, remap_off, anim_count * 2, "remap table", what)?;

    let names =
        super::tracks::read_name_table(chunk, names_off, &format!("{what}: TPT1 material names"))?;
    if names.len() != anim_count {
        anyhow::bail!(
            "{what}: TPT1 has {anim_count} anim rows but {} material names",
            names.len()
        );
    }

    let mut targets = Vec::with_capacity(anim_count);
    for (i, material) in names.iter().enumerate() {
        let row = i * 8;
        let what_row = format!("{what}: anim row {i}");
        let count = super::u16_at(table, row, "row sample count")? as usize;
        let index = super::u16_at(table, row + 2, "row sample index")? as usize;
        let texture_map_slot = super::u8_at(table, row + 4, "row tex-map slot")?;
        let end = index
            .checked_add(count)
            .ok_or_else(|| anyhow::anyhow!("{what_row}: sample range overflows"))?;
        if end > value_count {
            anyhow::bail!(
                "{what_row}: samples {index}..{end} exceed the {value_count}-entry value pool"
            );
        }
        let mut texture_indices = Vec::with_capacity(count);
        for k in 0..count {
            texture_indices.push(super::u16_at(values, (index + k) * 2, "texture index")?);
        }
        targets.push(BtpTarget {
            material: material.clone(),
            material_remap: super::u16_at(remap, i * 2, "material remap")?,
            texture_map_slot,
            texture_indices,
        });
    }

    Ok(BtpClip {
        duration_frames: super::u16_at(chunk, 0x0A, "TPT1 duration")?,
        loop_attribute: super::u8_at(chunk, 0x08, "TPT1 loop attribute")?,
        targets,
    })
}

#[cfg(test)]
pub(crate) mod fixtures {

    /// TPT1 with two rows: 3 stepped samples and 1 constant sample, an
    /// explicitly non-identity remap, and duplicate material names.
    pub(crate) fn build_btp() -> Vec<u8> {
        let mut chunk: Vec<u8> = vec![0; 0x20];
        chunk[0..4].copy_from_slice(b"TPT1");
        chunk[8] = 2; // loop
        chunk[9] = 0xFF; // pad
        chunk[0x0A..0x0C].copy_from_slice(&10u16.to_be_bytes()); // duration
        chunk[0x0C..0x0E].copy_from_slice(&2u16.to_be_bytes()); // anims
        chunk[0x0E..0x10].copy_from_slice(&4u16.to_be_bytes()); // values
        let table_off = 0x20;
        chunk[0x10..0x14].copy_from_slice(&(table_off as u32).to_be_bytes());
        // table (2 * 8)
        chunk.extend_from_slice(&3u16.to_be_bytes()); // row 0 count
        chunk.extend_from_slice(&0u16.to_be_bytes()); // index 0
        chunk.push(0); // tex no
        chunk.push(0);
        chunk.extend_from_slice(&0u16.to_be_bytes());
        chunk.extend_from_slice(&1u16.to_be_bytes()); // row 1 count
        chunk.extend_from_slice(&3u16.to_be_bytes()); // index 3
        chunk.push(1); // tex no
        chunk.push(0);
        chunk.extend_from_slice(&0u16.to_be_bytes());
        let values_off = chunk.len();
        chunk[0x14..0x18].copy_from_slice(&(values_off as u32).to_be_bytes());
        for v in [27u16, 7, 7, 9] {
            chunk.extend_from_slice(&v.to_be_bytes());
        }
        let remap_off = chunk.len();
        chunk[0x18..0x1C].copy_from_slice(&(remap_off as u32).to_be_bytes());
        for r in [14u16, 1] {
            chunk.extend_from_slice(&r.to_be_bytes());
        }
        let names_off = chunk.len();
        chunk[0x1C..0x20].copy_from_slice(&(names_off as u32).to_be_bytes());
        // ResNTAB: count 2, pad, entries, strings; duplicate name on purpose
        chunk.extend_from_slice(&2u16.to_be_bytes());
        chunk.extend_from_slice(&0xFFFFu16.to_be_bytes());
        let strings_off = 4 + 2 * 4;
        let put_off = |chunk: &mut Vec<u8>, off: u16| {
            chunk.extend_from_slice(&0u16.to_be_bytes());
            chunk.extend_from_slice(&off.to_be_bytes());
        };
        let first = strings_off;
        put_off(&mut chunk, first);
        let second = strings_off + "mouth\0".len() as u16;
        put_off(&mut chunk, second);
        chunk.extend_from_slice(b"mouth\0");
        chunk.extend_from_slice(b"mouth\0");
        while !chunk.len().is_multiple_of(0x20) {
            chunk.push(0);
        }
        let chunk_len = chunk.len();
        chunk[4..8].copy_from_slice(&(chunk_len as u32).to_be_bytes());

        let mut file = vec![0u8; 0x20];
        file[0..4].copy_from_slice(b"J3D1");
        file[4..8].copy_from_slice(b"btp1");
        file[8..12].copy_from_slice(&((0x20 + chunk.len()) as u32).to_be_bytes());
        file[12..16].copy_from_slice(&1u32.to_be_bytes());
        file.extend_from_slice(&chunk);
        file
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::build_btp;
    use super::parse;

    #[test]
    fn btp_synthetic_cases() {
        let data = build_btp();
        let clip = parse(&data, "synthetic.btp").unwrap();
        assert_eq!(clip.duration_frames, 10);
        assert_eq!(clip.loop_attribute, 2);
        assert_eq!(clip.targets.len(), 2);
        let t0 = &clip.targets[0];
        assert_eq!(t0.material, "mouth");
        assert_eq!(t0.material_remap, 14);
        assert_eq!(t0.texture_map_slot, 0);
        assert_eq!(t0.texture_indices, vec![27, 7, 7]);
        let t1 = &clip.targets[1];
        assert_eq!(t1.material, "mouth"); // duplicate name preserved as its own row
        assert_eq!(t1.material_remap, 1);
        assert_eq!(t1.texture_indices, vec![9]);
        // sample count (3) intentionally != duration (10)
        assert_ne!(t0.texture_indices.len() as u16, clip.duration_frames);
    }

    #[test]
    fn btp_malformed_inputs() {
        let good = build_btp();
        let table_off =
            u32::from_be_bytes([good[0x30], good[0x31], good[0x32], good[0x33]]) as usize;
        // sample run past the value pool
        let mut bad = good.clone();
        bad[0x20 + table_off..0x20 + table_off + 2].copy_from_slice(&99u16.to_be_bytes());
        assert!(parse(&bad, "m.btp").is_err());
        // name table shorter than anim rows
        let mut bad = good.clone();
        bad[0x20 + 0x0C..0x20 + 0x0E].copy_from_slice(&3u16.to_be_bytes());
        assert!(parse(&bad, "m.btp").is_err());
        // wrong chunk magic
        let mut bad = good.clone();
        bad[0x20..0x24].copy_from_slice(b"TTK1");
        assert!(parse(&bad, "m.btp").is_err());
    }
}
