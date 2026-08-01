//! MAT3 chunk: complete material parse. Structure (verified against
//! J3DModelLoader.h:43–75, J3DMaterialFactory.cpp, and gclib's
//! j3d_chunks/mat3.py, which the oracle uses): u16 count at +8, then 30
//! chunk-relative u32 list offsets from +0x0C. Each of the `count` material
//! slots resolves through a u16 remap table to a 0x14C-byte init record;
//! records hold u8/u16 *indices* into the per-property lists (sentinel
//! 0xFF/0xFFFF, or a zero list offset, means "absent"). Every byte lands in
//! a typed enum — parse-don't-validate.

use crate::be::{BeReader, BeResult};
use crate::bmd::{BmdError, read_name_table};
use crate::gx::types::*;

pub const MATERIAL_RECORD_SIZE: usize = 0x14C;

// Indices into the 30-entry list-offset header, in file order
// (gclib MAT3 field order).
const MATERIAL_DATA: usize = 0;
const REMAP_TABLE: usize = 1;
const NAME_TABLE: usize = 2;
const INDIRECT: usize = 3;
const CULL_MODE: usize = 4;
const MAT_COLOR: usize = 5;
const NUM_COLOR_CHANS: usize = 6;
const COLOR_CHANNEL: usize = 7;
const AMBIENT_COLOR: usize = 8;
const LIGHT_COLOR: usize = 9;
const NUM_TEX_GENS: usize = 10;
const TEX_COORD_GEN: usize = 11;
const POST_TEX_COORD_GEN: usize = 12;
const TEX_MATRIX: usize = 13;
const POST_TEX_MATRIX: usize = 14;
const TEXTURE_REMAP: usize = 15;
const TEV_ORDER: usize = 16;
const TEV_COLOR: usize = 17;
const TEV_KONST_COLOR: usize = 18;
const NUM_TEV_STAGES: usize = 19;
const TEV_STAGE: usize = 20;
const TEV_SWAP_MODE: usize = 21;
const TEV_SWAP_TABLE: usize = 22;
const FOG: usize = 23;
const ALPHA_COMPARE: usize = 24;
const BLEND_MODE: usize = 25;
const Z_MODE: usize = 26;
const Z_COMPARE: usize = 27;
const DITHER: usize = 28;
const NBT_SCALE: usize = 29;

pub type Rgba8 = [u8; 4];
pub type RgbaS16 = [i16; 4];

#[derive(Debug, Clone, PartialEq)]
pub struct ZMode {
    pub test: bool,
    pub func: CompareType,
    pub write: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColorChannel {
    pub lighting_enabled: bool,
    pub mat_src: ColorSrc,
    pub lit_mask: u8,
    pub diffuse: DiffuseFunction,
    pub attenuation: AttenuationFunction,
    pub amb_src: ColorSrc,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TexGen {
    pub ty: TexGenType,
    pub src: TexGenSrc,
    pub matrix: TexGenMatrix,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TexMatrix {
    pub projection: TexMtxProjection,
    pub map_mode: TexMtxMapMode,
    pub is_maya: bool,
    pub center: [f32; 3],
    pub scale: [f32; 2],
    pub rotation: u16,
    pub translation: [f32; 2],
    pub effect_matrix: [f32; 16],
}

#[derive(Debug, Clone, PartialEq)]
pub struct TevOrder {
    pub tex_coord: TexCoordId,
    pub tex_map: TexMapId,
    pub channel: ColorChannelId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TevStage {
    pub tev_mode: u8,
    pub color_in: [CombineColor; 4],
    pub color_op: TevOp,
    pub color_bias: TevBias,
    pub color_scale: TevScale,
    pub color_clamp: bool,
    pub color_reg: Register,
    pub alpha_in: [CombineAlpha; 4],
    pub alpha_op: TevOp,
    pub alpha_bias: TevBias,
    pub alpha_scale: TevScale,
    pub alpha_clamp: bool,
    pub alpha_reg: Register,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TevSwapMode {
    pub ras_sel: u8,
    pub tex_sel: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TevSwapTable {
    pub rgba: [u8; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fog {
    pub fog_type: FogType,
    pub enable: bool,
    pub center: u16,
    pub start_z: f32,
    pub end_z: f32,
    pub near_z: f32,
    pub far_z: f32,
    pub color: Rgba8,
    pub range_adjustments: [u16; 10],
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlphaCompare {
    pub comp0: CompareType,
    pub ref0: u8,
    pub op: AlphaOp,
    pub comp1: CompareType,
    pub ref1: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Blend {
    pub mode: BlendMode,
    pub src: BlendFactor,
    pub dst: BlendFactor,
    pub logic: LogicOp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NbtScale {
    pub enable: bool,
    pub scale: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Indirect {
    pub enable: bool,
    pub num_stages: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub pe_mode: PixelEngineMode,
    pub cull_mode: Option<CullMode>,
    pub num_color_chans: Option<u8>,
    pub num_tex_gens: Option<u8>,
    pub num_tev_stages: Option<u8>,
    pub z_compare_loc: Option<bool>,
    pub z_mode: Option<ZMode>,
    pub dither: Option<bool>,
    pub material_colors: [Option<Rgba8>; 2],
    pub color_channels: [Option<ColorChannel>; 4],
    pub ambient_colors: [Option<Rgba8>; 2],
    pub light_colors: [Option<Rgba8>; 8],
    pub tex_coord_gens: [Option<TexGen>; 8],
    pub post_tex_coord_gens: [Option<TexGen>; 8],
    pub tex_matrices: [Option<TexMatrix>; 10],
    pub post_tex_matrices: [Option<TexMatrix>; 20],
    pub texture_indices: [Option<u16>; 8],
    pub konst_colors: [Option<Rgba8>; 4],
    pub kcsels: [KonstColorSel; 16],
    pub kasels: [KonstAlphaSel; 16],
    pub tev_orders: [Option<TevOrder>; 16],
    pub tev_colors: [Option<RgbaS16>; 4],
    pub tev_stages: [Option<TevStage>; 16],
    pub swap_modes: [Option<TevSwapMode>; 16],
    pub swap_tables: [Option<TevSwapTable>; 16],
    pub fog: Option<Fog>,
    pub alpha_compare: Option<AlphaCompare>,
    pub blend: Option<Blend>,
    pub nbt_scale: Option<NbtScale>,
    pub indirect: Option<Indirect>,
}

pub struct Mat3 {
    pub names: Vec<String>,
    /// Per-slot remap indices into the shared init-data records; duplicate
    /// values mean two slots share one record (J3D material instancing).
    pub remap: Vec<u16>,
    pub materials: Vec<Material>,
}

pub fn parse(chunk: &[u8], tex_count: u16) -> Result<Mat3, BmdError> {
    let r = BeReader::new(chunk);
    let mut h = r.at(8);
    let count = h.u16()? as usize;
    h.skip(2)?;
    let mut offsets = [0usize; 30];
    for offset in &mut offsets {
        *offset = h.u32()? as usize;
    }

    let names = read_name_table(&r, offsets[NAME_TABLE])?;
    if names.len() != count {
        return Err(BmdError::Invariant(format!(
            "MAT3 has {count} materials but {} names",
            names.len()
        )));
    }

    let mut remap = Vec::with_capacity(count);
    let mut materials = Vec::with_capacity(count);
    for (slot, name) in names.iter().enumerate() {
        let remap_index = r.at(offsets[REMAP_TABLE] + slot * 2).u16()?;
        remap.push(remap_index);
        let record = offsets[MATERIAL_DATA] + remap_index as usize * MATERIAL_RECORD_SIZE;
        let ctx = Ctx {
            r: &r,
            offsets: &offsets,
            name,
        };
        let mut material = ctx.material(record)?;
        // gclib's presence heuristic: the indirect list exists iff its
        // offset differs from the (always-present) name table's.
        if offsets[INDIRECT] != offsets[NAME_TABLE] {
            material.indirect = Some(ctx.indirect(offsets[INDIRECT] + slot * 0x138)?);
        }
        for (i, tex) in material.texture_indices.iter().enumerate() {
            if let Some(tex) = *tex
                && tex >= tex_count
            {
                return Err(BmdError::Invariant(format!(
                    "material {name}: texture slot {i} references TEX1 entry {tex} of {tex_count}"
                )));
            }
        }
        materials.push(material);
    }
    Ok(Mat3 {
        names,
        remap,
        materials,
    })
}

/// Per-material parse context: the chunk reader, the list offsets, and the
/// material name for error messages.
struct Ctx<'a, 'b> {
    r: &'b BeReader<'a>,
    offsets: &'b [usize; 30],
    name: &'b str,
}

impl<'a> Ctx<'a, '_> {
    fn gx<T: TryFrom<u8, Error = GxEnumError>>(
        &self,
        value: u8,
        field: &str,
    ) -> Result<T, BmdError> {
        T::try_from(value).map_err(|source| BmdError::Gx {
            context: format!("material {}: {field}", self.name),
            source,
        })
    }

    fn boolean(&self, value: u8, field: &str) -> Result<bool, BmdError> {
        gx_bool(value, "bool").map_err(|source| BmdError::Gx {
            context: format!("material {}: {field}", self.name),
            source,
        })
    }

    /// Resolves a u8 record index into a sub-reader positioned at the
    /// element (`elem` bytes apart) in `list`; a 0xFF sentinel or a zero
    /// list offset means absent.
    fn idx8(&self, rec: &mut BeReader, list: usize, elem: usize) -> BeResult<Option<BeReader<'a>>> {
        let index = rec.u8()?;
        if index == 0xFF {
            return Ok(None);
        }
        Ok(self.at_list(list, index as usize, elem))
    }

    fn idx16(
        &self,
        rec: &mut BeReader,
        list: usize,
        elem: usize,
    ) -> BeResult<Option<BeReader<'a>>> {
        let index = rec.u16()?;
        if index == 0xFFFF {
            return Ok(None);
        }
        Ok(self.at_list(list, index as usize, elem))
    }

    fn at_list(&self, list: usize, index: usize, elem: usize) -> Option<BeReader<'a>> {
        let offset = self.offsets[list];
        if offset == 0 {
            return None; // valid index into a nonexistent list (gclib: cc.bmd)
        }
        Some(self.r.at(offset + index * elem))
    }

    /// One u16-indexed field.
    fn one<T>(
        &self,
        rec: &mut BeReader,
        list: usize,
        elem: usize,
        f: impl Fn(&Self, &mut BeReader<'a>) -> Result<T, BmdError>,
    ) -> Result<Option<T>, BmdError> {
        self.idx16(rec, list, elem)?
            .map(|mut r| f(self, &mut r))
            .transpose()
    }

    /// A fixed-size array of u16-indexed fields.
    fn arr<T, const N: usize>(
        &self,
        rec: &mut BeReader,
        list: usize,
        elem: usize,
        f: impl Fn(&Self, &mut BeReader<'a>) -> Result<T, BmdError>,
    ) -> Result<[Option<T>; N], BmdError> {
        let mut out = std::array::from_fn(|_| None);
        for slot in &mut out {
            *slot = self.one(rec, list, elem, &f)?;
        }
        Ok(out)
    }

    fn material(&self, record: usize) -> Result<Material, BmdError> {
        let rec = &mut self.r.at(record);
        let pe_mode = self.gx(rec.u8()?, "pixelEngineMode")?;
        // the cull-mode list stores u32 values (J3DMaterialBlock)
        let cull_mode = self
            .idx8(rec, CULL_MODE, 4)?
            .map(|mut r| {
                let raw = r.u32()?;
                let byte = u8::try_from(raw).map_err(|_| BmdError::Gx {
                    context: format!("material {}: cullMode", self.name),
                    source: GxEnumError {
                        kind: "CullMode",
                        value: raw,
                    },
                })?;
                self.gx::<CullMode>(byte, "cullMode")
            })
            .transpose()?;
        let num_color_chans = self
            .idx8(rec, NUM_COLOR_CHANS, 1)?
            .map(|mut r| r.u8())
            .transpose()?;
        let num_tex_gens = self
            .idx8(rec, NUM_TEX_GENS, 1)?
            .map(|mut r| r.u8())
            .transpose()?;
        let num_tev_stages = self
            .idx8(rec, NUM_TEV_STAGES, 1)?
            .map(|mut r| r.u8())
            .transpose()?;
        let z_compare_loc = self
            .idx8(rec, Z_COMPARE, 1)?
            .map(|mut r| self.boolean(r.u8()?, "zCompareLoc"))
            .transpose()?;
        let z_mode = self
            .idx8(rec, Z_MODE, 4)?
            .map(|mut r| self.z_mode(&mut r))
            .transpose()?;
        let dither = self
            .idx8(rec, DITHER, 1)?
            .map(|mut r| self.boolean(r.u8()?, "dither"))
            .transpose()?;

        let material_colors = self.arr(rec, MAT_COLOR, 4, |_, r| Ok(rgba8(r)?))?;
        let color_channels = self.arr(rec, COLOR_CHANNEL, 8, Self::color_channel)?;
        let ambient_colors = self.arr(rec, AMBIENT_COLOR, 4, |_, r| Ok(rgba8(r)?))?;
        let light_colors = self.arr(rec, LIGHT_COLOR, 4, |_, r| Ok(rgba8(r)?))?;
        let tex_coord_gens = self.arr(rec, TEX_COORD_GEN, 4, Self::tex_gen)?;
        let post_tex_coord_gens = self.arr(rec, POST_TEX_COORD_GEN, 4, Self::tex_gen)?;
        let tex_matrices = self.arr(rec, TEX_MATRIX, 0x64, Self::tex_matrix)?;
        let post_tex_matrices = self.arr(rec, POST_TEX_MATRIX, 0x64, Self::tex_matrix)?;
        let texture_indices = self.arr(rec, TEXTURE_REMAP, 2, |_, r| Ok(r.u16()?))?;
        let konst_colors = self.arr(rec, TEV_KONST_COLOR, 4, |_, r| Ok(rgba8(r)?))?;

        let mut kcsels = [KonstColorSel::One; 16];
        for sel in &mut kcsels {
            *sel = self.gx(rec.u8()?, "konstColorSel")?;
        }
        let mut kasels = [KonstAlphaSel::One; 16];
        for sel in &mut kasels {
            *sel = self.gx(rec.u8()?, "konstAlphaSel")?;
        }

        let tev_orders = self.arr(rec, TEV_ORDER, 4, Self::tev_order)?;
        let tev_colors = self.arr(rec, TEV_COLOR, 8, |_, r| Ok(rgba_s16(r)?))?;
        let tev_stages = self.arr(rec, TEV_STAGE, 20, Self::tev_stage)?;
        let swap_modes: [Option<TevSwapMode>; 16] = self.arr(rec, TEV_SWAP_MODE, 4, |_, r| {
            Ok(TevSwapMode {
                ras_sel: r.u8()?,
                tex_sel: r.u8()?,
            })
        })?;

        // Junk-index guard (mirrors gclib): swap-table slots beyond the
        // highest ras/tex select actually referenced hold garbage indices in
        // real files and must be skipped, not chased.
        let max_sel = swap_modes
            .iter()
            .flatten()
            .map(|m| m.ras_sel.max(m.tex_sel))
            .max();
        let mut swap_tables: [Option<TevSwapTable>; 16] = std::array::from_fn(|_| None);
        for (i, slot) in swap_tables.iter_mut().enumerate() {
            match max_sel {
                Some(max) if i > max as usize => rec.skip(2)?,
                _ => {
                    *slot = self.one(rec, TEV_SWAP_TABLE, 4, |_, r| {
                        Ok(TevSwapTable {
                            rgba: [r.u8()?, r.u8()?, r.u8()?, r.u8()?],
                        })
                    })?
                }
            }
        }

        let fog = self.one(rec, FOG, 0x2C, Self::fog)?;
        let alpha_compare = self.one(rec, ALPHA_COMPARE, 8, Self::alpha_compare)?;
        let blend = self.one(rec, BLEND_MODE, 4, Self::blend)?;
        let nbt_scale = self.one(rec, NBT_SCALE, 0x10, Self::nbt_scale)?;
        debug_assert_eq!(rec.pos(), record + MATERIAL_RECORD_SIZE);

        Ok(Material {
            pe_mode,
            cull_mode,
            num_color_chans,
            num_tex_gens,
            num_tev_stages,
            z_compare_loc,
            z_mode,
            dither,
            material_colors,
            color_channels,
            ambient_colors,
            light_colors,
            tex_coord_gens,
            post_tex_coord_gens,
            tex_matrices,
            post_tex_matrices,
            texture_indices,
            konst_colors,
            kcsels,
            kasels,
            tev_orders,
            tev_colors,
            tev_stages,
            swap_modes,
            swap_tables,
            fog,
            alpha_compare,
            blend,
            nbt_scale,
            indirect: None,
        })
    }

    fn z_mode(&self, r: &mut BeReader) -> Result<ZMode, BmdError> {
        Ok(ZMode {
            test: self.boolean(r.u8()?, "zMode.test")?,
            func: self.gx(r.u8()?, "zMode.func")?,
            write: self.boolean(r.u8()?, "zMode.write")?,
        })
    }

    fn color_channel(&self, r: &mut BeReader) -> Result<ColorChannel, BmdError> {
        Ok(ColorChannel {
            lighting_enabled: self.boolean(r.u8()?, "chan.lightingEnabled")?,
            mat_src: self.gx(r.u8()?, "chan.matSrc")?,
            lit_mask: r.u8()?,
            diffuse: self.gx(r.u8()?, "chan.diffuseFn")?,
            attenuation: self.gx(r.u8()?, "chan.attnFn")?,
            amb_src: self.gx(r.u8()?, "chan.ambSrc")?,
        })
    }

    fn tex_gen(&self, r: &mut BeReader) -> Result<TexGen, BmdError> {
        Ok(TexGen {
            ty: self.gx(r.u8()?, "texGen.type")?,
            src: self.gx(r.u8()?, "texGen.src")?,
            matrix: self.gx(r.u8()?, "texGen.matrix")?,
        })
    }

    fn tex_matrix(&self, r: &mut BeReader) -> Result<TexMatrix, BmdError> {
        let projection = self.gx(r.u8()?, "texMtx.projection")?;
        let info = r.u8()?;
        let map_mode = self.gx(info & 0x3F, "texMtx.mapMode")?;
        let is_maya = info & 0x80 != 0;
        r.skip(2)?;
        let center = [r.f32()?, r.f32()?, r.f32()?];
        let scale = [r.f32()?, r.f32()?];
        let rotation = r.u16()?;
        r.skip(2)?;
        let translation = [r.f32()?, r.f32()?];
        let mut effect_matrix = [0f32; 16];
        for v in &mut effect_matrix {
            *v = r.f32()?;
        }
        Ok(TexMatrix {
            projection,
            map_mode,
            is_maya,
            center,
            scale,
            rotation,
            translation,
            effect_matrix,
        })
    }

    fn tev_order(&self, r: &mut BeReader) -> Result<TevOrder, BmdError> {
        Ok(TevOrder {
            tex_coord: self.gx(r.u8()?, "tevOrder.texCoord")?,
            tex_map: self.gx(r.u8()?, "tevOrder.texMap")?,
            channel: self.gx(r.u8()?, "tevOrder.channel")?,
        })
    }

    fn tev_stage(&self, r: &mut BeReader) -> Result<TevStage, BmdError> {
        let tev_mode = r.u8()?;
        let mut color_in = [CombineColor::Zero; 4];
        for c in &mut color_in {
            *c = self.gx(r.u8()?, "tevStage.colorIn")?;
        }
        let color_op = self.gx(r.u8()?, "tevStage.colorOp")?;
        let color_bias = self.gx(r.u8()?, "tevStage.colorBias")?;
        let color_scale = self.gx(r.u8()?, "tevStage.colorScale")?;
        let color_clamp = self.boolean(r.u8()?, "tevStage.colorClamp")?;
        let color_reg = self.gx(r.u8()?, "tevStage.colorReg")?;
        let mut alpha_in = [CombineAlpha::Zero; 4];
        for a in &mut alpha_in {
            *a = self.gx(r.u8()?, "tevStage.alphaIn")?;
        }
        let alpha_op = self.gx(r.u8()?, "tevStage.alphaOp")?;
        let alpha_bias = self.gx(r.u8()?, "tevStage.alphaBias")?;
        let alpha_scale = self.gx(r.u8()?, "tevStage.alphaScale")?;
        let alpha_clamp = self.boolean(r.u8()?, "tevStage.alphaClamp")?;
        let alpha_reg = self.gx(r.u8()?, "tevStage.alphaReg")?;
        Ok(TevStage {
            tev_mode,
            color_in,
            color_op,
            color_bias,
            color_scale,
            color_clamp,
            color_reg,
            alpha_in,
            alpha_op,
            alpha_bias,
            alpha_scale,
            alpha_clamp,
            alpha_reg,
        })
    }

    fn fog(&self, r: &mut BeReader) -> Result<Fog, BmdError> {
        let fog_type = self.gx(r.u8()?, "fog.type")?;
        let enable = self.boolean(r.u8()?, "fog.enable")?;
        let center = r.u16()?;
        let start_z = r.f32()?;
        let end_z = r.f32()?;
        let near_z = r.f32()?;
        let far_z = r.f32()?;
        let color = rgba8(r)?;
        let mut range_adjustments = [0u16; 10];
        for adj in &mut range_adjustments {
            *adj = r.u16()?;
        }
        Ok(Fog {
            fog_type,
            enable,
            center,
            start_z,
            end_z,
            near_z,
            far_z,
            color,
            range_adjustments,
        })
    }

    fn alpha_compare(&self, r: &mut BeReader) -> Result<AlphaCompare, BmdError> {
        Ok(AlphaCompare {
            comp0: self.gx(r.u8()?, "alphaComp.comp0")?,
            ref0: r.u8()?,
            op: self.gx(r.u8()?, "alphaComp.op")?,
            comp1: self.gx(r.u8()?, "alphaComp.comp1")?,
            ref1: r.u8()?,
        })
    }

    fn blend(&self, r: &mut BeReader) -> Result<Blend, BmdError> {
        Ok(Blend {
            mode: self.gx(r.u8()?, "blend.mode")?,
            src: self.gx(r.u8()?, "blend.src")?,
            dst: self.gx(r.u8()?, "blend.dst")?,
            logic: self.gx(r.u8()?, "blend.logic")?,
        })
    }

    fn nbt_scale(&self, r: &mut BeReader) -> Result<NbtScale, BmdError> {
        let enable = self.boolean(r.u8()?, "nbtScale.enable")?;
        r.skip(3)?;
        Ok(NbtScale {
            enable,
            scale: [r.f32()?, r.f32()?, r.f32()?],
        })
    }

    fn indirect(&self, pos: usize) -> Result<Indirect, BmdError> {
        let mut r = self.r.at(pos);
        Ok(Indirect {
            enable: self.boolean(r.u8()?, "indirect.enable")?,
            num_stages: r.u8()?,
        })
    }
}

fn rgba8(r: &mut BeReader) -> BeResult<Rgba8> {
    Ok([r.u8()?, r.u8()?, r.u8()?, r.u8()?])
}

fn rgba_s16(r: &mut BeReader) -> BeResult<RgbaS16> {
    Ok([r.i16()?, r.i16()?, r.i16()?, r.i16()?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a, 'b>(r: &'b BeReader<'a>, offsets: &'b [usize; 30]) -> Ctx<'a, 'b> {
        Ctx {
            r,
            offsets,
            name: "test",
        }
    }

    #[test]
    fn tev_stage_decodes_hand_laid_bytes() {
        // mode 0xFF | color: a=ZERO b=TEXC c=RASC d=ZERO, ADD, bias ZERO,
        // scale ×2, clamp, REG0 | alpha: APREV/TEXA/RASA/KONST, SUB,
        // ADDHALF, ÷2, no clamp, PREV | pad
        let bytes: [u8; 20] = [
            0xFF, 0x0F, 0x08, 0x0A, 0x0F, 0x00, 0x00, 0x01, 0x01, 0x01, //
            0x00, 0x04, 0x05, 0x06, 0x01, 0x01, 0x03, 0x00, 0x00, 0xFF,
        ];
        let offsets = [0usize; 30];
        let r = BeReader::new(&bytes);
        let stage = ctx(&r, &offsets).tev_stage(&mut r.at(0)).unwrap();
        assert_eq!(stage.tev_mode, 0xFF);
        assert_eq!(
            stage.color_in,
            [
                CombineColor::Zero,
                CombineColor::TexC,
                CombineColor::RasC,
                CombineColor::Zero
            ]
        );
        assert_eq!(stage.color_op, TevOp::Add);
        assert_eq!(stage.color_scale, TevScale::Scale2);
        assert!(stage.color_clamp);
        assert_eq!(stage.color_reg, Register::Reg0);
        assert_eq!(
            stage.alpha_in,
            [
                CombineAlpha::APrev,
                CombineAlpha::TexA,
                CombineAlpha::RasA,
                CombineAlpha::Konst
            ]
        );
        assert_eq!(stage.alpha_op, TevOp::Sub);
        assert_eq!(stage.alpha_bias, TevBias::AddHalf);
        assert_eq!(stage.alpha_scale, TevScale::Divide2);
        assert!(!stage.alpha_clamp);
        assert_eq!(stage.alpha_reg, Register::Prev);
    }

    #[test]
    fn tev_stage_gap_value_is_gx_error() {
        let mut bytes = [0u8; 20];
        bytes[5] = 0x02; // color_op: gap between SUB=1 and COMP_R8_GT=8
        let offsets = [0usize; 30];
        let r = BeReader::new(&bytes);
        let err = ctx(&r, &offsets).tev_stage(&mut r.at(0)).unwrap_err();
        match err {
            BmdError::Gx { context, source } => {
                assert!(context.contains("colorOp"), "{context}");
                assert_eq!(source.value, 2);
            }
            other => panic!("expected Gx error, got {other:?}"),
        }
    }

    #[test]
    fn index_sentinels_and_zero_list_offsets_are_absent() {
        // buffer: a u8 list at offset 4 with values [7, 9]
        let bytes = [0xFF, 0xFF, 0x00, 0x01, 7u8, 9, 0, 0];
        let mut offsets = [0usize; 30];
        offsets[NUM_TEX_GENS] = 4;
        let r = BeReader::new(&bytes);
        let c = ctx(&r, &offsets);

        // index 1 → second element
        let mut rec = r.at(3); // byte 0x01
        let got = c
            .idx8(&mut rec, NUM_TEX_GENS, 1)
            .unwrap()
            .map(|mut r| r.u8().unwrap());
        assert_eq!(got, Some(9));
        // 0xFF sentinel → absent
        let mut rec = r.at(0);
        assert!(c.idx8(&mut rec, NUM_TEX_GENS, 1).unwrap().is_none());
        // valid index but zero list offset → absent (gclib: cc.bmd case)
        let mut rec = r.at(3);
        assert!(c.idx8(&mut rec, NUM_COLOR_CHANS, 1).unwrap().is_none());
        // u16 sentinel
        let mut rec = r.at(0);
        assert!(c.idx16(&mut rec, NUM_TEX_GENS, 1).unwrap().is_none());
    }

    #[test]
    #[ignore = "requires extracted assets (just extract-link); run via just link-verify-p2"]
    fn real_mat3_expectations() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/link/raw/cl.bdl");
        let Ok(data) = std::fs::read(path) else {
            eprintln!("skipping: {path} not present");
            return;
        };
        let model = crate::bmd::parse_model(&data).unwrap();
        let mat3 = &model.mat3;
        assert_eq!(mat3.materials.len(), 24);

        // recorded facts: the name table literally contains ear(2)..ear(8),
        // and only 11 distinct init records exist — face and the ear(N)
        // slots share record 0, the R-side eye/brow slots share the L-side
        // records (J3D material instancing via the remap table)
        assert_eq!(
            mat3.names,
            [
                "ear",
                "eyeL",
                "eyeLdamA",
                "eyeLdamB",
                "eyeR",
                "eyeRdamA",
                "eyeRdamB",
                "face",
                "mayuL",
                "mayuLdamA",
                "mayuLdamB",
                "mayuR",
                "mayuRdamA",
                "mayuRdamB",
                "mouth",
                "podA",
                "sleeve",
                "ear(2)",
                "ear(3)",
                "ear(4)",
                "ear(5)",
                "ear(6)",
                "ear(7)",
                "ear(8)"
            ]
        );
        assert_eq!(
            mat3.remap,
            [
                0, 1, 2, 3, 1, 4, 3, 0, 5, 6, 7, 5, 6, 7, 8, 9, 10, 0, 0, 0, 0, 0, 0, 0
            ]
        );

        // the frozen-subset gates the shaders rely on (phase_02.md)
        let mut srtg_seen = false;
        for (m, name) in mat3.materials.iter().zip(&mat3.names) {
            let stages = m.num_tev_stages.unwrap() as usize;
            assert!((1..=3).contains(&stages), "{name}: {stages} stages");
            if let Some(fog) = &m.fog {
                assert!(!fog.enable, "{name}: fog enabled");
            }
            if let Some(ind) = &m.indirect {
                assert!(!ind.enable, "{name}: indirect enabled");
            }
            for stage in m.tev_stages.iter().take(stages).flatten() {
                assert_eq!(stage.color_op, TevOp::Add, "{name}");
                assert_eq!(stage.alpha_op, TevOp::Add, "{name}");
                assert_eq!(stage.color_reg, Register::Prev, "{name}");
            }
            for texgen in m
                .tex_coord_gens
                .iter()
                .take(m.num_tex_gens.unwrap() as usize)
                .flatten()
            {
                if texgen.ty == TexGenType::Srtg {
                    assert_eq!(texgen.src, TexGenSrc::Color0);
                    srtg_seen = true;
                }
            }
        }
        assert!(
            srtg_seen,
            "no SRTG texgen found — the toon ramp path is missing"
        );
    }
}
