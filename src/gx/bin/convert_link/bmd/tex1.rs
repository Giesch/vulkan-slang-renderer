//! TEX1 chunk: an array of ResTIMG (BTI) headers plus a name table.
//! Layout (J3DModelLoader.h:121–125): u16 count at +8, u32 header-list
//! offset at +0x0C, u32 name-table offset at +0x10, all chunk-relative;
//! headers packed 0x20 apart. Parsers receive the chunk's own slice, so
//! every offset is naturally chunk-relative and bounds-checked against it.

use std::path::Path;

use crate::be::BeReader;
use crate::bmd::{BmdError, read_name_table};
use crate::bti::{self, BtiTexture};

#[derive(Debug)]
pub struct Tex1<'a> {
    pub entries: Vec<Tex1Entry<'a>>,
}

#[derive(Debug)]
pub struct Tex1Entry<'a> {
    pub name: String,
    pub texture: BtiTexture<'a>,
}

pub fn parse(chunk: &[u8]) -> Result<Tex1<'_>, BmdError> {
    let r = BeReader::new(chunk);
    let mut header = r.at(8);
    let count = header.u16()?;
    header.skip(2)?; // padding
    let header_list = header.u32()? as usize;
    let name_table = header.u32()? as usize;

    let names = read_name_table(&r, name_table)?;
    if names.len() != count as usize {
        return Err(BmdError::Invariant(format!(
            "TEX1 has {count} textures but {} names",
            names.len()
        )));
    }

    let entries = names
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            let texture = bti::parse(&r, header_list + i * bti::HEADER_SIZE, &name)?;
            Ok(Tex1Entry { name, texture })
        })
        .collect::<Result<Vec<_>, BmdError>>()?;
    Ok(Tex1 { entries })
}

/// Emits `NN_<name>.png` (decoded) and `NN_<name>.bti` (standalone re-emit,
/// GX bytes verbatim) for every entry. Names repeat in cl.bdl, so filenames
/// are always index-prefixed.
pub fn emit(tex1: &Tex1, tex_dir: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    std::fs::create_dir_all(tex_dir)?;
    for (i, entry) in tex1.entries.iter().enumerate() {
        let stem = format!("{i:02}_{}", entry.name);
        let png_path = tex_dir.join(format!("{stem}.png"));
        let image = bti::decode(&entry.texture, &entry.name)?;
        image
            .save(&png_path)
            .with_context(|| format!("writing {}", png_path.display()))?;
        let bti_path = tex_dir.join(format!("{stem}.bti"));
        std::fs::write(&bti_path, bti::write_standalone(&entry.texture))
            .with_context(|| format!("writing {}", bti_path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gx::types::ImageFormat;

    /// Builds a synthetic TEX1 chunk: 2 I4 8×8 textures named "a" and "bb".
    fn synth_tex1() -> Vec<u8> {
        let mut chunk = Vec::new();
        chunk.extend_from_slice(b"TEX1");
        chunk.extend_from_slice(&[0; 4]); // size, unchecked here
        chunk.extend_from_slice(&2u16.to_be_bytes());
        chunk.extend_from_slice(&[0xFF, 0xFF]);
        chunk.extend_from_slice(&0x20u32.to_be_bytes()); // header list
        let name_table_offset = 0x20 + 2 * 0x20 + 2 * 32; // after headers + image data
        chunk.extend_from_slice(&(name_table_offset as u32).to_be_bytes());
        chunk.resize(0x20, 0xFF);
        for i in 0..2u32 {
            // ResTIMG: I4, 8×8, image data right after both headers
            let image_offset = (2 - i) * 0x20 + i * 32; // header-relative
            let mut h = [0u8; 0x20];
            h[0] = 0x00; // I4
            h[3] = 8;
            h[5] = 8;
            h[0x14] = 1; // linear filters
            h[0x15] = 1;
            h[0x18] = 1; // mipmapCount
            h[0x1C..0x20].copy_from_slice(&image_offset.to_be_bytes());
            chunk.extend_from_slice(&h);
        }
        chunk.extend_from_slice(&[0xAA; 32]);
        chunk.extend_from_slice(&[0x55; 32]);
        // name table: count, pad, (hash, offset) × 2, strings
        assert_eq!(chunk.len(), name_table_offset);
        chunk.extend_from_slice(&2u16.to_be_bytes());
        chunk.extend_from_slice(&[0xFF, 0xFF]);
        chunk.extend_from_slice(&[0x00, 0x00, 0x00, 0x0C]); // "a" at +0x0C
        chunk.extend_from_slice(&[0x00, 0x00, 0x00, 0x0E]); // "bb" at +0x0E
        chunk.extend_from_slice(b"a\0bb\0");
        chunk
    }

    #[test]
    fn parses_synthetic_two_texture_chunk() {
        let chunk = synth_tex1();
        let tex1 = parse(&chunk).unwrap();
        assert_eq!(tex1.entries.len(), 2);
        assert_eq!(tex1.entries[0].name, "a");
        assert_eq!(tex1.entries[1].name, "bb");
        assert_eq!(tex1.entries[0].texture.header.format, ImageFormat::I4);
        assert_eq!(tex1.entries[0].texture.image, &[0xAA; 32]);
        assert_eq!(tex1.entries[1].texture.image, &[0x55; 32]);
    }

    #[test]
    fn name_count_mismatch_is_invariant_error() {
        let mut chunk = synth_tex1();
        // bump texture count to 3 without adding a name (header list bounds
        // would also fail, but the name check runs first)
        chunk[8..10].copy_from_slice(&3u16.to_be_bytes());
        let err = parse(&chunk).unwrap_err();
        assert!(matches!(err, BmdError::Invariant(_)), "{err:?}");
    }

    #[test]
    #[ignore = "requires extracted assets (just extract-link); run via just link-verify-p2"]
    fn real_tex1_inventory() {
        use crate::gx::types::{PaletteFormat, WrapMode};
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/link/raw/cl.bdl");
        let Ok(data) = std::fs::read(path) else {
            eprintln!("skipping: {path} not present");
            return;
        };
        let model = crate::bmd::parse_model(&data).unwrap();
        let entries = &model.tex1.entries;
        assert_eq!(entries.len(), 41);

        // recorded facts: phase_02.md — no mipmaps, everything clamped
        for e in entries {
            assert_eq!(e.texture.header.mip_levels(), 1, "{}", e.name);
            assert_eq!(e.texture.header.wrap_s, WrapMode::Clamp, "{}", e.name);
            assert_eq!(e.texture.header.wrap_t, WrapMode::Clamp, "{}", e.name);
        }

        // format histogram: CMPR ×14, I4 ×11, IA8 ×8, IA4 ×7, C8 ×1
        let count = |f: ImageFormat| {
            entries
                .iter()
                .filter(|e| e.texture.header.format == f)
                .count()
        };
        assert_eq!(count(ImageFormat::Cmpr), 14);
        assert_eq!(count(ImageFormat::I4), 11);
        assert_eq!(count(ImageFormat::Ia8), 8);
        assert_eq!(count(ImageFormat::Ia4), 7);
        assert_eq!(count(ImageFormat::C8), 1);

        // ZBtoonEX is the only runtime-injected (Z-prefixed) ramp slot
        let z_names: Vec<_> = entries.iter().filter(|e| e.name.starts_with('Z')).collect();
        assert_eq!(z_names.len(), 1);
        assert_eq!(z_names[0].name, "ZBtoonEX");
        assert_eq!(z_names[0].texture.header.format, ImageFormat::I4);

        // duplicate names are distinct entries; filenames must index-prefix
        for dup in [
            "eyeh.1",
            "linktexS3TC",
            "mouthS3TC.1",
            "podAS3TC",
            "mayuh.1",
        ] {
            assert_eq!(entries.iter().filter(|e| e.name == dup).count(), 2, "{dup}");
        }

        // the one palette texture: hitomi (pupils), C8 + RGB565 × 64
        let hitomi = entries.iter().find(|e| e.name == "hitomi").unwrap();
        assert_eq!(hitomi.texture.header.format, ImageFormat::C8);
        assert_eq!(hitomi.texture.header.palette_format, PaletteFormat::Rgb565);
        assert_eq!(hitomi.texture.header.num_colors, 64);
    }
}
