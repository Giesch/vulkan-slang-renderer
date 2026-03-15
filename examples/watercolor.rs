//! A painting program based on the watercolor simulation described in this paper:
//! https://grail.cs.washington.edu/projects/watercolor/paper_small.pdf

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ash::vk;
use facet::Facet;
use glam::{Vec2, Vec3, Vec4};

use vulkan_slang_renderer::editor::{Label, Slider};
use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    Compute, DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer,
    StorageBufferHandle, StorageTextureHandle, TextureHandle, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::paint_brush_compute;
use vulkan_slang_renderer::generated::shader_atlas::paint_display;
use vulkan_slang_renderer::generated::shader_atlas::wc_advect_and_transfer_pigment_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_blur_v_and_flow_outward_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_capillary_flow_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_divergence_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_gaussian_blur_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_pressure_jacobi_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_project_velocity_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_update_velocity_compute;
use vulkan_slang_renderer::util::manifest_path;

fn main() -> Result<(), anyhow::Error> {
    Watercolor::run()
}

#[derive(Facet)]
pub struct EditState {
    fps: Label,
    brush_concentration: Slider,
    debug_view: DebugView,
}

#[derive(Default, Clone, Copy, Facet)]
#[repr(u32)]
enum DebugView {
    #[default]
    Pigments = 0,
    WetAreaMask = 1,
}

const FRAME_HISTORY_SIZE: usize = 60;

const CANVAS_WIDTH: u32 = 1024;
const CANVAS_HEIGHT: u32 = 768;
const MAX_STROKE_POINTS_PER_FRAME: u32 = 256;
const JACOBI_ITERATIONS: u32 = 2;
// NOTE this must be even for correctness when reading pressure in later stages
const _: () = assert!(JACOBI_ITERATIONS % 2 == 0);
const SIM_STEPS_PER_FRAME: u32 = 1;

const WORKGROUP_SIZE: u32 = 16;

// Simulation parameters
const DT: f32 = 0.5;
const MU: f32 = 0.1;
const KAPPA: f32 = 0.05;
const ETA: f32 = 0.03;
const SLOPE_STRENGTH: f32 = 5.0;
const BRUSH_PRESSURE: f32 = 2.0;
const TRANSFER_RATE: f32 = 0.02;
const DIFFUSE_RATE: f32 = 0.03;
const CAPILLARY_CAPACITY: f32 = 1.0;
const CAPILLARY_SIGMA: f32 = 0.3;
const DRY_THRESHOLD: f32 = 0.05;

/// Ping-pong pair: two storage textures + two sampled aliases
struct PingPong {
    storage: [StorageTextureHandle; 2],
    sampled: [TextureHandle; 2],
}

impl PingPong {
    fn read(&self, parity: bool) -> &TextureHandle {
        &self.sampled[parity as usize]
    }
    fn write(&self, parity: bool) -> &StorageTextureHandle {
        &self.storage[!parity as usize]
    }
    fn read_storage(&self, parity: bool) -> &StorageTextureHandle {
        &self.storage[parity as usize]
    }
}

fn create_ping_pong(renderer: &mut Renderer, format: vk::Format) -> anyhow::Result<PingPong> {
    let s0 = renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, format)?;
    let s1 = renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, format)?;
    renderer.clear_storage_texture(&s0, [0.0, 0.0, 0.0, 0.0])?;
    renderer.clear_storage_texture(&s1, [0.0, 0.0, 0.0, 0.0])?;
    let t0 = renderer.storage_texture_as_sampled(&s0)?;
    let t1 = renderer.storage_texture_as_sampled(&s1)?;
    Ok(PingPong {
        storage: [s0, s1],
        sampled: [t0, t1],
    })
}

fn create_deposit_texture(
    renderer: &mut Renderer,
) -> anyhow::Result<(StorageTextureHandle, TextureHandle)> {
    let storage = renderer.create_storage_texture(
        CANVAS_WIDTH,
        CANVAS_HEIGHT,
        vk::Format::R32G32B32A32_SFLOAT,
    )?;
    renderer.clear_storage_texture(&storage, [0.0, 0.0, 0.0, 0.0])?;
    let sampled = renderer.storage_texture_as_sampled(&storage)?;
    Ok((storage, sampled))
}

struct Watercolor {
    // Simulation textures (kept alive for GPU; not read on CPU)
    #[expect(unused)]
    velocity_u: PingPong,
    #[expect(unused)]
    velocity_v: PingPong,
    #[expect(unused)]
    pressure: PingPong,
    #[expect(unused)]
    pigment_0_3: PingPong,
    #[expect(unused)]
    pigment_4_7: PingPong,
    #[expect(unused)]
    pigment_8_11: PingPong,
    #[expect(unused)]
    saturation: PingPong,
    #[expect(unused)]
    wet_mask: PingPong,
    #[expect(unused)]
    deposit_0_3: [StorageTextureHandle; 2],
    #[expect(unused)]
    deposit_4_7: [StorageTextureHandle; 2],
    #[expect(unused)]
    deposit_8_11: [StorageTextureHandle; 2],
    #[expect(unused)]
    divergence: StorageTextureHandle,
    #[expect(unused)]
    blur_temp: StorageTextureHandle,

    // Pipelines
    brush_pipelines: [PipelineHandle<Compute>; 2],
    update_velocity_pipelines: [PipelineHandle<Compute>; 2],
    divergence_pipelines: [PipelineHandle<Compute>; 2],
    pressure_jacobi_pipelines: [PipelineHandle<Compute>; 2],
    project_velocity_pipelines: [PipelineHandle<Compute>; 2],
    blur_h_pipelines: [PipelineHandle<Compute>; 2],
    blur_v_and_flow_pipelines: [PipelineHandle<Compute>; 2],
    advect_and_transfer_pipelines: [PipelineHandle<Compute>; 4], // [sim_parity * 2 + deposit_parity]
    capillary_flow_pipelines: [PipelineHandle<Compute>; 2],
    display_pipelines: [PipelineHandle<DrawVertexCount>; 4], // [wet_mask_parity * 2 + deposit_parity]

    // Buffers
    stroke_points_buffer: StorageBufferHandle<paint_brush_compute::StrokePoint>,
    brush_params_buffer: UniformBufferHandle<paint_brush_compute::BrushParams>,
    display_params_buffer: UniformBufferHandle<paint_display::DisplayParams>,
    update_vel_params_buffer: UniformBufferHandle<wc_update_velocity_compute::Params>,
    divergence_params_buffer: UniformBufferHandle<wc_divergence_compute::Params>,
    pressure_jacobi_params_buffer: UniformBufferHandle<wc_pressure_jacobi_compute::Params>,
    project_vel_params_buffer: UniformBufferHandle<wc_project_velocity_compute::Params>,
    blur_h_params_buffer: UniformBufferHandle<wc_gaussian_blur_compute::Params>,
    blur_v_and_flow_params_buffer: UniformBufferHandle<wc_blur_v_and_flow_outward_compute::Params>,
    advect_and_transfer_params_buffer:
        UniformBufferHandle<wc_advect_and_transfer_pigment_compute::Params>,
    capillary_flow_params_buffer: UniformBufferHandle<wc_capillary_flow_compute::Params>,

    // Parity tracking
    pressure_parity: bool, // flips 20x per frame (net 0), used in Jacobi loop
    sim_parity: bool,      // pigment + wet_mask + saturation (all flip 1x per frame)
    deposit_parity: bool,  // deposit double-buffer parity (flips 1x per frame)

    // Input state
    painting: bool,
    stroke_points: Vec<Vec2>,
    prev_mouse_pos: Option<Vec2>,

    // Brush settings
    active_pigment: Pigment,
    brush_radius: f32,
    brush_opacity: f32,

    edit_state: EditState,

    // FPS tracking
    last_frame_time: Instant,
    frame_times: VecDeque<Duration>,
}

fn compute_barrier(renderer: &mut FrameRenderer) {
    renderer.memory_barrier(
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::AccessFlags::SHADER_WRITE,
        vk::AccessFlags::SHADER_READ,
    );
}

// Pigment data from Curtis et al. "Computer-Generated Watercolor" Figure 5 (a-l)
#[derive(Clone, Copy)]
#[repr(u32)]
enum Pigment {
    QuinacridoneRose = 0,   // a
    IndianRed = 1,          // b
    CadmiumYellow = 2,      // c
    HookersGreen = 3,       // d
    CeruleanBlue = 4,       // e
    BurntUmber = 5,         // f
    CadmiumRed = 6,         // g
    BrilliantOrange = 7,    // h
    HansaYellow = 8,        // i
    PhthaloGreen = 9,       // j
    FrenchUltramarine = 10, // k
    InterferenceLilac = 11, // l
}

struct PigmentData {
    // Kubelka-Munk K/S values (absorption/scattering, Section 5.1)
    absorption: Vec3,
    scattering: Vec3,
    // Physical properties (ρ, ω, γ)
    density: f32,
    staining_power: f32,
    granulation: f32,
}

const PIGMENT_TABLE: [PigmentData; 12] = [
    // a: Quinacridone Rose
    PigmentData {
        absorption: Vec3::new(0.22, 1.47, 0.57),
        scattering: Vec3::new(0.05, 0.003, 0.03),
        density: 0.02,
        staining_power: 5.5,
        granulation: 0.81,
    },
    // b: Indian Red
    PigmentData {
        absorption: Vec3::new(0.46, 1.07, 1.50),
        scattering: Vec3::new(1.28, 0.38, 0.21),
        density: 0.05,
        staining_power: 7.0,
        granulation: 0.40,
    },
    // c: Cadmium Yellow
    PigmentData {
        absorption: Vec3::new(0.10, 0.36, 3.45),
        scattering: Vec3::new(0.97, 0.65, 0.007),
        density: 0.05,
        staining_power: 3.4,
        granulation: 0.81,
    },
    // d: Hookers Green
    PigmentData {
        absorption: Vec3::new(1.62, 0.61, 1.64),
        scattering: Vec3::new(0.01, 0.012, 0.003),
        density: 0.09,
        staining_power: 1.0,
        granulation: 0.41,
    },
    // e: Cerulean Blue
    PigmentData {
        absorption: Vec3::new(1.52, 0.32, 0.25),
        scattering: Vec3::new(0.06, 0.26, 0.40),
        density: 0.01,
        staining_power: 1.0,
        granulation: 0.31,
    },
    // f: Burnt Umber
    PigmentData {
        absorption: Vec3::new(0.74, 1.54, 2.10),
        scattering: Vec3::new(0.09, 0.09, 0.004),
        density: 0.09,
        staining_power: 9.3,
        granulation: 0.90,
    },
    // g: Cadmium Red
    PigmentData {
        absorption: Vec3::new(0.14, 1.08, 1.68),
        scattering: Vec3::new(0.77, 0.015, 0.018),
        density: 0.02,
        staining_power: 1.0,
        granulation: 0.63,
    },
    // h: Brilliant Orange
    PigmentData {
        absorption: Vec3::new(0.13, 0.81, 3.45),
        scattering: Vec3::new(0.005, 0.009, 0.007),
        density: 0.01,
        staining_power: 1.0,
        granulation: 0.14,
    },
    // i: Hansa Yellow
    PigmentData {
        absorption: Vec3::new(0.06, 0.21, 1.78),
        scattering: Vec3::new(0.50, 0.88, 0.009),
        density: 0.06,
        staining_power: 1.0,
        granulation: 0.08,
    },
    // j: Phthalo Green
    PigmentData {
        absorption: Vec3::new(1.55, 0.47, 0.63),
        scattering: Vec3::new(0.01, 0.05, 0.035),
        density: 0.02,
        staining_power: 1.0,
        granulation: 0.12,
    },
    // k: French Ultramarine
    PigmentData {
        absorption: Vec3::new(0.86, 0.86, 0.06),
        scattering: Vec3::new(0.005, 0.005, 0.09),
        density: 0.01,
        staining_power: 3.1,
        granulation: 0.91,
    },
    // l: Interference Lilac
    PigmentData {
        absorption: Vec3::new(0.08, 0.11, 0.07),
        scattering: Vec3::new(1.25, 0.42, 1.43),
        density: 0.06,
        staining_power: 1.0,
        granulation: 0.08,
    },
];

impl Pigment {
    fn km(self) -> paint_display::PigmentKM {
        let d = &PIGMENT_TABLE[self as usize];
        paint_display::PigmentKM {
            absorption: d.absorption,
            _padding_0: Default::default(),
            scattering: d.scattering,
            _padding_1: Default::default(),
        }
    }

    fn properties(self) -> wc_advect_and_transfer_pigment_compute::PigmentProperties {
        let d = &PIGMENT_TABLE[self as usize];
        wc_advect_and_transfer_pigment_compute::PigmentProperties {
            density: d.density,
            staining_power: d.staining_power,
            granulation: d.granulation,
            _padding_0: Default::default(),
        }
    }

    fn group_index(self) -> usize {
        self as usize / 4
    }

    fn channel_index(self) -> usize {
        self as usize % 4
    }
}

impl Game for Watercolor {
    type EditState = EditState;

    fn window_title() -> &'static str {
        "Watercolor"
    }

    fn initial_window_size() -> (u32, u32) {
        (CANVAS_WIDTH, CANVAS_HEIGHT)
    }

    fn render_scale() -> Option<f32> {
        Some(1.0)
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self> {
        // Create all ping-pong textures
        let velocity_u = create_ping_pong(renderer, vk::Format::R32_SFLOAT)?;
        let velocity_v = create_ping_pong(renderer, vk::Format::R32_SFLOAT)?;
        let pressure = create_ping_pong(renderer, vk::Format::R32_SFLOAT)?;
        let pigment_0_3 = create_ping_pong(renderer, vk::Format::R32G32B32A32_SFLOAT)?;
        let pigment_4_7 = create_ping_pong(renderer, vk::Format::R32G32B32A32_SFLOAT)?;
        let pigment_8_11 = create_ping_pong(renderer, vk::Format::R32G32B32A32_SFLOAT)?;
        let saturation = create_ping_pong(renderer, vk::Format::R32_SFLOAT)?;
        let wet_mask = create_ping_pong(renderer, vk::Format::R32_SFLOAT)?;

        // Deposit textures (3 groups × 2 for double-buffering)
        let (deposit_0_3_a, deposit_0_3_a_sampled) = create_deposit_texture(renderer)?;
        let (deposit_0_3_b, deposit_0_3_b_sampled) = create_deposit_texture(renderer)?;
        let (deposit_4_7_a, deposit_4_7_a_sampled) = create_deposit_texture(renderer)?;
        let (deposit_4_7_b, deposit_4_7_b_sampled) = create_deposit_texture(renderer)?;
        let (deposit_8_11_a, deposit_8_11_a_sampled) = create_deposit_texture(renderer)?;
        let (deposit_8_11_b, deposit_8_11_b_sampled) = create_deposit_texture(renderer)?;
        let deposit_0_3_storage = [deposit_0_3_a, deposit_0_3_b];
        let deposit_0_3_sampled = [deposit_0_3_a_sampled, deposit_0_3_b_sampled];
        let deposit_4_7_storage = [deposit_4_7_a, deposit_4_7_b];
        let deposit_4_7_sampled = [deposit_4_7_a_sampled, deposit_4_7_b_sampled];
        let deposit_8_11_storage = [deposit_8_11_a, deposit_8_11_b];
        let deposit_8_11_sampled = [deposit_8_11_a_sampled, deposit_8_11_b_sampled];

        let divergence =
            renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, vk::Format::R32_SFLOAT)?;
        renderer.clear_storage_texture(&divergence, [0.0, 0.0, 0.0, 0.0])?;
        let divergence_sampled = renderer.storage_texture_as_sampled(&divergence)?;

        let blur_temp =
            renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, vk::Format::R32_SFLOAT)?;
        renderer.clear_storage_texture(&blur_temp, [0.0, 0.0, 0.0, 0.0])?;
        let blur_temp_sampled = renderer.storage_texture_as_sampled(&blur_temp)?;

        // Paper height map
        let paper_height =
            renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, vk::Format::R32_SFLOAT)?;
        let height_data = load_paper_height_map(CANVAS_WIDTH, CANVAS_HEIGHT);
        renderer.write_storage_texture(&paper_height, &height_data)?;
        let paper_height_sampled = renderer.storage_texture_as_sampled(&paper_height)?;

        // Create buffers
        let stroke_points_buffer = renderer
            .create_storage_buffer::<paint_brush_compute::StrokePoint>(
                MAX_STROKE_POINTS_PER_FRAME,
            )?;
        let brush_params_buffer =
            renderer.create_uniform_buffer::<paint_brush_compute::BrushParams>()?;
        let display_params_buffer =
            renderer.create_uniform_buffer::<paint_display::DisplayParams>()?;
        let update_vel_params_buffer =
            renderer.create_uniform_buffer::<wc_update_velocity_compute::Params>()?;
        let divergence_params_buffer =
            renderer.create_uniform_buffer::<wc_divergence_compute::Params>()?;
        let pressure_jacobi_params_buffer =
            renderer.create_uniform_buffer::<wc_pressure_jacobi_compute::Params>()?;
        let project_vel_params_buffer =
            renderer.create_uniform_buffer::<wc_project_velocity_compute::Params>()?;
        let blur_h_params_buffer =
            renderer.create_uniform_buffer::<wc_gaussian_blur_compute::Params>()?;
        let blur_v_and_flow_params_buffer =
            renderer.create_uniform_buffer::<wc_blur_v_and_flow_outward_compute::Params>()?;
        let advect_and_transfer_params_buffer =
            renderer.create_uniform_buffer::<wc_advect_and_transfer_pigment_compute::Params>()?;
        let capillary_flow_params_buffer =
            renderer.create_uniform_buffer::<wc_capillary_flow_compute::Params>()?;

        let shaders = ShaderAtlas::init();

        // --- Create pipelines ---
        // Brush pipeline: 2 variants for wet_mask/pigment parity
        let brush_pipelines = {
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(shaders.paint_brush_compute.pipeline_config(
                    paint_brush_compute::Resources {
                        wet_mask: wet_mask.read_storage(false),
                        pressure: pressure.read_storage(false),
                        pigment_0_3: pigment_0_3.read_storage(false),
                        pigment_4_7: pigment_4_7.read_storage(false),
                        pigment_8_11: pigment_8_11.read_storage(false),
                        saturation: saturation.read_storage(false),
                        stroke_points: &stroke_points_buffer,
                        brush_params_buffer: &brush_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.paint_brush_compute.pipeline_config(
                    paint_brush_compute::Resources {
                        wet_mask: wet_mask.read_storage(true),
                        pressure: pressure.read_storage(false), // pressure always at 0
                        pigment_0_3: pigment_0_3.read_storage(true),
                        pigment_4_7: pigment_4_7.read_storage(true),
                        pigment_8_11: pigment_8_11.read_storage(true),
                        saturation: saturation.read_storage(true),
                        stroke_points: &stroke_points_buffer,
                        brush_params_buffer: &brush_params_buffer,
                    },
                ))?,
            ]
        };

        // Update velocity: 2 pipelines for vel parity
        let update_velocity_pipelines = [
            renderer.create_compute_pipeline(
                shaders.wc_update_velocity_compute.pipeline_config(
                    wc_update_velocity_compute::Resources {
                        u_in: velocity_u.read(false),
                        v_in: velocity_v.read(false),
                        pressure: pressure.read(false),
                        paper_height: &paper_height_sampled,
                        wet_mask: wet_mask.read(false),
                        u_out: velocity_u.write(false),
                        v_out: velocity_v.write(false),
                        params_buffer: &update_vel_params_buffer,
                    },
                ),
            )?,
            // Need a second ShaderAtlas instance for the second pipeline
            {
                let shaders2 = ShaderAtlas::init();
                renderer.create_compute_pipeline(
                    shaders2.wc_update_velocity_compute.pipeline_config(
                        wc_update_velocity_compute::Resources {
                            u_in: velocity_u.read(true),
                            v_in: velocity_v.read(true),
                            pressure: pressure.read(false), // pressure always at index 0
                            paper_height: &paper_height_sampled,
                            wet_mask: wet_mask.read(true),
                            u_out: velocity_u.write(true),
                            v_out: velocity_v.write(true),
                            params_buffer: &update_vel_params_buffer,
                        },
                    ),
                )?
            },
        ];

        // Divergence: 2 pipelines for vel parity (reads from velocity after update)
        let divergence_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(s0.wc_divergence_compute.pipeline_config(
                    wc_divergence_compute::Resources {
                        u_in: velocity_u.read(true), // after vel flip
                        v_in: velocity_v.read(true),
                        divergence: &divergence,
                        params_buffer: &divergence_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_divergence_compute.pipeline_config(
                    wc_divergence_compute::Resources {
                        u_in: velocity_u.read(false),
                        v_in: velocity_v.read(false),
                        divergence: &divergence,
                        params_buffer: &divergence_params_buffer,
                    },
                ))?,
            ]
        };

        // Pressure Jacobi: 2 pipelines for pressure parity
        let pressure_jacobi_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(s0.wc_pressure_jacobi_compute.pipeline_config(
                    wc_pressure_jacobi_compute::Resources {
                        pressure_in: pressure.read(false),
                        divergence: &divergence_sampled,
                        pressure_out: pressure.write(false),
                        params_buffer: &pressure_jacobi_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_pressure_jacobi_compute.pipeline_config(
                    wc_pressure_jacobi_compute::Resources {
                        pressure_in: pressure.read(true),
                        divergence: &divergence_sampled,
                        pressure_out: pressure.write(true),
                        params_buffer: &pressure_jacobi_params_buffer,
                    },
                ))?,
            ]
        };

        // Project velocity: 2 pipelines for vel parity (reads pressure after Jacobi)
        let project_velocity_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(
                    s0.wc_project_velocity_compute.pipeline_config(
                        wc_project_velocity_compute::Resources {
                            u: velocity_u.read_storage(true),
                            v: velocity_v.read_storage(true),
                            pressure: pressure.read(false),
                            wet_mask: wet_mask.read(false),
                            params_buffer: &project_vel_params_buffer,
                        },
                    ),
                )?,
                renderer.create_compute_pipeline(
                    s1.wc_project_velocity_compute.pipeline_config(
                        wc_project_velocity_compute::Resources {
                            u: velocity_u.read_storage(false),
                            v: velocity_v.read_storage(false),
                            pressure: pressure.read(false),
                            wet_mask: wet_mask.read(true),
                            params_buffer: &project_vel_params_buffer,
                        },
                    ),
                )?,
            ]
        };

        // Gaussian blur H: wet_mask → blur_temp (2 pipelines for wet_mask parity)
        let blur_h_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(s0.wc_gaussian_blur_compute.pipeline_config(
                    wc_gaussian_blur_compute::Resources {
                        input_tex: wet_mask.read(false),
                        output_tex: &blur_temp,
                        params_buffer: &blur_h_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_gaussian_blur_compute.pipeline_config(
                    wc_gaussian_blur_compute::Resources {
                        input_tex: wet_mask.read(true),
                        output_tex: &blur_temp,
                        params_buffer: &blur_h_params_buffer,
                    },
                ))?,
            ]
        };

        // Blur V + Flow outward (fused): vertical blur of blur_temp + flow formula into pressure
        // 2 pipelines for wet_mask parity
        let blur_v_and_flow_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(
                    s0.wc_blur_v_and_flow_outward_compute.pipeline_config(
                        wc_blur_v_and_flow_outward_compute::Resources {
                            input_tex: &blur_temp_sampled,
                            wet_mask: wet_mask.read(false),
                            pressure: pressure.read_storage(false),
                            saturation: saturation.read_storage(false),
                            params_buffer: &blur_v_and_flow_params_buffer,
                        },
                    ),
                )?,
                renderer.create_compute_pipeline(
                    s1.wc_blur_v_and_flow_outward_compute.pipeline_config(
                        wc_blur_v_and_flow_outward_compute::Resources {
                            input_tex: &blur_temp_sampled,
                            wet_mask: wet_mask.read(true),
                            pressure: pressure.read_storage(false),
                            saturation: saturation.read_storage(true),
                            params_buffer: &blur_v_and_flow_params_buffer,
                        },
                    ),
                )?,
            ]
        };

        // Advect + transfer pigment: 4 pipelines for (sim_parity × deposit_parity)
        // Index: sim_parity * 2 + deposit_parity
        // Reads deposit[!deposit_parity], writes deposit[deposit_parity]
        let advect_and_transfer_pipelines = {
            let mut pipelines = Vec::with_capacity(4);
            for sim in [false, true] {
                for dep in [false, true] {
                    let s = ShaderAtlas::init();
                    let dep_read = !dep as usize; // read from previous
                    let dep_write = dep as usize; // write to current
                    pipelines.push(renderer.create_compute_pipeline(
                        s.wc_advect_and_transfer_pigment_compute.pipeline_config(
                            wc_advect_and_transfer_pigment_compute::Resources {
                                pigment_in_0_3: pigment_0_3.read(sim),
                                pigment_in_4_7: pigment_4_7.read(sim),
                                pigment_in_8_11: pigment_8_11.read(sim),
                                u_in: velocity_u.read(sim),
                                v_in: velocity_v.read(sim),
                                wet_mask: wet_mask.read(sim),
                                paper_height: &paper_height_sampled,
                                pigment_out_0_3: pigment_0_3.write(sim),
                                pigment_out_4_7: pigment_4_7.write(sim),
                                pigment_out_8_11: pigment_8_11.write(sim),
                                deposit_in_0_3: &deposit_0_3_sampled[dep_read],
                                deposit_in_4_7: &deposit_4_7_sampled[dep_read],
                                deposit_in_8_11: &deposit_8_11_sampled[dep_read],
                                deposit_out_0_3: &deposit_0_3_storage[dep_write],
                                deposit_out_4_7: &deposit_4_7_storage[dep_write],
                                deposit_out_8_11: &deposit_8_11_storage[dep_write],
                                params_buffer: &advect_and_transfer_params_buffer,
                            },
                        ),
                    )?);
                }
            }
            [
                pipelines.remove(0),
                pipelines.remove(0),
                pipelines.remove(0),
                pipelines.remove(0),
            ]
        };

        // Capillary flow: 2 pipelines for saturation parity
        let capillary_flow_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(s0.wc_capillary_flow_compute.pipeline_config(
                    wc_capillary_flow_compute::Resources {
                        saturation_in: saturation.read(false),
                        wet_mask_in: wet_mask.read(false),
                        paper_height: &paper_height_sampled,
                        saturation_out: saturation.write(false),
                        wet_mask_out: wet_mask.write(false),
                        params_buffer: &capillary_flow_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_capillary_flow_compute.pipeline_config(
                    wc_capillary_flow_compute::Resources {
                        saturation_in: saturation.read(true),
                        wet_mask_in: wet_mask.read(true),
                        paper_height: &paper_height_sampled,
                        saturation_out: saturation.write(true),
                        wet_mask_out: wet_mask.write(true),
                        params_buffer: &capillary_flow_params_buffer,
                    },
                ))?,
            ]
        };

        // Display pipeline: 4 variants for (wet_mask_parity × deposit_parity)
        // Index: wet_mask_parity * 2 + deposit_parity
        // Display reads deposit[!deposit_parity] (previous frame's output)
        let display_pipelines = {
            let mut pipelines = Vec::with_capacity(4);
            for wm in [false, true] {
                for dep in [false, true] {
                    let s = ShaderAtlas::init();
                    let dep_read = !dep as usize; // display reads previous frame's output
                    pipelines.push(renderer.create_pipeline(s.paint_display.pipeline_config(
                        paint_display::Resources {
                            deposit_0_3: &deposit_0_3_sampled[dep_read],
                            deposit_4_7: &deposit_4_7_sampled[dep_read],
                            deposit_8_11: &deposit_8_11_sampled[dep_read],
                            paper_height: &paper_height_sampled,
                            wet_mask: wet_mask.read(wm),
                            display_params_buffer: &display_params_buffer,
                        },
                    ))?);
                }
            }
            [
                pipelines.remove(0),
                pipelines.remove(0),
                pipelines.remove(0),
                pipelines.remove(0),
            ]
        };

        Ok(Self {
            velocity_u,
            velocity_v,
            pressure,
            pigment_0_3,
            pigment_4_7,
            pigment_8_11,
            saturation,
            wet_mask,
            deposit_0_3: deposit_0_3_storage,
            deposit_4_7: deposit_4_7_storage,
            deposit_8_11: deposit_8_11_storage,
            divergence,
            blur_temp,

            brush_pipelines,
            update_velocity_pipelines,
            divergence_pipelines,
            project_velocity_pipelines,
            pressure_jacobi_pipelines,
            blur_h_pipelines,
            blur_v_and_flow_pipelines,
            advect_and_transfer_pipelines,
            capillary_flow_pipelines,
            display_pipelines,

            stroke_points_buffer,
            brush_params_buffer,
            display_params_buffer,
            update_vel_params_buffer,
            divergence_params_buffer,
            pressure_jacobi_params_buffer,
            project_vel_params_buffer,
            blur_h_params_buffer,
            blur_v_and_flow_params_buffer,
            advect_and_transfer_params_buffer,
            capillary_flow_params_buffer,

            pressure_parity: false,
            sim_parity: false,
            deposit_parity: false,

            painting: false,
            stroke_points: Vec::new(),
            prev_mouse_pos: None,

            active_pigment: Pigment::QuinacridoneRose,
            brush_radius: 20.0,
            brush_opacity: 0.5,

            edit_state: EditState {
                fps: Label::new("FPS: --"),
                brush_concentration: Slider::new(0.3, 0.01, 1.0),
                debug_view: DebugView::Pigments,
            },
            last_frame_time: Instant::now(),
            frame_times: VecDeque::with_capacity(FRAME_HISTORY_SIZE),
        })
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

    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("Watercolor", &mut self.edit_state))
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::MouseDown {
                button: MouseButton::Left,
                x,
                y,
            } => {
                self.painting = true;
                let pos = Vec2::new(x, y);
                self.stroke_points.push(pos);
                self.prev_mouse_pos = Some(pos);
            }

            Input::MouseMotion { x, y } if self.painting => {
                let pos = Vec2::new(x, y);
                if let Some(prev) = self.prev_mouse_pos {
                    let spacing = self.brush_radius * 0.3;
                    let dist = prev.distance(pos);

                    if dist > spacing {
                        let steps = (dist / spacing).ceil() as u32;
                        for i in 1..=steps {
                            let t = i as f32 / steps as f32;
                            self.stroke_points.push(prev.lerp(pos, t));
                        }
                    } else if dist > 1.0 {
                        self.stroke_points.push(pos);
                    }
                }
                self.prev_mouse_pos = Some(pos);
            }

            Input::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                self.painting = false;
                self.prev_mouse_pos = None;
            }

            Input::KeyDown(key) => match key {
                // Row 1: 1-4 = pigments a-d
                Key::Num1 => self.active_pigment = Pigment::QuinacridoneRose,
                Key::Num2 => self.active_pigment = Pigment::IndianRed,
                Key::Num3 => self.active_pigment = Pigment::CadmiumYellow,
                Key::Num4 => self.active_pigment = Pigment::HookersGreen,
                // Row 2: QWER = pigments e-h
                Key::Q => self.active_pigment = Pigment::CeruleanBlue,
                Key::W => self.active_pigment = Pigment::BurntUmber,
                Key::E => self.active_pigment = Pigment::CadmiumRed,
                Key::R => self.active_pigment = Pigment::BrilliantOrange,
                // Row 3: ASDF = pigments i-l
                Key::A => self.active_pigment = Pigment::HansaYellow,
                Key::S => self.active_pigment = Pigment::PhthaloGreen,
                Key::D => self.active_pigment = Pigment::FrenchUltramarine,
                Key::F => self.active_pigment = Pigment::InterferenceLilac,
                _ => {}
            },

            _ => {}
        }
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        renderer.enable_pipelined_compute();
        let workgroup_x = (CANVAS_WIDTH + WORKGROUP_SIZE - 1) / WORKGROUP_SIZE;
        let workgroup_y = (CANVAS_HEIGHT + WORKGROUP_SIZE - 1) / WORKGROUP_SIZE;

        let stroke_points = std::mem::take(&mut self.stroke_points);
        let point_count = stroke_points
            .len()
            .min(MAX_STROKE_POINTS_PER_FRAME as usize) as u32;

        // 1. Brush input
        if point_count > 0 {
            renderer.dispatch(
                &self.brush_pipelines[self.sim_parity as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);
        }

        for _step in 0..SIM_STEPS_PER_FRAME {
            let sim = self.sim_parity;

            // 2. Update velocity (advection + forces)
            renderer.dispatch(
                &self.update_velocity_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 3. Divergence (reads velocity after update)
            renderer.dispatch(
                &self.divergence_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 4. Pressure Jacobi iterations (ping-pong pressure)
            for _ in 0..JACOBI_ITERATIONS {
                let p_idx = self.pressure_parity as usize;
                renderer.dispatch(
                    &self.pressure_jacobi_pipelines[p_idx],
                    workgroup_x,
                    workgroup_y,
                    1,
                );
                self.pressure_parity = !self.pressure_parity;
                compute_barrier(&mut renderer);
            }

            // 5. Project velocity
            renderer.dispatch(
                &self.project_velocity_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            // No barrier needed: project velocity writes u/v, blur H reads wet_mask — no hazard

            // 6. Gaussian blur H (wet_mask → blur_temp)
            renderer.dispatch(
                &self.blur_h_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 7. Blur V + Flow outward (fused: vertical blur of blur_temp → flow formula into pressure)
            renderer.dispatch(
                &self.blur_v_and_flow_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 9. Advect + transfer pigment (combined)
            let advect_idx = sim as usize * 2 + self.deposit_parity as usize;
            renderer.dispatch(
                &self.advect_and_transfer_pipelines[advect_idx],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 10. Capillary flow (saturation + wet_mask at sim → writes to !sim)
            renderer.dispatch(
                &self.capillary_flow_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );

            // Flip simulation parity
            self.sim_parity = !self.sim_parity;
            self.deposit_parity = !self.deposit_parity;

            // Pipelined: graphics reads previous frame's results, so no compute→frag
            // barrier needed. A compute→compute barrier suffices for next frame's reads.
            compute_barrier(&mut renderer);
        }

        // 12. Display
        let grid_size = Vec2::new(CANVAS_WIDTH as f32, CANVAS_HEIGHT as f32);
        let texel_size = Vec2::new(1.0 / CANVAS_WIDTH as f32, 1.0 / CANVAS_HEIGHT as f32);
        let window_size = renderer.window_resolution();

        let active_pigment = self.active_pigment;
        let brush_radius = self.brush_radius;
        let brush_opacity = self.brush_opacity;

        let brush_params_buffer = &mut self.brush_params_buffer;
        let stroke_points_buffer = &mut self.stroke_points_buffer;
        let display_params_buffer = &mut self.display_params_buffer;
        let update_vel_params_buffer = &mut self.update_vel_params_buffer;
        let divergence_params_buffer = &mut self.divergence_params_buffer;
        let pressure_jacobi_params_buffer = &mut self.pressure_jacobi_params_buffer;
        let project_vel_params_buffer = &mut self.project_vel_params_buffer;
        let blur_h_params_buffer = &mut self.blur_h_params_buffer;
        let blur_v_and_flow_params_buffer = &mut self.blur_v_and_flow_params_buffer;
        let advect_and_transfer_params_buffer = &mut self.advect_and_transfer_params_buffer;
        let capillary_flow_params_buffer = &mut self.capillary_flow_params_buffer;

        let display_idx = self.sim_parity as usize * 2 + self.deposit_parity as usize;
        renderer.draw_vertex_count(&self.display_pipelines[display_idx], 3, |gpu| {
            // Upload stroke points
            if point_count > 0 {
                let gpu_points: Vec<paint_brush_compute::StrokePoint> = stroke_points
                    [..point_count as usize]
                    .iter()
                    .map(|&position| {
                        let canvas_pos = position * grid_size / window_size;
                        paint_brush_compute::StrokePoint {
                            position: canvas_pos,
                        }
                    })
                    .collect();
                gpu.write_storage(stroke_points_buffer, &gpu_points);
            }

            // Pigment color: concentration in the active group/channel
            let mut pigment_color_0_3 = Vec4::ZERO;
            let mut pigment_color_4_7 = Vec4::ZERO;
            let mut pigment_color_8_11 = Vec4::ZERO;
            let c = self.edit_state.brush_concentration.value;
            let group_colors = [
                &mut pigment_color_0_3,
                &mut pigment_color_4_7,
                &mut pigment_color_8_11,
            ];
            group_colors[active_pigment.group_index()][active_pigment.channel_index()] = c;

            gpu.write_uniform(
                brush_params_buffer,
                paint_brush_compute::BrushParams {
                    point_count,
                    brush_radius,
                    brush_opacity,
                    brush_pressure: BRUSH_PRESSURE,
                    pigment_color_0_3,
                    pigment_color_4_7,
                    pigment_color_8_11,
                    canvas_size: grid_size,
                    _padding_0: Default::default(),
                },
            );

            gpu.write_uniform(
                update_vel_params_buffer,
                wc_update_velocity_compute::Params {
                    grid_size,
                    texel_size,
                    dt: DT,
                    mu: MU,
                    kappa: KAPPA,
                    slope_strength: SLOPE_STRENGTH,
                },
            );

            gpu.write_uniform(
                divergence_params_buffer,
                wc_divergence_compute::Params {
                    grid_size,
                    _padding_0: Default::default(),
                },
            );

            gpu.write_uniform(
                pressure_jacobi_params_buffer,
                wc_pressure_jacobi_compute::Params {
                    grid_size,
                    _padding_0: Default::default(),
                },
            );

            gpu.write_uniform(
                project_vel_params_buffer,
                wc_project_velocity_compute::Params {
                    grid_size,
                    _padding_0: Default::default(),
                },
            );

            gpu.write_uniform(
                blur_h_params_buffer,
                wc_gaussian_blur_compute::Params {
                    grid_size,
                    direction: Vec2::new(1.0, 0.0),
                },
            );

            gpu.write_uniform(
                blur_v_and_flow_params_buffer,
                wc_blur_v_and_flow_outward_compute::Params {
                    grid_size,
                    eta: ETA,
                    _padding_0: Default::default(),
                },
            );

            gpu.write_uniform(
                advect_and_transfer_params_buffer,
                wc_advect_and_transfer_pigment_compute::Params {
                    grid_size,
                    dt: DT,
                    transfer_rate: TRANSFER_RATE,
                    pigment0: Pigment::QuinacridoneRose.properties(),
                    pigment1: Pigment::IndianRed.properties(),
                    pigment2: Pigment::CadmiumYellow.properties(),
                    pigment3: Pigment::HookersGreen.properties(),
                    pigment4: Pigment::CeruleanBlue.properties(),
                    pigment5: Pigment::BurntUmber.properties(),
                    pigment6: Pigment::CadmiumRed.properties(),
                    pigment7: Pigment::BrilliantOrange.properties(),
                    pigment8: Pigment::HansaYellow.properties(),
                    pigment9: Pigment::PhthaloGreen.properties(),
                    pigment10: Pigment::FrenchUltramarine.properties(),
                    pigment11: Pigment::InterferenceLilac.properties(),
                },
            );

            gpu.write_uniform(
                capillary_flow_params_buffer,
                wc_capillary_flow_compute::Params {
                    grid_size,
                    diffuse_rate: DIFFUSE_RATE,
                    capacity: CAPILLARY_CAPACITY,
                    sigma: CAPILLARY_SIGMA,
                    dry_threshold: DRY_THRESHOLD,
                    _padding_0: Default::default(),
                },
            );

            gpu.write_uniform(
                display_params_buffer,
                paint_display::DisplayParams {
                    texel_size,
                    debug_view: self.edit_state.debug_view as u32,
                    _padding_0: Default::default(),
                    pigment0: Pigment::QuinacridoneRose.km(),
                    pigment1: Pigment::IndianRed.km(),
                    pigment2: Pigment::CadmiumYellow.km(),
                    pigment3: Pigment::HookersGreen.km(),
                    pigment4: Pigment::CeruleanBlue.km(),
                    pigment5: Pigment::BurntUmber.km(),
                    pigment6: Pigment::CadmiumRed.km(),
                    pigment7: Pigment::BrilliantOrange.km(),
                    pigment8: Pigment::HansaYellow.km(),
                    pigment9: Pigment::PhthaloGreen.km(),
                    pigment10: Pigment::FrenchUltramarine.km(),
                    pigment11: Pigment::InterferenceLilac.km(),
                },
            );
        })?;

        Ok(())
    }
}

fn load_paper_height_map(width: u32, height: u32) -> Vec<f32> {
    let path = manifest_path(["textures", "watercolor", "paper_height.png"]);
    let img = image::open(&path).expect("missing paper texture — run `just paper-texture`");
    let gray = img.to_luma8();
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(gray.get_pixel(x, y).0[0] as f32 / 255.0);
        }
    }
    data
}
