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

mod animation_player;
mod animation_pose;
mod animation_validation;
mod generated;
mod modern;
mod skinning;
mod tev_pack;

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Context;
use facet::Facet;
use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};
use image::ImageReader;

use mltrs::editor::{Checkbox, Label, RGBPicker, RadioButton, Slider};
use mltrs::game::Game;
use mltrs::renderer::render_graph::DrawIndexedIndirectCommand;
use mltrs::shaders::atlas::ShaderAtlasRoot;
// The manifest's GX enums keep the `mm::` prefix. `mm::CullMode` and
// `mm::BlendMode` collide with the renderer's pipeline enums of the same name.
use gx::model_manifest::{self as mm, Batch, Manifest, MaterialEntry, TextureEntry};
use mltrs::renderer::{
    BindlessHandle, BlendMode, CullMode, DepthCompare, DrawError, DrawIndexedIndirect,
    FrameRenderer, ImmutableBufferHandle, MeshHandle, PipelineHandle, PushBlock, RasterState,
    Renderer, RgbaPixels, Sampler2D, SamplerOptions, SingletonBufferHandle, StencilMode,
    TextureColorSpace, TextureFilter, TextureHandle, TextureOptions, TextureWrap,
    UniformBufferHandle,
};

use crate::animation_player::{
    AnimationPlayer, CatalogRow, Command, LoopPolicy, LoopPreference, MAX_SPEED, MIN_SPEED,
    TransportState,
};
use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::tev::{GXAlphaOp, GXCompare};
use crate::generated::shader_atlas::toon_link::*;
use crate::modern::ToonLinkModern;
use crate::skinning::SkinningBuffers;
use gx::animation_manifest::ClipIdentity;

fn main() -> Result<(), anyhow::Error> {
    ToonLinkHost::run()
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
/// Passed to the GPU as a device address in the [`IndividualDraw`] table,
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

/// The converted BCK catalog. Missing or unreadable is not fatal: the player
/// stays in bind pose and reports it (see [`AnimationPlayer::new`]).
fn catalog_path() -> PathBuf {
    mltrs::manifest_path!["assets", "link", "animations", "converted", "catalog.json"]
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

#[derive(Debug, Clone, Copy)]
struct ModelVertex {
    position: Vec3,
    normal: Vec3,
    uv0: Vec2,
}

fn load_vertices(path: &Path, expected_count: u32) -> anyhow::Result<Vec<ModelVertex>> {
    let bytes = read_records(path, expected_count, VERTEX_STRIDE, "vertices")?;
    let read_f32 = |b: &[u8], i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    let vertices = bytes
        .as_chunks::<VERTEX_STRIDE>()
        .0
        .iter()
        .map(|v| ModelVertex {
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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    Ok(indices)
}

/// Check the manifest against the loaded buffers before building anything
/// from it.
fn validate_manifest(
    manifest: &Manifest,
    vertices: &[ModelVertex],
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
        stencil: StencilMode::DISABLED,
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
    pipelines: Vec<PipelineHandle<DrawIndexedIndirect, PushBlock<MultiDraw>>>,
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

/// The GPU-side draw list. `commands` and `individual_draws` hold one entry
/// per batch, in `draw_order`, and `runs` partitions them by pipeline.
struct DrawList {
    commands: Vec<DrawIndexedIndirectCommand>,
    individual_draws: Vec<IndividualDraw>,
    runs: Vec<Run>,
}

/// A span of consecutive `draw_order` entries that share one pipeline.
/// One Run corresponds with with one `cmd_draw_indexed_indirect` call.
struct Run {
    /// Index into [`MaterialTable::pipelines`].
    pipeline: usize,
    /// Index into [`ToonLink::args_buffer`] and the parallel
    /// [`ToonLink::individual_draw_buffer`], which hold the batches in
    /// `draw_order` rather than in INF1 order.
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
    let mut individual_draws = Vec::with_capacity(draw_order.len());
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
        let command = DrawIndexedIndirectCommand {
            index_count: batch.index_count,
            instance_count: 1,
            first_index: batch.first_index,
            vertex_offset: 0,
            first_instance: 0,
        };
        assert_eq!(
            command.vertex_offset, 0,
            "skin records are indexed by SV_VertexID; a nonzero vertex offset would mis-index them"
        );
        commands.push(command);
        let material = renderer.singleton_addr_at(materials_buffer, slot.raw() as u32);
        individual_draws.push(IndividualDraw { material });

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
        individual_draws,
        runs,
    })
}

pub struct ToonLink {
    start_time: Instant,
    host_spin: Option<f32>,
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
    individual_draw_buffer: SingletonBufferHandle<IndividualDraw>,
    /// The runs in `args_buffer` that share a pipeline, in draw order.
    /// Each run corresponds to one multi-draw-indirect command.
    runs: Vec<Run>,
    /// The joint palette and per-vertex influences behind `params.palette`
    /// and `params.skinning`. The palette is the bind pose (identity) until
    /// the animation player supplies a pose.
    skinning: SkinningBuffers,
    edit_state: EditState,
    last_frame_time: Instant,
    frame_times: VecDeque<Duration>,
}

/// The egui debug window, generated by reflection over these fields.
/// `debug_mode` is the shader's own generated enum, so its variants render as
/// radio buttons with no parallel list to keep in sync.
#[derive(Clone, Facet)]
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
    /// Why this mode renders the static model, or `None` when it animates.
    fn static_reason(&self) -> Option<&str> {
        self.skinning.static_reason()
    }

    /// Record `run`'s span of [`Self::args_buffer`] as one indirect draw.
    /// The push block is set once for the whole command, so the draw table
    /// pointer is what tells the sub-draws apart.
    fn queue_run(&self, renderer: &mut FrameRenderer, run: &Run) {
        let individual_draws = renderer.singleton_addr_at(&self.individual_draw_buffer, run.first);
        let push = MultiDraw { individual_draws };

        renderer.queue_draw_indexed_indirect_with_push_constants(
            &self.materials.pipelines[run.pipeline],
            &self.args_buffer,
            run.first,
            run.count,
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

        let classic_vertices: Vec<_> = vertices
            .iter()
            .map(|vertex| Vertex {
                position: vertex.position,
                normal: vertex.normal,
                uv0: vertex.uv0,
            })
            .collect();
        let mesh = renderer.create_mesh(&classic_vertices, &indices)?;

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
        let args_buffer = renderer.create_indirect_buffer(&draw_list.commands)?;
        let individual_draw_buffer =
            renderer.create_singleton_buffer(&draw_list.individual_draws)?;
        let skinning = SkinningBuffers::new(renderer, &dir, &manifest)?;

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
            host_spin: None,
            materials,
            params_buffer,
            args_buffer,
            individual_draw_buffer,
            runs: draw_list.runs,
            skinning,
            edit_state,
            last_frame_time: Instant::now(),
            frame_times: VecDeque::with_capacity(FRAME_HISTORY_SIZE),
        };

        Ok(game)
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        self.draw_posed(renderer, None)
    }
}

impl ToonLink {
    /// Draw with `palette` as this frame's joint palette, one
    /// `animated_world * inverse_bind_world` per skeleton joint. `None` is the
    /// bind pose (identity), used when there is no host or no player.
    fn draw_posed(
        &mut self,
        mut renderer: FrameRenderer,
        palette: Option<&[Mat4]>,
    ) -> Result<(), DrawError> {
        let spin = self
            .host_spin
            .unwrap_or_else(|| self.start_time.elapsed().as_secs_f32() * MODEL_SPIN);

        let model = Mat4::from_rotation_y(spin) * Mat4::from_scale(Vec3::splat(MODEL_SCALE));
        let target = Vec3::new(0.0, 0.62, 0.0);
        let eye = target + Vec3::new(0.0, 0.25, 2.8);
        let view = look_at_mat4(eye, target, Vec3::Y);
        let proj = directx::perspective(45f32.to_radians(), renderer.aspect_ratio(), 0.1, 20.0);

        for run in &self.runs {
            self.queue_run(&mut renderer, run);
        }
        // Queue-time addresses for this frame's flight slot; never retained.
        let skinning = self.skinning.addrs(&renderer);

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
            palette: skinning.palette,
            skinning: skinning.skinning,
            _padding_1: Default::default(),
        };

        let bind_pose;
        let palette = match palette {
            Some(palette) => palette,
            None => {
                bind_pose = self.skinning.identity_palette();
                &bind_pose
            }
        };

        renderer.submit_draws(|gpu| {
            // The material buffer is never written after setup, so the param
            // block and the palette are the per-frame uploads.
            gpu.write_uniform(&mut self.params_buffer, params);
            self.skinning.write_palette(gpu, palette);
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedMode {
    GameCube,
    Modern,
}

fn selected_mode(index: usize) -> SelectedMode {
    match index {
        0 => SelectedMode::GameCube,
        1 => SelectedMode::Modern,
        selected => panic!("invalid Toon Link mode index {selected}"),
    }
}

/// The host's renderer-independent share of the animation seam. It owns the
/// one [`AnimationPlayer`] both modes draw from, applies the UI's buffered
/// commands once before draw, advances the shared clock during update, and publishes the
/// palette for the selected mode's draw.
struct AnimationHost {
    player: Option<AnimationPlayer>,
    /// Why there is no player: the skeleton could not produce a bind pose.
    /// Both modes then keep rendering the static model (AC5).
    unavailable: Option<String>,
}

impl AnimationHost {
    fn new(skeleton: &mm::Skeleton, catalog: &Path) -> Self {
        match AnimationPlayer::new(skeleton, catalog) {
            Ok(player) => Self {
                player: Some(player),
                unavailable: None,
            },
            Err(error) => {
                let message =
                    format!("animation unavailable, rendering the static bind pose: {error:#}");
                eprintln!("toon_link: {message}");
                Self {
                    player: None,
                    unavailable: Some(message),
                }
            }
        }
    }

    /// Drop the player: a mode found its skin unusable, so its palette would
    /// never reach the GPU and playback would silently desync from the model.
    /// The UI then shows `reason` and every control is inert.
    fn disable(&mut self, reason: String) {
        self.player = None;
        self.unavailable = Some(reason);
    }

    /// Advance only the clock before UI. Commands issued by UI belong to the
    /// following draw, not the next update.
    fn update(&mut self, controls: &mut AnimationControls, elapsed_seconds: f32) {
        if let Some(player) = &mut self.player {
            player.advance(elapsed_seconds);
        }
        controls.sync(&self.view());
    }

    /// Production pre-draw seam: consume UI commands once, in order, then
    /// evaluate and publish. Refresh the view after evaluation, including failures.
    /// `None` means the modes must render their static bind pose.
    fn prepare_frame(&mut self, controls: &mut AnimationControls) -> Option<&[Mat4]> {
        let commands = std::mem::take(&mut controls.commands);
        if let Some(player) = &mut self.player {
            for command in commands {
                player.apply(command);
            }
            player.prepare_frame();
        }
        controls.sync(&self.view());

        self.player.as_ref().map(AnimationPlayer::published_palette)
    }

    fn view(&self) -> AnimationView {
        match &self.player {
            Some(player) => AnimationView::from_player(player),
            None => AnimationView {
                unavailable: self.unavailable.clone(),
                ..AnimationView::default()
            },
        }
    }
}

/// What the UI shows about the player, refreshed after clock advancement and
/// after preparation because the static `render_editor_ui` cannot reach the host.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationView {
    /// `Some` when [`AnimationHost`] has no player; every control is inert.
    unavailable: Option<String>,
    catalog_error: Option<String>,
    diagnostic: Option<String>,
    selected_identity: Option<ClipIdentity>,
    selected_label: Option<String>,
    /// `None` without a player.
    transport: Option<TransportState>,
    frame: f32,
    duration_frames: Option<u16>,
    speed: f32,
    loop_preference: LoopPreference,
    effective_policy: Option<LoopPolicy>,
    has_active_clip: bool,
    /// A valid zero-duration clip: frame 0, stopped, transport disabled.
    is_static: bool,
    can_play: bool,
    can_pause: bool,
    can_restart: bool,
    can_scrub: bool,
}

impl AnimationView {
    fn from_player(player: &AnimationPlayer) -> Self {
        Self {
            unavailable: None,
            catalog_error: player.catalog_error().map(str::to_owned),
            diagnostic: player.diagnostic().map(str::to_owned),
            selected_identity: player.selected_identity().cloned(),
            selected_label: player.selected_label().map(str::to_owned),
            transport: Some(player.transport()),
            frame: player.frame(),
            duration_frames: player.duration_frames(),
            speed: player.speed(),
            loop_preference: player.loop_preference(),
            effective_policy: player.effective_policy(),
            has_active_clip: player.has_active_clip(),
            is_static: player.is_static(),
            can_play: player.can_play(),
            can_pause: player.can_pause(),
            can_restart: player.can_restart(),
            can_scrub: player.can_scrub(),
        }
    }

    fn available(&self) -> bool {
        self.transport.is_some()
    }
}

/// The debug "Animation" section. The UI never touches the player: it reads
/// [`AnimationView`] and buffers intent as [`Command`]s that the host drains
/// before this frame's draw ([`AnimationHost::prepare_frame`]). Search and
/// scroll state live here, so they survive mode switches.
#[derive(Clone, Default)]
pub struct AnimationControls {
    /// Case-insensitive search over `archive/member`. Filtering only changes
    /// which rows are listed; it never issues a command.
    query: String,
    /// Every BCK row, in catalog order. Selection uses the row's identity,
    /// never its position in the filtered list.
    rows: Rc<[CatalogRow]>,
    view: AnimationView,
    /// Commands issued by the UI since the previous preparation, in order.
    commands: Vec<Command>,
    /// Slider positions. Synced after update and preparation; a change pushes a
    /// command rather than moving the player directly.
    scrub: f32,
    speed: f32,
}

const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 96, 96);
const DIAGNOSTIC_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 200, 80);
const CLIP_LIST_HEIGHT: f32 = 160.0;

impl AnimationControls {
    fn new(host: &AnimationHost) -> Self {
        let rows: Rc<[CatalogRow]> = host
            .player
            .as_ref()
            .map_or(&[][..], AnimationPlayer::catalog_rows)
            .into();
        let mut controls = Self {
            rows,
            ..Self::default()
        };
        controls.sync(&host.view());

        controls
    }

    fn sync(&mut self, view: &AnimationView) {
        self.scrub = view.frame;
        self.speed = view.speed;
        self.view = view.clone();
    }

    fn render_ui(&mut self, ui: &mut egui::Ui) {
        let view = self.view.clone();
        if let Some(error) = view
            .unavailable
            .as_deref()
            .or(view.catalog_error.as_deref())
        {
            ui.colored_label(ERROR_COLOR, error);
        }
        if let Some(diagnostic) = &view.diagnostic {
            ui.colored_label(DIAGNOSTIC_COLOR, diagnostic);
        }

        ui.add_enabled_ui(view.available(), |ui| {
            self.render_selector(ui, &view);
            self.render_transport(ui, &view);
            self.render_readouts(ui, &view);
        });
    }

    fn render_selector(&mut self, ui: &mut egui::Ui, view: &AnimationView) {
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("archive/member"));
        });

        let rows: Vec<&CatalogRow> = self
            .rows
            .iter()
            .filter(|row| row.matches(&self.query))
            .collect();
        ui.label(format!("{} of {} BCK clips", rows.len(), self.rows.len()));

        let row_height = ui.spacing().interact_size.y;
        let mut select = None;
        egui::ScrollArea::vertical()
            .id_salt("bck_clips")
            .max_height(CLIP_LIST_HEIGHT)
            .show_rows(ui, row_height, rows.len(), |ui, range| {
                for row in &rows[range] {
                    let selected = view.selected_identity.as_ref() == Some(&row.identity);
                    let clicked = ui.selectable_label(selected, &row.label).clicked();
                    // Selecting the active clip is a no-op; do not issue it.
                    let newly_selected = clicked && !selected;
                    if newly_selected {
                        select = Some(row.identity.clone());
                    }
                }
            });
        if let Some(identity) = select {
            self.commands.push(Command::Select(identity));
        }
    }

    fn render_transport(&mut self, ui: &mut egui::Ui, view: &AnimationView) {
        ui.horizontal(|ui| {
            let buttons = [
                ("Bind pose", view.has_active_clip, Command::BindPose),
                ("Play", view.can_play, Command::Play),
                ("Pause", view.can_pause, Command::Pause),
                ("Restart", view.can_restart, Command::Restart),
            ];
            for (label, enabled, command) in buttons {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    self.commands.push(command);
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("Scrub");
            let duration = f32::from(view.duration_frames.unwrap_or(0));
            let slider = egui::Slider::new(&mut self.scrub, 0.0..=duration);
            if ui.add_enabled(view.can_scrub, slider).changed() {
                self.commands.push(Command::Scrub(self.scrub));
            }
        });

        ui.horizontal(|ui| {
            ui.label("Speed");
            let slider = egui::Slider::new(&mut self.speed, MIN_SPEED..=MAX_SPEED);
            if ui.add(slider).changed() {
                self.commands.push(Command::SetSpeed(self.speed));
            }
        });

        ui.horizontal(|ui| {
            ui.label("Loop");
            let mut preference = view.loop_preference;
            for (label, value) in [
                ("Source", LoopPreference::Source),
                ("Repeat", LoopPreference::Repeat),
                ("Once", LoopPreference::Once),
            ] {
                ui.radio_value(&mut preference, value, label);
            }
            let changed = preference != view.loop_preference;
            if changed {
                self.commands.push(Command::SetLoop(preference));
            }
        });
    }

    fn render_readouts(&self, ui: &mut egui::Ui, view: &AnimationView) {
        let clip = view.selected_label.as_deref().unwrap_or("bind pose");
        ui.label(format!("Clip: {clip}"));
        let transport = match view.transport {
            Some(TransportState::Stopped) if view.is_static => "Stopped (static clip)",
            Some(TransportState::Stopped) => "Stopped",
            Some(TransportState::Playing) => "Playing",
            Some(TransportState::Paused) => "Paused",
            None => "unavailable",
        };
        ui.label(format!("Transport: {transport}"));
        let duration = view.duration_frames.unwrap_or(0);
        ui.label(format!("Frame: {:.2} / {duration}", view.frame));
        let policy = match view.effective_policy {
            Some(LoopPolicy::Repeat) => "Repeat",
            Some(LoopPolicy::Once) => "Once",
            None => "none",
        };
        ui.label(format!("Effective policy: {policy}"));
    }
}

/// Switches between displaying the two versions of the Game implementation.
/// Delegates to one or the other based on HostEditState.
pub struct ToonLinkHost {
    start_time: Instant,
    /// `start_time.elapsed()` at the previous update; the animation clock is
    /// the difference, so the player needs no wall clock of its own.
    last_update: Duration,
    animation: AnimationHost,
    classic: ToonLink,
    modern: ToonLinkModern,
    edit_state: HostEditState,
}

#[derive(Clone, Facet)]
pub struct HostEditState {
    mode: RadioButton,
    game_cube: EditState,
    modern: modern::ModernEditState,
    /// Shared by both modes; not reflected, it renders itself.
    #[facet(opaque)]
    animation: AnimationControls,
}

impl HostEditState {
    fn render_ui(&mut self, ui: &mut egui::Ui) {
        let changed = ui
            .horizontal(|ui| {
                ui.label("Mode");
                self.mode.render_ui(ui)
            })
            .inner;

        let game_cube = self.selected() == SelectedMode::GameCube;

        egui::CollapsingHeader::new("Animation")
            .id_salt("animation_controls")
            .default_open(true)
            .show(ui, |ui| self.animation.render_ui(ui));

        egui::CollapsingHeader::new("GameCube")
            .id_salt("game_cube_settings")
            .default_open(game_cube)
            .open(changed.then_some(game_cube))
            .show(ui, |ui| {
                mltrs::renderer::facet_egui::render_facet_ui(ui, &mut self.game_cube);
            });

        egui::CollapsingHeader::new("Modern")
            .id_salt("modern_settings")
            .default_open(!game_cube)
            .open(changed.then_some(!game_cube))
            .show(ui, |ui| {
                mltrs::renderer::facet_egui::render_facet_ui(ui, &mut self.modern);
            });
    }

    fn selected(&self) -> SelectedMode {
        selected_mode(self.mode.selected)
    }

    fn pull_selected(&mut self, classic: &EditState, modern: &modern::ModernEditState) {
        match self.selected() {
            SelectedMode::GameCube => self.game_cube = classic.clone(),
            SelectedMode::Modern => self.modern = modern.clone(),
        }
    }

    fn selected_settings(&self) -> SelectedSettings<'_> {
        match self.selected() {
            SelectedMode::GameCube => SelectedSettings::GameCube(&self.game_cube),
            SelectedMode::Modern => SelectedSettings::Modern(&self.modern),
        }
    }
}

enum SelectedSettings<'a> {
    GameCube(&'a EditState),
    Modern(&'a modern::ModernEditState),
}

impl Game for ToonLinkHost {
    type EditState = HostEditState;
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Toon Link"
    }

    fn needs_stencil() -> bool {
        true
    }

    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("Toon Link", &mut self.edit_state))
    }

    fn render_editor_ui(ui: &mut egui::Ui, debug_state: &mut Self::EditState) {
        debug_state.render_ui(ui);
    }

    fn frame_delay(&self) -> Duration {
        Duration::from_millis(5)
    }

    fn update(&mut self) {
        // Shared playback runs every frame, whichever mode is selected.
        let elapsed = self.start_time.elapsed();
        let delta = elapsed.saturating_sub(self.last_update);
        self.last_update = elapsed;
        self.animation
            .update(&mut self.edit_state.animation, delta.as_secs_f32());

        match self.edit_state.selected() {
            SelectedMode::GameCube => self.classic.update(),
            SelectedMode::Modern => self.modern.update(),
        }
        self.edit_state
            .pull_selected(&self.classic.edit_state, self.modern.edit_state());
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self> {
        let classic = ToonLink::setup(renderer, ShaderAtlas::init())?;
        let modern = ToonLinkModern::setup(renderer, shaders)?;
        let manifest = load_manifest(&converted_dir())?;
        let mut animation = AnimationHost::new(&manifest.skeleton, &catalog_path());
        // A mode that rejected its skin ignores every palette; playing a clip
        // into it would show a moving transport over a motionless model.
        let static_modes = [
            ("GameCube", classic.static_reason()),
            ("Modern", modern.static_reason()),
        ];
        for (mode, reason) in static_modes {
            let Some(reason) = reason else {
                continue;
            };

            let message =
                format!("animation disabled, rendering the static model: {mode} mode: {reason}");
            eprintln!("toon_link: {message}");
            animation.disable(message);
        }
        let edit_state = HostEditState {
            mode: RadioButton::new(&["GameCube", "Modern"]),
            game_cube: classic.edit_state.clone(),
            modern: modern.edit_state().clone(),
            animation: AnimationControls::new(&animation),
        };

        Ok(Self {
            start_time: Instant::now(),
            last_update: Duration::ZERO,
            animation,
            classic,
            modern,
            edit_state,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let spin = self.start_time.elapsed().as_secs_f32() * MODEL_SPIN;
        // One pose per frame, shared by both modes.
        let palette = self.animation.prepare_frame(&mut self.edit_state.animation);
        match self.edit_state.selected_settings() {
            SelectedSettings::GameCube(settings) => {
                self.classic.host_spin = Some(spin);
                self.classic.edit_state = settings.clone();
                self.classic.draw_posed(renderer, palette)
            }
            SelectedSettings::Modern(settings) => {
                self.modern.set_spin(spin);
                *self.modern.edit_state_mut() = settings.clone();
                self.modern.draw_posed(renderer, palette)
            }
        }
    }
}

#[cfg(test)]
mod host_tests {
    use super::*;

    fn assert_game<T: Game>() {}

    #[test]
    fn both_modes_satisfy_game_bound() {
        assert_game::<ToonLink>();
        assert_game::<ToonLinkModern>();
    }

    fn classic_settings() -> EditState {
        EditState {
            fps: Label::new("FPS: --"),
            debug_mode: DebugMode::default(),
            eflight: Checkbox::new(false),
            eflight_konst: RGBPicker::from_vec3(EFLIGHT_KONST),
            eflight_falloff: Slider::new(EFLIGHT_FALLOFF, 0.0, 1.0),
            eflight_elevation: Slider::new(EFLIGHT_ELEVATION, 0.0, -0.5),
            env_actor_c0: RGBPicker::from_vec3(ENV_ACTOR_C0),
            env_actor_k0: RGBPicker::from_vec3(ENV_ACTOR_K0),
        }
    }

    fn mode_settings() -> HostEditState {
        HostEditState {
            mode: RadioButton::new(&["GameCube", "Modern"]),
            game_cube: classic_settings(),
            modern: modern::ModernEditState::default(),
            animation: AnimationControls::default(),
        }
    }

    /// One painted text run from the last harness frame.
    #[derive(Clone, Debug)]
    struct PaintedText {
        text: String,
        bounds: egui::Rect,
        /// Below 1 inside a disabled `Ui`: egui fades a disabled widget by
        /// gamma-multiplying its shape colors, alpha included.
        opacity: f32,
        /// The first section's color; `colored_label` sets it.
        color: egui::Color32,
    }

    struct EditorHarness {
        ctx: egui::Context,
        labels: Vec<PaintedText>,
        header_ids: [egui::Id; 2],
    }

    impl EditorHarness {
        fn new(settings: &mut HostEditState) -> Self {
            let ctx = egui::Context::default();
            ctx.style_mut(|style| style.animation_time = 0.0);
            let mut harness = Self {
                ctx,
                labels: Vec::new(),
                header_ids: [egui::Id::NULL; 2],
            };
            harness.frame(settings, Vec::new());
            harness.frame(settings, Vec::new());

            harness
        }

        fn frame(&mut self, settings: &mut HostEditState, events: Vec<egui::Event>) {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 1600.0),
                )),
                events,
                ..Default::default()
            };
            let output = self.ctx.run(input, |ctx| {
                egui::Window::new("Toon Link").show(ctx, |ui| {
                    // CollapsingHeader::show creates a vertical child scope before
                    // combining its id_salt with the UI's persistent ID.
                    let header_scope = ui.id().with(egui::Id::new("child"));
                    self.header_ids = [
                        header_scope.with(egui::Id::new("game_cube_settings")),
                        header_scope.with(egui::Id::new("modern_settings")),
                    ];
                    <ToonLinkHost as Game>::render_editor_ui(ui, settings);
                });
            });
            self.labels.clear();
            for shape in &output.shapes {
                collect_labels(&shape.shape, &mut self.labels);
            }
        }

        /// The one painted text `label` below the Mode row, or on it for a
        /// mode radio button.
        fn painted(&self, label: &str, radio: bool) -> &PaintedText {
            let rows: Vec<_> = self
                .labels
                .iter()
                .filter(|painted| painted.text == "Mode")
                .collect();
            assert_eq!(rows.len(), 1, "missing or ambiguous Mode row");
            let row = rows[0].bounds;
            let targets: Vec<_> = self
                .labels
                .iter()
                .filter(|painted| {
                    let bounds = painted.bounds;
                    painted.text == label
                        && if radio {
                            bounds.center().y >= row.top() && bounds.center().y <= row.bottom()
                        } else {
                            bounds.top() > row.bottom()
                        }
                })
                .collect();
            assert_eq!(
                targets.len(),
                1,
                "missing or ambiguous {label} target (radio={radio})"
            );

            targets[0]
        }

        fn target(&self, label: &str, radio: bool) -> egui::Rect {
            self.painted(label, radio).bounds
        }

        fn has_text(&self, text: &str) -> bool {
            self.labels.iter().any(|painted| painted.text == text)
        }

        /// Whether the unique control labelled `label` is painted disabled.
        fn disabled(&self, label: &str) -> bool {
            self.painted(label, false).opacity < 1.0
        }

        /// Every text painted on the same row as the unique `label`, other
        /// than the label itself (a slider's value, for example).
        fn row_texts(&self, label: &str) -> Vec<&PaintedText> {
            let row = self.target(label, false);

            self.labels
                .iter()
                .filter(|painted| {
                    painted.text != label
                        && painted.bounds.center().y >= row.top()
                        && painted.bounds.center().y <= row.bottom()
                })
                .collect()
        }

        /// Focus the search field through its hint text, then type `text`.
        fn type_search(&mut self, settings: &mut HostEditState, text: &str) {
            self.click(settings, "archive/member", false);
            self.frame(settings, vec![egui::Event::Text(text.to_owned())]);
            self.frame(settings, Vec::new());
        }

        fn click(&mut self, settings: &mut HostEditState, label: &str, radio: bool) {
            let pos = self.target(label, radio).center();
            for pressed in [true, false] {
                self.frame(
                    settings,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            self.frame(settings, Vec::new());
        }

        fn assert_sections(&self, game_cube: bool, modern: bool) {
            for (id, expected) in self.header_ids.into_iter().zip([game_cube, modern]) {
                let state = egui::collapsing_header::CollapsingState::load(&self.ctx, id)
                    .expect("header state was not persisted");
                assert_eq!(state.is_open(), expected);
            }
            assert_eq!(self.has_text("FPS: --"), game_cube);
            assert_eq!(self.has_text("band_center"), modern);
            self.target("GameCube", false);
            self.target("Modern", false);
        }
    }

    fn collect_labels(shape: &egui::epaint::Shape, labels: &mut Vec<PaintedText>) {
        match shape {
            egui::epaint::Shape::Text(text) => labels.push(PaintedText {
                text: text.galley.text().to_owned(),
                bounds: text.galley.rect.translate(text.pos.to_vec2()),
                opacity: f32::from(text.fallback_color.a()) / 255.0,
                color: text
                    .galley
                    .job
                    .sections
                    .first()
                    .map_or(text.fallback_color, |section| section.format.color),
            }),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_labels(shape, labels);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn mode_radio_switches_section_visibility() {
        let mut settings = mode_settings();
        let mut ui = EditorHarness::new(&mut settings);
        ui.assert_sections(true, false);
        assert_eq!(settings.selected(), SelectedMode::GameCube);
        assert_ne!(ui.target("GameCube", true), ui.target("GameCube", false));
        assert_ne!(ui.target("Modern", true), ui.target("Modern", false));
        ui.click(&mut settings, "Modern", true);
        assert_eq!(settings.selected(), SelectedMode::Modern);
        ui.assert_sections(false, true);
        ui.click(&mut settings, "GameCube", true);
        assert_eq!(settings.selected(), SelectedMode::GameCube);
        ui.assert_sections(true, false);
    }

    #[test]
    fn manual_section_state_persists_until_mode_change() {
        let mut settings = mode_settings();
        let mut ui = EditorHarness::new(&mut settings);
        for (label, game_cube, modern) in [
            ("Modern", true, true),
            ("GameCube", false, true),
            ("Modern", false, false),
            ("GameCube", true, false),
            ("Modern", true, true),
        ] {
            ui.click(&mut settings, label, false);
            ui.frame(&mut settings, Vec::new());
            ui.assert_sections(game_cube, modern);
            assert_eq!(settings.selected(), SelectedMode::GameCube);
        }
        ui.click(&mut settings, "GameCube", true);
        ui.assert_sections(true, true);
        ui.click(&mut settings, "Modern", true);
        ui.assert_sections(false, true);
        assert_eq!(settings.selected(), SelectedMode::Modern);
    }

    #[test]
    fn section_switching_preserves_mode_settings() {
        let mut settings = mode_settings();
        settings.game_cube.eflight.checked = true;
        settings.game_cube.eflight_falloff.value = 0.37;
        settings.modern.band_center.value = 0.73;
        settings.modern.secondary.checked = true;
        let mut ui = EditorHarness::new(&mut settings);
        for (label, radio, selected) in [
            ("Modern", false, SelectedMode::GameCube),
            ("GameCube", false, SelectedMode::GameCube),
            ("Modern", true, SelectedMode::Modern),
            ("Modern", false, SelectedMode::Modern),
            ("GameCube", true, SelectedMode::GameCube),
        ] {
            ui.click(&mut settings, label, radio);
            assert_eq!(settings.selected(), selected);
            assert!(settings.game_cube.eflight.checked);
            assert_eq!(settings.game_cube.eflight_falloff.value, 0.37);
            assert_eq!(settings.modern.band_center.value, 0.73);
            assert!(settings.modern.secondary.checked);
        }
        ui.assert_sections(true, false);
    }

    #[test]
    fn default_is_gamecube() {
        assert_eq!(mode_settings().selected(), SelectedMode::GameCube);
    }

    #[test]
    fn host_routes_only_selected_mode() {
        let mut settings = mode_settings();
        assert!(matches!(
            settings.selected_settings(),
            SelectedSettings::GameCube(_)
        ));
        settings.mode.selected = 1;
        assert!(matches!(
            settings.selected_settings(),
            SelectedSettings::Modern(_)
        ));
    }

    #[test]
    fn mode_settings_survive_round_trip_and_inactive_interval() {
        let mut settings = mode_settings();
        settings.game_cube.eflight.checked = true;
        settings.modern.band_center.value = 0.73;

        let mut live_classic = classic_settings();
        let mut live_modern = modern::ModernEditState::default();
        live_classic.eflight.checked = false;
        live_modern.band_center.value = 0.21;

        settings.pull_selected(&live_classic, &live_modern);
        assert!(!settings.game_cube.eflight.checked);
        assert_eq!(settings.modern.band_center.value, 0.73);

        settings.mode.selected = 1;
        settings.pull_selected(&live_classic, &live_modern);
        assert!(!settings.game_cube.eflight.checked);
        assert_eq!(settings.modern.band_center.value, 0.21);

        live_classic.eflight.checked = true;
        live_modern.band_center.value = 0.88;
        settings.mode.selected = 0;
        settings.pull_selected(&live_classic, &live_modern);
        assert!(settings.game_cube.eflight.checked);
        assert_eq!(settings.modern.band_center.value, 0.21);
    }

    #[test]
    fn post_ui_state_routes_to_selected_params() {
        let mut settings = mode_settings();
        settings.mode.selected = 1;
        settings.modern.secondary.checked = true;
        settings.modern.secondary_intensity.value = 0.6;

        let SelectedSettings::Modern(modern) = settings.selected_settings() else {
            panic!("Modern was not selected");
        };
        let (_, _, intensity) = modern.secondary_parameters(0.0);
        assert_eq!(intensity, 0.6);

        settings.mode.selected = 0;
        settings.game_cube.eflight.checked = true;
        let SelectedSettings::GameCube(classic) = settings.selected_settings() else {
            panic!("GameCube was not selected");
        };
        assert!(classic.eflight.checked);
    }

    // --- BCK playback through the production host seam ----------------------

    use crate::animation_player::test_support::{self, Fixture};

    const WALK: &str = "LkAnm/bcks/walk.bck";
    const RUN: &str = "LkAnm/bcks/run.bck";
    const RISE: &str = "LkAnm/bcks/rise.bck";
    const FRAME_TOLERANCE: f32 = 1e-4;

    /// The renderer-independent host seam: [`AnimationHost`] (update and the
    /// draw's `prepare_frame`) and [`HostEditState`] (UI) exchange state only
    /// through [`AnimationControls`], driven by the real editor harness with
    /// fake elapsed time. No renderer, no wall clock, no game assets.
    struct PlaybackBench {
        _fixture: Fixture,
        host: AnimationHost,
        settings: HostEditState,
        ui: EditorHarness,
    }

    impl PlaybackBench {
        fn new(fixture: Fixture) -> Self {
            let host = AnimationHost::new(&test_support::skeleton(), &fixture.catalog);

            Self::with_host(fixture, host)
        }

        fn with_host(fixture: Fixture, host: AnimationHost) -> Self {
            let mut settings = mode_settings();
            settings.animation = AnimationControls::new(&host);
            let ui = EditorHarness::new(&mut settings);
            let mut bench = Self {
                _fixture: fixture,
                host,
                settings,
                ui,
            };
            bench.tick(0.0);

            bench
        }

        fn player(&self) -> &AnimationPlayer {
            self.host.player.as_ref().expect("the bench has a player")
        }

        /// One app frame: update, UI, then production preparation. Returns the
        /// number of consumed commands. A final UI-only refresh exposes the view
        /// published by draw for existing paint assertions.
        fn tick(&mut self, elapsed: f32) -> usize {
            self.host.update(&mut self.settings.animation, elapsed);
            self.ui.frame(&mut self.settings, Vec::new());
            let applied = self.commands().len();
            self.palette();
            self.ui.frame(&mut self.settings, Vec::new());

            applied
        }

        /// The draw half: the palette either mode would upload this frame.
        fn palette(&mut self) -> Vec<Mat4> {
            self.host
                .prepare_frame(&mut self.settings.animation)
                .map(<[Mat4]>::to_vec)
                .unwrap_or_default()
        }

        fn click(&mut self, label: &str) {
            self.ui.click(&mut self.settings, label, false);
        }

        fn select_mode(&mut self, label: &str) {
            self.ui.click(&mut self.settings, label, true);
        }

        fn commands(&self) -> &[Command] {
            &self.settings.animation.commands
        }

        /// Everything a mode switch must leave alone.
        fn playback(&mut self) -> (Option<ClipIdentity>, f32, TransportState, Vec<Mat4>) {
            let palette = self.palette();
            let player = self.player();

            (
                player.selected_identity().cloned(),
                player.frame(),
                player.transport(),
                palette,
            )
        }

        fn assert_frame(&self, expected: f32) {
            let actual = self.player().frame();
            assert!(
                (actual - expected).abs() <= FRAME_TOLERANCE,
                "frame {actual} != {expected}"
            );
        }

        /// The ramp clip's root x translation, which equals the evaluated frame.
        fn assert_root_x(&mut self, expected: f32) {
            let actual = self.palette()[0].w_axis.x;
            assert!(
                (actual - expected).abs() <= FRAME_TOLERANCE,
                "root x {actual} != {expected}"
            );
        }

        /// The unique painted text containing `needle`.
        fn text_containing(&self, needle: &str) -> &PaintedText {
            let found: Vec<_> = self
                .ui
                .labels
                .iter()
                .filter(|painted| painted.text.contains(needle))
                .collect();
            assert_eq!(
                found.len(),
                1,
                "missing or ambiguous text containing {needle:?}"
            );

            found[0]
        }

        fn assert_transport_controls(&self, expected: [(&str, bool); 4]) {
            for (label, enabled) in expected {
                assert_eq!(
                    !self.ui.disabled(label),
                    enabled,
                    "{label} enabled={enabled}"
                );
            }
        }

        fn assert_slider_enabled(&self, label: &str, enabled: bool) {
            let texts = self.ui.row_texts(label);
            assert!(!texts.is_empty(), "{label} slider paints no value");
            for painted in texts {
                assert_eq!(painted.opacity >= 1.0, enabled, "{label} enabled={enabled}");
            }
        }
    }

    #[test]
    fn bck_mode_switch_retains_pose() {
        let fixture = test_support::fixture(
            "host-mode-switch",
            &[("bcks/walk.bck", Some(test_support::ramp_clip(20, 2)))],
        );
        let walk = fixture.identity(0);
        let mut bench = PlaybackBench::new(fixture);
        bench.settings.game_cube.eflight.checked = true;
        bench.settings.modern.band_center.value = 0.73;
        let bind = bench.palette();
        assert_eq!(bind, vec![Mat4::IDENTITY; 2]);

        bench.click(WALK);
        assert_eq!(bench.commands(), [Command::Select(walk.clone())]);
        assert_eq!(bench.tick(0.0), 1);
        assert_eq!(bench.tick(0.1), 0);
        let playing = bench.playback();
        assert_eq!(playing.0.as_ref(), Some(&walk));
        assert_eq!(playing.2, TransportState::Playing);
        bench.assert_frame(3.0);
        assert_ne!(playing.3, bind);

        // Switching with no commands changes nothing about playback.
        for mode in ["Modern", "GameCube", "Modern"] {
            bench.select_mode(mode);
            assert!(bench.commands().is_empty());
            assert_eq!(bench.tick(0.0), 0);
            assert_eq!(bench.playback(), playing);
        }
        // Playback keeps running while Modern is selected.
        assert_eq!(bench.settings.selected(), SelectedMode::Modern);
        assert_eq!(bench.tick(0.1), 0);
        bench.assert_frame(6.0);
        assert_ne!(bench.palette(), playing.3);

        // Paused: elapsed time across switches is discarded.
        bench.click("Pause");
        assert_eq!(bench.commands(), [Command::Pause]);
        assert_eq!(bench.tick(0.0), 1);
        let paused = bench.playback();
        assert_eq!(paused.2, TransportState::Paused);
        bench.assert_frame(6.0);
        for mode in ["GameCube", "Modern", "GameCube"] {
            bench.select_mode(mode);
            assert_eq!(bench.tick(0.5), 0);
            assert_eq!(bench.playback(), paused);
        }
        assert_eq!(bench.settings.selected(), SelectedMode::GameCube);

        // The independent mode settings are untouched too.
        assert!(bench.settings.game_cube.eflight.checked);
        assert_eq!(bench.settings.modern.band_center.value, 0.73);
        assert_eq!(bench.player().preparation_count(), 1);
    }

    #[test]
    fn bck_ui_first_draw_uses_commands_without_another_update() {
        let fixture = test_support::fixture(
            "host-immediate-draw",
            &[("bcks/walk.bck", Some(test_support::ramp_clip(20, 2)))],
        );
        let mut bench = PlaybackBench::new(fixture);
        bench.host.update(&mut bench.settings.animation, 0.1);
        bench.click(WALK);
        bench.assert_root_x(0.0);
        assert!(bench.commands().is_empty());
        assert_eq!(bench.tick(0.1), 0);
        bench.assert_root_x(3.0);

        // Click the scrub track, immediately to the right of its label.
        bench.host.update(&mut bench.settings.animation, 0.1);
        let label = bench.ui.target("Scrub", false);
        let pos = egui::pos2(label.right() + 45.0, label.center().y);
        for pressed in [true, false] {
            bench.ui.frame(
                &mut bench.settings,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let target = match bench.commands().last() {
            Some(Command::Scrub(frame)) => *frame,
            other => panic!("slider must issue a scrub: {other:?}"),
        };
        bench.assert_root_x(target);
        assert_eq!(bench.settings.animation.view.frame, target);
        assert_eq!(bench.tick(0.1), 0);
        bench.assert_root_x(target);

        bench.host.update(&mut bench.settings.animation, 0.1);
        bench.click("Restart");
        bench.assert_root_x(0.0);
        assert_eq!(bench.tick(0.1), 0);
        bench.assert_root_x(3.0);

        bench.host.update(&mut bench.settings.animation, 0.1);
        bench.click("Bind pose");
        assert_eq!(bench.palette(), vec![Mat4::IDENTITY; 2]);
        assert!(!bench.player().has_active_clip());
        assert_eq!(bench.tick(0.1), 0);
        assert_eq!(bench.palette(), vec![Mat4::IDENTITY; 2]);
    }

    #[test]
    fn bck_ui_commands_apply_once() {
        let fixture = test_support::fixture(
            "host-apply-once",
            &[("bcks/walk.bck", Some(test_support::ramp_clip(20, 2)))],
        );
        let mut bench = PlaybackBench::new(fixture);
        bench.click(WALK);
        assert_eq!(bench.tick(0.0), 1);
        assert_eq!(
            bench.player().evaluation_count(),
            1,
            "selection evaluates frame 0"
        );
        bench.assert_root_x(0.0);
        assert_eq!(bench.player().evaluation_count(), 1, "the draw reuses it");

        bench.click("Pause");
        assert_eq!(bench.tick(0.0), 1);
        assert_eq!(bench.player().transport(), TransportState::Paused);

        // One click buffers one command, however many UI frames follow.
        bench.click("Play");
        assert_eq!(bench.commands(), [Command::Play]);
        bench.ui.frame(&mut bench.settings, Vec::new());
        assert_eq!(bench.commands(), [Command::Play]);

        // It applies once, after that update's advance, so the draw shows it.
        assert_eq!(bench.tick(0.5), 1);
        assert!(bench.commands().is_empty());
        assert_eq!(bench.player().transport(), TransportState::Playing);
        bench.assert_frame(0.0);
        bench.assert_root_x(0.0);
        assert_eq!(bench.player().evaluation_count(), 1);

        // Later updates advance from it and never re-apply it.
        assert_eq!(bench.tick(0.1), 0);
        bench.assert_frame(3.0);
        bench.assert_root_x(3.0);
        assert_eq!(bench.player().evaluation_count(), 2);
        assert_eq!(bench.tick(0.0), 0);
        bench.palette();
        assert_eq!(
            bench.player().evaluation_count(),
            2,
            "unchanged frames do not evaluate"
        );

        // A scrub requested by the UI wins over that frame's advancement.
        bench.settings.animation.commands.push(Command::Scrub(7.5));
        assert_eq!(bench.tick(1.0), 1);
        assert_eq!(bench.player().transport(), TransportState::Paused);
        bench.assert_frame(7.5);
        bench.assert_root_x(7.5);
        assert_eq!(bench.player().evaluation_count(), 3);
        assert_eq!(
            bench.settings.animation.scrub, 7.5,
            "the slider follows the player"
        );
        assert_eq!(bench.player().preparation_count(), 1);
    }

    #[test]
    fn bck_ui_failed_candidate_keeps_diagnostic_and_retained_view() {
        use gx::animation_manifest::{KeyF32, TrackF32};
        let mut clip = test_support::ramp_clip(20, 2);
        clip.joints[0].axes[0].scale = TrackF32::Keyed {
            tangent_type: 1,
            keys: vec![
                KeyF32 {
                    time: 0.0,
                    value: 1.0,
                    tangent_in: 0.0,
                    tangent_out: f32::MAX,
                },
                KeyF32 {
                    time: 10.0,
                    value: 1.0,
                    tangent_in: -f32::MAX,
                    tangent_out: 0.0,
                },
            ],
        };
        let fixture =
            test_support::fixture("host-failed-candidate", &[("bcks/walk.bck", Some(clip))]);
        let mut bench = PlaybackBench::new(fixture);
        bench.click(WALK);
        let retained = bench.palette();
        bench.settings.animation.commands.push(Command::Scrub(3.0));
        assert_eq!(bench.palette(), retained);
        let error = bench.settings.animation.view.diagnostic.clone().unwrap();
        bench.settings.animation.commands.push(Command::Scrub(3.0));
        assert_eq!(
            bench.settings.animation.view.diagnostic.as_deref(),
            Some(error.as_str())
        );
        assert_eq!(bench.palette(), retained);
        assert_eq!(
            bench.settings.animation.view.diagnostic.as_deref(),
            Some(error.as_str())
        );
        assert_eq!(bench.settings.animation.view.frame, 0.0);
        assert_eq!(bench.player().transport(), TransportState::Paused);
        bench.settings.animation.commands.push(Command::Scrub(10.0));
        assert!(bench.settings.animation.view.diagnostic.is_some());
        bench.assert_root_x(10.0);
        assert_eq!(bench.settings.animation.view.diagnostic, None);
        assert_eq!(bench.settings.animation.view.frame, 10.0);
    }

    #[test]
    fn bck_selector_search_never_changes_playback() {
        let fixture = test_support::fixture(
            "host-search",
            &[
                ("bcks/walk.bck", Some(test_support::ramp_clip(20, 2))),
                ("bcks/run.bck", Some(test_support::ramp_clip(40, 2))),
            ],
        );
        let walk = fixture.identity(0);
        let mut bench = PlaybackBench::new(fixture);
        assert!(bench.ui.has_text("2 of 2 BCK clips"));
        bench.click(WALK);
        assert_eq!(bench.tick(0.0), 1);
        assert_eq!(bench.tick(0.1), 0);
        let playing = bench.playback();
        assert_eq!(playing.0.as_ref(), Some(&walk));

        // Filtering hides the selected row and issues nothing.
        bench.ui.type_search(&mut bench.settings, "RUN");
        assert_eq!(bench.settings.animation.query, "RUN");
        assert!(bench.ui.has_text(RUN));
        assert!(!bench.ui.has_text(WALK));
        assert!(bench.ui.has_text("1 of 2 BCK clips"));
        assert!(bench.commands().is_empty());
        assert_eq!(bench.tick(0.0), 0);
        assert_eq!(bench.playback(), playing);

        // The list is the player's own search.
        let shown: Vec<_> = bench
            .settings
            .animation
            .rows
            .iter()
            .filter(|row| row.matches(&bench.settings.animation.query))
            .map(|row| row.label.clone())
            .collect();
        let expected: Vec<_> = bench
            .player()
            .search("RUN")
            .map(|row| row.label.clone())
            .collect();
        assert_eq!(shown, expected);
        assert_eq!(shown, [RUN]);

        // The query survives a mode switch, and so does playback.
        bench.select_mode("Modern");
        assert_eq!(bench.settings.animation.query, "RUN");
        assert!(bench.ui.has_text(RUN));
        assert!(!bench.ui.has_text(WALK));
        assert_eq!(bench.tick(0.1), 0);
        assert_eq!(bench.player().selected_identity(), Some(&walk));
        bench.assert_frame(6.0);
        assert_eq!(bench.player().preparation_count(), 1);
    }

    #[test]
    fn bck_static_clip_ui_disabled() {
        let mut rise = test_support::clip(0, 2);
        rise.joints[0].axes[0].translation =
            gx::animation_manifest::TrackF32::Constant { value: 4.0 };
        let fixture = test_support::fixture(
            "host-static",
            &[
                ("bcks/rise.bck", Some(rise)),
                ("bcks/walk.bck", Some(test_support::ramp_clip(20, 2))),
            ],
        );
        let mut bench = PlaybackBench::new(fixture);
        bench.click(RISE);
        assert_eq!(bench.tick(0.0), 1);
        assert!(bench.player().is_static());
        assert_eq!(bench.player().transport(), TransportState::Stopped);
        assert!(bench.ui.has_text("Transport: Stopped (static clip)"));
        assert!(bench.ui.has_text("Frame: 0.00 / 0"));
        assert!(bench.ui.has_text("Effective policy: none"));
        bench.assert_transport_controls([
            ("Bind pose", true),
            ("Play", false),
            ("Pause", false),
            ("Restart", false),
        ]);
        bench.assert_slider_enabled("Scrub", false);
        bench.assert_slider_enabled("Speed", true);

        // Disabled controls issue nothing; the pose is frame 0, not bind.
        for label in ["Play", "Restart", "Pause"] {
            bench.click(label);
        }
        assert!(bench.commands().is_empty());
        assert_eq!(bench.tick(1.0), 0);
        assert_eq!(bench.player().transport(), TransportState::Stopped);
        bench.assert_root_x(4.0);

        // A positive-duration clip re-enables the transport.
        bench.click(WALK);
        assert_eq!(bench.tick(0.0), 1);
        assert!(!bench.player().is_static());
        bench.assert_transport_controls([
            ("Bind pose", true),
            ("Play", false),
            ("Pause", true),
            ("Restart", true),
        ]);
        bench.assert_slider_enabled("Scrub", true);
        assert!(bench.ui.has_text("Transport: Playing"));
    }

    #[test]
    fn bck_catalog_error_shown() {
        let fixture = test_support::fixture("host-missing-catalog", &[]);
        let missing = fixture.dir.join("absent").join("catalog.json");
        let host = AnimationHost::new(&test_support::skeleton(), &missing);
        let mut bench = PlaybackBench::with_host(fixture, host);
        let error = bench
            .player()
            .catalog_error()
            .expect("catalog error")
            .to_owned();
        assert!(error.contains("absent"));

        let painted = bench.text_containing("BCK catalog");
        assert_eq!(painted.text, error);
        assert_eq!(painted.color, ERROR_COLOR);
        assert!(bench.ui.has_text("0 of 0 BCK clips"));
        assert!(bench.ui.has_text("Clip: bind pose"));
        assert!(bench.ui.has_text("Transport: Stopped"));
        bench.assert_transport_controls([
            ("Bind pose", false),
            ("Play", false),
            ("Pause", false),
            ("Restart", false),
        ]);
        bench.assert_slider_enabled("Scrub", false);

        // Bind pose, and the controls are inert.
        for label in ["Play", "Restart", "Bind pose"] {
            bench.click(label);
        }
        assert!(bench.commands().is_empty());
        assert_eq!(bench.tick(0.5), 0);
        assert_eq!(bench.player().transport(), TransportState::Stopped);
        assert_eq!(bench.player().selected_identity(), None);
        assert_eq!(bench.palette(), vec![Mat4::IDENTITY; 2]);
        assert_eq!(bench.player().evaluation_count(), 0);
    }

    #[test]
    fn bck_animation_unavailable_static_fallback() {
        let fixture = test_support::fixture(
            "host-unavailable",
            &[("bcks/walk.bck", Some(test_support::ramp_clip(20, 2)))],
        );
        let mut skeleton = test_support::skeleton();
        skeleton.scaling_rule = gx::model_manifest::ScalingRule::Basic;
        let host = AnimationHost::new(&skeleton, &fixture.catalog);
        assert!(host.player.is_none());
        let mut bench = PlaybackBench::with_host(fixture, host);

        // No palette: both modes draw the static bind pose.
        assert!(bench.palette().is_empty());
        let painted = bench.text_containing("animation unavailable");
        assert_eq!(painted.color, ERROR_COLOR);
        assert!(bench.ui.has_text("Transport: unavailable"));
        assert!(bench.ui.has_text("0 of 0 BCK clips"));
        bench.assert_transport_controls([
            ("Bind pose", false),
            ("Play", false),
            ("Pause", false),
            ("Restart", false),
        ]);
        for label in ["Play", "Restart"] {
            bench.click(label);
        }
        assert!(bench.commands().is_empty());
        assert_eq!(bench.tick(0.5), 0);
        assert!(bench.palette().is_empty());
    }

    /// A mode whose skin failed to load reports a static reason, and the host
    /// disables playback rather than animating a palette the mode ignores.
    /// The `SkinningBuffers` → `setup` link itself needs a renderer; this
    /// covers everything after it.
    #[test]
    fn bck_skin_failure_disables_animation_ui() {
        let fixture = test_support::fixture(
            "host-skin-failure",
            &[("bcks/walk.bck", Some(test_support::ramp_clip(20, 2)))],
        );
        let mut host = AnimationHost::new(&test_support::skeleton(), &fixture.catalog);
        assert!(host.player.is_some(), "the skeleton alone is valid");

        let reason =
            "animation disabled, rendering the static model: GameCube mode: skin.bin: bad length";
        host.disable(reason.to_owned());
        assert!(host.player.is_none());
        let mut bench = PlaybackBench::with_host(fixture, host);

        assert!(bench.palette().is_empty());
        let painted = bench.text_containing("animation disabled");
        assert_eq!(painted.text, reason);
        assert_eq!(painted.color, ERROR_COLOR);
        assert!(bench.ui.has_text("Transport: unavailable"));
        assert!(bench.ui.has_text("0 of 0 BCK clips"));
        bench.assert_transport_controls([
            ("Bind pose", false),
            ("Play", false),
            ("Pause", false),
            ("Restart", false),
        ]);
        bench.assert_slider_enabled("Scrub", false);
        for label in ["Play", "Restart", "Bind pose"] {
            bench.click(label);
        }
        assert!(bench.commands().is_empty());
        assert_eq!(bench.tick(0.5), 0);
        assert!(bench.palette().is_empty());
    }
}
