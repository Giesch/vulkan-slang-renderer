//! Renders Toon Link from The Wind Waker — P7 of the link rendering plan
//! (`llm_notes/link_rendering.md`): all 24 batches drawn from one shared mesh
//! through 24 per-material pipelines, now with the model's **real albedo
//! textures**, alpha-cutout brows/lashes, complete per-material raster state
//! (cull + the exact `Less_Equal` depth func + honest `z_write` + blend), a
//! J3D opaque-before-translucent draw order, and gamma-correct output. TEV
//! stages, the lighting channel and ramp sampling are P8; `tex1` is bound but
//! never read.
//!
//! Requires converted assets on disk (gitignored — you need the disc image):
//! `just extract-link && just convert-link`.
//!
//! Controls live in the egui debug window (debug builds only):
//! - `debug_mode`: albedo / world-normals / uv0 / alpha-as-gray
//! - `isolate_batch` + `batch`: draw one batch instead of all of them
//! - `batch_info`: what the `batch` slider is currently pointing at

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use facet::Facet;
use glam::{Mat3, Mat4, UVec4, Vec2, Vec3};
use image::{DynamicImage, ImageReader, Rgba, RgbaImage};

use vulkan_slang_renderer::editor::{Checkbox, IntSlider, Label};
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
    /// Per-material alpha-compare codes, in `MaterialSlot` order (parallel to
    /// `pipelines`).
    alpha_compares: Vec<AlphaCompareCodes>,
    /// Batches partitioned opaque-before-translucent, INF1 order within each
    /// group (J3D two-pass draw ordering). Walked by `draw` instead of the raw
    /// manifest order.
    draw_order: Vec<BatchIndex>,
    edit_state: EditState,
}

/// The egui debug window, generated by reflection over these fields.
/// `debug_mode` is the shader's own generated enum, so its variants render as
/// radio buttons without a parallel list here to keep in sync.
#[derive(Facet)]
struct EditState {
    debug_mode: DebugMode,
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
        let mut alpha_compares = vec![];
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
            alpha_compares.push(alpha_compare_codes(material));
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

        println!(
            "toon_link: {} batches, {} materials, {} vertices\n\
             draw order (batch idx): {:?}\n\
             debug controls are in the egui window",
            manifest.batches.len(),
            manifest.materials.len(),
            manifest.buffers.vertex_count,
            draw_order.iter().map(|b| b.raw()).collect::<Vec<_>>(),
        );

        let last_batch = manifest.batches.len() as i64 - 1;
        let edit_state = EditState {
            debug_mode: DebugMode::Albedo,
            isolate_batch: Checkbox::new(false),
            batch: IntSlider::new(0, 0, last_batch),
            batch_info: Label::new(""),
        };

        let mut game = Self {
            start_time: Instant::now(),
            manifest,
            mesh,
            pipelines,
            alpha_compares,
            draw_order,
            edit_state,
        };
        game.update_batch_info();

        Ok(game)
    }

    fn update(&mut self) {
        self.update_batch_info();
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
        renderer.submit_draws(|gpu| {
            for ((_, params_buffer), codes) in self.pipelines.iter_mut().zip(&self.alpha_compares) {
                gpu.write_uniform(
                    params_buffer,
                    ToonLinkParams {
                        mvp,
                        alpha_compare: codes.compare,
                        alpha_compare_op: codes.op,
                        debug_mode,
                        _padding_0: [0; 8],
                    },
                );
            }
        })
    }
}
