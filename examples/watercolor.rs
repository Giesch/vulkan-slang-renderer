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
use vulkan_slang_renderer::util::manifest_path;

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
const JACOBI_ITERATIONS: u32 = 10;
// NOTE this must be even for parity correctness when reading pressure in later stages
const _: () = assert!(JACOBI_ITERATIONS % 2 == 0);
const SIM_STEPS_PER_FRAME: u32 = 1;

const WORKGROUP_SIZE: u32 = 16;

/// Δt — simulation time step for the shallow-water equations (Section 4.1).
const DT: f32 = 0.5;
/// μ — viscous drag coefficient that damps fluid velocity over time (Section 4.1, Eq. 1).
const MU: f32 = 0.1;
/// κ — diffusion constant controlling how pressure spreads water across the paper (Section 4.2).
const KAPPA: f32 = 0.05;
/// η — evaporation rate at which water leaves the shallow layer each time step (Section 4.4).
const ETA: f32 = 0.03;
/// Strength of the gravitational slope term that drives water flow on a tilted canvas (Section 4.1).
const SLOPE_STRENGTH: f32 = 5.0;
/// Multiplier for water deposited by the brush onto the shallow layer (Section 3, brush model).
const BRUSH_PRESSURE: f32 = 2.0;
/// Rate of pigment adsorption from the shallow water layer into the paper fibers (Section 5.2).
const TRANSFER_RATE: f32 = 0.02;
/// Rate of pigment diffusion within the shallow water layer (Section 5.1).
const DIFFUSE_RATE: f32 = 0.03;
/// Maximum water capacity of the paper's capillary layer (Section 4.3).
const CAPILLARY_CAPACITY: f32 = 1.0;
/// σ — controls the saturation-dependent spread of capillary flow between cells (Section 4.3).
const CAPILLARY_SIGMA: f32 = 0.3;
/// Water height below which a cell is considered dry and flow/pigment transfer stops.
const DRY_THRESHOLD: f32 = 0.05;
/// Outward velocity strength injected at wet/dry edges after projection.
/// Drives pigment toward stroke edges, producing the edge-darkening effect from Curtis et al.
const OUTWARD_STRENGTH: f32 = 30.0;

/// Ping-pong pair: two storage textures + two sampled aliases
struct PingPong {
    storage: [StorageTextureHandle; 2],
    sampled: [TextureHandle; 2],
}

impl PingPong {
    fn read_sampled(&self, parity: bool) -> &TextureHandle {
        &self.sampled[parity as usize]
    }

    fn write_storage(&self, parity: bool) -> &StorageTextureHandle {
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

struct Watercolor {
    // Simulation textures (kept alive for GPU; not read on CPU)
    #[expect(unused)]
    velocity_u: PingPong,
    #[expect(unused)]
    velocity_v: PingPong,
    #[expect(unused)]
    pressure: PingPong,
    #[expect(unused)]
    pigment: PingPong,
    #[expect(unused)]
    saturation: PingPong,
    #[expect(unused)]
    wet_mask: PingPong,
    #[expect(unused)]
    deposit: StorageTextureHandle,
    #[expect(unused)]
    divergence: StorageTextureHandle,
    #[expect(unused)]
    blur_temp: StorageTextureHandle,
    #[expect(unused)]
    blurred_mask: StorageTextureHandle,

    // Pipelines
    brush_pipelines: [PipelineHandle<Compute>; 2],
    update_velocity_pipelines: [PipelineHandle<Compute>; 2],
    divergence_pipelines: [PipelineHandle<Compute>; 2],
    pressure_jacobi_pipelines: [PipelineHandle<Compute>; 2],
    project_velocity_pipelines: [PipelineHandle<Compute>; 2],
    blur_h_pipelines: [PipelineHandle<Compute>; 2],
    blur_v_and_flow_pipelines: [PipelineHandle<Compute>; 2],
    advect_and_transfer_pipelines: [PipelineHandle<Compute>; 2],
    capillary_flow_pipelines: [PipelineHandle<Compute>; 2],
    display_pipelines: [PipelineHandle<DrawVertexCount>; 2],

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

    // Input state
    painting: bool,
    stroke_points: Vec<Vec2>,
    prev_mouse_pos: Option<Vec2>,

    // Brush settings
    active_pigment: Pigment,
    brush_radius: f32,
    brush_opacity: f32,

    edit_state: EditState,

    // Frame counter for temporal effects
    frame_counter: u32,

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

fn compute_to_frag_barrier(renderer: &mut FrameRenderer) {
    renderer.memory_barrier(
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::PipelineStageFlags::FRAGMENT_SHADER,
        vk::AccessFlags::SHADER_WRITE,
        vk::AccessFlags::SHADER_READ,
    );
}

// Pigment data from Curtis et al. "Computer-Generated Watercolor" Figure 5
#[derive(Clone, Copy)]
#[repr(u32)]
enum Pigment {
    FrenchUltramarine = 0,
    HansaYellow = 1,
    QuinacridoneRose = 2,
    HookersGreen = 3,
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

const PIGMENT_TABLE: [PigmentData; 4] = [
    // French Ultramarine (k)
    PigmentData {
        absorption: Vec3::new(0.86, 0.86, 0.06),
        scattering: Vec3::new(0.005, 0.005, 0.09),
        density: 0.01,
        staining_power: 3.1,
        granulation: 0.91,
    },
    // Hansa Yellow (i)
    PigmentData {
        absorption: Vec3::new(0.06, 0.21, 1.78),
        scattering: Vec3::new(0.50, 0.88, 0.009),
        density: 0.06,
        staining_power: 1.0,
        granulation: 0.08,
    },
    // Quinacridone Rose (a)
    PigmentData {
        absorption: Vec3::new(0.22, 1.47, 0.57),
        scattering: Vec3::new(0.05, 0.003, 0.03),
        density: 0.02,
        staining_power: 5.5,
        granulation: 0.81,
    },
    // Hookers Green (d)
    PigmentData {
        absorption: Vec3::new(1.62, 0.61, 1.64),
        scattering: Vec3::new(0.01, 0.012, 0.003),
        density: 0.09,
        staining_power: 1.0,
        granulation: 0.41,
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

    fn channel_index(self) -> usize {
        self as usize
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
        let pigment = create_ping_pong(renderer, vk::Format::R32G32B32A32_SFLOAT)?;
        let saturation = create_ping_pong(renderer, vk::Format::R32_SFLOAT)?;
        let wet_mask = create_ping_pong(renderer, vk::Format::R32_SFLOAT)?;

        // Single textures
        let deposit = renderer.create_storage_texture(
            CANVAS_WIDTH,
            CANVAS_HEIGHT,
            vk::Format::R32G32B32A32_SFLOAT,
        )?;
        renderer.clear_storage_texture(&deposit, [0.0, 0.0, 0.0, 0.0])?;
        let deposit_sampled = renderer.storage_texture_as_sampled(&deposit)?;

        let divergence =
            renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, vk::Format::R32_SFLOAT)?;
        renderer.clear_storage_texture(&divergence, [0.0, 0.0, 0.0, 0.0])?;
        let divergence_sampled = renderer.storage_texture_as_sampled(&divergence)?;

        let blur_temp =
            renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, vk::Format::R32_SFLOAT)?;
        renderer.clear_storage_texture(&blur_temp, [0.0, 0.0, 0.0, 0.0])?;
        let blur_temp_sampled = renderer.storage_texture_as_sampled(&blur_temp)?;

        let blurred_mask =
            renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, vk::Format::R32_SFLOAT)?;
        renderer.clear_storage_texture(&blurred_mask, [0.0, 0.0, 0.0, 0.0])?;
        let blurred_mask_sampled = renderer.storage_texture_as_sampled(&blurred_mask)?;

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
                        pigment: pigment.read_storage(false),
                        saturation: saturation.read_storage(false),
                        stroke_points: &stroke_points_buffer,
                        brush_params_buffer: &brush_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.paint_brush_compute.pipeline_config(
                    paint_brush_compute::Resources {
                        wet_mask: wet_mask.read_storage(true),
                        pressure: pressure.read_storage(false), // pressure always at 0
                        pigment: pigment.read_storage(true),
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
                        u_in: velocity_u.read_sampled(false),
                        v_in: velocity_v.read_sampled(false),
                        pressure: pressure.read_sampled(false),
                        paper_height: &paper_height_sampled,
                        wet_mask: wet_mask.read_sampled(false),
                        u_out: velocity_u.write_storage(false),
                        v_out: velocity_v.write_storage(false),
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
                            u_in: velocity_u.read_sampled(true),
                            v_in: velocity_v.read_sampled(true),
                            pressure: pressure.read_sampled(false), // pressure always at index 0
                            paper_height: &paper_height_sampled,
                            wet_mask: wet_mask.read_sampled(true),
                            u_out: velocity_u.write_storage(true),
                            v_out: velocity_v.write_storage(true),
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
                        u_in: velocity_u.read_sampled(true), // after vel flip
                        v_in: velocity_v.read_sampled(true),
                        divergence: &divergence,
                        params_buffer: &divergence_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_divergence_compute.pipeline_config(
                    wc_divergence_compute::Resources {
                        u_in: velocity_u.read_sampled(false),
                        v_in: velocity_v.read_sampled(false),
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
                        pressure_in: pressure.read_sampled(false),
                        divergence: &divergence_sampled,
                        pressure_out: pressure.write_storage(false),
                        params_buffer: &pressure_jacobi_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_pressure_jacobi_compute.pipeline_config(
                    wc_pressure_jacobi_compute::Resources {
                        pressure_in: pressure.read_sampled(true),
                        divergence: &divergence_sampled,
                        pressure_out: pressure.write_storage(true),
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
                            pressure: pressure.read_sampled(false),
                            wet_mask: wet_mask.read_sampled(false),
                            blurred_mask: &blurred_mask_sampled,
                            params_buffer: &project_vel_params_buffer,
                        },
                    ),
                )?,
                renderer.create_compute_pipeline(
                    s1.wc_project_velocity_compute.pipeline_config(
                        wc_project_velocity_compute::Resources {
                            u: velocity_u.read_storage(false),
                            v: velocity_v.read_storage(false),
                            pressure: pressure.read_sampled(false),
                            wet_mask: wet_mask.read_sampled(true),
                            blurred_mask: &blurred_mask_sampled,
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
                        input_tex: wet_mask.read_sampled(false),
                        output_tex: &blur_temp,
                        params_buffer: &blur_h_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_gaussian_blur_compute.pipeline_config(
                    wc_gaussian_blur_compute::Resources {
                        input_tex: wet_mask.read_sampled(true),
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
                            wet_mask: wet_mask.read_sampled(false),
                            pressure: pressure.read_storage(false),
                            saturation: saturation.read_storage(false),
                            blurred_mask_out: &blurred_mask,
                            params_buffer: &blur_v_and_flow_params_buffer,
                        },
                    ),
                )?,
                renderer.create_compute_pipeline(
                    s1.wc_blur_v_and_flow_outward_compute.pipeline_config(
                        wc_blur_v_and_flow_outward_compute::Resources {
                            input_tex: &blur_temp_sampled,
                            wet_mask: wet_mask.read_sampled(true),
                            pressure: pressure.read_storage(false),
                            saturation: saturation.read_storage(true),
                            blurred_mask_out: &blurred_mask,
                            params_buffer: &blur_v_and_flow_params_buffer,
                        },
                    ),
                )?,
            ]
        };

        // Advect + transfer pigment: 2 pipelines for pigment parity
        // Reads pigment at sim parity, writes to !sim parity, plus deposit RW
        let advect_and_transfer_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(
                    s0.wc_advect_and_transfer_pigment_compute.pipeline_config(
                        wc_advect_and_transfer_pigment_compute::Resources {
                            pigment_in: pigment.read_sampled(false),
                            u_in: velocity_u.read_sampled(false),
                            v_in: velocity_v.read_sampled(false),
                            wet_mask: wet_mask.read_sampled(false),
                            paper_height: &paper_height_sampled,
                            saturation: saturation.read_sampled(false),
                            pigment_out: pigment.write_storage(false),
                            deposit: &deposit,
                            params_buffer: &advect_and_transfer_params_buffer,
                        },
                    ),
                )?,
                renderer.create_compute_pipeline(
                    s1.wc_advect_and_transfer_pigment_compute.pipeline_config(
                        wc_advect_and_transfer_pigment_compute::Resources {
                            pigment_in: pigment.read_sampled(true),
                            u_in: velocity_u.read_sampled(true),
                            v_in: velocity_v.read_sampled(true),
                            wet_mask: wet_mask.read_sampled(true),
                            paper_height: &paper_height_sampled,
                            saturation: saturation.read_sampled(true),
                            pigment_out: pigment.write_storage(true),
                            deposit: &deposit,
                            params_buffer: &advect_and_transfer_params_buffer,
                        },
                    ),
                )?,
            ]
        };

        // Capillary flow: 2 pipelines for saturation parity
        let capillary_flow_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(s0.wc_capillary_flow_compute.pipeline_config(
                    wc_capillary_flow_compute::Resources {
                        saturation_in: saturation.read_sampled(false),
                        wet_mask_in: wet_mask.read_sampled(false),
                        paper_height: &paper_height_sampled,
                        saturation_out: saturation.write_storage(false),
                        wet_mask_out: wet_mask.write_storage(false),
                        params_buffer: &capillary_flow_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_capillary_flow_compute.pipeline_config(
                    wc_capillary_flow_compute::Resources {
                        saturation_in: saturation.read_sampled(true),
                        wet_mask_in: wet_mask.read_sampled(true),
                        paper_height: &paper_height_sampled,
                        saturation_out: saturation.write_storage(true),
                        wet_mask_out: wet_mask.write_storage(true),
                        params_buffer: &capillary_flow_params_buffer,
                    },
                ))?,
            ]
        };

        // Display pipeline: 2 variants for wet_mask parity
        let display_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_pipeline(s0.paint_display.pipeline_config(
                    paint_display::Resources {
                        deposit: &deposit_sampled,
                        paper_height: &paper_height_sampled,
                        wet_mask: wet_mask.read_sampled(false),
                        display_params_buffer: &display_params_buffer,
                    },
                ))?,
                renderer.create_pipeline(s1.paint_display.pipeline_config(
                    paint_display::Resources {
                        deposit: &deposit_sampled,
                        paper_height: &paper_height_sampled,
                        wet_mask: wet_mask.read_sampled(true),
                        display_params_buffer: &display_params_buffer,
                    },
                ))?,
            ]
        };

        Ok(Self {
            velocity_u,
            velocity_v,
            pressure,
            pigment,
            saturation,
            wet_mask,
            deposit,
            divergence,
            blur_temp,
            blurred_mask,

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

            painting: false,
            stroke_points: Vec::new(),
            prev_mouse_pos: None,

            active_pigment: Pigment::FrenchUltramarine,
            brush_radius: 20.0,
            brush_opacity: 0.5,

            edit_state: EditState {
                fps: Label::new("FPS: --"),
                brush_concentration: Slider::new(0.3, 0.01, 1.0),
                debug_view: DebugView::Pigments,
            },
            frame_counter: 0,
            last_frame_time: Instant::now(),
            frame_times: VecDeque::with_capacity(FRAME_HISTORY_SIZE),
        })
    }

    fn update(&mut self) {
        self.frame_counter = self.frame_counter.wrapping_add(1);

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
                Key::Num1 => self.active_pigment = Pigment::FrenchUltramarine,
                Key::Num2 => self.active_pigment = Pigment::HansaYellow,
                Key::Num3 => self.active_pigment = Pigment::QuinacridoneRose,
                Key::Num4 => self.active_pigment = Pigment::HookersGreen,
                _ => {}
            },

            _ => {}
        }
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
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

        for step in 0..SIM_STEPS_PER_FRAME {
            let sim = self.sim_parity;

            // 2. Gaussian blur H (wet_mask → blur_temp)
            renderer.dispatch(
                &self.blur_h_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 3. Blur V + Flow outward (fused: vertical blur of blur_temp → blurred_mask + flow into pressure)
            renderer.dispatch(
                &self.blur_v_and_flow_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 4. Update velocity (advection + forces)
            renderer.dispatch(
                &self.update_velocity_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 5. Divergence (reads velocity after update)
            renderer.dispatch(
                &self.divergence_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 6. Pressure Jacobi iterations (ping-pong pressure)
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

            // 7. Project velocity
            renderer.dispatch(
                &self.project_velocity_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 8. Advect + transfer pigment (combined)
            renderer.dispatch(
                &self.advect_and_transfer_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 9. Capillary flow (saturation + wet_mask at sim → writes to !sim)
            renderer.dispatch(
                &self.capillary_flow_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );

            // Flip simulation parity
            self.sim_parity = !self.sim_parity;

            // Use compute→frag barrier only after the last step; compute barrier otherwise
            if step == SIM_STEPS_PER_FRAME - 1 {
                compute_to_frag_barrier(&mut renderer);
            } else {
                compute_barrier(&mut renderer);
            }
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

        renderer.draw_vertex_count(
            &self.display_pipelines[self.sim_parity as usize],
            3,
            |gpu| {
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

                // Pigment color: concentration in the active channel
                let mut pigment_color = Vec4::ZERO;
                let c = self.edit_state.brush_concentration.value;
                pigment_color[active_pigment.channel_index()] = c;

                gpu.write_uniform(
                    brush_params_buffer,
                    paint_brush_compute::BrushParams {
                        point_count,
                        brush_radius,
                        brush_opacity,
                        brush_pressure: BRUSH_PRESSURE,
                        pigment_color,
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
                        outward_strength: OUTWARD_STRENGTH,
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
                        pigment0: Pigment::FrenchUltramarine.properties(),
                        pigment1: Pigment::HansaYellow.properties(),
                        pigment2: Pigment::QuinacridoneRose.properties(),
                        pigment3: Pigment::HookersGreen.properties(),
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
                        frame_index: self.frame_counter,
                        _padding_0: Default::default(),
                    },
                );

                gpu.write_uniform(
                    display_params_buffer,
                    paint_display::DisplayParams {
                        texel_size,
                        debug_view: self.edit_state.debug_view as u32,
                        _padding_0: Default::default(),
                        pigment0: Pigment::FrenchUltramarine.km(),
                        pigment1: Pigment::HansaYellow.km(),
                        pigment2: Pigment::QuinacridoneRose.km(),
                        pigment3: Pigment::HookersGreen.km(),
                    },
                );
            },
        )?;

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
