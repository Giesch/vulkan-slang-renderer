//! Renders Toon Link from The Wind Waker — P6 of the link rendering plan
//! (`llm_notes/link_rendering.md`): all 24 batches drawn from one shared mesh
//! through 24 per-material pipelines, with a v0 debug fragment shader
//! (world-space normals as color, UVs as a second mode). Textures, TEV and
//! lighting are later phases.
//!
//! Requires converted assets on disk (gitignored — you need the disc image):
//! `just extract-link && just convert-link`.
//!
//! Controls:
//! - Num1 / Num2: normals-as-color / UV-as-color debug mode
//! - Q / E: isolate previous / next batch (prints which one to stdout)
//! - Space: clear isolation, draw all batches

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use glam::{Mat3, Mat4, Vec2, Vec3};

use vulkan_slang_renderer::game::{Game, Input, Key};
use vulkan_slang_renderer::model_manifest::{Manifest, MaterialEntry};
use vulkan_slang_renderer::renderer::{
    BlendMode, CullMode, DepthCompare, DrawError, DrawIndexed, FrameRenderer, MeshHandle,
    PipelineHandle, RasterState, Renderer, UniformBufferHandle,
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
    Ok(bytes
        .chunks_exact(VERTEX_STRIDE)
        .map(|v| Vertex {
            position: Vec3::new(f(v, 0), f(v, 1), f(v, 2)),
            normal: Vec3::new(f(v, 3), f(v, 4), f(v, 5)),
            uv0: Vec2::new(f(v, 6), f(v, 7)),
        })
        .collect())
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
    Ok(bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect())
}

fn raster_state(material: &MaterialEntry) -> anyhow::Result<RasterState> {
    let cull = match CULL_OVERRIDE {
        Some(cull) => cull,
        None => match material.cull.as_str() {
            "Cull_Back" => CullMode::Back,
            "Cull_None" => CullMode::None,
            "Cull_Front" => CullMode::Front,
            other => anyhow::bail!("unmapped GX cull mode {other:?}"),
        },
    };
    // Deliberately partial mapping: blend modes, alpha compare and the exact
    // Less_Equal depth function are P7. GX disables depth writes whenever the
    // compare is off, hence the paired Always/no-write.
    let (depth_test, depth_write) = if material.z_test {
        (DepthCompare::Less, true)
    } else {
        (DepthCompare::Always, false)
    };
    Ok(RasterState {
        blend: BlendMode::Opaque,
        cull,
        depth_test,
        depth_write,
        ..Default::default()
    })
}

pub struct ToonLink {
    start_time: Instant,
    manifest: Manifest,
    #[allow(unused)]
    mesh: MeshHandle<Vertex>,
    // indexed by material slot; batches reference slots 1:1 in cl.bdl
    pipelines: Vec<(
        PipelineHandle<DrawIndexed>,
        UniformBufferHandle<ToonLinkParams>,
    )>,
    debug_mode: u32,
    isolate: Option<usize>,
}

impl ToonLink {
    fn print_isolation(&self) {
        match self.isolate {
            Some(i) => {
                let batch = &self.manifest.batches[i];
                let material = &self.manifest.materials[batch.material as usize];
                println!(
                    "batch {i}: shape {} material {} {:?} [{}..+{}]",
                    batch.shape,
                    batch.material,
                    material.name,
                    batch.first_index,
                    batch.index_count
                );
            }
            None => println!("isolation cleared: drawing all batches"),
        }
    }
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

        // sized by the manifest's own counts, not hardcoded expectations
        let vertices = load_vertices(
            &dir.join(&manifest.buffers.vertices),
            manifest.buffers.vertex_count,
        )?;
        let indices = load_indices(
            &dir.join(&manifest.buffers.indices),
            manifest.buffers.index_count,
        )?;

        // honesty checks on the converter's own invariants
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
                (batch.material as usize) < manifest.materials.len(),
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

        let mesh = renderer.create_mesh(&vertices, &indices)?;

        let mut pipelines = vec![];
        for material in &manifest.materials {
            let params_buffer = renderer.create_uniform_buffer::<ToonLinkParams>()?;
            let resources = Resources {
                vertices: vec![],
                indices: vec![],
                params_buffer: &params_buffer,
            };
            let pipeline_config = Shader::init()
                .pipeline_config(resources)
                .with_shared_mesh(&mesh)
                .with_raster_state(raster_state(material)?);
            let pipeline = renderer.create_pipeline(pipeline_config)?;
            pipelines.push((pipeline, params_buffer));
        }

        Ok(Self {
            start_time: Instant::now(),
            manifest,
            mesh,
            pipelines,
            debug_mode: 0,
            isolate: None,
        })
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = (Instant::now() - self.start_time).as_secs_f32();
        let orbit = elapsed * 20f32.to_radians();

        let model = Mat4::from_scale(Vec3::splat(MODEL_SCALE));
        let target = Vec3::new(0.0, 0.62, 0.0);
        let eye = target + Mat3::from_rotation_y(orbit) * Vec3::new(0.0, 0.25, 2.8);
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        let proj = Mat4::perspective_rh(45f32.to_radians(), renderer.aspect_ratio(), 0.1, 20.0);

        // one index-range draw per batch, in INF1 (manifest) order
        for (i, batch) in self.manifest.batches.iter().enumerate() {
            if self.isolate.is_some_and(|only| only != i) {
                continue;
            }
            let (pipeline, _) = &self.pipelines[batch.material as usize];
            renderer.queue_draw_index_range(pipeline, batch.first_index, batch.index_count);
        }

        let mvp = MVPMatrices { model, view, proj };
        let debug_mode = self.debug_mode;
        renderer.submit_draws(|gpu| {
            for (_, params_buffer) in self.pipelines.iter_mut() {
                gpu.write_uniform(
                    params_buffer,
                    ToonLinkParams {
                        mvp: mvp.clone(),
                        debug_mode,
                        _padding_0: [0; 12],
                    },
                );
            }
        })
    }

    fn input(&mut self, input: Input) {
        let Input::KeyDown(key) = input else {
            return;
        };
        let batch_count = self.manifest.batches.len();
        match key {
            Key::Num1 => self.debug_mode = 0,
            Key::Num2 => self.debug_mode = 1,
            Key::Q => {
                self.isolate = Some(match self.isolate {
                    None => 0,
                    Some(i) => (i + batch_count - 1) % batch_count,
                });
                self.print_isolation();
            }
            Key::E => {
                self.isolate = Some(match self.isolate {
                    None => 0,
                    Some(i) => (i + 1) % batch_count,
                });
                self.print_isolation();
            }
            Key::Space => {
                self.isolate = None;
                self.print_isolation();
            }
            _ => {}
        }
    }
}
