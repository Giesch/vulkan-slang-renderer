//! BTK (likely Binary Texture Key; J3D1btk1 / TTK1) decoding: keyed face
//! texture SRT (Scale, Rotation, Translation) animation.
//!
//! TTK1 can be read as Texture Transform Key, version 1. Both BTK and TTK1
//! expansions are inferred, not verified official names.
//!
//! TTK1 (single chunk at 0x20), a 0x60 header. Main set: loop u8@0x08,
//! decimal shift u8@0x09, duration u16@0x0A, track count u16@0x0C (three
//! table entries per target), pool counts u16@0x0E/0x10/0x12, then
//! chunk-relative offsets: tables u32@0x14, remap u32@0x18, names u32@0x1C,
//! per-target tex-matrix selectors (u8) u32@0x20, centers (3×f32) u32@0x24,
//! scale pool u32@0x28, rotation pool u32@0x2C, translation pool u32@0x30.
//!
//! Post set (0x34..0x60): track count u16@0x34, pool counts u16@0x36/0x38/
//! 0x3A, tables u32@0x3C, remap u32@0x40, names u32@0x44, selectors u32@0x48,
//! centers u32@0x4C, pools u32@0x50/0x54/0x58, matrix calculation flag
//! u32@0x5C (0 basic, 1 Maya; the J3D loader maps anything else to basic).
//!
//! The main table uses the same axis-major grouping as ANK1: target *t*
//! occupies entries `3t`, `3t+1`, `3t+2` (texture axes s, t, q).

use anyhow::{Context, Result, bail};

use gx::animation_manifest::{BtkClip, BtkPostSet, BtkTarget};

use super::tracks::{AxisDescriptors, read_name_table};

pub fn parse(data: &[u8], what: &str) -> Result<BtkClip> {
    let header = super::j3d_single_chunk(data, what, "btk1", b"TTK1")
        .with_context(|| format!("{what}: not a readable BTK"))?;
    let chunk = super::chunk_slice(data, &header);

    let track_count = super::u16_at(chunk, 0x0C, "TTK1 track count")? as usize;
    if !track_count.is_multiple_of(3) {
        bail!(
            "{what}: TTK1 track count {track_count} is not a multiple of 3 (one triplet per target)"
        );
    }
    let target_count = track_count / 3;
    let main = TargetSet::read(chunk, target_count, 0x14, &format!("{what}: main set"))?;
    let post_names_off = super::u32_at(chunk, 0x44, "TTK1 post names offset")? as usize;
    let post_track_count = super::u16_at(chunk, 0x34, "TTK1 post track count")? as usize;
    if !post_track_count.is_multiple_of(3) {
        bail!("{what}: TTK1 post track count {post_track_count} is not a multiple of 3");
    }
    let post_target_count = post_track_count / 3;
    let post = if post_names_off != 0 || post_target_count != 0 {
        if post_names_off == 0 || post_target_count == 0 {
            bail!(
                "{what}: inconsistent TTK1 post set (names offset {post_names_off:#x}, post track count {post_track_count})"
            );
        }
        let set = TargetSet::read(chunk, post_target_count, 0x3C, &format!("{what}: post set"))?;
        let names = read_name_table(
            chunk,
            post_names_off,
            &format!("{what}: TTK1 post material names"),
        )?;
        if names.len() != post_target_count {
            bail!(
                "{what}: TTK1 post set has {post_target_count} rows but {} names",
                names.len()
            );
        }
        Some(BtkPostSet {
            targets: set.targets_with(names, chunk)?,
        })
    } else {
        None
    };

    let names = read_name_table(
        chunk,
        main.names_off,
        &format!("{what}: TTK1 material names"),
    )?;
    if names.len() != target_count {
        bail!(
            "{what}: TTK1 has {target_count} rows but {} material names",
            names.len()
        );
    }
    Ok(BtkClip {
        duration_frames: super::u16_at(chunk, 0x0A, "TTK1 duration")?,
        loop_attribute: super::u8_at(chunk, 0x08, "TTK1 loop attribute")?,
        rotation_decimal_shift: super::u8_at(chunk, 0x09, "TTK1 decimal shift")?,
        matrix_calc_type: super::u32_at(chunk, 0x5C, "TTK1 matrix calc type")?,
        targets: main.targets_with(names, chunk)?,
        post,
    })
}

/// Offsets block for one table set (main at base 0x14, post at base 0x3C).
/// Field order in the file: tables, remap, names, selectors, centers, then
/// pools S/R/T; the three pool *counts* sit at `base - 6/4/2`.
struct TargetSet {
    target_count: usize,
    tables_off: usize,
    remap_off: usize,
    names_off: usize,
    selectors_off: usize,
    centers_off: usize,
    scale_off: usize,
    rot_off: usize,
    trans_off: usize,
    scale_count: usize,
    rot_count: usize,
    trans_count: usize,
    what: String,
}

impl TargetSet {
    fn read(chunk: &[u8], target_count: usize, base: usize, what: &str) -> Result<Self> {
        let word = |off: usize, name: &str| super::u32_at(chunk, off, name).map(|v| v as usize);
        let half = |off: usize, name: &str| super::u16_at(chunk, off, name).map(|v| v as usize);
        Ok(Self {
            target_count,
            tables_off: word(base, "tables offset")?,
            remap_off: word(base + 4, "remap offset")?,
            names_off: word(base + 8, "names offset")?,
            selectors_off: word(base + 12, "selector offset")?,
            centers_off: word(base + 16, "center offset")?,
            scale_off: word(base + 20, "scale pool offset")?,
            rot_off: word(base + 24, "rotation pool offset")?,
            trans_off: word(base + 28, "translation pool offset")?,
            scale_count: half(base - 6, "scale pool count")?,
            rot_count: half(base - 4, "rotation pool count")?,
            trans_count: half(base - 2, "translation pool count")?,
            what: what.to_string(),
        })
    }

    fn targets_with(self, names: Vec<String>, chunk: &[u8]) -> Result<Vec<BtkTarget>> {
        let n = self.target_count;
        let table = super::pool(
            chunk,
            self.tables_off,
            n * 3 * 0x12,
            "key tables",
            &self.what,
        )?;
        let remap = super::pool(chunk, self.remap_off, n * 2, "remap", &self.what)?;
        let selectors = super::pool(chunk, self.selectors_off, n, "selectors", &self.what)?;
        let centers = super::pool(chunk, self.centers_off, n * 12, "centers", &self.what)?;
        let scale_pool = super::pool(
            chunk,
            self.scale_off,
            self.scale_count * 4,
            "scale pool",
            &self.what,
        )?;
        let rot_pool = super::pool(
            chunk,
            self.rot_off,
            self.rot_count * 2,
            "rotation pool",
            &self.what,
        )?;
        let trans_pool = super::pool(
            chunk,
            self.trans_off,
            self.trans_count * 4,
            "translation pool",
            &self.what,
        )?;

        let mut targets = Vec::with_capacity(n);
        for (t, target_name) in names.iter().enumerate() {
            let what_t = format!("{}: target {t}", self.what);
            let mut axes = Vec::with_capacity(3);
            for axis in 0..3 {
                let what_track = format!("{what_t} axis {axis}");
                let desc = AxisDescriptors::read(table, (t * 3 + axis) * 0x12)
                    .with_context(|| what_track.clone())?;
                for (d, pool_len, elem, kind) in [
                    (desc.scale, self.scale_count, 4usize, "scale"),
                    (desc.rotation, self.rot_count, 2, "rotation"),
                    (desc.translation, self.trans_count, 4, "translation"),
                ] {
                    super::check_descriptor(d, pool_len, elem, &format!("{what_track} {kind}"))?;
                }
                axes.push(desc.decode(scale_pool, rot_pool, trans_pool, &what_track)?);
            }
            let axes = <[_; 3]>::try_from(axes).expect("three axes per target");
            let mut center = [0.0f32; 3];
            for (i, c) in center.iter_mut().enumerate() {
                let off = t * 12 + i * 4;
                *c = f32::from_bits(super::u32_at(centers, off, "center component")?);
                if !c.is_finite() {
                    bail!("{what_t}: non-finite center component {i}");
                }
            }
            targets.push(BtkTarget {
                material: target_name.clone(),
                material_remap: super::u16_at(remap, t * 2, "material remap")?,
                texgen_selector: super::u8_at(selectors, t, "texgen selector")?,
                center,
                axes,
            });
        }
        Ok(targets)
    }
}

#[cfg(any(test, feature = "fixtures"))]
#[doc(hidden)]
pub mod fixtures {

    pub struct Built {
        pub file: Vec<u8>,
    }

    /// TTK1 with one main target (keyed translation per axis, split tangents)
    /// and, when `post` is true, one post target. Exercises non-identity
    /// remap, explicit selector, and a configurable matrix mode.
    pub fn build_btk(post: bool, matrix_mode: u32) -> Built {
        let mut c: Vec<u8> = vec![0; 0x60];
        c[0..4].copy_from_slice(b"TTK1");
        c[8] = 2;
        c[9] = 1; // rotation decimal shift
        c[0x0A..0x0C].copy_from_slice(&20u16.to_be_bytes());
        // pools: scale 3 words (1 per axis), rot 1, trans: 3 axes x 2 keys x 4
        let scale = [1.0f32, 2.0, 3.0];
        let rot = [0x4000i16];
        let one_axis_trans = [0.0f32, 0.0, 0.0, 0.25, 5.0, 1.0, 0.5, 0.125];
        let mut trans = Vec::new();
        for _ in 0..3 {
            trans.extend_from_slice(&one_axis_trans);
        }
        let mut body: Vec<u8> = Vec::new();
        let tables_off = 0x60;
        // All offsets below are absolute chunk offsets = 0x60 + body length.
        let abs = |len: usize| 0x60 + len;
        // main tables: 1 target x 3 axes x 0x12
        for axis in 0..3 {
            body.extend_from_slice(&1u16.to_be_bytes()); // S count 1
            body.extend_from_slice(&(axis as u16).to_be_bytes());
            body.extend_from_slice(&0u16.to_be_bytes());
            body.extend_from_slice(&1u16.to_be_bytes()); // R count 1
            body.extend_from_slice(&0u16.to_be_bytes());
            body.extend_from_slice(&0u16.to_be_bytes());
            body.extend_from_slice(&2u16.to_be_bytes()); // T count 2, split tangents
            body.extend_from_slice(&(axis as u16 * 8).to_be_bytes());
            body.extend_from_slice(&1u16.to_be_bytes()); // tangent type 1
        }
        let remap_off = abs(body.len());
        body.extend_from_slice(&7u16.to_be_bytes()); // non-identity remap
        let names_off = abs(body.len());
        let name_bytes = name_table(&["eyeL"]);
        body.extend_from_slice(&name_bytes);
        let selectors_off = abs(body.len());
        body.extend_from_slice(&[2u8]); // texgen selector
        let centers_off = abs(body.len());
        for v in [0.5f32, -0.5, 0.25] {
            body.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        let scale_off = abs(body.len());
        for v in scale {
            body.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        let rot_off = abs(body.len());
        for v in rot {
            body.extend_from_slice(&v.to_be_bytes());
        }
        let trans_off = abs(body.len());
        for v in trans {
            body.extend_from_slice(&v.to_bits().to_be_bytes());
        }

        // post set appended after the main set
        let (
            post_tables_off,
            post_remap_off,
            post_names_off,
            post_selectors_off,
            post_centers_off,
            post_scale_off,
            post_rot_off,
            post_trans_off,
            post_track_count,
        );
        if post {
            post_track_count = 3u16;
            post_tables_off = abs(body.len());
            // one post target with all-default tracks: 3 axes x 3 descriptors
            // (count/index/tangent) of 6 bytes each
            for _ in 0..3 * 3 {
                body.extend_from_slice(&0u16.to_be_bytes());
                body.extend_from_slice(&0u16.to_be_bytes());
                body.extend_from_slice(&0u16.to_be_bytes());
            }
            post_remap_off = abs(body.len());
            body.extend_from_slice(&0u16.to_be_bytes());
            post_names_off = abs(body.len());
            body.extend_from_slice(&name_table(&["eyeR"]));
            post_selectors_off = abs(body.len());
            body.extend_from_slice(&[1u8]);
            post_centers_off = abs(body.len());
            for v in [0.0f32, 0.0, 0.0] {
                body.extend_from_slice(&v.to_bits().to_be_bytes());
            }
            post_scale_off = abs(body.len());
            post_rot_off = post_scale_off;
            post_trans_off = post_scale_off;
        } else {
            post_track_count = 0;
            post_tables_off = 0;
            post_remap_off = 0;
            post_names_off = 0;
            post_selectors_off = 0;
            post_centers_off = 0;
            post_scale_off = 0;
            post_rot_off = 0;
            post_trans_off = 0;
        }

        // main header fields
        c[0x0C..0x0E].copy_from_slice(&3u16.to_be_bytes()); // track count = 1 target * 3
        c[0x0E..0x10].copy_from_slice(&3u16.to_be_bytes()); // scale count
        c[0x10..0x12].copy_from_slice(&1u16.to_be_bytes()); // rot count
        c[0x12..0x14].copy_from_slice(&24u16.to_be_bytes()); // trans count (3x8)
        c[0x14..0x18].copy_from_slice(&(tables_off as u32).to_be_bytes());
        c[0x18..0x1C].copy_from_slice(&(remap_off as u32).to_be_bytes());
        c[0x1C..0x20].copy_from_slice(&(names_off as u32).to_be_bytes());
        c[0x20..0x24].copy_from_slice(&(selectors_off as u32).to_be_bytes());
        c[0x24..0x28].copy_from_slice(&(centers_off as u32).to_be_bytes());
        c[0x28..0x2C].copy_from_slice(&(scale_off as u32).to_be_bytes());
        c[0x2C..0x30].copy_from_slice(&(rot_off as u32).to_be_bytes());
        c[0x30..0x34].copy_from_slice(&(trans_off as u32).to_be_bytes());
        // post header fields
        c[0x34..0x36].copy_from_slice(&post_track_count.to_be_bytes());
        c[0x36..0x38].copy_from_slice(&0u16.to_be_bytes());
        c[0x38..0x3A].copy_from_slice(&0u16.to_be_bytes());
        c[0x3A..0x3C].copy_from_slice(&0u16.to_be_bytes());
        c[0x3C..0x40].copy_from_slice(&(post_tables_off as u32).to_be_bytes());
        c[0x40..0x44].copy_from_slice(&(post_remap_off as u32).to_be_bytes());
        c[0x44..0x48].copy_from_slice(&(post_names_off as u32).to_be_bytes());
        c[0x48..0x4C].copy_from_slice(&(post_selectors_off as u32).to_be_bytes());
        c[0x4C..0x50].copy_from_slice(&(post_centers_off as u32).to_be_bytes());
        c[0x50..0x54].copy_from_slice(&(post_scale_off as u32).to_be_bytes());
        c[0x54..0x58].copy_from_slice(&(post_rot_off as u32).to_be_bytes());
        c[0x58..0x5C].copy_from_slice(&(post_trans_off as u32).to_be_bytes());
        c[0x5C..0x60].copy_from_slice(&matrix_mode.to_be_bytes());

        let mut file = vec![0u8; 0x20];
        file[0..4].copy_from_slice(b"J3D1");
        file[4..8].copy_from_slice(b"btk1");
        file[12..16].copy_from_slice(&1u32.to_be_bytes());
        file.extend_from_slice(&c);
        file.extend_from_slice(&body);
        let chunk_len = 0x60 + body.len();
        file[8..12].copy_from_slice(&((0x20 + chunk_len) as u32).to_be_bytes());
        // chunk size word (in the chunk header)
        file[0x24..0x28].copy_from_slice(&(chunk_len as u32).to_be_bytes());
        Built { file }
    }

    pub fn name_table(names: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(names.len() as u16).to_be_bytes());
        out.extend_from_slice(&0xFFFFu16.to_be_bytes());
        let mut data_off = 4 + 4 * names.len();
        let mut entries = Vec::new();
        let mut strings = Vec::new();
        for n in names {
            entries.extend_from_slice(&0u16.to_be_bytes());
            entries.extend_from_slice(&(data_off as u16).to_be_bytes());
            strings.extend_from_slice(n.as_bytes());
            strings.push(0);
            data_off += n.len() + 1;
        }
        out.extend_from_slice(&entries);
        out.extend_from_slice(&strings);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::build_btk;
    use super::parse;
    use gx::animation_manifest::{TrackF32, TrackI16};

    #[test]
    fn btk_synthetic_cases() {
        let built = build_btk(false, 1);
        let clip = parse(&built.file, "synthetic.btk").unwrap();
        assert_eq!(clip.duration_frames, 20);
        assert_eq!(clip.rotation_decimal_shift, 1);
        assert_eq!(clip.matrix_calc_type, 1);
        assert!(clip.post.is_none());
        assert_eq!(clip.targets.len(), 1);
        let t = &clip.targets[0];
        assert_eq!(t.material, "eyeL");
        assert_eq!(t.material_remap, 7);
        assert_eq!(t.texgen_selector, 2);
        assert_eq!(t.center, [0.5, -0.5, 0.25]);
        for (axis, a) in t.axes.iter().enumerate() {
            assert_eq!(
                a.scale,
                TrackF32::Constant {
                    value: axis as f32 + 1.0
                }
            );
            assert_eq!(a.rotation, TrackI16::Constant { value: 0x4000 });
            match &a.translation {
                TrackF32::Keyed { tangent_type, keys } => {
                    assert_eq!(tangent_type, &1);
                    assert_eq!(keys.len(), 2);
                    assert_eq!(keys[0].time, 0.0);
                    assert_eq!(keys[1].time, 5.0);
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn btk_post_tracks_preserved() {
        let built = build_btk(true, 0);
        let clip = parse(&built.file, "synthetic-post.btk").unwrap();
        let post = clip.post.as_ref().expect("post set");
        assert_eq!(post.targets.len(), 1);
        let t = &post.targets[0];
        assert_eq!(t.material, "eyeR");
        assert_eq!(t.texgen_selector, 1);
        for a in t.axes.iter() {
            assert!(matches!(a.scale, TrackF32::Default));
            assert!(matches!(a.rotation, TrackI16::Default));
        }
    }

    #[test]
    fn btk_malformed_inputs() {
        let built = build_btk(false, 0);
        let good = built.file;
        // track count not a multiple of 3
        let mut bad = good.clone();
        bad[0x20 + 0x0C..0x20 + 0x0E].copy_from_slice(&4u16.to_be_bytes());
        assert!(parse(&bad, "m.btk").is_err());
        // inconsistent post set: names offset set but zero post tracks
        let mut bad = good.clone();
        bad[0x20 + 0x44..0x20 + 0x48].copy_from_slice(&0x80u32.to_be_bytes());
        assert!(parse(&bad, "m.btk").is_err());
        // pools out of the chunk
        let mut bad = good.clone();
        bad[0x20 + 0x2C..0x20 + 0x30].copy_from_slice(&0xFFFFu32.to_be_bytes());
        assert!(parse(&bad, "m.btk").is_err());
        // wrong file type
        let mut bad = good.clone();
        bad[4..8].copy_from_slice(b"bck1");
        assert!(parse(&bad, "m.btk").is_err());
        // zero targets is fine (empty clip), but a bad name-table offset is not
        let mut bad = good.clone();
        bad[0x20 + 0x1C..0x20 + 0x20].copy_from_slice(&0xFFFFu32.to_be_bytes());
        assert!(parse(&bad, "m.btk").is_err());
    }
}
