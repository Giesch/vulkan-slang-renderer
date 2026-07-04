//! GX texture tile decoders → RGBA8. Arithmetic matches gclib's
//! texture_utils.py exactly (the P2 pixel gate diffs the two decoders over
//! identical bytes): bit expansion by replication, CMPR thirds by floor
//! division, intensity formats replicate I into alpha.
//!
//! GX stores images as row-major tiles over dimensions rounded up to the
//! tile size; pixels within a tile are row-major. Reference: YAGCD §17.

use image::{Rgba, RgbaImage};

use crate::bmd::BmdError;
use crate::gx::types::{ImageFormat, PaletteFormat};

/// (tile width, tile height, bytes per tile)
pub fn tile_spec(format: ImageFormat) -> (u32, u32, usize) {
    use ImageFormat::*;
    match format {
        I4 => (8, 8, 32),
        I8 => (8, 4, 32),
        Ia4 => (8, 4, 32),
        Ia8 => (4, 4, 32),
        Rgb565 => (4, 4, 32),
        Rgb5a3 => (4, 4, 32),
        Rgba8 => (4, 4, 64),
        C4 => (8, 8, 32),
        C8 => (8, 4, 32),
        C14x2 => (4, 4, 32),
        Cmpr => (8, 8, 32),
    }
}

/// Total image bytes for one mip level: full tiles cover the rounded-up size.
pub fn image_byte_len(format: ImageFormat, width: u16, height: u16) -> usize {
    let (tw, th, bytes) = tile_spec(format);
    let tiles_x = (width as u32).div_ceil(tw) as usize;
    let tiles_y = (height as u32).div_ceil(th) as usize;
    tiles_x * tiles_y * bytes
}

pub fn decode_palette(format: PaletteFormat, data: &[u8]) -> Vec<[u8; 4]> {
    data.chunks_exact(2)
        .map(|c| {
            let raw = u16::from_be_bytes([c[0], c[1]]);
            match format {
                PaletteFormat::Ia8 => ia8_color(raw),
                PaletteFormat::Rgb565 => rgb565_color(raw),
                PaletteFormat::Rgb5a3 => rgb5a3_color(raw),
            }
        })
        .collect()
}

/// Decodes one mip-0 image. `palette` must be the decoded palette for
/// C4/C8/C14X2 and is ignored otherwise. Out-of-range palette indices are
/// legal only in the padding outside `width`×`height` (present in real
/// files); inside the visible area they are a hard error.
pub fn decode(
    name: &str,
    format: ImageFormat,
    image: &[u8],
    palette: &[[u8; 4]],
    width: u16,
    height: u16,
) -> Result<RgbaImage, BmdError> {
    let (tw, th, tile_bytes) = tile_spec(format);
    let (w, h) = (width as u32, height as u32);
    let needed = image_byte_len(format, width, height);
    if image.len() < needed {
        return Err(BmdError::Texture {
            name: name.into(),
            what: format!("{} bytes of image data, need {needed}", image.len()),
        });
    }

    // Palette formats decode to Idx first so range errors can be confined to
    // visible pixels; everything else decodes straight to RGBA.
    let mut tile = vec![Texel::Rgba([0; 4]); (tw * th) as usize];
    let tiles_x = w.div_ceil(tw);
    let tiles_y = h.div_ceil(th);
    let mut out = RgbaImage::new(w, h);
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let offset = ((ty * tiles_x + tx) as usize) * tile_bytes;
            decode_tile(format, &image[offset..offset + tile_bytes], &mut tile);
            for (i, texel) in tile.iter().enumerate() {
                let x = tx * tw + i as u32 % tw;
                let y = ty * th + i as u32 / tw;
                if x >= w || y >= h {
                    continue; // tile padding past the image edge
                }
                let rgba = match *texel {
                    Texel::Rgba(c) => c,
                    Texel::Idx(idx) => {
                        *palette.get(idx as usize).ok_or_else(|| BmdError::Texture {
                            name: name.into(),
                            what: format!(
                                "palette index {idx} out of range ({} colors) at ({x}, {y})",
                                palette.len()
                            ),
                        })?
                    }
                };
                out.put_pixel(x, y, Rgba(rgba));
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum Texel {
    Rgba([u8; 4]),
    Idx(u16),
}

/// Decodes one tile into `out` (tile-row-major, `tile_spec` pixels).
fn decode_tile(format: ImageFormat, bytes: &[u8], out: &mut [Texel]) {
    use ImageFormat::*;
    match format {
        I4 => {
            for (i, &b) in bytes.iter().enumerate() {
                out[i * 2] = Texel::Rgba(i4_color(b >> 4));
                out[i * 2 + 1] = Texel::Rgba(i4_color(b & 0xF));
            }
        }
        I8 => {
            for (i, &b) in bytes.iter().enumerate() {
                out[i] = Texel::Rgba([b, b, b, b]);
            }
        }
        Ia4 => {
            // low nibble intensity, high nibble alpha
            for (i, &b) in bytes.iter().enumerate() {
                let v = expand4(b & 0xF);
                out[i] = Texel::Rgba([v, v, v, expand4(b >> 4)]);
            }
        }
        Ia8 => {
            for (i, texel) in out.iter_mut().enumerate() {
                *texel = Texel::Rgba(ia8_color(be16(bytes, i * 2)));
            }
        }
        Rgb565 => {
            for (i, texel) in out.iter_mut().enumerate() {
                *texel = Texel::Rgba(rgb565_color(be16(bytes, i * 2)));
            }
        }
        Rgb5a3 => {
            for (i, texel) in out.iter_mut().enumerate() {
                *texel = Texel::Rgba(rgb5a3_color(be16(bytes, i * 2)));
            }
        }
        Rgba8 => {
            // two 32-byte planes: A,R pairs then G,B pairs
            for (i, texel) in out.iter_mut().enumerate() {
                let (a, r) = (bytes[i * 2], bytes[i * 2 + 1]);
                let (g, b) = (bytes[i * 2 + 32], bytes[i * 2 + 33]);
                *texel = Texel::Rgba([r, g, b, a]);
            }
        }
        C4 => {
            for (i, &b) in bytes.iter().enumerate() {
                out[i * 2] = Texel::Idx((b >> 4) as u16);
                out[i * 2 + 1] = Texel::Idx((b & 0xF) as u16);
            }
        }
        C8 => {
            for (i, &b) in bytes.iter().enumerate() {
                out[i] = Texel::Idx(b as u16);
            }
        }
        C14x2 => {
            for (i, texel) in out.iter_mut().enumerate() {
                *texel = Texel::Idx(be16(bytes, i * 2) & 0x3FFF);
            }
        }
        Cmpr => {
            // four DXT1-style 4×4 sub-blocks: upper-left, upper-right,
            // lower-left, lower-right; 8 bytes each
            for sub in 0..4 {
                let sb = &bytes[sub * 8..sub * 8 + 8];
                let colors = cmpr_colors(be16(sb, 0), be16(sb, 2));
                let indices = u32::from_be_bytes([sb[4], sb[5], sb[6], sb[7]]);
                let (sx, sy) = ((sub % 2) * 4, (sub / 2) * 4);
                for i in 0..16 {
                    let color = colors[(indices >> ((15 - i) * 2) & 3) as usize];
                    out[sx + sy * 8 + (i / 4) * 8 + i % 4] = Texel::Rgba(color);
                }
            }
        }
    }
}

fn be16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

// Bit expansion by replication, e.g. expand5: 00012345 → 12345123.
fn expand3(v: u8) -> u8 {
    (v << 5) | (v << 2) | (v >> 1)
}

fn expand4(v: u8) -> u8 {
    (v << 4) | v
}

fn expand5(v: u8) -> u8 {
    (v << 3) | (v >> 2)
}

fn expand6(v: u8) -> u8 {
    (v << 2) | (v >> 4)
}

fn i4_color(v: u8) -> [u8; 4] {
    let v = expand4(v);
    [v, v, v, v]
}

fn ia8_color(raw: u16) -> [u8; 4] {
    let i = (raw & 0xFF) as u8;
    [i, i, i, (raw >> 8) as u8]
}

fn rgb565_color(raw: u16) -> [u8; 4] {
    [
        expand5((raw >> 11) as u8 & 0x1F),
        expand6((raw >> 5) as u8 & 0x3F),
        expand5(raw as u8 & 0x1F),
        255,
    ]
}

fn rgb5a3_color(raw: u16) -> [u8; 4] {
    if raw & 0x8000 == 0 {
        // 0AAARRRRGGGGBBBB
        [
            expand4((raw >> 8) as u8 & 0xF),
            expand4((raw >> 4) as u8 & 0xF),
            expand4(raw as u8 & 0xF),
            expand3((raw >> 12) as u8 & 0x7),
        ]
    } else {
        // 1RRRRRGGGGGBBBBB
        [
            expand5((raw >> 10) as u8 & 0x1F),
            expand5((raw >> 5) as u8 & 0x1F),
            expand5(raw as u8 & 0x1F),
            255,
        ]
    }
}

/// DXT1-style palette: c0 > c1 → two floor-div thirds; else half-and-half
/// with index 3 transparent black.
fn cmpr_colors(c0: u16, c1: u16) -> [[u8; 4]; 4] {
    let a = rgb565_color(c0);
    let b = rgb565_color(c1);
    let third = |x: u8, y: u8| ((2 * x as u16 + y as u16) / 3) as u8;
    if c0 > c1 {
        [
            a,
            b,
            [third(a[0], b[0]), third(a[1], b[1]), third(a[2], b[2]), 255],
            [third(b[0], a[0]), third(b[1], a[1]), third(b[2], a[2]), 255],
        ]
    } else {
        [
            a,
            b,
            [
                a[0] / 2 + b[0] / 2,
                a[1] / 2 + b[1] / 2,
                a[2] / 2 + b[2] / 2,
                255,
            ],
            [0, 0, 0, 0],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders an image as one RRGGBBAA hex token per pixel for snapshots.
    fn grid(img: &RgbaImage) -> String {
        let mut s = String::new();
        for y in 0..img.height() {
            for x in 0..img.width() {
                let p = img.get_pixel(x, y).0;
                if x > 0 {
                    s.push(' ');
                }
                s.push_str(&format!("{:02x}{:02x}{:02x}{:02x}", p[0], p[1], p[2], p[3]));
            }
            s.push('\n');
        }
        s
    }

    fn decode_ok(format: ImageFormat, image: &[u8], pal: &[[u8; 4]], w: u16, h: u16) -> String {
        grid(&decode("test", format, image, pal, w, h).unwrap())
    }

    #[test]
    fn i4_tile() {
        // 8×8 tile, nibbles 0,1,2,...,F repeating; asserts high-nibble-first
        // order and 4→8 replication (0xF → 0xFF, 0x8 → 0x88)
        let bytes: Vec<u8> = (0..32)
            .map(|i| (((i * 2) % 16) << 4 | ((i * 2 + 1) % 16)) as u8)
            .collect();
        insta::assert_snapshot!(decode_ok(ImageFormat::I4, &bytes, &[], 8, 8));
    }

    #[test]
    fn i8_and_ia4_and_ia8_tiles() {
        let bytes: Vec<u8> = (0..32).map(|i| (i * 8) as u8).collect();
        insta::assert_snapshot!("i8", decode_ok(ImageFormat::I8, &bytes, &[], 8, 4));
        // IA4: low nibble intensity, high nibble alpha — 0x2F must be I=0xFF A=0x22
        insta::assert_snapshot!("ia4", decode_ok(ImageFormat::Ia4, &[0x2F; 32], &[], 8, 4));
        // IA8: first byte alpha, second intensity — 0x40,0x80 → I=0x80 A=0x40
        insta::assert_snapshot!(
            "ia8",
            decode_ok(ImageFormat::Ia8, &[0x40, 0x80].repeat(16), &[], 4, 4)
        );
    }

    #[test]
    fn rgb565_bit_replication() {
        // 0xFFFF → pure white (0x1F → 0xFF via replication, not 0xF8)
        // 0x0841 → r=1,g=2,b=1 → 0x08, 0x08, 0x08
        let mut bytes = [0u8; 32];
        bytes[0] = 0xFF;
        bytes[1] = 0xFF;
        bytes[2] = 0x08;
        bytes[3] = 0x41;
        insta::assert_snapshot!(decode_ok(ImageFormat::Rgb565, &bytes, &[], 4, 4));
    }

    #[test]
    fn rgb5a3_both_modes() {
        // 0x8000 set: RGB555 opaque red 0x7C00 → ff0000ff
        // clear: A3RGB444 0x3F0F → a=1(0x24... expand3(1)=0b00100100=0x24) r=0xFF g=0x00 b=0xFF
        let mut bytes = [0u8; 32];
        bytes[0] = 0xFC;
        bytes[1] = 0x00;
        bytes[2] = 0x3F;
        bytes[3] = 0x0F;
        insta::assert_snapshot!(decode_ok(ImageFormat::Rgb5a3, &bytes, &[], 4, 4));
    }

    #[test]
    fn rgba8_two_plane_split() {
        // pixel 0: A=0x11 R=0x22 (plane 1), G=0x33 B=0x44 (plane 2)
        let mut bytes = [0u8; 64];
        bytes[0] = 0x11;
        bytes[1] = 0x22;
        bytes[32] = 0x33;
        bytes[33] = 0x44;
        insta::assert_snapshot!(decode_ok(ImageFormat::Rgba8, &bytes, &[], 4, 4));
    }

    #[test]
    fn c4_c8_palette_lookup() {
        let pal: Vec<[u8; 4]> = vec![[10, 0, 0, 255], [0, 20, 0, 255], [0, 0, 30, 255]];
        // C4: byte 0x01 → pixels [pal 0, pal 1]
        let mut c4 = [0u8; 32];
        c4[0] = 0x01;
        c4[1] = 0x22;
        insta::assert_snapshot!("c4", decode_ok(ImageFormat::C4, &c4, &pal, 8, 8));
        let mut c8 = [0u8; 32];
        c8[0] = 2;
        c8[1] = 1;
        insta::assert_snapshot!("c8", decode_ok(ImageFormat::C8, &c8, &pal, 8, 4));
    }

    #[test]
    fn palette_formats_decode() {
        // IA8 palette entry 0x40FF → I=0xFF A=0x40; RGB5A3 translucent mode
        let ia8 = decode_palette(PaletteFormat::Ia8, &[0x40, 0xFF]);
        assert_eq!(ia8, vec![[0xFF, 0xFF, 0xFF, 0x40]]);
        let rgb565 = decode_palette(PaletteFormat::Rgb565, &[0xFF, 0xFF]);
        assert_eq!(rgb565, vec![[0xFF, 0xFF, 0xFF, 0xFF]]);
        // 0x3F0F: top bit clear → A3RGB444, a=3 → expand3(3)=0x6D
        let rgb5a3 = decode_palette(PaletteFormat::Rgb5a3, &[0x3F, 0x0F]);
        assert_eq!(rgb5a3, vec![[0xFF, 0x00, 0xFF, 0x6D]]);
    }

    #[test]
    fn palette_index_out_of_range_in_visible_area_is_error() {
        let pal: Vec<[u8; 4]> = vec![[0, 0, 0, 255]];
        let mut c8 = [0u8; 32];
        c8[0] = 5; // visible pixel (0,0) with only 1 palette color
        let err = decode("test", ImageFormat::C8, &c8, &pal, 8, 4).unwrap_err();
        assert!(matches!(err, BmdError::Texture { .. }), "{err:?}");
        // ...but junk indices in the padding area are fine (bleed past edge)
        let mut c8 = [0u8; 32];
        c8[7] = 99; // pixel (7,0) — outside a 4-wide image
        assert!(decode("test", ImageFormat::C8, &c8, &pal, 4, 4).is_ok());
    }

    #[test]
    fn cmpr_tile_both_modes() {
        // sub-block 0: c0 (0xF800 red) > c1 (0x001F blue): thirds mode,
        // indices 0,1,2,3 in the first row
        // sub-block 1: c0 (0x0000) <= c1 (0xF800): half mode + transparent 3
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&[0xF8, 0x00, 0x00, 0x1F, 0x1B, 0, 0, 0]);
        bytes[8..16].copy_from_slice(&[0x00, 0x00, 0xF8, 0x00, 0x1B, 0, 0, 0]);
        insta::assert_snapshot!(decode_ok(ImageFormat::Cmpr, &bytes, &[], 8, 8));
    }

    #[test]
    fn cmpr_thirds_use_floor_division() {
        // c0=0xFFFF c1=0x0021: g components 0xFF, expand6(1)=0x04 →
        // (2*255+4)/3 = 171 (floor, not round), (255+2*4)/3 = 87
        let colors = cmpr_colors(0xFFFF, 0x0021);
        assert_eq!(colors[2][1], ((2 * 255 + 4) / 3) as u8);
        assert_eq!(colors[3][1], ((255 + 2 * 4) / 3) as u8);
        // half mode rounding: 0x04/2 + 0xFF/2 = 2 + 127 = 129 (not 130)
        let colors = cmpr_colors(0x0021, 0xFFFF);
        assert_eq!(colors[2][1], 129);
        assert_eq!(colors[3], [0, 0, 0, 0]);
    }

    #[test]
    fn non_tile_aligned_dims_clip_padding() {
        // 5×3 RGB565: 2×1 tiles, padding pixels dropped, output exactly 5×3
        let bytes = [0xFF; 64];
        let img = decode("test", ImageFormat::Rgb565, &bytes, &[], 5, 3).unwrap();
        assert_eq!((img.width(), img.height()), (5, 3));
        assert!(img.pixels().all(|p| p.0 == [255, 255, 255, 255]));
    }

    #[test]
    fn image_byte_len_rounds_up_to_tiles() {
        assert_eq!(
            image_byte_len(ImageFormat::Cmpr, 160, 96),
            160 / 8 * (96 / 8) * 32
        );
        assert_eq!(image_byte_len(ImageFormat::I4, 256, 8), 32 * 1 * 32);
        assert_eq!(image_byte_len(ImageFormat::Rgb565, 5, 3), 2 * 32);
        assert_eq!(image_byte_len(ImageFormat::Ia8, 96, 96), 24 * 24 * 32);
    }
}
