use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ash::vk;
use facet::Facet;
use glam::{Vec2, Vec3, Vec4};

use vulkan_slang_renderer::editor::Label;
use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    Compute, DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer,
    StorageBufferHandle, StorageTextureHandle, TextureHandle, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::paint_brush_compute;
use vulkan_slang_renderer::generated::shader_atlas::paint_display;
use vulkan_slang_renderer::generated::shader_atlas::wc_capillary_flow_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_divergence_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_flow_outward_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_gaussian_blur_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_move_pigment_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_pressure_jacobi_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_project_velocity_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_transfer_pigment_compute;
use vulkan_slang_renderer::generated::shader_atlas::wc_update_velocity_compute;

fn main() -> Result<(), anyhow::Error> {
    Watercolor::run()
}

#[derive(Facet)]
pub struct EditState {
    fps: Label,
}

const FRAME_HISTORY_SIZE: usize = 60;

const CANVAS_WIDTH: u32 = 1024;
const CANVAS_HEIGHT: u32 = 768;
const MAX_STROKE_POINTS_PER_FRAME: u32 = 256;
const JACOBI_ITERATIONS: u32 = 20;
const SIM_STEPS_PER_FRAME: u32 = 1;

// Simulation parameters
const DT: f32 = 0.5;
const MU: f32 = 0.1;
const KAPPA: f32 = 0.01;
const ETA: f32 = 0.03;
const SLOPE_STRENGTH: f32 = 5.0;
const BRUSH_PRESSURE: f32 = 2.0;
const TRANSFER_RATE: f32 = 0.02;
const ABSORB_RATE: f32 = 0.05;
const DIFFUSE_RATE: f32 = 0.03;
const CAPILLARY_CAPACITY: f32 = 1.0;
const CAPILLARY_SIGMA: f32 = 0.3;

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

fn create_ping_pong_r32f(renderer: &mut Renderer) -> anyhow::Result<PingPong> {
    let s0 =
        renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, vk::Format::R32_SFLOAT)?;
    let s1 =
        renderer.create_storage_texture(CANVAS_WIDTH, CANVAS_HEIGHT, vk::Format::R32_SFLOAT)?;
    renderer.clear_storage_texture(&s0, [0.0, 0.0, 0.0, 0.0])?;
    renderer.clear_storage_texture(&s1, [0.0, 0.0, 0.0, 0.0])?;
    let t0 = renderer.storage_texture_as_sampled(&s0)?;
    let t1 = renderer.storage_texture_as_sampled(&s1)?;
    Ok(PingPong {
        storage: [s0, s1],
        sampled: [t0, t1],
    })
}

fn create_ping_pong_rgba32f(renderer: &mut Renderer) -> anyhow::Result<PingPong> {
    let s0 = renderer.create_storage_texture(
        CANVAS_WIDTH,
        CANVAS_HEIGHT,
        vk::Format::R32G32B32A32_SFLOAT,
    )?;
    let s1 = renderer.create_storage_texture(
        CANVAS_WIDTH,
        CANVAS_HEIGHT,
        vk::Format::R32G32B32A32_SFLOAT,
    )?;
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
    blur_v_pipeline: PipelineHandle<Compute>,
    flow_outward_pipelines: [PipelineHandle<Compute>; 2],
    move_pigment_pipelines: [PipelineHandle<Compute>; 2],
    transfer_pigment_pipelines: [PipelineHandle<Compute>; 2],
    capillary_flow_pipelines: [PipelineHandle<Compute>; 2],
    display_pipeline: PipelineHandle<DrawVertexCount>,

    // Buffers
    stroke_points_buffer: StorageBufferHandle<paint_brush_compute::StrokePoint>,
    brush_params_buffer: UniformBufferHandle<paint_brush_compute::BrushParams>,
    display_params_buffer: UniformBufferHandle<paint_display::DisplayParams>,
    update_vel_params_buffer: UniformBufferHandle<wc_update_velocity_compute::Params>,
    divergence_params_buffer: UniformBufferHandle<wc_divergence_compute::Params>,
    pressure_jacobi_params_buffer: UniformBufferHandle<wc_pressure_jacobi_compute::Params>,
    project_vel_params_buffer: UniformBufferHandle<wc_project_velocity_compute::Params>,
    blur_h_params_buffer: UniformBufferHandle<wc_gaussian_blur_compute::Params>,
    blur_v_params_buffer: UniformBufferHandle<wc_gaussian_blur_compute::Params>,
    flow_outward_params_buffer: UniformBufferHandle<wc_flow_outward_compute::Params>,
    move_pigment_params_buffer: UniformBufferHandle<wc_move_pigment_compute::Params>,
    transfer_pigment_params_buffer: UniformBufferHandle<wc_transfer_pigment_compute::Params>,
    capillary_flow_params_buffer: UniformBufferHandle<wc_capillary_flow_compute::Params>,

    // Parity tracking
    pressure_parity: bool, // flips 20x per frame (net 0), used in Jacobi loop
    sim_parity: bool,      // pigment + wet_mask + saturation (all flip 1x per frame)

    // Input state
    painting: bool,
    stroke_points: Vec<Vec2>,
    prev_mouse_pos: Option<Vec2>,

    // Brush settings
    active_pigment: u32,
    brush_radius: f32,
    brush_opacity: f32,

    // FPS tracking
    edit_state: EditState,
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

// Default Kubelka-Munk pigment presets
fn pigment_km(index: u32) -> paint_display::PigmentKM {
    match index {
        0 => paint_display::PigmentKM {
            // Ultramarine Blue
            k: Vec3::new(0.1, 0.2, 0.01),
            pad0: 0.0,
            s: Vec3::new(0.5, 0.3, 1.0),
            pad1: 0.0,
        },
        1 => paint_display::PigmentKM {
            // Cadmium Yellow
            k: Vec3::new(0.01, 0.01, 0.5),
            pad0: 0.0,
            s: Vec3::new(1.0, 0.8, 0.1),
            pad1: 0.0,
        },
        2 => paint_display::PigmentKM {
            // Alizarin Crimson
            k: Vec3::new(0.05, 0.8, 0.7),
            pad0: 0.0,
            s: Vec3::new(0.8, 0.1, 0.1),
            pad1: 0.0,
        },
        _ => paint_display::PigmentKM {
            // Burnt Sienna
            k: Vec3::new(0.2, 0.5, 0.8),
            pad0: 0.0,
            s: Vec3::new(0.5, 0.3, 0.1),
            pad1: 0.0,
        },
    }
}

fn pigment_properties(index: u32) -> wc_transfer_pigment_compute::PigmentProperties {
    match index {
        0 => wc_transfer_pigment_compute::PigmentProperties {
            density: 2.0,
            staining_power: 1.5,
            granulation: 0.7,
            pad0: 0.0,
        },
        1 => wc_transfer_pigment_compute::PigmentProperties {
            density: 1.5,
            staining_power: 0.8,
            granulation: 0.2,
            pad0: 0.0,
        },
        2 => wc_transfer_pigment_compute::PigmentProperties {
            density: 1.8,
            staining_power: 3.0,
            granulation: 0.3,
            pad0: 0.0,
        },
        _ => wc_transfer_pigment_compute::PigmentProperties {
            density: 2.5,
            staining_power: 2.0,
            granulation: 0.8,
            pad0: 0.0,
        },
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
        let velocity_u = create_ping_pong_r32f(renderer)?;
        let velocity_v = create_ping_pong_r32f(renderer)?;
        let pressure = create_ping_pong_r32f(renderer)?;
        let pigment = create_ping_pong_rgba32f(renderer)?;
        let saturation = create_ping_pong_r32f(renderer)?;
        let wet_mask = create_ping_pong_r32f(renderer)?;

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
        let height_data = noise::generate_paper_height_map(CANVAS_WIDTH, CANVAS_HEIGHT);
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
        let blur_v_params_buffer =
            renderer.create_uniform_buffer::<wc_gaussian_blur_compute::Params>()?;
        let flow_outward_params_buffer =
            renderer.create_uniform_buffer::<wc_flow_outward_compute::Params>()?;
        let move_pigment_params_buffer =
            renderer.create_uniform_buffer::<wc_move_pigment_compute::Params>()?;
        let transfer_pigment_params_buffer =
            renderer.create_uniform_buffer::<wc_transfer_pigment_compute::Params>()?;
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
                        stroke_points: &stroke_points_buffer,
                        brush_params_buffer: &brush_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.paint_brush_compute.pipeline_config(
                    paint_brush_compute::Resources {
                        wet_mask: wet_mask.read_storage(true),
                        pressure: pressure.read_storage(false), // pressure always at 0
                        pigment: pigment.read_storage(true),
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
                            u_in: velocity_u.read(true),
                            v_in: velocity_v.read(true),
                            pressure: pressure.read(false), // will be set correctly at dispatch time
                            wet_mask: wet_mask.read(false),
                            u_out: velocity_u.write(true),
                            v_out: velocity_v.write(true),
                            params_buffer: &project_vel_params_buffer,
                        },
                    ),
                )?,
                renderer.create_compute_pipeline(
                    s1.wc_project_velocity_compute.pipeline_config(
                        wc_project_velocity_compute::Resources {
                            u_in: velocity_u.read(false),
                            v_in: velocity_v.read(false),
                            pressure: pressure.read(false), // pressure parity always returns to false after even Jacobi iters
                            wet_mask: wet_mask.read(true),
                            u_out: velocity_u.write(false),
                            v_out: velocity_v.write(false),
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

        // Gaussian blur V: blur_temp → blurred_mask (single pipeline, no parity)
        let blur_v_pipeline = {
            let s = ShaderAtlas::init();
            renderer.create_compute_pipeline(s.wc_gaussian_blur_compute.pipeline_config(
                wc_gaussian_blur_compute::Resources {
                    input_tex: &blur_temp_sampled,
                    output_tex: &blurred_mask,
                    params_buffer: &blur_v_params_buffer,
                },
            ))?
        };

        // Flow outward: reads blurred_mask + wet_mask, writes pressure in-place
        // 2 pipelines for wet_mask parity (pressure is RW so always same handle)
        let flow_outward_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(s0.wc_flow_outward_compute.pipeline_config(
                    wc_flow_outward_compute::Resources {
                        blurred_mask: &blurred_mask_sampled,
                        wet_mask: wet_mask.read(false),
                        pressure: pressure.read_storage(false),
                        params_buffer: &flow_outward_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_flow_outward_compute.pipeline_config(
                    wc_flow_outward_compute::Resources {
                        blurred_mask: &blurred_mask_sampled,
                        wet_mask: wet_mask.read(true),
                        pressure: pressure.read_storage(false), // pressure always at index 0
                        params_buffer: &flow_outward_params_buffer,
                    },
                ))?,
            ]
        };

        // Move pigment: 2 pipelines for pigment parity
        // After vel update+project, vel_parity returns to starting value = same as pigment_parity
        // wet_mask hasn't changed yet at this point
        let move_pigment_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(s0.wc_move_pigment_compute.pipeline_config(
                    wc_move_pigment_compute::Resources {
                        pigment_in: pigment.read(false),
                        u_in: velocity_u.read(false),
                        v_in: velocity_v.read(false),
                        wet_mask: wet_mask.read(false),
                        pigment_out: pigment.write(false),
                        params_buffer: &move_pigment_params_buffer,
                    },
                ))?,
                renderer.create_compute_pipeline(s1.wc_move_pigment_compute.pipeline_config(
                    wc_move_pigment_compute::Resources {
                        pigment_in: pigment.read(true),
                        u_in: velocity_u.read(true),
                        v_in: velocity_v.read(true),
                        wet_mask: wet_mask.read(true),
                        pigment_out: pigment.write(true),
                        params_buffer: &move_pigment_params_buffer,
                    },
                ))?,
            ]
        };

        // Transfer pigment: in-place on pigment+deposit, 2 pipelines for pigment parity
        let transfer_pigment_pipelines = {
            let s0 = ShaderAtlas::init();
            let s1 = ShaderAtlas::init();
            [
                renderer.create_compute_pipeline(
                    s0.wc_transfer_pigment_compute.pipeline_config(
                        wc_transfer_pigment_compute::Resources {
                            pigment: pigment.read_storage(false),
                            deposit: &deposit,
                            paper_height: &paper_height_sampled,
                            wet_mask: wet_mask.read(false),
                            params_buffer: &transfer_pigment_params_buffer,
                        },
                    ),
                )?,
                renderer.create_compute_pipeline(
                    s1.wc_transfer_pigment_compute.pipeline_config(
                        wc_transfer_pigment_compute::Resources {
                            pigment: pigment.read_storage(true),
                            deposit: &deposit,
                            paper_height: &paper_height_sampled,
                            wet_mask: wet_mask.read(true),
                            params_buffer: &transfer_pigment_params_buffer,
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
                        saturation_in: saturation.read(false),
                        wet_mask_in: wet_mask.read(false),
                        pressure: pressure.read(false),
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
                        pressure: pressure.read(false), // pressure always at index 0
                        paper_height: &paper_height_sampled,
                        saturation_out: saturation.write(true),
                        wet_mask_out: wet_mask.write(true),
                        params_buffer: &capillary_flow_params_buffer,
                    },
                ))?,
            ]
        };

        // Display pipeline
        let display_pipeline = {
            let s = ShaderAtlas::init();
            let display_resources = paint_display::Resources {
                deposit: &deposit_sampled,
                paper_height: &paper_height_sampled,
                display_params_buffer: &display_params_buffer,
            };
            renderer.create_pipeline(s.paint_display.pipeline_config(display_resources))?
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
            blur_v_pipeline,
            flow_outward_pipelines,
            move_pigment_pipelines,
            transfer_pigment_pipelines,
            capillary_flow_pipelines,
            display_pipeline,

            stroke_points_buffer,
            brush_params_buffer,
            display_params_buffer,
            update_vel_params_buffer,
            divergence_params_buffer,
            pressure_jacobi_params_buffer,
            project_vel_params_buffer,
            blur_h_params_buffer,
            blur_v_params_buffer,
            flow_outward_params_buffer,
            move_pigment_params_buffer,
            transfer_pigment_params_buffer,
            capillary_flow_params_buffer,

            pressure_parity: false,
            sim_parity: false,

            painting: false,
            stroke_points: Vec::new(),
            prev_mouse_pos: None,

            active_pigment: 0,
            brush_radius: 20.0,
            brush_opacity: 0.5,

            edit_state: EditState {
                fps: Label::new("FPS: --"),
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

            Input::KeyDown(key) => {
                match key {
                    Key::Num1 => self.active_pigment = 0, // Blue
                    Key::Num2 => self.active_pigment = 1, // Yellow
                    Key::Num3 => self.active_pigment = 2, // Red
                    Key::Num4 => self.active_pigment = 3, // Brown
                    _ => {}
                }
            }

            _ => {}
        }
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let workgroup_x = (CANVAS_WIDTH + 15) / 16;
        let workgroup_y = (CANVAS_HEIGHT + 15) / 16;

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
            compute_barrier(&mut renderer);

            // 6. Gaussian blur H (wet_mask → blur_temp)
            renderer.dispatch(
                &self.blur_h_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 7. Gaussian blur V (blur_temp → blurred_mask)
            renderer.dispatch(&self.blur_v_pipeline, workgroup_x, workgroup_y, 1);
            compute_barrier(&mut renderer);

            // 8. Flow outward (modify pressure in-place)
            renderer.dispatch(
                &self.flow_outward_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 9. Move pigment (advection)
            renderer.dispatch(
                &self.move_pigment_pipelines[sim as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 10. Transfer pigment (in-place on pigment at !sim)
            renderer.dispatch(
                &self.transfer_pigment_pipelines[(!sim) as usize],
                workgroup_x,
                workgroup_y,
                1,
            );
            compute_barrier(&mut renderer);

            // 11. Capillary flow (saturation + wet_mask at sim → writes to !sim)
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
        let blur_v_params_buffer = &mut self.blur_v_params_buffer;
        let flow_outward_params_buffer = &mut self.flow_outward_params_buffer;
        let move_pigment_params_buffer = &mut self.move_pigment_params_buffer;
        let transfer_pigment_params_buffer = &mut self.transfer_pigment_params_buffer;
        let capillary_flow_params_buffer = &mut self.capillary_flow_params_buffer;

        renderer.draw_vertex_count(&self.display_pipeline, 3, |gpu| {
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
            match active_pigment {
                0 => pigment_color.x = 1.0,
                1 => pigment_color.y = 1.0,
                2 => pigment_color.z = 1.0,
                _ => pigment_color.w = 1.0,
            }

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
                blur_v_params_buffer,
                wc_gaussian_blur_compute::Params {
                    grid_size,
                    direction: Vec2::new(0.0, 1.0),
                },
            );

            gpu.write_uniform(
                flow_outward_params_buffer,
                wc_flow_outward_compute::Params {
                    grid_size,
                    eta: ETA,
                    _padding_0: Default::default(),
                },
            );

            gpu.write_uniform(
                move_pigment_params_buffer,
                wc_move_pigment_compute::Params {
                    grid_size,
                    dt: DT,
                    _padding_0: Default::default(),
                },
            );

            gpu.write_uniform(
                transfer_pigment_params_buffer,
                wc_transfer_pigment_compute::Params {
                    grid_size,
                    transfer_rate: TRANSFER_RATE,
                    pad: 0.0,
                    pigment0: pigment_properties(0),
                    pigment1: pigment_properties(1),
                    pigment2: pigment_properties(2),
                    pigment3: pigment_properties(3),
                },
            );

            gpu.write_uniform(
                capillary_flow_params_buffer,
                wc_capillary_flow_compute::Params {
                    grid_size,
                    absorb_rate: ABSORB_RATE,
                    diffuse_rate: DIFFUSE_RATE,
                    capacity: CAPILLARY_CAPACITY,
                    sigma: CAPILLARY_SIGMA,
                    _padding_0: Default::default(),
                },
            );

            gpu.write_uniform(
                display_params_buffer,
                paint_display::DisplayParams {
                    texel_size,
                    pad: Vec2::ZERO,
                    pigment0: pigment_km(0),
                    pigment1: pigment_km(1),
                    pigment2: pigment_km(2),
                    pigment3: pigment_km(3),
                },
            );
        })?;

        Ok(())
    }
}

mod noise {
    fn hash(n: f32) -> f32 {
        let s = (n * 127.1).sin() * 43758.5453;
        s - s.floor()
    }

    fn hash2(x: f32, y: f32) -> (f32, f32) {
        let a = hash(x * 127.1 + y * 311.7);
        let b = hash(x * 269.5 + y * 183.3);
        (a, b)
    }

    fn smoothstep(t: f32) -> f32 {
        t * t * (3.0 - 2.0 * t)
    }

    fn grad(ix: i32, iy: i32, fx: f32, fy: f32) -> f32 {
        let (gx, gy) = hash2(ix as f32, iy as f32);
        let gx = gx * 2.0 - 1.0;
        let gy = gy * 2.0 - 1.0;
        gx * fx + gy * fy
    }

    fn perlin(x: f32, y: f32) -> f32 {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - x.floor();
        let fy = y - y.floor();

        let ux = smoothstep(fx);
        let uy = smoothstep(fy);

        let n00 = grad(ix, iy, fx, fy);
        let n10 = grad(ix + 1, iy, fx - 1.0, fy);
        let n01 = grad(ix, iy + 1, fx, fy - 1.0);
        let n11 = grad(ix + 1, iy + 1, fx - 1.0, fy - 1.0);

        let nx0 = n00 + ux * (n10 - n00);
        let nx1 = n01 + ux * (n11 - n01);
        nx0 + uy * (nx1 - nx0)
    }

    fn perlin_fbm(x: f32, y: f32) -> f32 {
        let mut value = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        for _ in 0..3 {
            value += amplitude * perlin(x * frequency, y * frequency);
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        value
    }

    fn worley(x: f32, y: f32, jitter: f32) -> (f32, f32) {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - x.floor();
        let fy = y - y.floor();

        let mut f1 = f32::MAX;
        let mut f2 = f32::MAX;

        for dy in -1..=1 {
            for dx in -1..=1 {
                let (px, py) = hash2((ix + dx) as f32, (iy + dy) as f32);
                let px = 0.5 + jitter * (px - 0.5);
                let py = 0.5 + jitter * (py - 0.5);
                let vx = dx as f32 + px - fx;
                let vy = dy as f32 + py - fy;
                let d = vx * vx + vy * vy;
                if d < f1 {
                    f2 = f1;
                    f1 = d;
                } else if d < f2 {
                    f2 = d;
                }
            }
        }

        (f1.sqrt(), f2.sqrt())
    }

    pub fn generate_paper_height_map(width: u32, height: u32) -> Vec<f32> {
        let perlin_scale = 8.0;
        let worley_scale = 48.0;
        let jitter = 0.25;

        let mut data = Vec::with_capacity((width * height) as usize);

        for y in 0..height {
            for x in 0..width {
                let nx = x as f32 / width as f32;
                let ny = y as f32 / height as f32;

                let p = perlin_fbm(nx * perlin_scale, ny * perlin_scale);

                let (f1a, f2a) = worley(nx * worley_scale, ny * worley_scale, jitter);
                let w = (0.5 * (f2a - f1a)).sqrt();

                let h = 0.7 * p + 0.3 * w;
                data.push(h);
            }
        }

        let min = data.iter().copied().fold(f32::MAX, f32::min);
        let max = data.iter().copied().fold(f32::MIN, f32::max);
        let range = max - min;
        if range > 0.0 {
            for v in &mut data {
                *v = (*v - min) / range;
            }
        }

        data
    }
}
