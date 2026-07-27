//! Renders Toon Link from The Wind Waker — P8 of the link rendering plan
//! (`llm_notes/link_rendering.md`): all 24 batches drawn from one shared mesh
//! through 24 per-material pipelines, with the model's real albedo textures,
//! complete per-material raster state, a J3D opaque-before-translucent draw
//! order, gamma-correct output, and — new in P8 — the **full GX TEV
//! interpreter**: a real color channel driving the `ZBtoonEX` ramp through an
//! SRTG texgen, the stage chain with its swap tables and konst selects, and the
//! `TEXMTX1` pupil offset. Rotating the light sweeps the terminator.
//!
//! Requires converted assets on disk (gitignored — you need the disc image):
//! `just extract-link && just convert-link`.
//!
//! Controls:
//! - W / A / S / D: rotate the light (elevation / azimuth), held
//! - R / F: next / previous debug mode
//! - Num1 / Num2 / Num3 / Num4: jump to mode 0 / 1 / 2 / 3
//! - Q / E: isolate previous / next batch (prints its TEV state to stdout)
//! - Space: clear isolation, draw all batches
//!
//! Debug modes: 0 final TEV, 1 world normals, 2 uv0, 3 final TEV alpha,
//! 4 rasterized COLOR0, 5 texgen-1 coord, 6 raw tex0, 7 raw tex1,
//! 8 channel per-fragment, 9 texgen matrices forced to identity.

use std::f32::consts::{PI, TAU};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use glam::{Mat3, Mat4, UVec4, Vec2, Vec3, Vec4};
use image::{DynamicImage, ImageReader, Rgba, RgbaImage};

use vulkan_slang_renderer::game::{Game, Input, Key};
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

/// Index into `Manifest::batches`, in INF1 draw order. This is what the Q/E
/// isolation walks — batches, not material slots (see [`MaterialSlot`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct BatchIndex(usize);

impl BatchIndex {
    const FIRST: Self = Self(0);

    fn from_raw(index: usize) -> Self {
        Self(index)
    }

    fn raw(self) -> usize {
        self.0
    }

    fn next(self, count: usize) -> Self {
        Self((self.0 + 1) % count)
    }

    fn prev(self, count: usize) -> Self {
        Self((self.0 + count - 1) % count)
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

/// Debug views, matching `toon_link.shader.slang`'s `DEBUG_*` constants.
/// NOTE keep in sync with the module doc comment and the startup legend.
const DEBUG_MODE_NAMES: [&str; 10] = [
    "final TEV",
    "world normals",
    "uv0",
    "final TEV alpha",
    "rasterized COLOR0",
    "texgen-1 coord",
    "raw tex0",
    "raw tex1",
    "channel per-fragment",
    "texgen matrices = identity",
];

/// Held-key light controls, per the `examples/ray_marching.rs` pattern.
#[derive(Default)]
struct Intent {
    az_left: bool,
    az_right: bool,
    el_up: bool,
    el_down: bool,
}

/// Radians per second of light rotation while a key is held.
const LIGHT_SPIN: f32 = 1.2;

/// The two GX lights `lit_mask == 3` selects.
///
/// **Hand-tuned daytime seeds, not ground truth** (master plan risk #8): the
/// manifest's `light_colors` is null on every material because the game writes
/// them per frame from `dKy_tevstr_c`, and reading those out of emulated RAM is
/// a deferred escalation.
///
/// They are not arbitrary, though. The `ZBtoonEX` ramp's terminator is a sharp
/// step at ≈0.49 in both of its axes, and the manifest's ambient is a fixed
/// 50/255 ≈ 0.196, so `illum = 0.196 + Σ max(N·L, 0)·color` has to *straddle*
/// 0.49 across the model or there are no bands at all: too dim and everything
/// is shadow, too bright and everything is lit. Light 0 at ~0.75 puts a
/// full-facing surface at 0.95 and a 60°-off surface at 0.57 (both lit) while
/// grazing surfaces fall to the ambient 0.196 (shadow). Light 1 is the fill from
/// behind, deliberately small enough that it cannot push the shadow side over
/// the threshold on its own.
const LIGHT0_COLOR: Vec3 = Vec3::new(0.75, 0.735, 0.69);
const LIGHT1_COLOR: Vec3 = Vec3::new(0.22, 0.22, 0.26);
const LIGHT0_AZIMUTH: f32 = 0.6;
const LIGHT0_ELEVATION: f32 = 0.7;
/// Elevation clamp, just shy of straight up/down so the direction never
/// degenerates.
const MAX_ELEVATION: f32 = 1.4;

/// Light 0's orientation; light 1 is derived from it so all four keys move the
/// rig coherently.
struct LightRig {
    azimuth: f32,
    elevation: f32,
}

impl LightRig {
    /// `lightDir[i]` points **from the surface toward light i**, in world space.
    /// The shader does not negate — this is the one place the convention is
    /// established, and it is the classic sign-flip site.
    fn directions(&self) -> [Vec4; 2] {
        let dir = |az: f32, el: f32| {
            let d = Vec3::new(el.cos() * az.sin(), el.sin(), el.cos() * az.cos()).normalize();
            Vec4::new(d.x, d.y, d.z, 0.0)
        };
        [
            dir(self.azimuth, self.elevation),
            // the fill: opposite side, mirrored in elevation
            dir(self.azimuth + PI, -self.elevation * 0.5),
        ]
    }

    fn colors(&self) -> [Vec4; 2] {
        [
            Vec4::new(LIGHT0_COLOR.x, LIGHT0_COLOR.y, LIGHT0_COLOR.z, 1.0),
            Vec4::new(LIGHT1_COLOR.x, LIGHT1_COLOR.y, LIGHT1_COLOR.z, 1.0),
        ]
    }
}

impl Default for LightRig {
    fn default() -> Self {
        Self {
            azimuth: LIGHT0_AZIMUTH,
            elevation: LIGHT0_ELEVATION,
        }
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

fn raster_state(material: &MaterialEntry) -> anyhow::Result<RasterState> {
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

    Ok(RasterState {
        blend: blend_mode(material)?,
        cull,
        depth_test,
        depth_write,
        ..Default::default()
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
            // GX's dst-alpha blend, dst_alpha·src + (1−dst_alpha)·dst, reduces
            // *exactly* to src wherever the framebuffer alpha is 1 — and it is 1
            // at these pixels today: the clear is alpha 1.0, every opaque albedo
            // is alpha-255, and the four dst-alpha materials (eyeL/eyeR/mayuL/
            // mayuR) are the first translucent batches drawn, before anything
            // writes a non-1 alpha over the face. So Opaque is exact here.
            // PRECONDITION: a new albedo with alpha<255, a different draw order,
            // or --casual textures silently break this; real BlendMode::DstAlpha
            // lands with the eye write-mask pass in P9. See phase_07.md
            // decision 2 / risk 3.
            (DestinationAlpha, InverseDestinationAlpha) => return Ok(BlendMode::Opaque),
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
    /// `pipelines`. Everything except `mvp`, the two light fields and
    /// `debug_mode` is static, so `draw` patches those four and writes.
    params: Vec<ToonLinkParams>,
    /// Batches partitioned opaque-before-translucent, INF1 order within each
    /// group (J3D two-pass draw ordering). Walked by `draw` instead of the raw
    /// manifest order.
    draw_order: Vec<BatchIndex>,
    debug_mode: u32,
    isolate: Option<BatchIndex>,
    light: LightRig,
    intent: Intent,
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

    fn print_isolation(&self) {
        let Some(index) = self.isolate else {
            println!("isolation cleared: drawing all batches");
            return;
        };
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

impl ToonLink {
    fn set_debug_mode(&mut self, mode: u32) {
        self.debug_mode = mode % DEBUG_MODE_NAMES.len() as u32;
        println!(
            "debug mode {}: {}",
            self.debug_mode, DEBUG_MODE_NAMES[self.debug_mode as usize]
        );
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
    type EditState = ();

    fn window_title() -> &'static str {
        "Toon Link"
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
            let params_buffer = renderer.create_uniform_buffer::<ToonLinkParams>()?;
            let resources = Resources {
                tex0: resolve_texmap(material, 0, &textures, &white_square),
                tex1: resolve_texmap(material, 1, &textures, &white_square),
                params_buffer: &params_buffer,
            };
            let pipeline_config = Shader::init()
                .pipeline_config(resources)
                .with_shared_mesh(&mesh)
                .with_raster_state(raster_state(material)?);
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
                debug_mode: 0,
                _padding_0: [0; 8],
            });
        }

        // J3D two-pass order: opaque batches and then translucent ones,
        // both in INF1 scene graph order.
        let mut opaque = vec![];
        let mut translucent = vec![];
        for (i, batch) in manifest.batches.iter().enumerate() {
            let material = &manifest.materials[MaterialSlot::from_manifest(batch.material).raw()];
            match pe_mode(material)? {
                PeMode::Opaque => opaque.push(BatchIndex::from_raw(i)),
                PeMode::Translucent => translucent.push(BatchIndex::from_raw(i)),
            }
        }
        let draw_order: Vec<BatchIndex> = opaque.iter().chain(&translucent).copied().collect();

        // NOTE keep in sync with the module doc comment
        let modes: Vec<String> = DEBUG_MODE_NAMES
            .iter()
            .enumerate()
            .map(|(i, name)| format!("{i} {name}"))
            .collect();
        println!(
            "toon_link: {} batches, {} materials, {} vertices\n\
             draw order (batch idx): {:?}\n\
             controls:\n\
             \x20 W / A / S / D              rotate the light (held)\n\
             \x20 R / F                      next / previous debug mode\n\
             \x20 Num1..Num4                 jump to debug mode 0..3\n\
             \x20 Q / E                      isolate previous / next batch\n\
             \x20 Space                      clear isolation, draw all batches\n\
             debug modes: {}",
            manifest.batches.len(),
            manifest.materials.len(),
            manifest.buffers.vertex_count,
            draw_order.iter().map(|b| b.raw()).collect::<Vec<_>>(),
            modes.join(", "),
        );

        Ok(Self {
            start_time: Instant::now(),
            manifest,
            mesh,
            pipelines,
            params,
            draw_order,
            debug_mode: 0,
            isolate: None,
            light: LightRig::default(),
            intent: Intent::default(),
        })
    }

    fn update(&mut self) {
        let dt = self.frame_delay().as_secs_f32();
        let axis = |neg: bool, pos: bool| (pos as i32 - neg as i32) as f32 * LIGHT_SPIN * dt;
        self.light.azimuth =
            (self.light.azimuth + axis(self.intent.az_left, self.intent.az_right)).rem_euclid(TAU);
        self.light.elevation = (self.light.elevation
            + axis(self.intent.el_down, self.intent.el_up))
        .clamp(-MAX_ELEVATION, MAX_ELEVATION);
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = (Instant::now() - self.start_time).as_secs_f32();
        let orbit = elapsed * 20f32.to_radians();

        let model = Mat4::from_scale(Vec3::splat(MODEL_SCALE));
        let target = Vec3::new(0.0, 0.62, 0.0);
        let eye = target + Mat3::from_rotation_y(orbit) * Vec3::new(0.0, 0.25, 2.8);
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        let proj = Mat4::perspective_rh(45f32.to_radians(), renderer.aspect_ratio(), 0.1, 20.0);

        // one index-range draw per batch, in opaque-before-translucent order
        for &index in &self.draw_order {
            if self.isolate.is_some_and(|only| only != index) {
                continue;
            }
            let batch = self.batch(index);
            let pipeline = self.pipeline(MaterialSlot::from_manifest(batch.material));
            renderer.queue_draw_index_range(pipeline, batch.first_index, batch.index_count);
        }

        let mvp = MVPMatrices { model, view, proj };
        let debug_mode = self.debug_mode;
        let light_dir = self.light.directions();
        let light_color = self.light.colors();
        renderer.submit_draws(|gpu| {
            // Everything else in `params` was built once from the manifest.
            for ((_, params_buffer), params) in
                self.pipelines.iter_mut().zip(self.params.iter_mut())
            {
                params.mvp = mvp;
                params.tev.light_dir = light_dir;
                params.tev.light_color = light_color;
                params.debug_mode = debug_mode;
                gpu.write_uniform(params_buffer, *params);
            }
        })
    }

    fn input(&mut self, input: Input) {
        let batch_count = self.manifest.batches.len();
        match input {
            Input::KeyDown(key) => match key {
                // held: the light rig, integrated in `update`
                Key::A => self.intent.az_left = true,
                Key::D => self.intent.az_right = true,
                Key::W => self.intent.el_up = true,
                Key::S => self.intent.el_down = true,

                Key::R => self.set_debug_mode(self.debug_mode + 1),
                Key::F => self.set_debug_mode(self.debug_mode + DEBUG_MODE_NAMES.len() as u32 - 1),
                Key::Num1 => self.set_debug_mode(0),
                Key::Num2 => self.set_debug_mode(1),
                Key::Num3 => self.set_debug_mode(2),
                Key::Num4 => self.set_debug_mode(3),

                Key::Q => {
                    self.isolate = Some(match self.isolate {
                        None => BatchIndex::FIRST,
                        Some(index) => index.prev(batch_count),
                    });
                    self.print_isolation();
                }
                Key::E => {
                    self.isolate = Some(match self.isolate {
                        None => BatchIndex::FIRST,
                        Some(index) => index.next(batch_count),
                    });
                    self.print_isolation();
                }

                Key::Space => {
                    self.isolate = None;
                    self.print_isolation();
                }
            },

            Input::KeyUp(key) => match key {
                Key::A => self.intent.az_left = false,
                Key::D => self.intent.az_right = false,
                Key::W => self.intent.el_up = false,
                Key::S => self.intent.el_down = false,
                _ => {}
            },

            _ => {}
        }
    }
}
