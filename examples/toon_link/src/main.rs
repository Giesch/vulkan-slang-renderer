//! Renders Toon Link from The Wind Waker.
//!
//! All 24 batches draw from one shared mesh, through 5 pipelines and one
//! bindless `Material` buffer. They record as 7 `cmd_draw_indexed_indirect`
//! commands, one per run of consecutive batches that share a pipeline. Each
//! sub-draw resolves its material through `SV_DrawIndex`. See [`Run`].
//!
//! The example applies the model's albedo textures, the per-material raster
//! state, the GX TEV interpreter (`shaders/source/tev.slang`), and
//! gamma-correct output. The eye and brow
//! decals deposit coverage in destination alpha, then composite through the
//! hair with `BlendMode::DstAlpha`. See [`DrawGroups`].
//!
//! Light 0 is fixed in world space. The model turns under it, which sweeps
//! the terminator across Link. The eflight is fixed in model space, so it
//! turns with him and its highlight stays pinned to his front.
//!
//! The example needs converted assets on disk. Run
//! `just toon_link extract-link && just toon_link convert-link`. The assets
//! are gitignored and need the disc image.
//!
//! Debug builds show an egui window. [`EditState`] documents each control.
//! The shader's `DebugMode` enum documents the debug views.

mod generated;
mod tev_pack;

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context;
use facet::Facet;
use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};
use image::ImageReader;

use mltrs::editor::{Checkbox, Label, RGBPicker, Slider};
use mltrs::game::Game;
// The manifest's GX enums keep the `mm::` prefix. `mm::CullMode` and
// `mm::BlendMode` collide with the renderer's pipeline enums of the same name.
use gx::model_manifest::{self as mm, Batch, Manifest, MaterialEntry, TextureEntry};
use mltrs::renderer::{
    BindlessHandle, BlendMode, CullMode, DepthCompare, DrawError, DrawIndexedIndirect,
    DrawIndexedIndirectCommand, FrameRenderer, ImmutableBufferHandle, MeshHandle, PipelineHandle,
    PushBlock, RasterState, Renderer, RgbaPixels, Sampler2D, SamplerOptions, SingletonBufferHandle,
    TextureColorSpace, TextureFilter, TextureHandle, TextureOptions, TextureWrap,
    UniformBufferHandle,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::tev::{GXAlphaOp, GXCompare};
use crate::generated::shader_atlas::toon_link::*;

fn main() -> Result<(), anyhow::Error> {
    ToonLink::run()
}

/// Winding debug knob. `Some(CullMode::None)` shows every triangle regardless
/// of winding. `None` uses each material's cull mode from the manifest.
const CULL_OVERRIDE: Option<CullMode> = None;

/// Link is 124 model units tall, with his feet at Y = 0. This scales him to
/// 1.24 world units.
const MODEL_SCALE: f32 = 0.01;

/// Radians per second the model turns about Y. Light 0 is fixed in world
/// space, so this rotation sweeps the terminator.
const MODEL_SPIN: f32 = 20.0 * (PI / 180.0);

/// The number of frames in the rolling FPS average.
const FRAME_HISTORY_SIZE: usize = 60;

/// `link.vtx.bin` is interleaved little-endian f32: pos[3] nrm[3] uv0[2].
const VERTEX_STRIDE: usize = 32;

/// An index into `Manifest::materials`, into [`MaterialTable::base`], and into
/// the GPU's `Material` buffer.
///
/// Passed to the GPU as a device address in the [`DrawSlot`] table,
/// read in the shader with `SV_DrawIndex`.
///
/// Not interchangeable with [`BatchIndex`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MaterialSlot(usize);

impl MaterialSlot {
    fn from_manifest(material: u16) -> Self {
        Self(material as usize)
    }

    fn raw(self) -> usize {
        self.0
    }
}

/// An index into `Manifest::batches`, in INF1 draw order.
/// Not interchangeable with [`MaterialSlot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct BatchIndex(usize);

impl BatchIndex {
    fn from_raw(index: usize) -> Self {
        Self(index)
    }

    fn raw(self) -> usize {
        self.0
    }
}

/// Whether the J3D pixel-engine mode is translucent. This mode separates the
/// eye and brow decals from the opaque model.
fn is_translucent(material: &MaterialEntry) -> anyhow::Result<bool> {
    match material.pe_mode {
        mm::PixelEngineMode::Opaque => Ok(false),
        mm::PixelEngineMode::Translucent => Ok(true),
        // cl.bdl has none
        other => anyhow::bail!("unmapped pe_mode {other} on material {:?}", material.name),
    }
}

/// The face. Matched by name because no state signature separates it from the
/// other 8 opaque materials. `hideHatAndBackle`
/// (`tww/src/d/actor/d_a_player_main.cpp:1512-1514`) names both material
/// strings verbatim, so the name is the game's own contract.
const FACE_MATERIAL: &str = "face";
/// The bangs. The eye composite reads through this material.
const HAIR_MATERIAL: &str = "ear(2)";

/// One of GX's 3 eye and brow decal passes. `daPy_lk_c` names them
/// `mpZOnShape`, `mpZOffBlendShape` and `mpZOffNoneShape`.
///
/// The 12 translucent batches are 3 passes × 4 features, not 12 BTP frames.
/// The 3 shapes of a feature are byte-identical geometry, authored 3 times so
/// the material state can differ. The game draws all 12 every frame.
///
/// This depends on the renderer clearing swapchain alpha to 0 rather than 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecalRole {
    /// `*damA`. Z-tested, source-alpha blended, color writes off. It deposits
    /// the feature's coverage in destination alpha. The z-test against the
    /// already-drawn geometry stops eyes appearing through walls.
    Mask,
    /// `eyeL`, `eyeR`, `mayuL` and `mayuR`. Dst-alpha blended, depth test off.
    /// It composites the feature through whatever was drawn over it. The
    /// renderer clears alpha to 0, as GX does, so destination alpha outside
    /// the feature is 0 and the pass leaves those pixels untouched.
    Composite,
    /// `*damB`. Blending off, TEV alpha identically 0. It zeroes the mask so
    /// the mask cannot leak into later alpha-buffer effects. Its RGB is black.
    Erase,
}

/// Classify by state, never by name. `playerInit`
/// (`tww/src/d/actor/d_a_player_main.cpp:12150-12178`) derives its 3 arrays
/// from `(z_compare_enable, blend_type)` on the materials under the `CL_EYE`
/// and `CL_MAYU` joints, then asserts 4/4/4. The names serve only as the
/// assertion message.
///
/// Returns `Ok(None)` for every opaque material.
fn decal_role(material: &MaterialEntry) -> anyhow::Result<Option<DecalRole>> {
    if !is_translucent(material)? {
        return Ok(None);
    }

    if material.z_test {
        return Ok(Some(DecalRole::Mask));
    }

    // Keyed on the blend mode, not the factors. GX ignores src and dst when
    // the mode is None_, so the `*damB` materials carry
    // Source_Alpha/Inverse_Source_Alpha in MAT3 without blending.
    match material.blend.as_ref().map(|blend| blend.mode) {
        Some(mm::BlendMode::Blend) => Ok(Some(DecalRole::Composite)),
        Some(mm::BlendMode::None) | None => Ok(Some(DecalRole::Erase)),
        Some(other @ (mm::BlendMode::Logic | mm::BlendMode::Subtract)) => {
            let mat = &material.name;
            anyhow::bail!("translucent material {mat:?} has unclassifiable GX blend mode {other}")
        }
    }
}

/// The 5 groups of the hardware's draw order, each in INF1 order.
struct DrawGroups {
    /// Group 1. Deposits the eye and brow coverage in destination alpha,
    /// z-tested against the already-drawn geometry.
    mask: Vec<BatchIndex>,
    /// Group 2. The face and the bangs, drawn with color and depth writes but
    /// no alpha writes, so the mask survives underneath them. They draw ahead
    /// of the composite. The game hides both for P1, so they draw once.
    face_hair: Vec<BatchIndex>,
    /// Group 3. Composites `out = eye·dstA + fb·(1−dstA)` with the depth test
    /// off. The eyes read through the hair.
    composite: Vec<BatchIndex>,
    /// Group 4. Zeroes the mask so the mask cannot leak into later
    /// alpha-buffer effects.
    erase: Vec<BatchIndex>,
    /// Group 5. The rest of the model, which the game draws in P1.
    rest: Vec<BatchIndex>,
}

impl DrawGroups {
    /// The 5 groups concatenated. This must list every group: `group_batches`
    /// pushes each batch into exactly one group, so a group omitted here drops
    /// its batches. `setup` checks the length.
    fn draw_order(&self) -> Vec<BatchIndex> {
        [
            &self.mask,
            &self.face_hair,
            &self.composite,
            &self.erase,
            &self.rest,
        ]
        .into_iter()
        .flatten()
        .copied()
        .collect()
    }
}

/// Classify every batch into its group, preserving INF1 order within each
/// group.
fn group_batches(manifest: &Manifest) -> anyhow::Result<DrawGroups> {
    let material_of = |batch: &Batch| -> &MaterialEntry {
        &manifest.materials[MaterialSlot::from_manifest(batch.material).raw()]
    };
    let (mut mask, mut face_hair, mut composite, mut erase, mut rest) =
        (vec![], vec![], vec![], vec![], vec![]);
    for (i, batch) in manifest.batches.iter().enumerate() {
        let index = BatchIndex::from_raw(i);
        let material = material_of(batch);
        match decal_role(material)? {
            Some(DecalRole::Mask) => mask.push(index),
            Some(DecalRole::Composite) => composite.push(index),
            Some(DecalRole::Erase) => erase.push(index),
            None if matches!(material.name.as_str(), FACE_MATERIAL | HAIR_MATERIAL) => {
                face_hair.push(index)
            }
            None => rest.push(index),
        }
    }

    // The same assertion `playerInit` makes. It fires if `--casual` or a
    // converter change perturbs the material table. It also covers "every
    // translucent batch was consumed": `decal_role` returns `Some` for every
    // translucent material, or bails.
    anyhow::ensure!(
        mask.len() == 4 && composite.len() == 4 && erase.len() == 4,
        "expected 4 mask / 4 composite / 4 erase eye-brow decals covering all 12 \
         translucent batches, got {} / {} / {} (total {}); `playerInit` asserts \
         zon_cnt == 4 && zoff_blend_cnt == 4 && zoff_none_cnt == 4",
        mask.len(),
        composite.len(),
        erase.len(),
        mask.len() + composite.len() + erase.len()
    );

    // Bail on a missing or duplicated name. A wrong name moves the wrong batch
    // into the face and hair group, and the symptom is subtle: the eyes
    // composite over the wrong surface.
    let face_hair_names: Vec<&str> = face_hair
        .iter()
        .map(|b: &BatchIndex| material_of(&manifest.batches[b.raw()]).name.as_str())
        .collect();
    anyhow::ensure!(
        face_hair_names.len() == 2
            && face_hair_names.contains(&FACE_MATERIAL)
            && face_hair_names.contains(&HAIR_MATERIAL),
        "expected exactly one {FACE_MATERIAL:?} batch and one {HAIR_MATERIAL:?} batch \
         to pull ahead of the eye composite, found {face_hair_names:?}"
    );

    Ok(DrawGroups {
        mask,
        face_hair,
        composite,
        erase,
        rest,
    })
}

/// The manifest's GX alpha-compare state as the shader's `GXAlphaCompare`
/// block. A material with no record gets GX's default of "Always OR Always",
/// which keeps every fragment.
fn alpha_compare(material: &MaterialEntry) -> GXAlphaCompare {
    let (comp0, ref0, comp1, ref1, op) = match &material.alpha_compare {
        None => (GXCompare::Always, 0, GXCompare::Always, 0, GXAlphaOp::Or),
        Some(ac) => (
            gx_compare(ac.comp0),
            ac.ref0 as u32,
            gx_compare(ac.comp1),
            ac.ref1 as u32,
            gx_alpha_op(ac.op),
        ),
    };

    GXAlphaCompare {
        comp0,
        ref0,
        comp1,
        ref1,
        op,
    }
}

/// The manifest's `mm::CompareType` as the shader's generated `GXCompare`.
/// The match is exhaustive rather than a numeric cast. A `repr(u32)` enum that
/// holds an undeclared value is UB, so no code crosses by value.
fn gx_compare(comp: mm::CompareType) -> GXCompare {
    match comp {
        mm::CompareType::Never => GXCompare::Never,
        mm::CompareType::Less => GXCompare::Less,
        mm::CompareType::Equal => GXCompare::Equal,
        mm::CompareType::LessEqual => GXCompare::LessEqual,
        mm::CompareType::Greater => GXCompare::Greater,
        mm::CompareType::NotEqual => GXCompare::NotEqual,
        mm::CompareType::GreaterEqual => GXCompare::GreaterEqual,
        mm::CompareType::Always => GXCompare::Always,
    }
}

fn gx_alpha_op(op: mm::AlphaOp) -> GXAlphaOp {
    match op {
        mm::AlphaOp::And => GXAlphaOp::And,
        mm::AlphaOp::Or => GXAlphaOp::Or,
        mm::AlphaOp::Xor => GXAlphaOp::Xor,
        mm::AlphaOp::Xnor => GXAlphaOp::Xnor,
    }
}

/// The 2 GX lights that `lit_mask == 3` selects. Each light carries exactly
/// one channel, as in the game. `ZBtoonEX` is a separable 2D ramp: its red
/// varies only with u, its green only with v, and both step sharply at 0.49.
/// The SRTG texgen feeds it `(color0.r, color0.g)`. The two axes are
/// independent lookups only because the lights write to different channels.
///
/// - Light 0 is red-only (`tww/src/d/d_kankyo.cpp:1494-1499`, green and
///   blue zeroed at `:1545-1547`). Its ramp axis drives stage 0's toon band.
/// - Light 1 is green-only. It stays dark unless an "eflight" such as a torch
///   or a sword glow is nearby (`:2557-2559`, gated at `:2527-2531`). Its ramp
///   axis drives stage 2's warm additive highlight.
///   [`LightRig::eflight`] turns it on.
///
/// Ambient is fixed at 50/255 = 0.196 on every channel. `illum.r` crosses the
/// ramp's 0.49 step at `N·L = 0.294`. `illum.g` stays below the step until the
/// eflight comes on.
const LIGHT0_COLOR: Vec3 = Vec3::new(1.0, 0.0, 0.0);
const LIGHT1_COLOR: Vec3 = Vec3::new(0.0, 0.0, 0.0);
/// Light 1 with the eflight on. The game ramps the green byte with distance
/// and flicker (`tww/src/d/d_kankyo.cpp:2542-2557`). This example takes it at
/// full.
const EFLIGHT_COLOR: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// Stage 2's additive tint while the eflight is on. It replaces the manifest's
/// `konst_colors[1]`, because `setLightTevColorType_sub` overwrites K1
/// whenever that stage runs (`tww/src/d/d_kankyo.cpp:1780`). The value is the
/// treasure chest's steady glow, verbatim from
/// `tww/src/d/actor/d_a_tbox.cpp:302-304`. It seeds the debug window's
/// `eflight_konst` picker, and `draw` writes the picker value.
const EFLIGHT_KONST: Vec3 = rgb8(255, 255, 100);
/// How much of [`EFLIGHT_KONST`] reaches K1. The game scales the registered
/// color by `bright²`, where `bright = 1 - distance/power`
/// (`tww/src/d/d_kankyo.cpp:1567-1584`). This value is that factor at half the
/// light's radius: `(1 - 0.5)² = 0.25`. The unscaled near-white glow saturates
/// the tunic. It seeds the debug window's `eflight_falloff` slider.
const EFLIGHT_FALLOFF: f32 = 0.25;

/// Light 0's fixed orientation, in world space. The game's key light is the
/// sun, the moon, or the nearest torch, so it does not move with the actor.
/// The terminator sweeps because [`MODEL_SPIN`] turns Link under the light.
const LIGHT0_AZIMUTH: f32 = 0.6;
const LIGHT0_ELEVATION: f32 = 0.7;

/// The eflight's orientation, in model space. It rotates with Link, so the
/// highlight stays pinned to his front while light 0's terminator sweeps past.
/// That is the arrangement when the glow comes from something he faces, such
/// as a treasure chest's light 50 units above the chest
/// (`tww/src/d/actor/d_a_tbox.cpp:301`). Azimuth 0 is straight ahead. The
/// model faces +Z, measured off `cl.bdl`.
///
/// The elevation is negative because that light sits near Link's waist and
/// shines up at his torso. `cl.bdl` spans `y = 0..124`. `-0.35` is
/// `atan2(50 - 85, 90)`: the light at 50, his upper chest at 85, and a
/// separation of about one body length. It seeds the debug window's
/// `eflight_elevation` slider.
const EFLIGHT_AZIMUTH: f32 = 0.0;
const EFLIGHT_ELEVATION: f32 = -0.35;

/// The 2 endpoints of stage 0's toon lerp, `PREV = mix(REG0, K0, ramp.r)`.
///
/// These values are measured, not authored. `scripts/link_env_colors.py` reads
/// them out of the ocean stage's `Pale` chunk. Run `just link-env-colors`. The
/// values come from the 150-270 schedule plateau, about 10:00 to 18:00, which
/// is the one band that needs no time-of-day blend.
///
/// The game overwrites both registers every frame in
/// `setLightTevColorType_sub` (`tww/src/d/d_kankyo.cpp:1817-1829`), so the
/// manifest's values are only the defaults J3D loaded. `setLight_actor`
/// (`tww/src/d/d_kankyo.cpp:1328-1353`) wires `Pale` to `dKy_tevstr_c`.
///
/// These seed the debug window's `env_actor_c0` and `env_actor_k0` pickers,
/// and `draw` writes the picker values. Another time of day's plateau needs no
/// rebuild.
const ENV_ACTOR_C0: Vec3 = rgb8(156, 140, 134);
const ENV_ACTOR_K0: Vec3 = rgb8(255, 255, 255);

/// A GX color, written as the bytes the decomp and the disc data hold. The
/// constants stay greppable against their sources.
const fn rgb8(r: u8, g: u8, b: u8) -> Vec3 {
    Vec3::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// Light 0 is fixed in the world and the eflight is fixed relative to Link.
/// Only the eflight varies: whether it is lit, and where it sits.
struct LightRig {
    /// Whether a nearby eflight is lighting light 1's green channel. Off is
    /// the common case in the game. See [`LIGHT1_COLOR`].
    eflight: bool,
    /// Radians above the horizontal. It is negative for a light below Link.
    /// See [`EFLIGHT_ELEVATION`].
    eflight_elevation: f32,
}

impl Default for LightRig {
    fn default() -> Self {
        Self {
            eflight: false,
            eflight_elevation: EFLIGHT_ELEVATION,
        }
    }
}

impl LightRig {
    /// `lightDir[i]` points from the surface toward light i, in world space.
    /// The shader does not negate. This function is the only place that sets
    /// the sign convention.
    ///
    /// The 2 lights live in different frames. Light 0 is anchored in the
    /// world, so `spin` sweeps its terminator across Link. The eflight is
    /// anchored to Link, so its highlight turns with him.
    fn directions(&self, spin: f32) -> [Vec4; 2] {
        let dir = |az: f32, el: f32| {
            Vec3::new(el.cos() * az.sin(), el.sin(), el.cos() * az.cos()).normalize()
        };
        [
            dir(LIGHT0_AZIMUTH, LIGHT0_ELEVATION).extend(0.0),
            // Model space to world space by the same Y rotation the vertices
            // get. That rotation pins the light to Link. The light shows only
            // when `eflight` is on, because its color is otherwise black.
            (Mat3::from_rotation_y(spin) * dir(EFLIGHT_AZIMUTH, self.eflight_elevation))
                .extend(0.0),
        ]
    }

    fn colors(&self) -> [Vec4; 2] {
        let light1 = if self.eflight {
            EFLIGHT_COLOR
        } else {
            LIGHT1_COLOR
        };
        [LIGHT0_COLOR.extend(1.0), light1.extend(1.0)]
    }
}

fn converted_dir() -> PathBuf {
    // The directory is gitignored and machine-local. It sits inside this crate
    // like every other example's assets. `just toon_link extract-link` writes
    // it here.
    mltrs::manifest_path!["assets", "link", "converted"]
}

fn load_manifest(dir: &Path) -> anyhow::Result<Manifest> {
    let path = dir.join("link.manifest.json");
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "{}: not found. Run `just toon_link extract-link && just toon_link convert-link` \
             first. The assets are gitignored and need the disc image.",
            path.display()
        )
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Read a whole binary file. It must hold exactly `count` records of `stride`
/// bytes.
fn read_records(path: &Path, count: u32, stride: usize, what: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(
        bytes.len() == count as usize * stride,
        "{}: expected {count} {what} × {stride} bytes, got {} bytes",
        path.display(),
        bytes.len()
    );
    Ok(bytes)
}

fn load_vertices(path: &Path, expected_count: u32) -> anyhow::Result<Vec<Vertex>> {
    let bytes = read_records(path, expected_count, VERTEX_STRIDE, "vertices")?;
    let read_f32 = |b: &[u8], i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    let vertices = bytes
        .chunks_exact(VERTEX_STRIDE)
        .map(|v| Vertex {
            position: Vec3::new(read_f32(v, 0), read_f32(v, 1), read_f32(v, 2)),
            normal: Vec3::new(read_f32(v, 3), read_f32(v, 4), read_f32(v, 5)),
            uv0: Vec2::new(read_f32(v, 6), read_f32(v, 7)),
        })
        .collect();
    Ok(vertices)
}

fn load_indices(path: &Path, expected_count: u32) -> anyhow::Result<Vec<u32>> {
    let bytes = read_records(path, expected_count, 4, "u32 indices")?;
    let indices = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    Ok(indices)
}

/// Check the manifest against the loaded buffers before building anything
/// from it.
fn validate_manifest(
    manifest: &Manifest,
    vertices: &[Vertex],
    indices: &[u32],
) -> anyhow::Result<()> {
    anyhow::ensure!(
        indices.len().is_multiple_of(3),
        "index count not a triangle list"
    );
    // The debug window's isolation slider indexes the batch list.
    anyhow::ensure!(!manifest.batches.is_empty(), "manifest has no batches");
    let max_index = indices.iter().copied().max().unwrap_or(0);
    anyhow::ensure!(
        (max_index as usize) < vertices.len(),
        "index {max_index} out of range for {} vertices",
        vertices.len()
    );

    let mut next_first_index = 0;
    for (i, batch) in manifest.batches.iter().enumerate() {
        anyhow::ensure!(
            batch.first_index == next_first_index,
            "batch {i} starts at {} but the previous batch ended at {next_first_index}",
            batch.first_index
        );
        anyhow::ensure!(
            MaterialSlot::from_manifest(batch.material).raw() < manifest.materials.len(),
            "batch {i} references material {} of {}",
            batch.material,
            manifest.materials.len()
        );
        next_first_index += batch.index_count;
    }
    anyhow::ensure!(
        next_first_index == manifest.buffers.index_count,
        "batches cover {next_first_index} of {} indices",
        manifest.buffers.index_count
    );

    // The shader binds 2 texmap slots. A model that uses a third must fail
    // here rather than lose its texture silently.
    for material in &manifest.materials {
        anyhow::ensure!(
            material.texmaps.iter().skip(2).all(Option::is_none),
            "material {:?} uses a texmap slot >= 2; the shader binds only slots 0 and 1",
            material.name
        );
    }
    Ok(())
}

fn texture_options(entry: &TextureEntry) -> anyhow::Result<TextureOptions> {
    let wrap = |mode: mm::WrapMode| match mode {
        mm::WrapMode::Clamp => TextureWrap::ClampToEdge,
        mm::WrapMode::Repeat => TextureWrap::Repeat,
        mm::WrapMode::Mirror => TextureWrap::MirroredRepeat,
    };
    // TextureFilter has no spelling for GX's 4 mipmapping filters. All of
    // cl.bdl's textures are Linear.
    let filter = match entry.filter {
        mm::FilterMode::Linear => TextureFilter::Linear,
        mm::FilterMode::Nearest => TextureFilter::Nearest,
        other => anyhow::bail!("unmapped GX texture filter {other}"),
    };
    Ok(TextureOptions {
        sampler: SamplerOptions {
            filter,
            wrap_u: wrap(entry.wrap_u),
            wrap_v: wrap(entry.wrap_v),
        },
        // GX has no sRGB, so the stored texels are raw values the shader
        // consumes directly. The fragment shader applies its own sRGB decode
        // for the _SRGB color target.
        color_space: TextureColorSpace::Unorm,
    })
}

/// One entry per manifest texture index. Only the 7 of 41 entries that a
/// material's `texmaps` references are loaded. The other 34 are BTP eye and
/// brow animation frames, which no code reaches without BTP support.
/// Unreferenced slots stay `None`.
fn load_textures(
    renderer: &mut Renderer,
    dir: &Path,
    manifest: &Manifest,
) -> anyhow::Result<Vec<Option<TextureHandle>>> {
    let mut referenced = vec![false; manifest.textures.len()];
    for material in &manifest.materials {
        for texmap in material.texmaps.iter().flatten() {
            referenced[*texmap as usize] = true;
        }
    }

    let mut textures: Vec<Option<TextureHandle>> = Vec::with_capacity(manifest.textures.len());
    for (i, entry) in manifest.textures.iter().enumerate() {
        if !referenced[i] {
            textures.push(None);
            continue;
        }
        // `entry.file` is manifest-relative.
        let image = ImageReader::open(dir.join(&entry.file))
            .with_context(|| format!("opening texture {}", entry.file))?
            .decode()
            .with_context(|| format!("decoding texture {}", entry.file))?
            .to_rgba8();
        let handle = renderer.create_texture_with_options(
            entry.file.clone(),
            RgbaPixels::new(image.width(), image.height(), &image)?,
            texture_options(entry)?,
        )?;
        textures.push(Some(handle));
    }

    Ok(textures)
}

/// The texture the shader reads for `material` at `slot`, as a heap handle.
/// It is the referenced albedo or ramp if the material has a texmap there, and
/// the 1×1 dummy otherwise. A handle is data: it goes in the `Material`
/// struct, not into a descriptor set, so one pipeline serves every material.
fn resolve_texmap(
    material: &MaterialEntry,
    slot: usize,
    textures: &[Option<TextureHandle>],
    dummy: &TextureHandle,
) -> BindlessHandle<Sampler2D> {
    material
        .texmaps
        .get(slot)
        .copied()
        .flatten()
        .and_then(|index| textures[index as usize].as_ref())
        .unwrap_or(dummy)
        .bindless_handle()
}

fn raster_state(material: &MaterialEntry, role: Option<DecalRole>) -> anyhow::Result<RasterState> {
    let cull = match CULL_OVERRIDE {
        Some(cull) => cull,
        None => match material.cull {
            mm::CullMode::Back => CullMode::Back,
            mm::CullMode::None => CullMode::None,
            mm::CullMode::Front => CullMode::Front,
            // cl.bdl does not use All.
            mm::CullMode::All => anyhow::bail!("unmapped GX cull mode {}", material.cull),
        },
    };

    // Honor z_func when the test is enabled. Pass unconditionally otherwise.
    // All 24 materials use Less_Equal.
    let depth_test = if material.z_test {
        match material.z_func {
            mm::CompareType::LessEqual => DepthCompare::LessEqual,
            mm::CompareType::Less => DepthCompare::Less,
            mm::CompareType::Always => DepthCompare::Always,
            other => anyhow::bail!(
                "unmapped GX depth func {other} on material {:?}",
                material.name
            ),
        }
    } else {
        DepthCompare::Always
    };

    // Honor z_write directly rather than tying it to z_test. The layered
    // `*damA` eye and brow decals composite only if they skip depth writes.
    let depth_write = material.z_write;

    // Alpha writes are on for the mask and erase passes only.
    // `l_onCupOffAupPacket2` is the last P0 packet
    // (`tww/src/m_Do/m_Do_ext.cpp:1845-1853`), so P1 also runs with
    // alphaUpdate = 0. The swapchain uses CompositeAlphaFlagsKHR::OPAQUE, so
    // nothing outside the frame reads framebuffer alpha.
    let color_write = match role {
        Some(DecalRole::Mask | DecalRole::Erase) => [false, false, false, true],
        Some(DecalRole::Composite) | None => [true, true, true, false],
    };

    // Every field is listed rather than `..Default::default()`, so a new
    // RasterState field is a compile error here.
    Ok(RasterState {
        blend: blend_mode(material)?,
        cull,
        depth_test,
        depth_write,
        color_write,
    })
}

fn blend_mode(material: &MaterialEntry) -> anyhow::Result<BlendMode> {
    let Some(blend) = &material.blend else {
        return Ok(BlendMode::Opaque);
    };

    // GX's None_ disables blending regardless of the factors.
    if blend.mode == mm::BlendMode::None {
        return Ok(BlendMode::Opaque);
    }

    if blend.mode == mm::BlendMode::Blend {
        use mm::BlendFactor::*;
        match (blend.src, blend.dst) {
            (SourceAlpha, InverseSourceAlpha) => return Ok(BlendMode::Alpha),
            // GX's dst-alpha blend. The mask pass writes the eye and brow
            // coverage into destination alpha. This mode composites through
            // that coverage, so the eyes read through the hair.
            (DestinationAlpha, InverseDestinationAlpha) => return Ok(BlendMode::DstAlpha),
            _ => {}
        }
    }

    anyhow::bail!(
        "unmapped blend mode {} (src {}, dst {}) on material {:?}",
        blend.mode,
        blend.src,
        blend.dst,
        material.name
    )
}

/// The distinct raster states (and pipelines created from them),
/// taken from `link.manifest.json`:
///
/// | cull, depth test, depth write, blend, color write | materials |
/// |---|---|
/// | Back, LessEqual, write, Opaque, RGB | 11 |
/// | Back, Always, no-write, Blend(DstA, InvDstA), RGB — `Composite` | 4 |
/// | Back, LessEqual, no-write, Blend(SrcA, InvSrcA), A — `Mask` | 4 |
/// | Back, Always, no-write, Opaque, A — `Erase` | 4 |
/// | None, LessEqual, write, Opaque, RGB — `sleeve` | 1 |
const EXPECTED_RASTER_STATES: usize = 5;

/// The pipeline set and the material table, built in one pass over
/// `Manifest::materials`. Push order defines a [`MaterialSlot`].
struct MaterialTable {
    /// One pipeline per distinct [`RasterState`], in first-use order.
    pipelines: Vec<PipelineHandle<DrawIndexedIndirect, PushBlock<ToonLinkDraw>>>,
    /// Maps a `MaterialSlot` to an index into [`Self::pipelines`]. Many slots
    /// share one pipeline.
    pipeline_of_slot: Vec<usize>,
    /// Maps a `MaterialSlot` to the manifest's values verbatim. It seeds the
    /// GPU material buffer once and never changes.
    /// Per-frame values belong in [`ToonLinkParams`].
    base: Vec<Material>,
}

/// Build the material table. Pipelines are deduplicated by raster state.
fn build_materials(
    renderer: &mut Renderer,
    shader: &Shader,
    manifest: &Manifest,
    mesh: &MeshHandle<Vertex>,
    params_buffer: &UniformBufferHandle<ToonLinkParams>,
    textures: &[Option<TextureHandle>],
    dummy: &TextureHandle,
) -> anyhow::Result<MaterialTable> {
    let mut pipelines = Vec::new();
    let mut raster_states: Vec<RasterState> = Vec::new();
    let mut pipeline_of_slot = Vec::with_capacity(manifest.materials.len());
    let mut base = Vec::with_capacity(manifest.materials.len());

    for material in &manifest.materials {
        let raster = raster_state(material, decal_role(material)?)?;
        let pipeline_index = match raster_states.iter().position(|&seen| seen == raster) {
            Some(index) => index,
            None => {
                // Every pipeline shares the one params buffer and the one
                // mesh. Only the raster state differs.
                let pipeline_config = shader
                    .pipeline_config(Resources { params_buffer })
                    .with_shared_mesh(mesh)
                    .with_raster_state(raster)
                    .indirect();
                pipelines.push(renderer.create_pipeline(pipeline_config)?);
                raster_states.push(raster);
                pipelines.len() - 1
            }
        };
        pipeline_of_slot.push(pipeline_index);

        base.push(Material {
            tex0: resolve_texmap(material, 0, textures, dummy),
            tex1: resolve_texmap(material, 1, textures, dummy),
            tev: tev_pack::pack(material)?,
            alpha_compare: alpha_compare(material),
            _padding_0: Default::default(),
        });
    }

    anyhow::ensure!(
        pipelines.len() == EXPECTED_RASTER_STATES,
        "expected {EXPECTED_RASTER_STATES} distinct raster states across {} materials, got {}: {:#?}",
        manifest.materials.len(),
        pipelines.len(),
        raster_states,
    );

    Ok(MaterialTable {
        pipelines,
        pipeline_of_slot,
        base,
    })
}

/// The GPU-side draw list. `commands` and `slots` hold one entry per batch, in
/// `draw_order`, and `runs` partitions them by pipeline.
struct DrawList {
    commands: Vec<DrawIndexedIndirectCommand>,
    slots: Vec<DrawSlot>,
    runs: Vec<Run>,
}

/// A span of consecutive `draw_order` entries that share one pipeline.
/// One Run corresponds with with one `cmd_draw_indexed_indirect` call.
struct Run {
    /// Index into [`MaterialTable::pipelines`].
    pipeline: usize,
    /// Index into [`ToonLink::args_buffer`] and the parallel
    /// [`ToonLink::slot_buffer`], which hold the batches in `draw_order`
    /// rather than in INF1 order.
    first: u32,
    count: u32,
}

/// Flatten `draw_order` into indirect commands, and group consecutive entries
/// that share a pipeline into runs.
fn build_draw_list(
    manifest: &Manifest,
    draw_order: &[BatchIndex],
    materials: &MaterialTable,
    renderer: &Renderer,
    materials_buffer: &SingletonBufferHandle<Material>,
) -> anyhow::Result<DrawList> {
    let mut commands = Vec::with_capacity(draw_order.len());
    let mut slots = Vec::with_capacity(draw_order.len());
    let mut runs: Vec<Run> = Vec::new();

    for &index in draw_order {
        let batch = &manifest.batches[index.raw()];
        let slot = MaterialSlot::from_manifest(batch.material);
        // singleton_addr_at asserts the same bound, but a panic there names no
        // batch; a bad manifest must fail as a setup error with batch context.
        anyhow::ensure!(
            slot.raw() < materials.base.len(),
            "batch {} references material {} of {}",
            index.raw(),
            slot.raw(),
            materials.base.len()
        );

        let command_idx = commands.len() as u32;
        let indirect_command = DrawIndexedIndirectCommand {
            index_count: batch.index_count,
            instance_count: 1,
            first_index: batch.first_index,
            vertex_offset: 0,
            first_instance: 0,
        };
        commands.push(indirect_command);
        let material = renderer.singleton_addr_at(materials_buffer, slot.raw() as u32);
        slots.push(DrawSlot { material });

        let pipeline = materials.pipeline_of_slot[slot.raw()];

        match runs.last_mut() {
            Some(run) if run.pipeline == pipeline => {
                run.count += 1;
            }

            _ => {
                runs.push(Run {
                    pipeline,
                    first: command_idx,
                    count: 1,
                });
            }
        }
    }

    Ok(DrawList {
        commands,
        slots,
        runs,
    })
}

pub struct ToonLink {
    start_time: Instant,
    /// The pipelines and the per-slot material records. Index them with
    /// [`Self::pipeline`], never with a [`BatchIndex`].
    materials: MaterialTable,
    /// One block for the whole example, shared by all 5 pipelines. It holds
    /// the frame globals, and `draw` uploads it.
    params_buffer: UniformBufferHandle<ToonLinkParams>,
    /// One indirect command per batch, in `draw_order`.
    args_buffer: ImmutableBufferHandle<DrawIndexedIndirectCommand>,
    /// The material pointer of the matching args_buffer entry.
    /// Each run's push block points at its own span; see [`Self::queue_run`].
    slot_buffer: SingletonBufferHandle<DrawSlot>,
    /// The runs in `args_buffer` that share a pipeline, in draw order.
    /// Each run corresponds to one multi-draw-indirect command.
    runs: Vec<Run>,
    edit_state: EditState,
    last_frame_time: Instant,
    frame_times: VecDeque<Duration>,
}

/// The egui debug window, generated by reflection over these fields.
/// `debug_mode` is the shader's own generated enum, so its variants render as
/// radio buttons with no parallel list to keep in sync.
#[derive(Facet)]
pub struct EditState {
    /// The rolling average over the last [`FRAME_HISTORY_SIZE`] frames.
    fps: Label,
    debug_mode: DebugMode,
    /// The second, green-channel light. Off is the common case in the game.
    eflight: Checkbox,
    /// Stage 2's additive tint, before `eflight_falloff` scales it. It reaches
    /// K1 only while `eflight` is checked. See [`EFLIGHT_KONST`].
    eflight_konst: RGBPicker,
    /// How much of `eflight_konst` reaches K1. See [`EFLIGHT_FALLOFF`].
    eflight_falloff: Slider,
    /// How far below Link the eflight sits, in radians. It runs from `0`, level
    /// with him, to `-0.5`. See [`EFLIGHT_ELEVATION`]. It is visible only while
    /// `eflight` is checked, because light 1 is otherwise black.
    eflight_elevation: Slider,
    /// Stage 0's toon lerp endpoints. The shadow end goes to `reg[1]` and the
    /// lit end to `konst[0]`. See [`ENV_ACTOR_C0`] and [`ENV_ACTOR_K0`].
    env_actor_c0: RGBPicker,
    env_actor_k0: RGBPicker,
}

impl ToonLink {
    // REVIEW let's change this to take a &Run as an argument
    /// Record `count` commands of [`Self::args_buffer`] starting at `first` as
    /// one indirect draw. The push block is set once for the whole command, so
    /// the slot table pointer is what tells the sub-draws apart.
    fn queue_run(&self, renderer: &mut FrameRenderer, pipeline: usize, first: u32, count: u32) {
        let draw_slots = renderer.singleton_addr_at(&self.slot_buffer, first);
        let push = ToonLinkDraw { draw_slots };

        renderer.queue_draw_indexed_indirect_with_push_constants(
            &self.materials.pipelines[pipeline],
            &self.args_buffer,
            first,
            count,
            &push,
        );
    }
}

impl Game for ToonLink {
    type EditState = EditState;
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Toon Link"
    }

    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("Toon Link", &mut self.edit_state))
    }

    fn frame_delay(&self) -> Duration {
        Duration::from_millis(5)
    }

    fn update(&mut self) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame_time);
        self.last_frame_time = now;

        self.frame_times.push_back(delta);
        if self.frame_times.len() > FRAME_HISTORY_SIZE {
            self.frame_times.pop_front();
        }

        let total: Duration = self.frame_times.iter().sum();
        let avg_frame_time = total.as_secs_f64() / self.frame_times.len() as f64;
        let fps = 1.0 / avg_frame_time;
        self.edit_state.fps.set(format!("{fps:.0}"));
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let dir = converted_dir();
        let manifest = load_manifest(&dir)?;

        let vertices = load_vertices(
            &dir.join(&manifest.buffers.vertices),
            manifest.buffers.vertex_count,
        )?;
        let indices = load_indices(
            &dir.join(&manifest.buffers.indices),
            manifest.buffers.index_count,
        )?;

        validate_manifest(&manifest, &vertices, &indices)?;

        let mesh = renderer.create_mesh(&vertices, &indices)?;

        let textures = load_textures(renderer, &dir, &manifest)?;
        let white_square = renderer.create_texture_with_options(
            "toon_link_white_square",
            RgbaPixels::new(1, 1, &[255; 4])?,
            TextureOptions {
                sampler: SamplerOptions {
                    filter: TextureFilter::Linear,
                    wrap_u: TextureWrap::ClampToEdge,
                    wrap_v: TextureWrap::ClampToEdge,
                },
                color_space: TextureColorSpace::Unorm,
            },
        )?;

        let params_buffer = renderer.create_uniform_buffer::<ToonLinkParams>()?;
        let materials = build_materials(
            renderer,
            &shaders.toon_link,
            &manifest,
            &mesh,
            &params_buffer,
            &textures,
            &white_square,
        )?;
        let materials_buffer = renderer.create_singleton_buffer(&materials.base)?;

        let groups = group_batches(&manifest)?;
        let draw_order = groups.draw_order();
        // A dropped batch has no other symptom than a missing decal. See
        // `DrawGroups::draw_order`.
        anyhow::ensure!(
            draw_order.len() == manifest.batches.len(),
            "draw order covers {} of {} batches",
            draw_order.len(),
            manifest.batches.len()
        );

        let draw_list = build_draw_list(
            &manifest,
            &draw_order,
            &materials,
            renderer,
            &materials_buffer,
        )?;
        let mut args_buffer = renderer.create_immutable_buffer(draw_list.commands.len() as u32)?;
        renderer.write_immutable_all_frames(&mut args_buffer, &draw_list.commands);
        let slot_buffer = renderer.create_singleton_buffer(&draw_list.slots)?;

        let edit_state = EditState {
            fps: Label::new("FPS: --"),
            debug_mode: DebugMode::default(),
            eflight: Checkbox::new(LightRig::default().eflight),
            eflight_konst: RGBPicker::from_vec3(EFLIGHT_KONST),
            eflight_falloff: Slider::new(EFLIGHT_FALLOFF, 0.0, 1.0),
            eflight_elevation: Slider::new(EFLIGHT_ELEVATION, 0.0, -0.5),
            env_actor_c0: RGBPicker::from_vec3(ENV_ACTOR_C0),
            env_actor_k0: RGBPicker::from_vec3(ENV_ACTOR_K0),
        };

        let game = Self {
            start_time: Instant::now(),
            materials,
            params_buffer,
            args_buffer,
            slot_buffer,
            runs: draw_list.runs,
            edit_state,
            last_frame_time: Instant::now(),
            frame_times: VecDeque::with_capacity(FRAME_HISTORY_SIZE),
        };

        Ok(game)
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let spin = self.start_time.elapsed().as_secs_f32() * MODEL_SPIN;

        let model = Mat4::from_rotation_y(spin) * Mat4::from_scale(Vec3::splat(MODEL_SCALE));
        let target = Vec3::new(0.0, 0.62, 0.0);
        let eye = target + Vec3::new(0.0, 0.25, 2.8);
        let view = look_at_mat4(eye, target, Vec3::Y);
        let proj = directx::perspective(45f32.to_radians(), renderer.aspect_ratio(), 0.1, 20.0);

        for run in &self.runs {
            self.queue_run(&mut renderer, run.pipeline, run.first, run.count);
        }

        let light = LightRig {
            eflight: self.edit_state.eflight.checked,
            eflight_elevation: self.edit_state.eflight_elevation.value,
        };

        let eflight =
            self.edit_state.eflight_konst.to_vec3() * self.edit_state.eflight_falloff.value;
        let params = ToonLinkParams {
            mvp: MVPMatrices { model, view, proj },
            lights: GXLights {
                dir: light.directions(spin),
                color: light.colors(),
            },
            env: GXTevColorOverride {
                actor_c0: self.edit_state.env_actor_c0.to_vec3().extend(0.0),
                actor_k0: self.edit_state.env_actor_k0.to_vec3().extend(0.0),
                eflight_konst: eflight.extend(0.0),
                eflight: light.eflight as u32,
                _padding_0: Default::default(),
            },
            debug_mode: self.edit_state.debug_mode,
            _padding_0: Default::default(),
        };

        renderer.submit_draws(|gpu| {
            // The material buffer is never written after setup, so the param
            // block is the only per-frame upload.
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }
}
