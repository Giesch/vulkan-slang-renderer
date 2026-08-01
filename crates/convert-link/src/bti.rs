//! ResTIMG — the 0x20-byte BTI texture header — shared by TEX1 entries and
//! standalone `.bti` files. Layout verified against
//! ../tww/include/JSystem/JUtility/JUTTexture.h:14–37. The image/palette
//! offsets are relative to the header's own start (gclib BTI.read agrees).

use crate::be::BeReader;
use crate::bmd::BmdError;
use crate::gx::texture;
use crate::gx::types::{FilterMode, GxEnumError, ImageFormat, PaletteFormat, WrapMode};

pub const HEADER_SIZE: usize = 0x20;

/// Fields we interpret are typed; everything else is a raw pass-through byte
/// copied verbatim into standalone re-emits (no interpretation, no loss).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtiHeader {
    pub format: ImageFormat,
    pub alpha_setting: u8,
    pub width: u16,
    pub height: u16,
    pub wrap_s: WrapMode,
    pub wrap_t: WrapMode,
    pub palettes_enabled: u8,
    pub palette_format: PaletteFormat,
    pub num_colors: u16,
    pub palette_data_offset: u32,
    pub mipmap_enabled: u8,
    pub do_edge_lod: u8,
    pub bias_clamp: u8,
    pub max_anisotropy: u8,
    pub min_filter: FilterMode,
    pub mag_filter: FilterMode,
    pub min_lod: u8,
    pub max_lod: u8,
    pub mipmap_count: u8,
    pub unknown: u8,
    pub lod_bias: i16,
    pub image_data_offset: u32,
}

impl BtiHeader {
    pub fn uses_palette(&self) -> bool {
        matches!(
            self.format,
            ImageFormat::C4 | ImageFormat::C8 | ImageFormat::C14x2
        )
    }

    /// A stored count of 0 means 1 (gclib normalizes the same way).
    pub fn mip_levels(&self) -> u8 {
        self.mipmap_count.max(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtiTexture<'a> {
    pub header: BtiHeader,
    pub image: &'a [u8],
    pub palette: &'a [u8],
}

fn gx(name: &str, field: &str) -> impl Fn(GxEnumError) -> BmdError + use<> {
    let context = format!("texture {name}: {field}");
    move |source| BmdError::Gx {
        context: context.clone(),
        source,
    }
}

pub fn parse_header(r: &BeReader, pos: usize, name: &str) -> Result<BtiHeader, BmdError> {
    let mut r = r.at(pos);
    Ok(BtiHeader {
        format: ImageFormat::try_from(r.u8()?).map_err(gx(name, "format"))?,
        alpha_setting: r.u8()?,
        width: r.u16()?,
        height: r.u16()?,
        wrap_s: WrapMode::try_from(r.u8()?).map_err(gx(name, "wrapS"))?,
        wrap_t: WrapMode::try_from(r.u8()?).map_err(gx(name, "wrapT"))?,
        palettes_enabled: r.u8()?,
        palette_format: PaletteFormat::try_from(r.u8()?).map_err(gx(name, "paletteFormat"))?,
        num_colors: r.u16()?,
        palette_data_offset: r.u32()?,
        mipmap_enabled: r.u8()?,
        do_edge_lod: r.u8()?,
        bias_clamp: r.u8()?,
        max_anisotropy: r.u8()?,
        min_filter: FilterMode::try_from(r.u8()?).map_err(gx(name, "minFilter"))?,
        mag_filter: FilterMode::try_from(r.u8()?).map_err(gx(name, "magFilter"))?,
        min_lod: r.u8()?,
        max_lod: r.u8()?,
        mipmap_count: r.u8()?,
        unknown: r.u8()?,
        lod_bias: r.i16()?,
        image_data_offset: r.u32()?,
    })
}

/// Parses a header at `header_pos` and slices its image/palette data
/// (offsets are header-relative). Multi-mip textures are out of scope for
/// this converter — no such texture exists in Link's inputs.
pub fn parse<'a>(
    r: &BeReader<'a>,
    header_pos: usize,
    name: &str,
) -> Result<BtiTexture<'a>, BmdError> {
    let header = parse_header(r, header_pos, name)?;
    if header.mip_levels() != 1 {
        return Err(BmdError::Texture {
            name: name.into(),
            what: format!(
                "mipmapCount {} unsupported (P2 decodes single-level only)",
                header.mipmap_count
            ),
        });
    }
    let image_len = texture::image_byte_len(header.format, header.width, header.height);
    let image = r
        .at(header_pos + header.image_data_offset as usize)
        .bytes(image_len)?;
    let palette = if header.uses_palette() {
        r.at(header_pos + header.palette_data_offset as usize)
            .bytes(header.num_colors as usize * 2)?
    } else {
        &[]
    };
    Ok(BtiTexture {
        header,
        image,
        palette,
    })
}

pub fn decode(tex: &BtiTexture, name: &str) -> Result<image::RgbaImage, BmdError> {
    let h = &tex.header;
    let palette = texture::decode_palette(h.palette_format, tex.palette);
    texture::decode(name, h.format, tex.image, &palette, h.width, h.height)
}

/// Serializes as a standalone `.bti`: verbatim header (offsets rebased to the
/// standalone layout) + verbatim image and palette bytes. Never re-encodes
/// pixel data — the P2 gate depends on the GX bytes being byte-identical to
/// the source file.
pub fn write_standalone(tex: &BtiTexture) -> Vec<u8> {
    let h = &tex.header;
    let image_offset = HEADER_SIZE as u32;
    let palette_offset = if tex.palette.is_empty() {
        0
    } else {
        image_offset + tex.image.len() as u32
    };
    let mut out = Vec::with_capacity(HEADER_SIZE + tex.image.len() + tex.palette.len());
    out.push(h.format as u8);
    out.push(h.alpha_setting);
    out.extend_from_slice(&h.width.to_be_bytes());
    out.extend_from_slice(&h.height.to_be_bytes());
    out.push(h.wrap_s as u8);
    out.push(h.wrap_t as u8);
    out.push(h.palettes_enabled);
    out.push(h.palette_format as u8);
    out.extend_from_slice(&h.num_colors.to_be_bytes());
    out.extend_from_slice(&palette_offset.to_be_bytes());
    out.push(h.mipmap_enabled);
    out.push(h.do_edge_lod);
    out.push(h.bias_clamp);
    out.push(h.max_anisotropy);
    out.push(h.min_filter as u8);
    out.push(h.mag_filter as u8);
    out.push(h.min_lod);
    out.push(h.max_lod);
    out.push(h.mipmap_count);
    out.push(h.unknown);
    out.extend_from_slice(&h.lod_bias.to_be_bytes());
    out.extend_from_slice(&image_offset.to_be_bytes());
    debug_assert_eq!(out.len(), HEADER_SIZE);
    out.extend_from_slice(tex.image);
    out.extend_from_slice(tex.palette);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic 8×4 C8 texture: header (all fields distinct), 32 bytes of
    /// indices, 2-color RGB565 palette.
    fn synth_bti() -> Vec<u8> {
        let mut data = vec![
            0x09, 0x01, // format C8, alphaSetting 1
            0x00, 0x08, // width 8
            0x00, 0x04, // height 4
            0x00, 0x01, // wrap clamp, repeat
            0x01, 0x01, // palettesEnabled, paletteFormat RGB565
            0x00, 0x02, // numColors 2
            0x00, 0x00, 0x00, 0x40, // paletteOffset (0x20 header + 0x20 image)
            0x00, 0x01, 0x00, 0x02, // mipmapEnabled, doEdgeLOD, biasClamp, maxAniso
            0x01, 0x00, // filters linear, nearest
            0x00, 0x01, // minLOD, maxLOD
            0x01, 0xAB, // mipmapCount 1, unknown 0xAB
            0xFF, 0x9C, // lodBias -100
            0x00, 0x00, 0x00, 0x20, // imageOffset
        ];
        data.extend_from_slice(&[0x01; 32]); // image: all palette index 1
        data.extend_from_slice(&[0xF8, 0x00, 0x07, 0xE0]); // palette: red, green
        data
    }

    #[test]
    fn parses_synthetic_header_field_for_field() {
        let data = synth_bti();
        let r = BeReader::new(&data);
        let tex = parse(&r, 0, "synth").unwrap();
        let h = &tex.header;
        assert_eq!(h.format, ImageFormat::C8);
        assert_eq!(h.alpha_setting, 1);
        assert_eq!((h.width, h.height), (8, 4));
        assert_eq!((h.wrap_s, h.wrap_t), (WrapMode::Clamp, WrapMode::Repeat));
        assert_eq!(h.palette_format, PaletteFormat::Rgb565);
        assert_eq!(h.num_colors, 2);
        assert_eq!(
            (h.min_filter, h.mag_filter),
            (FilterMode::Linear, FilterMode::Nearest)
        );
        assert_eq!(h.lod_bias, -100);
        assert_eq!(h.unknown, 0xAB);
        assert_eq!(tex.image, &[0x01; 32]);
        assert_eq!(tex.palette, &[0xF8, 0x00, 0x07, 0xE0]);
        // decodes to all-green (palette entry 1)
        let img = decode(&tex, "synth").unwrap();
        assert!(img.pixels().all(|p| p.0 == [0, 0xFF, 0, 0xFF]));
    }

    #[test]
    fn standalone_roundtrip() {
        let data = synth_bti();
        let r = BeReader::new(&data);
        let tex = parse(&r, 0, "synth").unwrap();
        let standalone = write_standalone(&tex);
        let r2 = BeReader::new(&standalone);
        let tex2 = parse(&r2, 0, "synth2").unwrap();
        assert_eq!(tex.header, tex2.header);
        assert_eq!(tex.image, tex2.image);
        assert_eq!(tex.palette, tex2.palette);
        // this synthetic file is already in the standalone layout
        assert_eq!(standalone, data);
    }

    #[test]
    fn multi_mip_is_typed_error() {
        let mut data = synth_bti();
        data[0x18] = 2; // mipmapCount
        let r = BeReader::new(&data);
        let err = parse(&r, 0, "synth").unwrap_err();
        assert!(matches!(err, BmdError::Texture { .. }), "{err:?}");
    }

    #[test]
    fn bad_format_byte_is_gx_error() {
        let mut data = synth_bti();
        data[0] = 0x07; // gap in ImageFormat
        let r = BeReader::new(&data);
        let err = parse(&r, 0, "synth").unwrap_err();
        match err {
            BmdError::Gx { context, source } => {
                assert!(context.contains("format"), "{context}");
                assert_eq!(source.value, 7);
            }
            other => panic!("expected Gx error, got {other:?}"),
        }
    }

    #[test]
    fn image_length_table_matches_hand_computed_sizes() {
        // cl.bdl's real shapes: CMPR 160×96 = 7680, IA8 96×96 = 18432,
        // I4 8×8 = 32, IA4 64×64 = 4096, C8 96×96 = 9216
        use texture::image_byte_len;
        assert_eq!(image_byte_len(ImageFormat::Cmpr, 160, 96), 7680);
        assert_eq!(image_byte_len(ImageFormat::Ia8, 96, 96), 18432);
        assert_eq!(image_byte_len(ImageFormat::I4, 8, 8), 32);
        assert_eq!(image_byte_len(ImageFormat::Ia4, 64, 64), 4096);
        assert_eq!(image_byte_len(ImageFormat::C8, 96, 96), 9216);
    }
}
