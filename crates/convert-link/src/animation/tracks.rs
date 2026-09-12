//! Shared J3D key-table decoding for ANK1/TTK1 tracks.
//!
//! Both BCK (body) and BTK (texture SRT) store, per target and per axis, a
//! triplet of 6-byte key descriptors (`u16 count, u16 index, u16
//! tangent_type`) indexing into a shared value pool. The J3D reader semantics
//! (`J3DAnmKeyLoader_v15` / `J3DAnimation.cpp`) are:
//!
//! - `count == 0` — the track uses the built-in default (scale 1.0, rotation
//!   0, translation 0 depending on track kind); the index word is ignored.
//! - `count == 1` — a single value read from `pool[index]`; the tangent word
//!   is ignored.
//! - `count >= 2` — `count` keys starting at `pool[index]`, stride 3 when
//!   `tangent_type == 0` (time, value, one shared tangent) and stride 4 for
//!   any nonzero tangent word (time, value, tangent-in, tangent-out). The
//!   raw tangent word is preserved even when it exceeds 1.
//!
//! Every read is bounds-checked against the chunk-local pool slice and every
//! multi-key time sequence must be strictly increasing; failures return
//! contextual errors identifying the track and offset.

use anyhow::{Context, Result, anyhow, bail};

use gx::animation_manifest::{KeyF32, KeyI16, TrackF32, TrackI16};

/// A 6-byte key descriptor as stored in the table.
#[derive(Debug, Clone, Copy)]
pub struct KeyDescriptor {
    pub count: u16,
    pub index: u16,
    pub tangent_type: u16,
}

impl KeyDescriptor {
    /// Read the descriptor at `offset` inside `table` (chunk-local slice).
    pub fn read(table: &[u8], offset: usize) -> Result<Self> {
        let bytes = table.get(offset..offset + 6).ok_or_else(|| {
            anyhow!(
                "key descriptor at {offset:#x} outside the {:#x}-byte table",
                table.len()
            )
        })?;
        Ok(Self {
            count: u16::from_be_bytes([bytes[0], bytes[1]]),
            index: u16::from_be_bytes([bytes[2], bytes[3]]),
            tangent_type: u16::from_be_bytes([bytes[4], bytes[5]]),
        })
    }

    /// Words per key in the pool for this descriptor's tangent type.
    fn stride(&self) -> usize {
        if self.tangent_type == 0 { 3 } else { 4 }
    }
}

fn strictly_increasing_i16(times: impl Iterator<Item = i16>, what: &str) -> Result<()> {
    let mut prev: Option<i16> = None;
    for t in times {
        if let Some(p) = prev
            && t <= p
        {
            bail!("{what}: key time {t} does not strictly increase (previous {p})");
        }
        prev = Some(t);
    }
    Ok(())
}

fn strictly_increasing_f32(times: impl Iterator<Item = f32>, what: &str) -> Result<()> {
    let mut prev: Option<f32> = None;
    for t in times {
        if !t.is_finite() {
            bail!("{what}: non-finite key time {t}");
        }
        if let Some(p) = prev
            && t <= p
        {
            bail!("{what}: key time {t} does not strictly increase (previous {p})");
        }
        prev = Some(t);
    }
    Ok(())
}

/// Decode an f32 track (scale / translation) from `pool` (4-byte words).
pub fn read_track_f32(desc: KeyDescriptor, pool: &[u8], what: &str) -> Result<TrackF32> {
    let read_word = |i: usize| -> Result<f32> {
        let b = pool.get(i * 4..i * 4 + 4).ok_or_else(|| {
            anyhow!(
                "{what}: pool word {i} outside the {word}-word pool",
                word = pool.len() / 4
            )
        })?;
        Ok(f32::from_bits(u32::from_be_bytes([b[0], b[1], b[2], b[3]])))
    };
    match desc.count {
        0 => Ok(TrackF32::Default),
        1 => {
            let value = read_word(desc.index as usize).map_err(|e| e.context(what.to_string()))?;
            if !value.is_finite() {
                bail!("{what}: non-finite constant value {value}");
            }
            Ok(TrackF32::Constant { value })
        }
        count => {
            let stride = desc.stride();
            let mut keys = Vec::with_capacity(count as usize);
            let mut words = desc.index as usize;
            for k in 0..count as usize {
                let time =
                    read_word(words).map_err(|e| e.context(format!("{what}: key {k} time")))?;
                let value = read_word(words + 1)
                    .map_err(|e| e.context(format!("{what}: key {k} value")))?;
                let tangent_in = read_word(words + 2)
                    .map_err(|e| e.context(format!("{what}: key {k} tangent-in")))?;
                let tangent_out = if stride == 4 {
                    read_word(words + 3)
                        .map_err(|e| e.context(format!("{what}: key {k} tangent-out")))?
                } else {
                    tangent_in
                };
                words += stride;
                keys.push(KeyF32 {
                    time,
                    value,
                    tangent_in,
                    tangent_out,
                });
            }
            strictly_increasing_f32(keys.iter().map(|k| k.time), what)?;
            if keys.iter().any(|k| {
                !k.value.is_finite() || !k.tangent_in.is_finite() || !k.tangent_out.is_finite()
            }) {
                bail!("{what}: non-finite key data");
            }
            Ok(TrackF32::Keyed {
                tangent_type: desc.tangent_type,
                keys,
            })
        }
    }
}

/// Decode an i16 track (rotation) from `pool` (2-byte words).
pub fn read_track_i16(desc: KeyDescriptor, pool: &[u8], what: &str) -> Result<TrackI16> {
    let read_word = |i: usize| -> Result<i16> {
        let b = pool.get(i * 2..i * 2 + 2).ok_or_else(|| {
            anyhow!(
                "{what}: pool word {i} outside the {word}-word pool",
                word = pool.len() / 2
            )
        })?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    };
    match desc.count {
        0 => Ok(TrackI16::Default),
        1 => Ok(TrackI16::Constant {
            value: read_word(desc.index as usize).map_err(|e| e.context(what.to_string()))?,
        }),
        count => {
            let stride = desc.stride();
            let mut keys = Vec::with_capacity(count as usize);
            let mut words = desc.index as usize;
            for k in 0..count as usize {
                let time =
                    read_word(words).map_err(|e| e.context(format!("{what}: key {k} time")))?;
                let value = read_word(words + 1)
                    .map_err(|e| e.context(format!("{what}: key {k} value")))?;
                let tangent_in = read_word(words + 2)
                    .map_err(|e| e.context(format!("{what}: key {k} tangent-in")))?;
                let tangent_out = if stride == 4 {
                    read_word(words + 3)
                        .map_err(|e| e.context(format!("{what}: key {k} tangent-out")))?
                } else {
                    tangent_in
                };
                words += stride;
                keys.push(KeyI16 {
                    time,
                    value,
                    tangent_in,
                    tangent_out,
                });
            }
            strictly_increasing_i16(keys.iter().map(|k| k.time), what)?;
            Ok(TrackI16::Keyed {
                tangent_type: desc.tangent_type,
                keys,
            })
        }
    }
}

/// Read one axis triplet {S, R, T} of descriptors at `offset` (0x12 bytes).
pub struct AxisDescriptors {
    pub scale: KeyDescriptor,
    pub rotation: KeyDescriptor,
    pub translation: KeyDescriptor,
}

impl AxisDescriptors {
    pub fn read(table: &[u8], offset: usize) -> Result<Self> {
        Ok(Self {
            scale: KeyDescriptor::read(table, offset).context("scale track")?,
            rotation: KeyDescriptor::read(table, offset + 6).context("rotation track")?,
            translation: KeyDescriptor::read(table, offset + 12).context("translation track")?,
        })
    }

    pub fn decode(
        &self,
        scale_pool: &[u8],
        rot_pool: &[u8],
        trans_pool: &[u8],
        what: &str,
    ) -> Result<gx::animation_manifest::AxisSrt> {
        use gx::animation_manifest::AxisSrt;
        Ok(AxisSrt {
            scale: read_track_f32(self.scale, scale_pool, &format!("{what} scale"))?,
            rotation: read_track_i16(self.rotation, rot_pool, &format!("{what} rotation"))?,
            translation: read_track_f32(
                self.translation,
                trans_pool,
                &format!("{what} translation"),
            )?,
        })
    }
}

/// Read a bounded NUL-terminated ASCII string at `offset` inside `data`.
pub fn read_cstr(data: &[u8], offset: usize, what: &str) -> Result<String> {
    let rest = data
        .get(offset..)
        .ok_or_else(|| anyhow!("{what}: string offset {offset:#x} out of bounds"))?;
    let len = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| anyhow!("{what}: string at {offset:#x} is not NUL-terminated"))?;
    std::str::from_utf8(&rest[..len])
        .map(|s| s.to_owned())
        .map_err(|_| anyhow!("{what}: string at {offset:#x} is not UTF-8/ASCII"))
}

/// Read a JSystem ResNTAB name table at `offset` inside `data`:
/// `{u16 count, u16 pad, {u16 hash, u16 data-offset}[count], names…}` where
/// each name's offset is relative to the table start.
pub fn read_name_table(data: &[u8], offset: usize, what: &str) -> Result<Vec<String>> {
    if data.len() < offset + 4 {
        bail!("{what}: name table at {offset:#x} out of bounds");
    }
    let count = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
    let mut names = Vec::with_capacity(count);
    for i in 0..count {
        let entry = offset + 4 + 4 * i;
        if data.len() < entry + 4 {
            bail!("{what}: name entry {i} at {entry:#x} out of bounds");
        }
        let name_off = u16::from_be_bytes([data[entry + 2], data[entry + 3]]) as usize;
        names.push(read_cstr(
            data,
            offset + name_off,
            &format!("{what} name {i}"),
        )?);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(count: u16, index: u16, tangent: u16) -> KeyDescriptor {
        KeyDescriptor {
            count,
            index,
            tangent_type: tangent,
        }
    }

    fn f32_pool(words: &[f32]) -> Vec<u8> {
        words
            .iter()
            .flat_map(|w| w.to_bits().to_be_bytes())
            .collect()
    }

    fn i16_pool(words: &[i16]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    #[test]
    fn track_kinds_default_constant_keyed() {
        let pool = f32_pool(&[1.0, 2.0, 0.0, 3.0, 4.0, 0.5, 6.0, 7.0, 0.0]);
        assert_eq!(
            read_track_f32(desc(0, 99, 0), &pool, "t").unwrap(),
            TrackF32::Default
        );
        assert_eq!(
            read_track_f32(desc(1, 1, 0), &pool, "t").unwrap(),
            TrackF32::Constant { value: 2.0 }
        );
        // Shared tangent (type 0): stride 3, tangent_out == tangent_in.
        // Words 3..9: (3,4,0.5) (6,7,0) — the trailing 0.0 pads the last
        // shared-tangent word so the 2-key run fits.
        let keyed = read_track_f32(desc(2, 3, 0), &pool, "t").unwrap();
        match keyed {
            TrackF32::Keyed { tangent_type, keys } => {
                assert_eq!(tangent_type, 0);
                assert_eq!(keys.len(), 2);
                assert_eq!(
                    keys[0],
                    KeyF32 {
                        time: 3.0,
                        value: 4.0,
                        tangent_in: 0.5,
                        tangent_out: 0.5
                    }
                );
                assert_eq!(
                    keys[1],
                    KeyF32 {
                        time: 6.0,
                        value: 7.0,
                        tangent_in: 0.0,
                        tangent_out: 0.0
                    }
                );
            }
            other => panic!("{other:?}"),
        }
        // Non-1 nonzero tangent word: stride 4, preserved raw.
        let pool2 = f32_pool(&[0.0, 10.0, 1.0, 2.0, 5.0, 20.0, 3.0, 4.0]);
        match read_track_f32(desc(2, 0, 7), &pool2, "t").unwrap() {
            TrackF32::Keyed { tangent_type, keys } => {
                assert_eq!(tangent_type, 7);
                assert_eq!(keys[0].tangent_in, 1.0);
                assert_eq!(keys[0].tangent_out, 2.0);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn i16_tracks_and_rotation_pool() {
        let pool = i16_pool(&[0, 16383, -505, 12, 4, 20, 30, 40, 50, 0, 0]);
        assert_eq!(
            read_track_i16(desc(0, 0, 0), &pool, "r").unwrap(),
            TrackI16::Default
        );
        assert_eq!(
            read_track_i16(desc(1, 1, 0), &pool, "r").unwrap(),
            TrackI16::Constant { value: 16383 }
        );
        match read_track_i16(desc(2, 3, 1), &pool, "r").unwrap() {
            TrackI16::Keyed { tangent_type, keys } => {
                assert_eq!(tangent_type, 1);
                assert_eq!(
                    keys[0],
                    KeyI16 {
                        time: 12,
                        value: 4,
                        tangent_in: 20,
                        tangent_out: 30
                    }
                );
                assert_eq!(
                    keys[1],
                    KeyI16 {
                        time: 40,
                        value: 50,
                        tangent_in: 0,
                        tangent_out: 0
                    }
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn malformed_tracks_error_not_panic() {
        let pool = f32_pool(&[1.0]);
        // Out-of-range pool index.
        assert!(read_track_f32(desc(1, 5, 0), &pool, "t").is_err());
        // Key run past the pool end.
        assert!(read_track_f32(desc(2, 0, 0), &pool, "t").is_err());
        // Non-increasing times.
        let bad = f32_pool(&[5.0, 1.0, 0.0, 1.0, 1.0, 0.0]);
        assert!(read_track_f32(desc(2, 0, 0), &bad, "t").is_err());
        // Non-finite data.
        let inf = f32_pool(&[f32::INFINITY, 1.0, 0.0]);
        assert!(read_track_f32(desc(1, 0, 0), &inf, "t").is_err());
        // Descriptor outside the table.
        assert!(KeyDescriptor::read(&[0u8; 4], 0).is_err());
    }

    #[test]
    fn name_table_reads_resntab() {
        // count=2, pad, {hash,off0},{hash,off1}, "mouth\0" "eyeL\0"
        let mut data = vec![0u8];
        data.clear();
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(&0xFFFFu16.to_be_bytes());
        data.extend_from_slice(&[0x12, 0x34, 0x00, 0x0C]); // entry 0 -> off 12
        data.extend_from_slice(&[0x56, 0x78, 0x00, 0x12]); // entry 1 -> off 18
        data.extend_from_slice(b"mouth\0");
        data.extend_from_slice(b"eyeL\0");
        assert_eq!(
            read_name_table(&data, 0, "nt").unwrap(),
            vec!["mouth".to_string(), "eyeL".to_string()]
        );
        // Truncated table.
        assert!(read_name_table(&data[..5], 0, "nt").is_err());
        // Unterminated name.
        let mut bad = data.clone();
        bad.truncate(bad.len() - 1);
        assert!(read_name_table(&bad, 0, "nt").is_err());
    }
}
