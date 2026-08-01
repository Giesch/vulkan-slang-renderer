//! Renders Toon Link from The Wind Waker: all 24 batches drawn from one shared
//! mesh through 24 per-material pipelines, with the model's real albedo
//! textures, complete per-material raster state, gamma-correct output, the full
//! GX TEV interpreter (`shaders/source/tev.slang`), and the eye/brow write-mask
//! multi-pass, in which the decals deposit coverage in destination alpha and
//! composite *through* the hair via `BlendMode::DstAlpha` (see [`DrawGroups`]).
//! Both lights are fixed in world space and the model turns under them, as in
//! the game — which sweeps the terminator across Link.
//! Plan and decision log: `llm_notes/link_rendering.md`.
//!
//! Requires converted assets on disk (gitignored — you need the disc image):
//! `just extract-link && just convert-link`.
//!
//! Controls live in the egui debug window (debug builds only) and are
//! documented on [`EditState`]; the debug views are documented on the shader's
//! `DebugMode` enum.

use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use facet::Facet;
use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};
use image::{DynamicImage, ImageReader, Rgba, RgbaImage};

use vulkan_slang_renderer::editor::{Checkbox, IntSlider, Label, RGBPicker, Slider};
use vulkan_slang_renderer::game::Game;
// The manifest's GX enums are named `mm::CullMode` / `mm::BlendMode` throughout:
// they collide with the renderer's same-named pipeline enums, and each mapping
// below reads as the GX-value → Vulkan-state translation it is.
use vulkan_slang_renderer::gx::model_manifest::{
    self as mm, Batch, Manifest, MaterialEntry, TextureEntry,
};
use vulkan_slang_renderer::gx::tev_pack;
use vulkan_slang_renderer::renderer::{
    BlendMode, CullMode, DepthCompare, DrawError, DrawIndexed, FrameRenderer, MeshHandle,
    PipelineHandle, RasterState, Renderer, TextureColorSpace, TextureFilter, TextureHandle,
    TextureOptions, TextureWrap, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::tev::{GXAlphaOp, GXCompare};
use vulkan_slang_renderer::generated::shader_atlas::toon_link::*;

fn main() -> Result<(), anyhow::Error> {
    ToonLink::run()
}

/// Winding bring-up knob (master plan risk #3): `Some(CullMode::None)` shows
/// every triangle regardless of winding; `None` uses each material's cull
/// mode from the manifest. Committed state: `None`.
const CULL_OVERRIDE: Option<CullMode> = None;

/// Link is ~124 model units tall (feet at Y ≈ 0); scale to ~1.24 world units.
const MODEL_SCALE: f32 = 0.01;

/// Radians per second the model turns about Y. Both lights are fixed in world
/// space, so this is what sweeps the terminator.
const MODEL_SPIN: f32 = 20.0 * (PI / 180.0);

/// `link.vtx.bin` is interleaved little-endian f32: pos[3] nrm[3] uv0[2].
const VERTEX_STRIDE: usize = 32;

/// Index into `Manifest::materials`, and by construction into
/// [`ToonLink::pipelines`] (one pipeline is baked per material slot, in slot
/// order).
///
/// Deliberately *not* interchangeable with [`BatchIndex`]. cl.bdl's batches
/// reference material slots in a **permuted** order — batch 1 uses slot 17,
/// batch 2 uses slot 18 — so while both spaces happen to be 24 long and the
/// mapping is a bijection, using one where the other belongs silently draws the
/// wrong material. Mixing them is now a compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MaterialSlot(usize);

impl MaterialSlot {
    /// The only place a raw `Batch::material` becomes a slot.
    fn from_manifest(material: u16) -> Self {
        Self(material as usize)
    }

    fn raw(self) -> usize {
        self.0
    }
}

/// Index into `Manifest::batches`, in INF1 draw order. This is what the debug
/// window's isolation slider walks — batches, not material slots (see
/// [`MaterialSlot`]).
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

/// Whether the J3D pixel-engine mode is translucent — the key that separates
/// the eye/brow decals from the opaque model.
fn is_translucent(material: &MaterialEntry) -> anyhow::Result<bool> {
    match material.pe_mode {
        mm::PixelEngineMode::Opaque => Ok(false),
        mm::PixelEngineMode::Translucent => Ok(true),
        // cl.bdl has none
        other => anyhow::bail!("unmapped pe_mode {other} on material {:?}", material.name),
    }
}

/// The face. Matched by name because no state signature separates it from the
/// other eight opaque materials — `hideHatAndBackle`
/// (`../tww/src/d/actor/d_a_player_main.cpp:1512-1514`) names both material
/// strings verbatim, so this is the game's own contract, not our convention.
/// See `llm_notes/link_rendering/phase_09_eyes.md` decision 2.
const FACE_MATERIAL: &str = "face";
/// The bangs, which the eye composite reads *through*.
const HAIR_MATERIAL: &str = "ear(2)";

/// One of GX's three eye/brow decal passes, as `daPy_lk_c` names them:
/// `mpZOnShape` / `mpZOffBlendShape` / `mpZOffNoneShape`.
///
/// The twelve translucent batches are **3 passes × 4 features**, not 12 BTP
/// frames — the three shapes of a feature are byte-identical geometry, authored
/// three times so the material state can differ. The game draws all twelve every
/// frame too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecalRole {
    /// `*damA` — z-tested, source-alpha blended, color writes off: deposits the
    /// feature's coverage in destination alpha. The z-test against what was
    /// already drawn is what stops eyes appearing through walls.
    Mask,
    /// `eyeL`/`eyeR`/`mayuL`/`mayuR` — dst-alpha blended, depth test off:
    /// composites the feature *through* whatever was drawn over it.
    ///
    /// Its `Greater 0` alpha compare is load-bearing for correctness, not an
    /// optimization. Our clear is alpha 1.0 (GX's is 0), so the mask pass leaves
    /// destination alpha ≥ 0.75 across the *whole* quad — including fully
    /// transparent texels. Only the shader-side discard stops this pass
    /// repainting an opaque rectangle there.
    Composite,
    /// `*damB` — blending off, TEV alpha identically 0: zeroes the mask so it
    /// cannot leak into later alpha-buffer effects. Its RGB is the black we used
    /// to draw.
    Erase,
}

/// Classify by state, never by name. `playerInit`
/// (`../tww/src/d/actor/d_a_player_main.cpp:12150-12178`) derives its three
/// arrays from `(z_compare_enable, blend_type)` on the materials under the
/// `CL_EYE` and `CL_MAYU` joints and asserts 4/4/4; the names serve only as the
/// assertion message. See phase_09_eyes.md decision 1.
///
/// `Ok(None)` for every opaque material.
fn decal_role(material: &MaterialEntry) -> anyhow::Result<Option<DecalRole>> {
    if !is_translucent(material)? {
        return Ok(None);
    }
    if material.z_test {
        return Ok(Some(DecalRole::Mask));
    }
    // Keyed on the blend *mode*, not the factors: GX ignores src/dst when the
    // mode is None_, so the `*damB` materials still carry
    // Source_Alpha/Inverse_Source_Alpha in MAT3 despite not blending at all.
    match material.blend.as_ref().map(|blend| blend.mode) {
        Some(mm::BlendMode::Blend) => Ok(Some(DecalRole::Composite)),
        Some(mm::BlendMode::None) | None => Ok(Some(DecalRole::Erase)),
        Some(other @ (mm::BlendMode::Logic | mm::BlendMode::Subtract)) => anyhow::bail!(
            "translucent material {:?} has unclassifiable GX blend mode {other}",
            material.name
        ),
    }
}

/// The five groups of the hardware's draw order (phase_09_eyes.md, "What the
/// game does"), each in INF1 order.
struct DrawGroups {
    /// 1: deposits the eye/brow coverage in destination alpha, z-tested
    /// against what is already drawn.
    mask: Vec<BatchIndex>,
    /// 2: the face and the bangs, drawn with color + depth but *no* alpha
    /// writes, so the mask survives underneath them. Pulled ahead of the
    /// composite; the game hides both for P1 so they still draw exactly once.
    face_hair: Vec<BatchIndex>,
    /// 3: composites `out = eye·dstA + fb·(1−dstA)` with the depth test off —
    /// the eyes read through the hair.
    composite: Vec<BatchIndex>,
    /// 4: zeroes the mask so it cannot leak into later alpha-buffer effects.
    erase: Vec<BatchIndex>,
    /// 5: the rest of the model (P1).
    rest: Vec<BatchIndex>,
}

impl DrawGroups {
    /// The five groups concatenated. A permutation of the batches by
    /// construction — [`group_batches`]'s single pass pushes each index into
    /// exactly one group — so only a group forgotten *here* could break that,
    /// which `setup`'s length check catches.
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

    fn print_summary(&self, manifest: &Manifest) {
        let raw =
            |batches: &[BatchIndex]| -> Vec<usize> { batches.iter().map(|b| b.raw()).collect() };
        println!(
            "toon_link: {} batches, {} materials, {} vertices\n\
             draw order (batch idx):\n\
             \x20  1 mask       {:?}   alpha-only writes, z-tested\n\
             \x20  2 face+hair  {:?}   color + depth, no alpha\n\
             \x20  3 composite  {:?}   dst-alpha blend, no depth test\n\
             \x20  4 erase      {:?}   alpha-only writes, zeroes the mask\n\
             \x20  5 rest       {:?}\n\
             debug controls are in the egui window",
            manifest.batches.len(),
            manifest.materials.len(),
            manifest.buffers.vertex_count,
            raw(&self.mask),
            raw(&self.face_hair),
            raw(&self.composite),
            raw(&self.erase),
            raw(&self.rest),
        );
    }
}

/// Classify every batch into its five-group role, preserving INF1 order within
/// each group.
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

    // The same assertion `playerInit` makes. This is what fires loudly if
    // --casual or a converter change perturbs the material table. It also
    // subsumes "every translucent batch was consumed": `decal_role` returns
    // `Some` for every translucent material or bails.
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

    // Bail on a missing *or duplicated* name rather than silently degrading:
    // getting this wrong moves the wrong batch into the face+hair group and the
    // symptom (eyes compositing over the wrong surface) is subtle.
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
/// block. No record → GX's default "Always OR Always", a no-op that keeps
/// every fragment.
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
        _padding_0: [0; 12],
    }
}

/// `mm::CompareType` (the manifest boundary) → the shader's generated
/// `GXCompare`. An exhaustive match rather than a numeric cast: a `repr(u32)`
/// enum holding an undeclared value is UB, so codes never cross by value.
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

/// The two GX lights `lit_mask == 3` selects. **Each carries exactly one
/// channel**, as in the game: `ZBtoonEX` is a *separable* 2D ramp — its red
/// varies only with u and its green only with v, both stepping sharply at
/// ≈0.49 — and the SRTG texgen feeds it `(color0.r, color0.g)`, so the two
/// axes are independent lookups only because the lights write to different
/// channels.
///
/// - **Light 0 is red-only** (`../tww/src/d/d_kankyo.cpp:1494-1499`, green and
///   blue hard-zeroed at `:1545-1547`). Its ramp axis drives stage 0's toon
///   band.
/// - **Light 1 is green-only, and dark unless an "eflight" (torch, sword glow)
///   is nearby** (`:2557-2559`, gated at `:2527-2531`). Its ramp axis drives
///   stage 2's warm additive highlight; [`LightRig::eflight`] toggles it on.
///
/// With ambient fixed at 50/255 ≈ 0.196 on every channel, `illum.r` crosses
/// the ramp's 0.49 step at `N·L ≈ 0.294` and `illum.g` stays below it until
/// the eflight comes on.
const LIGHT0_COLOR: Vec3 = Vec3::new(1.0, 0.0, 0.0);
const LIGHT1_COLOR: Vec3 = Vec3::new(0.0, 0.0, 0.0);
/// Light 1 with the eflight on. The game ramps the green byte with distance and
/// flicker (`d_kankyo.cpp:2542-2557`); we take it at full.
const EFLIGHT_COLOR: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// Stage 2's additive tint while the eflight is on, replacing the manifest's
/// `konst_colors[1]`: `setLightTevColorType_sub` overwrites K1 whenever that
/// stage runs at all (`d_kankyo.cpp:1780`). The value is the treasure chest's
/// steady glow, verbatim from `d_a_tbox.cpp:302-304`. Seeds the debug window's
/// `eflight_konst` picker, which is what `draw` actually writes.
const EFLIGHT_KONST: Vec3 = rgb8(255, 255, 100);
/// How much of [`EFLIGHT_KONST`] actually reaches K1. The game scales the
/// registered color by `bright²` where `bright = 1 - distance/power`
/// (`d_kankyo.cpp:1567-1584`); this is that factor at half the light's radius,
/// `(1 - 0.5)² = 0.25`. Unscaled, the near-white glow saturates the tunic.
/// Seeds the debug window's `eflight_falloff` slider.
const EFLIGHT_FALLOFF: f32 = 0.25;

/// Light 0's fixed orientation, **world space**. The game's key light does not
/// move with the actor — it is the sun, the moon, or the nearest torch — so the
/// terminator sweeps because [`MODEL_SPIN`] turns Link under it, not because the
/// light swings.
const LIGHT0_AZIMUTH: f32 = 0.6;
const LIGHT0_ELEVATION: f32 = 0.7;

/// The eflight's orientation, **model space** — it rotates with Link, so the
/// highlight stays pinned to his front while light 0's terminator sweeps past:
/// the arrangement when the glow comes from something he is facing, like the
/// chest's light 50 units above the chest (`d_a_tbox.cpp:301`). Azimuth 0 is
/// straight ahead — the model faces **+Z**, measured off `cl.bdl` itself.
///
/// The elevation is **negative** because that light lands around Link's waist
/// (`cl.bdl` spans `y = 0..124`) and shines *up* at the torso: `-0.35` is
/// `atan2(50 - 85, 90)` — the light at 50, his upper chest at 85, standing
/// about a body length away. Seeds the debug window's `eflight_elevation`
/// slider, which walks the plausible range without a rebuild.
const EFLIGHT_AZIMUTH: f32 = 0.0;
const EFLIGHT_ELEVATION: f32 = -0.35;

/// The two endpoints of stage 0's toon lerp, `PREV = mix(REG0, K0, ramp.r)`.
///
/// Measured, not seeded: `scripts/link_env_colors.py` reads them out of the
/// ocean stage's `Pale` chunk (`just link-env-colors`) at the 150–270 schedule
/// plateau — roughly 10:00–18:00, the one band that needs no time-of-day
/// blend. The game overwrites both registers every frame in
/// `setLightTevColorType_sub` (`../tww/src/d/d_kankyo.cpp:1817-1829`), so the
/// manifest's values are only the defaults J3D loaded; the Pale →
/// `dKy_tevstr_c` wiring is `setLight_actor`, `d_kankyo.cpp:1328-1353`.
///
/// These seed the debug window's `env_actor_c0` / `env_actor_k0` pickers,
/// which are what `draw` actually writes — so another time of day's plateau
/// can be dialed in without a rebuild.
const ENV_ACTOR_C0: Vec3 = rgb8(156, 140, 134);
const ENV_ACTOR_K0: Vec3 = rgb8(255, 255, 255);

/// A GX color, written as the bytes it is in the decomp and the disc data so the
/// constants below stay greppable against their sources.
const fn rgb8(r: u8, g: u8, b: u8) -> Vec3 {
    Vec3::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// Overwrite a GX color register's RGB and leave its alpha alone, the way
/// `setLightTevColorType_sub` does.
fn set_rgb(dst: &mut Vec4, rgb: Vec3) {
    *dst = rgb.extend(dst.w);
}

/// Light 0 is fixed in the world and the eflight is fixed relative to Link, so
/// the only mutable state is the eflight: whether it is lit, and how far above or
/// below him it sits.
struct LightRig {
    /// Whether a nearby "eflight" is lighting light 1's green channel. Off is the
    /// common case in the game — see [`LIGHT1_COLOR`].
    eflight: bool,
    /// Radians above the horizontal, negative for a light below him — see
    /// [`EFLIGHT_ELEVATION`].
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
    /// `lightDir[i]` points **from the surface toward light i**, in world space.
    /// The shader does not negate — this is the one place the convention is
    /// established, and it is the classic sign-flip site.
    ///
    /// The two lights live in *different* frames, which is the whole point: light
    /// 0 is anchored in the world, so `spin` sweeps its terminator across Link,
    /// while the eflight is anchored to Link, so its highlight rides along with
    /// him. Watching the two decouple as he turns is the clearest demonstration
    /// that the ramp's red and green axes are independent.
    fn directions(&self, spin: f32) -> [Vec4; 2] {
        let dir = |az: f32, el: f32| {
            Vec3::new(el.cos() * az.sin(), el.sin(), el.cos() * az.cos()).normalize()
        };
        [
            dir(LIGHT0_AZIMUTH, LIGHT0_ELEVATION).extend(0.0),
            // Model space → world by the same Y rotation the vertices get, which
            // is what pins it to Link. It only shows up when `eflight` is on,
            // since otherwise its color is black.
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/link/converted")
}

fn load_manifest(dir: &Path) -> anyhow::Result<Manifest> {
    let path = dir.join("link.manifest.json");
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "{}: not found — run `just extract-link && just convert-link` first \
             (assets are gitignored; you need the disc image, see \
             llm_notes/link_rendering/phase_00.md)",
            path.display()
        )
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Read a whole binary file, checking it holds exactly `count` records of
/// `stride` bytes.
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

/// Sanity-check the manifest against the loaded buffers before building
/// anything from it.
fn validate_manifest(
    manifest: &Manifest,
    vertices: &[Vertex],
    indices: &[u32],
) -> anyhow::Result<()> {
    anyhow::ensure!(
        indices.len().is_multiple_of(3),
        "index count not a triangle list"
    );
    // the debug window's isolation slider indexes this
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

    // P7 binds exactly two texmap slots; a future model that uses a third
    // should say so loudly rather than have its texture silently dropped
    // (decision 1).
    for material in &manifest.materials {
        anyhow::ensure!(
            material.texmaps.iter().skip(2).all(Option::is_none),
            "material {:?} uses a texmap slot >= 2; P7 binds only slots 0 and 1",
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
    // The four mipmapping filters have no TextureFilter spelling;
    // cl.bdl's textures are all Linear.
    let filter = match entry.filter {
        mm::FilterMode::Linear => TextureFilter::Linear,
        mm::FilterMode::Nearest => TextureFilter::Nearest,
        other => anyhow::bail!("unmapped GX texture filter {other}"),
    };
    Ok(TextureOptions {
        filter,
        wrap_u: wrap(entry.wrap_u),
        wrap_v: wrap(entry.wrap_v),
        mipmaps: entry.mipmaps,
        // Hardcoded Unorm on purpose:
        // GX has no sRGB anywhere, so the stored texels are raw values
        // the shader consumes directly (the fragment shader
        // applies its own sRGB decode to survive the _SRGB color target).
        color_space: TextureColorSpace::Unorm,
    })
}

/// One texture per manifest index, but only the entries some material's
/// `texmaps` actually references are loaded (7 of 41). The other 34 are BTP
/// eye/brow animation frames, unreachable without BTP (see follow_up.md);
/// loading them would quietly claim we understand them. Unreferenced slots stay
/// `None`.
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
    let mut loaded = 0;
    for (i, entry) in manifest.textures.iter().enumerate() {
        if !referenced[i] {
            textures.push(None);
            continue;
        }
        // entry.file is manifest-relative, and the per-entry context beats
        // util::load_image's generic error message here
        let image = ImageReader::open(dir.join(&entry.file))
            .with_context(|| format!("opening texture {}", entry.file))?
            .decode()
            .with_context(|| format!("decoding texture {}", entry.file))?;
        let handle = renderer.create_texture_with_options(
            entry.file.clone(),
            &image,
            texture_options(entry)?,
        )?;
        textures.push(Some(handle));
        loaded += 1;
    }

    println!(
        "toon_link: loaded {loaded} of {} textures ({} unreferenced BTP frames skipped)",
        manifest.textures.len(),
        manifest.textures.len() - loaded,
    );

    Ok(textures)
}

/// The texture bound into shader slot `slot` for `material`: the referenced
/// albedo/ramp if the material has a texmap there, otherwise the 1×1 dummy.
fn resolve_texmap<'a>(
    material: &MaterialEntry,
    slot: usize,
    textures: &'a [Option<TextureHandle>],
    dummy: &'a TextureHandle,
) -> &'a TextureHandle {
    material
        .texmaps
        .get(slot)
        .copied()
        .flatten()
        .and_then(|index| textures[index as usize].as_ref())
        .unwrap_or(dummy)
}

fn raster_state(material: &MaterialEntry, role: Option<DecalRole>) -> anyhow::Result<RasterState> {
    let cull = match CULL_OVERRIDE {
        Some(cull) => cull,
        None => match material.cull {
            mm::CullMode::Back => CullMode::Back,
            mm::CullMode::None => CullMode::None,
            mm::CullMode::Front => CullMode::Front,
            // unused by cl.bdl
            mm::CullMode::All => anyhow::bail!("unmapped GX cull mode {}", material.cull),
        },
    };

    // Depth test: honor z_func exactly when the test is enabled, else pass
    // unconditionally. All 24 materials are Less_Equal in practice.
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

    // Honor z_write directly rather than tying it to z_test: not writing depth
    // is what lets the layered *damA eye/brow decals composite at all.
    let depth_write = material.z_write;

    // One rule with no exceptions (phase_09_eyes.md decision 3): alpha writes
    // are on for exactly the mask and erase passes and off everywhere else.
    // That is what the game does — `l_onCupOffAupPacket2` is the last P0 packet
    // (`../tww/src/m_Do/m_Do_ext.cpp:1845-1853`), so all of P1 runs with
    // alphaUpdate = 0 too. Masking alpha globally is safe here: the swapchain is
    // created with CompositeAlphaFlagsKHR::OPAQUE, so nothing outside the frame
    // observes framebuffer alpha.
    let color_write = match role {
        Some(DecalRole::Mask | DecalRole::Erase) => [false, false, false, true],
        Some(DecalRole::Composite) | None => [true, true, true, false],
    };

    // NOTE every field listed rather than `..Default::default()`: each one now
    // has a phase-9 reason, and a future RasterState field should be a compile
    // error here rather than silently defaulted.
    Ok(RasterState {
        blend: blend_mode(material)?,
        cull,
        depth_test,
        depth_write,
        color_write,
    })
}

fn blend_mode(material: &MaterialEntry) -> anyhow::Result<BlendMode> {
    // No blend record → blending off.
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
            // GX's dst-alpha blend: the mask pass writes the eye/brow coverage
            // into destination alpha and this composites through it, which is
            // how the eyes read through the hair.
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

/// Bake one pipeline, uniform buffer and base uniform block per material, in
/// `MaterialSlot` order: push order is what defines the slot.
fn build_material_pipelines(
    renderer: &mut Renderer,
    manifest: &Manifest,
    mesh: &MeshHandle<Vertex>,
    textures: &[Option<TextureHandle>],
    dummy: &TextureHandle,
) -> anyhow::Result<Vec<MaterialPipeline>> {
    let mut pipelines = Vec::with_capacity(manifest.materials.len());
    for material in &manifest.materials {
        let role = decal_role(material)?;
        let params_buffer = renderer.create_uniform_buffer::<ToonLinkParams>()?;
        let pipeline_config = Shader::init()
            .pipeline_config(Resources {
                tex0: resolve_texmap(material, 0, textures, dummy),
                tex1: resolve_texmap(material, 1, textures, dummy),
                params_buffer: &params_buffer,
            })
            .with_shared_mesh(mesh)
            .with_raster_state(raster_state(material, role)?);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        // The whole per-material uniform, built once. `tev_pack::pack` is a
        // second gate on top of the converter's `tev_ir.rs`: this example
        // loads whatever manifest is on disk, which may predate it.
        let base_params = ToonLinkParams {
            mvp: MVPMatrices {
                model: Mat4::IDENTITY,
                view: Mat4::IDENTITY,
                proj: Mat4::IDENTITY,
            },
            tev: tev_pack::pack(material)?,
            alpha_compare: alpha_compare(material),
            // patched every frame from the debug window; this is only the
            // value the buffer holds before the first `draw`
            debug_mode: DebugMode::default(),
            _padding_0: [0; 12],
        };

        pipelines.push(MaterialPipeline {
            pipeline,
            params_buffer,
            base_params,
        });
    }
    Ok(pipelines)
}

/// Everything baked for one material slot. Building all three together makes
/// "pipeline and params out of sync" unrepresentable.
struct MaterialPipeline {
    pipeline: PipelineHandle<DrawIndexed>,
    params_buffer: UniformBufferHandle<ToonLinkParams>,
    /// The manifest's values verbatim, never mutated after construction:
    /// `draw` copies this and patches `mvp`, the two light fields,
    /// `debug_mode` and the environment override onto the copy.
    base_params: ToonLinkParams,
}

pub struct ToonLink {
    start_time: Instant,
    manifest: Manifest,
    /// One per material slot, in `MaterialSlot` order — index with
    /// [`Self::pipeline`], never with a [`BatchIndex`].
    pipelines: Vec<MaterialPipeline>,
    /// The hardware's five-group order: mask, face+hair, composite, erase, then
    /// the rest of the model.
    draw_order: Vec<BatchIndex>,
    /// The selection [`Self::update`] last refreshed the label and dumped TEV
    /// state for, so both happen on a change rather than every frame.
    last_selection: Option<BatchIndex>,
    edit_state: EditState,
}

/// The egui debug window, generated by reflection over these fields.
/// `debug_mode` is the shader's own generated enum, so its variants render as
/// radio buttons without a parallel list here to keep in sync.
#[derive(Facet)]
pub struct EditState {
    debug_mode: DebugMode,
    /// The second, green-channel light. Off is the common case in the game.
    eflight: Checkbox,
    /// Stage 2's additive tint, before `eflight_falloff` scales it. Only reaches
    /// K1 while `eflight` is checked — see [`EFLIGHT_KONST`].
    eflight_konst: RGBPicker,
    /// How much of `eflight_konst` actually reaches K1 — see [`EFLIGHT_FALLOFF`].
    eflight_falloff: Slider,
    /// How far below Link the eflight sits, in radians. Runs `0` (level with him)
    /// down to `-0.5` — see [`EFLIGHT_ELEVATION`]. Only visible while `eflight` is
    /// checked, since light 1 is otherwise black.
    eflight_elevation: Slider,
    /// Stage 0's toon lerp endpoints: the shadow end goes to `reg[1]` and the lit
    /// end to `konst[0]`. See [`ENV_ACTOR_C0`] and [`ENV_ACTOR_K0`].
    env_actor_c0: RGBPicker,
    env_actor_k0: RGBPicker,
    isolate_batch: Checkbox,
    /// A [`BatchIndex`] in disguise: 0..=batches-1, only read when
    /// `isolate_batch` is checked.
    batch: IntSlider,
    batch_info: Label,
}

impl ToonLink {
    fn batch(&self, index: BatchIndex) -> &Batch {
        &self.manifest.batches[index.raw()]
    }

    fn material(&self, slot: MaterialSlot) -> &MaterialEntry {
        &self.manifest.materials[slot.raw()]
    }

    fn pipeline(&self, slot: MaterialSlot) -> &PipelineHandle<DrawIndexed> {
        &self.pipelines[slot.raw()].pipeline
    }

    /// The batch the debug window has selected, or `None` while it's drawing
    /// every batch.
    fn isolate(&self) -> Option<BatchIndex> {
        self.edit_state
            .isolate_batch
            .checked
            .then(|| self.selected())
    }

    /// The batch the slider points at, isolated or not. The slider clamps to
    /// its own range, so this is always in bounds.
    fn selected(&self) -> BatchIndex {
        BatchIndex::from_raw(self.edit_state.batch.value as usize)
    }

    fn describe_batch(&self, index: BatchIndex) -> String {
        let batch = self.batch(index);
        let slot = MaterialSlot::from_manifest(batch.material);
        format!(
            "batch {}: shape {} material {} {:?} [{}..+{}]",
            index.raw(),
            batch.shape,
            slot.raw(),
            self.material(slot).name,
            batch.first_index,
            batch.index_count
        )
    }

    /// Refresh the read-only description of the selected batch.
    fn update_batch_info(&mut self) {
        let text = self.describe_batch(self.selected());
        self.edit_state.batch_info.set(text);
    }

    /// The full TEV dump, which is far too much text for the label. `update`
    /// gates this on the selection actually changing: dumping a material's
    /// whole TEV state at frame rate would bury the terminal.
    fn dump_selection(&mut self) {
        let index = self.selected();
        let slot = MaterialSlot::from_manifest(self.batch(index).material);
        let material = self.material(slot);
        println!("{}", self.describe_batch(index));
        // Isolating a batch has a way to legitimately render nothing, and during
        // bring-up that is indistinguishable from a bug. Say when it is the
        // former: the mask and erase passes draw with color writes off, so
        // isolating one of those eight batches yields a *black frame*.
        let role = decal_role(material).ok().flatten();
        println!("  role {role:?}");
        if matches!(role, Some(DecalRole::Mask | DecalRole::Erase)) {
            println!(
                "  -> expect a black frame: this pass touches destination alpha and nothing else"
            );
        }
        self.print_tev_state(material);
    }

    /// The material's TEV configuration, with the equation lines byte-identical
    /// to `assets/link/converted/mat3_dump.txt`'s so the two can be diffed
    /// directly — plus the two things the dump does *not* carry: the resolved
    /// konst selectors (it prints a bare `KONST`) and the swap-table contents.
    /// That gap is exactly where the phase-8 plan's worked example was wrong.
    fn print_tev_state(&self, material: &MaterialEntry) {
        let texmap = |slot: usize| match material.texmaps.get(slot).copied().flatten() {
            Some(i) => match self.manifest.textures.get(i as usize) {
                Some(t) => format!("{i} {}", t.name),
                None => format!("{i} <out of range>"),
            },
            None => "-".to_string(),
        };
        println!("  tex0={}  tex1={}", texmap(0), texmap(1));

        for (label, slot) in [("chan0 COLOR0", 0), ("chan1 ALPHA0", 1)] {
            let Some(c) = material.channels.get(slot) else {
                continue;
            };
            println!(
                "  {label}: lit={} mask=0x{:02x} diffuse={} attn={} mat={} amb={}",
                c.lighting_enabled, c.lit_mask, c.diffuse, c.attenuation, c.mat_src, c.amb_src
            );
        }
        println!(
            "  mat={}  amb={}",
            rgba_str(material.material_colors.first().copied().flatten()),
            rgba_str(material.ambient_colors.first().copied().flatten()),
        );

        // Named, not numeric — mat3_dump.txt prints these lines the same way, so
        // a wrong material can be diffed against the dump without translating.
        for (i, g) in material.texgens.iter().enumerate() {
            println!(
                "  texgen{i}: {} from {} via {}",
                gx_name::<mm::TexGenType>(g.ty),
                gx_name::<mm::TexGenSrc>(g.src),
                gx_name::<mm::TexGenMatrix>(g.matrix),
            );
        }

        let equations = match tev_pack::stage_equations(material) {
            Ok(lines) => lines,
            Err(e) => {
                println!("  <cannot render equations: {e:#}>");
                return;
            }
        };
        for (i, chunk) in equations.chunks(2).enumerate() {
            let order = material.tev.orders.get(i).and_then(Option::as_ref);
            let swap = material.tev.swap_modes.get(i).and_then(Option::as_ref);
            let (ras_sel, tex_sel) = swap.map_or((0, 0), |s| (s.ras_sel, s.tex_sel));
            let table = material
                .tev
                .swap_tables
                .get(tex_sel as usize)
                .and_then(Option::as_ref)
                .map_or("-".to_string(), |t| format!("{t:?}"));
            if let Some(o) = order {
                println!(
                    "  stage{i} order: coord={} map={} chan={}   swap ras={ras_sel} tex={tex_sel} {table}",
                    gx_name::<mm::TexCoordId>(o.tex_coord),
                    gx_name::<mm::TexMapId>(o.tex_map),
                    gx_name::<mm::ColorChannelId>(o.channel),
                );
            }
            let (kcsel, kasel) = tev_pack::stage_konst_selects(material, i)
                .unwrap_or_else(|_| ("?".into(), "?".into()));
            println!("  {}   kcsel={kcsel}", chunk[0]);
            if let Some(alpha) = chunk.get(1) {
                println!("  {alpha}   kasel={kasel}");
            }
        }

        let konst: Vec<String> = material
            .tev
            .konst_colors
            .iter()
            .map(|c| rgba_str(*c))
            .collect();
        println!("  konst = {}", konst.join("  "));
        // reg_colors[i] loads REG{i}, not PREV — see src/gx/tev_pack.rs. PREV has no
        // MAT3 value and reg_colors[3] is never loaded at all.
        for (i, c) in material.tev.reg_colors.iter().take(3).enumerate() {
            print!(
                "  REG{i}={}",
                c.map_or("-".to_string(), |c| format!("{c:?}"))
            );
        }
        println!("   (PREV: no MAT3 value; reg_colors[3] never loaded)");
    }
}

/// A raw manifest GX byte as its canonical name, or the raw value if it is not
/// a known variant (the printout is a diagnostic — it must never panic).
fn gx_name<T>(value: u8) -> String
where
    T: TryFrom<u8, Error = mm::GxEnumError> + std::fmt::Display,
{
    T::try_from(value).map_or_else(|_| format!("<{value}>"), |v| v.to_string())
}

fn rgba_str(c: Option<[u8; 4]>) -> String {
    c.map_or("-".to_string(), |c| {
        format!("{},{},{},{}", c[0], c[1], c[2], c[3])
    })
}

impl Game for ToonLink {
    type EditState = EditState;

    fn window_title() -> &'static str {
        "Toon Link"
    }

    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("Toon Link", &mut self.edit_state))
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
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
        let white_square_image =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([255; 4])));
        let white_square = renderer.create_texture_with_options(
            "toon_link_white_square",
            &white_square_image,
            TextureOptions {
                filter: TextureFilter::Linear,
                wrap_u: TextureWrap::ClampToEdge,
                wrap_v: TextureWrap::ClampToEdge,
                mipmaps: false,
                color_space: TextureColorSpace::Unorm,
            },
        )?;

        let pipelines =
            build_material_pipelines(renderer, &manifest, &mesh, &textures, &white_square)?;

        let groups = group_batches(&manifest)?;
        let draw_order = groups.draw_order();
        // A dropped decal would just vanish; see DrawGroups::draw_order.
        anyhow::ensure!(
            draw_order.len() == manifest.batches.len(),
            "draw order covers {} of {} batches",
            draw_order.len(),
            manifest.batches.len()
        );
        groups.print_summary(&manifest);

        let last_batch = manifest.batches.len() as i64 - 1;
        let edit_state = EditState {
            debug_mode: DebugMode::default(),
            eflight: Checkbox::new(LightRig::default().eflight),
            eflight_konst: RGBPicker::from_vec3(EFLIGHT_KONST),
            eflight_falloff: Slider::new(EFLIGHT_FALLOFF, 0.0, 1.0),
            eflight_elevation: Slider::new(EFLIGHT_ELEVATION, 0.0, -0.5),
            env_actor_c0: RGBPicker::from_vec3(ENV_ACTOR_C0),
            env_actor_k0: RGBPicker::from_vec3(ENV_ACTOR_K0),
            isolate_batch: Checkbox::new(false),
            batch: IntSlider::new(0, 0, last_batch),
            batch_info: Label::new(""),
        };

        let mut game = Self {
            start_time: Instant::now(),
            manifest,
            pipelines,
            draw_order,
            last_selection: None,
            edit_state,
        };
        game.update_batch_info();

        Ok(game)
    }

    fn update(&mut self) {
        let selection = self.selected();
        if self.last_selection == Some(selection) {
            return;
        }
        self.last_selection = Some(selection);
        self.update_batch_info();
        self.dump_selection();
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        // The model turns under a fixed camera and a fixed light, which is the
        // game's arrangement: the sun does not move, Link does.
        let spin = self.start_time.elapsed().as_secs_f32() * MODEL_SPIN;

        // Uniform scale commutes with rotation, so the order is readability only.
        // Normals survive this: `rotateDirection` (shaders/source/mvp.slang) is
        // exact for rotation plus *uniform* scale, and the fragment shader
        // renormalizes anyway.
        let model = Mat4::from_rotation_y(spin) * Mat4::from_scale(Vec3::splat(MODEL_SCALE));
        let target = Vec3::new(0.0, 0.62, 0.0);
        let eye = target + Vec3::new(0.0, 0.25, 2.8);
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        let proj = Mat4::perspective_rh(45f32.to_radians(), renderer.aspect_ratio(), 0.1, 20.0);

        // one index-range draw per batch, in the five-group order
        let isolate = self.isolate();
        for &index in &self.draw_order {
            if isolate.is_some_and(|only| only != index) {
                continue;
            }
            let batch = self.batch(index);
            let pipeline = self.pipeline(MaterialSlot::from_manifest(batch.material));
            renderer.queue_draw_index_range(pipeline, batch.first_index, batch.index_count);
        }

        let mvp = MVPMatrices { model, view, proj };
        let debug_mode = self.edit_state.debug_mode;
        let light = LightRig {
            eflight: self.edit_state.eflight.checked,
            eflight_elevation: self.edit_state.eflight_elevation.value,
        };
        let light_dir = light.directions(spin);
        let light_color = light.colors();
        let eflight = light.eflight;

        let env_actor_c0 = self.edit_state.env_actor_c0.to_vec3();
        let env_actor_k0 = self.edit_state.env_actor_k0.to_vec3();
        let eflight_konst =
            self.edit_state.eflight_konst.to_vec3() * self.edit_state.eflight_falloff.value;

        renderer.submit_draws(|gpu| {
            for material in &mut self.pipelines {
                let mut params = material.base_params;

                params.mvp = mvp;
                params.tev.light_dir = light_dir;
                params.tev.light_color = light_color;
                params.debug_mode = debug_mode;

                // The environment override, mirroring `setLightTevColorType_sub`
                // (`../tww/src/d/d_kankyo.cpp:1817-1829`): the game rewrites
                // stage 0's two lerp endpoints from `dKy_tevstr_c` every frame,
                // so the manifest's `reg_colors[0]` / `konst_colors[0]` are only
                // the defaults J3D loaded.
                //
                // Gated on the color channel actually being lit, as the game
                // gates on `mLightMode != 0`: the eye and brow decals keep their
                // MAT3 values. RGB only — the game copies the existing alpha back
                // before writing (`:1820`, `:1826`), and `sleeve` stage 1's alpha
                // reads K0's, so clobbering it would change the cutout.
                if params.tev.chan_control[0].x != 0 {
                    set_rgb(&mut params.tev.reg[1], env_actor_c0);
                    set_rgb(&mut params.tev.konst[0], env_actor_k0);
                    if eflight {
                        set_rgb(&mut params.tev.konst[1], eflight_konst);
                    }
                }

                gpu.write_uniform(&mut material.params_buffer, params);
            }
        })
    }
}
