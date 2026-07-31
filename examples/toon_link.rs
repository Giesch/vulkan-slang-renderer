//! Renders Toon Link from The Wind Waker — P9 of the link rendering plan
//! (`llm_notes/link_rendering.md`): all 24 batches drawn from one shared mesh
//! through 24 per-material pipelines, with the model's real albedo textures,
//! complete per-material raster state, gamma-correct output, the **full GX TEV
//! interpreter** (a real color channel driving the `ZBtoonEX` ramp through an
//! SRTG texgen, the stage chain with its swap tables and konst selects, and the
//! `TEXMTX1` pupil offset), and — new in P9 — the **eye/brow write-mask
//! multi-pass**: a five-group draw order in which the eye and brow decals
//! deposit their coverage in destination alpha, the bangs draw over them without
//! touching alpha, and the features then composite *through* the hair via
//! `BlendMode::DstAlpha`. See `llm_notes/link_rendering/phase_09_eyes.md`.
//! Both lights are fixed in world space and the model turns under them, as in
//! the game — which sweeps the terminator across Link.
//!
//! Requires converted assets on disk (gitignored — you need the disc image):
//! `just extract-link && just convert-link`.
//!
//! Controls live in the egui debug window (debug builds only):
//! - `debug_mode`: the ten `DebugMode` variants from the shader, as radio
//!   buttons
//! - `eflight`: toggle the second, green-channel light
//! - `isolate_batch` + `batch`: draw one batch instead of all of them
//! - `batch_info`: what the `batch` slider is currently pointing at; selecting a
//!   batch also dumps its TEV state to stdout

use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use facet::Facet;
use glam::{Mat3, Mat4, UVec4, Vec2, Vec3, Vec4};
use image::{DynamicImage, ImageReader, Rgba, RgbaImage};

use vulkan_slang_renderer::editor::{Checkbox, ColorPicker, IntSlider, Label, Slider};
use vulkan_slang_renderer::game::Game;
// The manifest's GX enums are named `mm::CullMode` / `mm::BlendMode` throughout:
// they collide with the renderer's same-named pipeline enums, and each mapping
// below reads as the GX-value → Vulkan-state translation it is.
use vulkan_slang_renderer::model_manifest::{
    self as mm, Batch, Manifest, MaterialEntry, TextureEntry,
};
use vulkan_slang_renderer::renderer::{
    BlendMode, CullMode, DepthCompare, DrawError, DrawIndexed, FrameRenderer, MeshHandle,
    PipelineHandle, RasterState, Renderer, TextureColorSpace, TextureFilter, TextureHandle,
    TextureOptions, TextureWrap, UniformBufferHandle,
};
use vulkan_slang_renderer::tev_pack;

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

/// J3D pixel-engine mode, reduced to the two-pass ordering key P7 needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeMode {
    Opaque,
    Translucent,
}

fn pe_mode(material: &MaterialEntry) -> anyhow::Result<PeMode> {
    match material.pe_mode {
        mm::PixelEngineMode::Opaque => Ok(PeMode::Opaque),
        mm::PixelEngineMode::Translucent => Ok(PeMode::Translucent),
        // cl.bdl has none
        other => anyhow::bail!("unmapped pe_mode {other} on material {:?}", material.name),
    }
}

/// The face. Matched by name because no state signature separates it from the
/// other eight opaque materials — `hideHatAndBackle`
/// (`../tww/src/d/actor/d_a_player_main.cpp:1509-1531`) names both material
/// strings verbatim at `:1512-1514`, so this is the game's own contract, not our
/// convention. P6's per-batch isolation map independently confirms batches 4
/// and 1. See `llm_notes/link_rendering/phase_09_eyes.md` decision 2.
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
    /// repainting an opaque rectangle there, which would be the black quad again
    /// by another route.
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
    if pe_mode(material)? != PeMode::Translucent {
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

/// Every batch drawn exactly once. The one invariant a mis-grouped batch would
/// otherwise break silently — a duplicated decal would double-composite, a
/// dropped one would just vanish.
fn check_permutation(order: &[BatchIndex], batch_count: usize, what: &str) -> anyhow::Result<()> {
    let mut seen: Vec<BatchIndex> = order.to_vec();
    seen.sort_unstable();
    seen.dedup();
    anyhow::ensure!(
        order.len() == batch_count && seen.len() == batch_count,
        "{what} draw order is not a permutation of the {batch_count} batches: \
         {} entries, {} distinct",
        order.len(),
        seen.len()
    );
    Ok(())
}

/// GX alpha-compare state as the raw codes the shader's `switch`es expect:
/// `[comp0, ref0, comp1, ref1]` plus the combiner op.
#[derive(Debug, Clone, Copy)]
struct AlphaCompareCodes {
    compare: UVec4,
    op: u32,
}

fn alpha_compare_codes(material: &MaterialEntry) -> AlphaCompareCodes {
    // No record → GX's default "Always OR Always", a no-op that keeps every fragment.
    let Some(ac) = &material.alpha_compare else {
        return AlphaCompareCodes {
            compare: UVec4::new(
                mm::CompareType::Always as u32,
                0,
                mm::CompareType::Always as u32,
                0,
            ),
            op: mm::AlphaOp::Or as u32,
        };
    };
    AlphaCompareCodes {
        compare: UVec4::new(
            ac.comp0 as u32,
            ac.ref0 as u32,
            ac.comp1 as u32,
            ac.ref1 as u32,
        ),
        op: ac.op as u32,
    }
}

/// Radians per second the model turns about Y. Both lights are fixed in world
/// space, so this is what sweeps the terminator — and it replaces the camera
/// orbit that used to sit here, which showed every side of Link but never moved
/// the shading.
const MODEL_SPIN: f32 = 20.0 * (PI / 180.0);

/// The two GX lights `lit_mask == 3` selects. **Each carries exactly one
/// channel**, and that is not a simplification — it is how the game does it.
///
/// `ZBtoonEX` is a *separable* 2D ramp (phase_08 risk #1): its red varies only
/// with u and its green only with v, both stepping sharply at ≈0.49. The SRTG
/// texgen feeds it `(color0.r, color0.g)`, so the two axes are two independent
/// lookups — which only works if the lights write to different channels. They do:
///
/// - **Light 0 is red-only.** `../tww/src/d/d_kankyo.cpp:1494-1499` sets
///   `mColor.r` (255 with no nearby point light and no flicker) and `:1545-1547`
///   hard-zero green and blue; `dKy_tevstr_init` repeats it at `:3410-3412`.
///   Its ramp axis drives stage 0's toon band.
/// - **Light 1 is green-only, and dark unless an "eflight" (torch, sword glow)
///   is nearby** — `:2557-2559`, gated by `lightMask = 1` with no eflight versus
///   `3` with one (`:2527-2531`). Its ramp axis drives stage 2's warm additive
///   highlight. Black here makes the manifest's `lit_mask == 3` behave exactly
///   like the runtime's `setLightMask(1)`; [`LightRig::eflight`] toggles it on.
///
/// Getting this wrong is what made the first pass strongly yellow: near-neutral
/// light colors give `r ≈ g`, so green saturated wherever red did and stage 2's
/// `konst1 = (160,90,0)` fired over the *whole* lit band instead of nothing.
///
/// With ambient fixed at 50/255 ≈ 0.196 on every channel, `illum.r` crosses the
/// ramp's 0.49 step at `N·L ≈ 0.294` and `illum.g` stays at 0.196 — below it,
/// always, until the eflight comes on.
const LIGHT0_COLOR: Vec3 = Vec3::new(1.0, 0.0, 0.0);
const LIGHT1_COLOR: Vec3 = Vec3::new(0.0, 0.0, 0.0);
/// Light 1 with the eflight on. The game ramps the green byte with distance and
/// flicker (`d_kankyo.cpp:2542-2557`); we take it at full.
const EFLIGHT_COLOR: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// Stage 2's additive tint while the eflight is on, replacing the manifest's
/// `konst_colors[1]`. `setLightTevColorType_sub` overwrites K1 with the eflight's
/// own color whenever that stage runs at all (`d_kankyo.cpp:1780`), so the
/// manifest value is a default the game never actually shows.
///
/// The treasure chest's glow, verbatim from `d_a_tbox.cpp:302-304`. A chest is a
/// steady eflight rather than a decaying flash, so this is one stable value
/// instead of a row picked off a decay curve.
///
/// The debug window's `eflight_konst` picker starts here; it, and not this
/// constant, is what `draw` writes.
const EFLIGHT_KONST: Vec3 = rgb8(255, 255, 100);
/// How much of [`EFLIGHT_KONST`] actually reaches K1.
///
/// **A demo choice, but a grounded one.** The game never writes the registered
/// color straight through: `settingTevStruct_eflightcol_plus` scales it by
/// `bright²` where `bright = 1 - distance/power` (`d_kankyo.cpp:1567-1584`), so
/// the full value only appears standing exactly at the light. This is that factor
/// at half the light's radius — `(1 - 0.5)² = 0.25`. Unscaled, the chest's near-
/// white glow saturates the tunic and the ramp's second axis stops being legible.
///
/// The debug window's `eflight_falloff` slider starts here, so the whole `bright²`
/// range is walkable without a rebuild.
const EFLIGHT_FALLOFF: f32 = 0.25;

/// Light 0's fixed orientation, **world space**. The game's key light does not
/// move with the actor — it is the sun, the moon, or the nearest torch — so the
/// terminator sweeps because [`MODEL_SPIN`] turns Link under it, not because the
/// light swings.
const LIGHT0_AZIMUTH: f32 = 0.6;
const LIGHT0_ELEVATION: f32 = 0.7;

/// The eflight's orientation, **model space** — it rotates with Link rather than
/// staying put in the world, so the highlight stays pinned to his front while
/// light 0's terminator sweeps past. That is the arrangement when the glow comes
/// from something he is facing: `d_a_tbox.cpp:301` puts the chest's light 50
/// units above the chest, and Link stands in front of it during the opening.
///
/// Azimuth 0 is straight ahead: the model faces **+Z**, measured off `cl.bdl`
/// itself — the `mouth` batch's mean vertex normal is `+0.82` in Z and `mayuL`'s
/// is `+0.90`, with the eyes at `z = +16.2`.
///
/// The elevation is **negative**, which the `+50` above is easy to misread: that
/// offset is off the chest's origin sitting on the floor, and `cl.bdl` spans
/// `y = 0..124`, so the light lands around Link's waist and shines *up* at the
/// torso it lights. `-0.35` is `atan2(50 - 85, 90)` — the light at 50, his upper
/// chest at 85, standing about a body length away. The distance is the soft part
/// of that, so the debug window's `eflight_elevation` slider walks the whole
/// plausible range without a rebuild; this constant only seeds it.
const EFLIGHT_AZIMUTH: f32 = 0.0;
const EFLIGHT_ELEVATION: f32 = -0.35;

/// The two endpoints of stage 0's toon lerp, `PREV = mix(REG0, K0, ramp.r)`.
///
/// Measured, not seeded: `scripts/link_env_colors.py` reads them out of the ocean
/// stage's `Pale` chunk (`just link-env-colors`) at `EnvR[0][0] → Colo[0][2] →
/// Pale[2]`, the 150–270 schedule plateau — roughly 10:00–18:00, the widest
/// daytime band and the only one whose two schedule endpoints name the same slot,
/// so it needs no time-of-day blend.
///
/// The game overwrites both every frame in `setLightTevColorType_sub`
/// (`../tww/src/d/d_kankyo.cpp:1817-1829`), which is why the manifest's values
/// (`reg_colors[0]` = mid-gray, `konst_colors[0]` = white) are only defaults.
/// Note that the lit end really is pure white at midday — the manifest's default
/// happens to be right here, and would not be at dawn or sunset.
///
/// Pale → `dKy_tevstr_c` wiring is `setLight_actor`, `d_kankyo.cpp:1328-1353`.
///
/// These two seed the debug window's `env_actor_c0` / `env_actor_k0` pickers,
/// which are what `draw` actually writes — so another time of day's plateau can
/// be dialed in and compared without a rebuild.
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
    *dst = Vec4::new(rgb.x, rgb.y, rgb.z, dst.w);
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
        let world = |d: Vec3| Vec4::new(d.x, d.y, d.z, 0.0);
        [
            world(dir(LIGHT0_AZIMUTH, LIGHT0_ELEVATION)),
            // Model space → world by the same Y rotation the vertices get, which
            // is what pins it to Link. It only shows up when `eflight` is on,
            // since otherwise its color is black.
            world(Mat3::from_rotation_y(spin) * dir(EFLIGHT_AZIMUTH, self.eflight_elevation)),
        ]
    }

    fn colors(&self) -> [Vec4; 2] {
        let light1 = if self.eflight {
            EFLIGHT_COLOR
        } else {
            LIGHT1_COLOR
        };
        [
            Vec4::new(LIGHT0_COLOR.x, LIGHT0_COLOR.y, LIGHT0_COLOR.z, 1.0),
            Vec4::new(light1.x, light1.y, light1.z, 1.0),
        ]
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

fn load_vertices(path: &Path, expected_count: u32) -> anyhow::Result<Vec<Vertex>> {
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(
        bytes.len() == expected_count as usize * VERTEX_STRIDE,
        "{}: expected {} vertices × {VERTEX_STRIDE} bytes, got {} bytes",
        path.display(),
        expected_count,
        bytes.len()
    );
    let f = |b: &[u8], i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    let vertices = bytes
        .chunks_exact(VERTEX_STRIDE)
        .map(|v| Vertex {
            position: Vec3::new(f(v, 0), f(v, 1), f(v, 2)),
            normal: Vec3::new(f(v, 3), f(v, 4), f(v, 5)),
            uv0: Vec2::new(f(v, 6), f(v, 7)),
        })
        .collect();
    Ok(vertices)
}

fn load_indices(path: &Path, expected_count: u32) -> anyhow::Result<Vec<u32>> {
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(
        bytes.len() == expected_count as usize * 4,
        "{}: expected {} u32 indices, got {} bytes",
        path.display(),
        expected_count,
        bytes.len()
    );
    let indicies = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    Ok(indicies)
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
        // util::load_image hardcodes the textures/ dir, so read directly;
        // entry.file is manifest-relative.
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
    // unconditionally. All 24 materials are Less_Equal in practice (this makes
    // P6's `Less` placeholder correct).
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

    // Honor z_write directly rather than tying it to z_test (P6 did the latter,
    // which forced the four *damA eye/brow decals to write depth; not writing is
    // what lets those layered decals composite at all).
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
            // GX's dst-alpha blend, and it is real now: the mask pass writes the
            // eye/brow coverage into destination alpha and this composites
            // through it, which is how the eyes read through the hair. See
            // `llm_notes/link_rendering/phase_09_eyes.md`.
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

pub struct ToonLink {
    start_time: Instant,
    manifest: Manifest,
    #[allow(unused)]
    mesh: MeshHandle<Vertex>,
    /// One per material slot, in `MaterialSlot` order — index with
    /// [`Self::pipeline`], never with a [`BatchIndex`].
    pipelines: Vec<(
        PipelineHandle<DrawIndexed>,
        UniformBufferHandle<ToonLinkParams>,
    )>,
    /// One fully-built uniform block per material slot, parallel to
    /// `pipelines`. The manifest's values verbatim, never mutated after
    /// construction: `draw` copies each one and patches `mvp`, the two light
    /// fields, `debug_mode` and the environment override onto the copy.
    params: Vec<ToonLinkParams>,
    /// The hardware's five-group order: mask, face+hair, composite, erase, then
    /// the rest of the model.
    draw_order: Vec<BatchIndex>,
    /// Which batch [`Self::update`] last dumped TEV state for, so the dump
    /// happens on a change rather than every frame.
    dumped: Option<BatchIndex>,
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
    eflight_konst: ColorPicker,
    /// How much of `eflight_konst` actually reaches K1 — see [`EFLIGHT_FALLOFF`].
    eflight_falloff: Slider,
    /// How far below Link the eflight sits, in radians. Runs `0` (level with him)
    /// down to `-0.5` — see [`EFLIGHT_ELEVATION`]. Only visible while `eflight` is
    /// checked, since light 1 is otherwise black.
    eflight_elevation: Slider,
    /// Stage 0's toon lerp endpoints: the shadow end goes to `reg[1]` and the lit
    /// end to `konst[0]`. See [`ENV_ACTOR_C0`] and [`ENV_ACTOR_K0`].
    env_actor_c0: ColorPicker,
    env_actor_k0: ColorPicker,
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
        &self.pipelines[slot.raw()].0
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

    /// Refresh the read-only description of the selected batch — the debug
    /// window's replacement for the old Q/E printouts.
    fn update_batch_info(&mut self) {
        let index = self.selected();
        let batch = self.batch(index);
        let slot = MaterialSlot::from_manifest(batch.material);
        let text = format!(
            "batch {}: shape {} material {} {:?} [{}..+{}]",
            index.raw(),
            batch.shape,
            slot.raw(),
            self.material(slot).name,
            batch.first_index,
            batch.index_count
        );
        self.edit_state.batch_info.set(text);
    }

    /// The full TEV dump, which is far too much text for the label. Gated on the
    /// selection actually changing: `update` runs every frame, and dumping a
    /// material's whole TEV state at frame rate would bury the terminal.
    fn dump_selection(&mut self) {
        let index = self.selected();
        if self.dumped == Some(index) {
            return;
        }
        self.dumped = Some(index);

        let batch = self.batch(index);
        let slot = MaterialSlot::from_manifest(batch.material);
        let material = self.material(slot);
        println!(
            "batch {}: shape {} material {} {:?} [{}..+{}]",
            index.raw(),
            batch.shape,
            slot.raw(),
            material.name,
            batch.first_index,
            batch.index_count
        );
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
        // reg_colors[i] loads REG{i}, not PREV — see src/tev_pack.rs. PREV has no
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

        anyhow::ensure!(indices.len() % 3 == 0, "index count not a triangle list");
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

        // push order defines MaterialSlot: pipelines[slot] is materials[slot]
        let mut pipelines = vec![];
        let mut params = vec![];
        for material in &manifest.materials {
            let role = decal_role(material)?;
            let params_buffer = renderer.create_uniform_buffer::<ToonLinkParams>()?;
            let tex0 = resolve_texmap(material, 0, &textures, &white_square);
            let tex1 = resolve_texmap(material, 1, &textures, &white_square);
            let pipeline_config = Shader::init()
                .pipeline_config(Resources {
                    tex0,
                    tex1,
                    params_buffer: &params_buffer,
                })
                .with_shared_mesh(&mesh)
                .with_raster_state(raster_state(material, role)?);
            let pipeline = renderer.create_pipeline(pipeline_config)?;

            pipelines.push((pipeline, params_buffer));

            // The whole per-material uniform, built once. `tev_pack::pack` is a
            // second gate on top of the converter's `tev_ir.rs`: this example
            // loads whatever manifest is on disk, which may predate it.
            let codes = alpha_compare_codes(material);
            params.push(ToonLinkParams {
                mvp: MVPMatrices {
                    model: Mat4::IDENTITY,
                    view: Mat4::IDENTITY,
                    proj: Mat4::IDENTITY,
                },
                tev: tev_pack::pack(material)?,
                alpha_compare: codes.compare,
                alpha_compare_op: codes.op,
                // patched every frame from the debug window; this is only the
                // value the buffer holds before the first `draw`
                debug_mode: DebugMode::default(),
                _padding_0: [0; 8],
            });
        }

        // The hardware's five-group order (phase_09_eyes.md, "What the game
        // does"). One pass over the batches, so INF1 order is preserved within
        // each group for free.
        let material_of = |batch: &Batch| -> &MaterialEntry {
            &manifest.materials[MaterialSlot::from_manifest(batch.material).raw()]
        };
        let (mut mask, mut early, mut composite, mut erase, mut rest) =
            (vec![], vec![], vec![], vec![], vec![]);
        for (i, batch) in manifest.batches.iter().enumerate() {
            let index = BatchIndex::from_raw(i);
            let material = material_of(batch);
            match decal_role(material)? {
                Some(DecalRole::Mask) => mask.push(index),
                Some(DecalRole::Composite) => composite.push(index),
                Some(DecalRole::Erase) => erase.push(index),
                // Pulled ahead of the composite so the mask survives *under*
                // the bangs, which is how the eyes read through the hair. The
                // game hides both for P1 so they still draw exactly once.
                None if matches!(material.name.as_str(), FACE_MATERIAL | HAIR_MATERIAL) => {
                    early.push(index)
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
        // getting this wrong moves the wrong batch into the early group and the
        // symptom (eyes compositing over the wrong surface) is subtle.
        let early_names: Vec<&str> = early
            .iter()
            .map(|&b| material_of(&manifest.batches[b.raw()]).name.as_str())
            .collect();
        anyhow::ensure!(
            early_names.len() == 2
                && early_names.contains(&FACE_MATERIAL)
                && early_names.contains(&HAIR_MATERIAL),
            "expected exactly one {FACE_MATERIAL:?} batch and one {HAIR_MATERIAL:?} batch \
             to pull ahead of the eye composite, found {early_names:?}"
        );

        // 1 mask deposits the eye/brow coverage in destination alpha, z-tested
        // against what is already drawn. 2 draws the bangs *without* touching
        // alpha, so the mask survives underneath them. 3 composites
        // `out = eye·dstA + fb·(1−dstA)` with the depth test off — the eyes read
        // through the hair. 4 zeroes the mask. 5 is the rest of the model (P1).
        let draw_order: Vec<BatchIndex> = mask
            .iter()
            .chain(&early)
            .chain(&composite)
            .chain(&erase)
            .chain(&rest)
            .copied()
            .collect();

        check_permutation(&draw_order, manifest.batches.len(), "five-group")?;

        // `draw`'s uniform loop zips these two and would silently skip the tail
        // if they ever diverged.
        anyhow::ensure!(
            pipelines.len() == params.len(),
            "pipeline/params arrays desynced ({} / {})",
            pipelines.len(),
            params.len()
        );

        let group =
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
            group(&mask),
            group(&early),
            group(&composite),
            group(&erase),
            group(&rest),
        );

        let last_batch = manifest.batches.len() as i64 - 1;
        let edit_state = EditState {
            debug_mode: DebugMode::default(),
            eflight: Checkbox::new(LightRig::default().eflight),
            eflight_konst: ColorPicker::from_vec3(EFLIGHT_KONST),
            eflight_falloff: Slider::new(EFLIGHT_FALLOFF, 0.0, 1.0),
            eflight_elevation: Slider::new(EFLIGHT_ELEVATION, 0.0, -0.5),
            env_actor_c0: ColorPicker::from_vec3(ENV_ACTOR_C0),
            env_actor_k0: ColorPicker::from_vec3(ENV_ACTOR_K0),
            isolate_batch: Checkbox::new(false),
            batch: IntSlider::new(0, 0, last_batch),
            batch_info: Label::new(""),
        };

        let mut game = Self {
            start_time: Instant::now(),
            manifest,
            mesh,
            pipelines,
            params,
            draw_order,
            dumped: None,
            edit_state,
        };
        game.update_batch_info();

        Ok(game)
    }

    fn update(&mut self) {
        self.update_batch_info();
        self.dump_selection();
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        // The model turns under a fixed camera and a fixed light, which is the
        // game's arrangement: the sun does not move, Link does. It also sweeps
        // the toon terminator across him, which is what the old W/A/S/D light
        // controls were for.
        let elapsed = (Instant::now() - self.start_time).as_secs_f32();
        let spin = elapsed * MODEL_SPIN;

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
        // Built from the widgets rather than stored, so they are the one source
        // of truth and there is no per-frame sync to forget.
        let light = LightRig {
            eflight: self.edit_state.eflight.checked,
            eflight_elevation: self.edit_state.eflight_elevation.value,
        };
        let light_dir = light.directions(spin);
        let light_color = light.colors();
        let eflight = light.eflight;

        // Read out here rather than inside the closure: it borrows `self.pipelines`
        // mutably, so it can't also reach into `self.edit_state`.
        let env_actor_c0 = self.edit_state.env_actor_c0.to_vec3();
        let env_actor_k0 = self.edit_state.env_actor_k0.to_vec3();
        let eflight_konst =
            self.edit_state.eflight_konst.to_vec3() * self.edit_state.eflight_falloff.value;

        renderer.submit_draws(|gpu| {
            for ((_, params_buffer), base) in self.pipelines.iter_mut().zip(&self.params) {
                let mut params = *base;

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

                gpu.write_uniform(params_buffer, params);
            }
        })
    }
}
