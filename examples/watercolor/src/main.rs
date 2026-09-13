//! A painting program based on the watercolor simulation described in this paper:
//! https://grail.cs.washington.edu/projects/watercolor/paper_small.pdf

mod generated;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ash::vk;
use facet::Facet;
use glam::{Vec2, Vec3, Vec4};

use mltrs::editor::{Label, Slider};
use mltrs::game::*;
use mltrs::renderer::{
    ComputeNode, ComputeNodeWithPush, DrawError, DrawVertexCountNode, FrameRenderer, GraphFormat,
    LoopCount, OptionalNode, PreparedRenderGraph, RenderGraph, Renderer, RepeatNode,
    ResourcePlanner, StorageBufferHandle, StorageSlot, StorageTextureHandle, TextureHandle,
    UniformBufferHandle, UploadNode, optional,
    render_graph::{dispatch, draw_vertex_count},
    repeat, upload,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::paint_brush_compute;
use crate::generated::shader_atlas::paint_display::{self, DebugView};
use crate::generated::shader_atlas::wc_advect_and_transfer_pigment_compute;
use crate::generated::shader_atlas::wc_capillary_flow_compute;
use crate::generated::shader_atlas::wc_divergence_compute;
use crate::generated::shader_atlas::wc_flow_outward_compute;
use crate::generated::shader_atlas::wc_gaussian_blur_compute;
use crate::generated::shader_atlas::wc_pressure_jacobi_compute;
use crate::generated::shader_atlas::wc_project_velocity_compute;
use crate::generated::shader_atlas::wc_update_velocity_compute;
use mltrs::manifest_path;

fn main() -> Result<(), anyhow::Error> {
    Watercolor::run()
}

#[derive(Facet)]
pub struct EditState {
    fps: Label,
    brush_concentration: Slider,
    debug_view: DebugView,
}

const FRAME_HISTORY_SIZE: usize = 60;

const CANVAS_WIDTH: u32 = 2048;
const CANVAS_HEIGHT: u32 = 1536;
const MAX_STROKE_POINTS_PER_FRAME: u32 = 256;
/// The number of times to dispatch the water pressure compute shader
/// higher = less divergence & more accurate water pressure
const JACOBI_ITERATIONS: u32 = 2;

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

type WcGraph = PreparedRenderGraph<(
    OptionalNode<(
        UploadNode<paint_brush_compute::StrokePoint>,
        ComputeNode<paint_brush_compute::BrushParams>,
    )>,
    ComputeNode<wc_update_velocity_compute::Params>,
    ComputeNode<wc_divergence_compute::Params>,
    RepeatNode<
        ComputeNodeWithPush<
            wc_pressure_jacobi_compute::Params,
            wc_pressure_jacobi_compute::JacobiDispatch,
        >,
    >,
    ComputeNode<wc_project_velocity_compute::Params>,
    ComputeNodeWithPush<wc_gaussian_blur_compute::Params, wc_gaussian_blur_compute::BlurDispatch>,
    ComputeNodeWithPush<wc_gaussian_blur_compute::Params, wc_gaussian_blur_compute::BlurDispatch>,
    ComputeNode<wc_flow_outward_compute::Params>,
    ComputeNode<wc_advect_and_transfer_pigment_compute::Params>,
    ComputeNode<wc_capillary_flow_compute::Params>,
    DrawVertexCountNode<paint_display::DisplayParams>,
)>;

struct Watercolor {
    graph: WcGraph,

    // Paper height is game-owned: written once at setup, bound externally.
    _paper_height: StorageTextureHandle,
    _paper_height_sampled: TextureHandle,

    // The graph holds slots into these buffers; the handles stay here for
    // eventual cleanup.
    _stroke_points_buffer: StorageBufferHandle<paint_brush_compute::StrokePoint>,
    _brush_params_buffer: UniformBufferHandle<paint_brush_compute::BrushParams>,
    _display_params_buffer: UniformBufferHandle<paint_display::DisplayParams>,
    _update_vel_params_buffer: UniformBufferHandle<wc_update_velocity_compute::Params>,
    _divergence_params_buffer: UniformBufferHandle<wc_divergence_compute::Params>,
    _pressure_jacobi_params_buffer: UniformBufferHandle<wc_pressure_jacobi_compute::Params>,
    _project_vel_params_buffer: UniformBufferHandle<wc_project_velocity_compute::Params>,
    _blur_h_params_buffer: UniformBufferHandle<wc_gaussian_blur_compute::Params>,
    _blur_v_params_buffer: UniformBufferHandle<wc_gaussian_blur_compute::Params>,
    _flow_outward_params_buffer: UniformBufferHandle<wc_flow_outward_compute::Params>,
    _advect_and_transfer_params_buffer:
        UniformBufferHandle<wc_advect_and_transfer_pigment_compute::Params>,
    _capillary_flow_params_buffer: UniformBufferHandle<wc_capillary_flow_compute::Params>,

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

/// Compute the number of workgroups needed to cover the canvas for a given shader's workgroup size.
fn workgroups(wg_size: [u32; 3]) -> [u32; 3] {
    [
        CANVAS_WIDTH.div_ceil(wg_size[0]),
        CANVAS_HEIGHT.div_ceil(wg_size[1]),
        1,
    ]
}

/// Map a mouse position (in window coordinates) to canvas coordinates using crop-to-fill scaling.
///
/// Each axis is transformed by scaling its normalized coordinate around the center (0.5):
///   canvas_coord = ((mouse / window - 0.5) * scale + 0.5) * canvas_size
///
/// The axis that fills the window maps 1:1 (scale = 1.0), while the cropped axis is
/// compressed toward center (scale < 1.0). Clamping keeps the result within canvas bounds
/// when the mouse is in the cropped region.
fn window_to_canvas(position: Vec2, window_size: Vec2, canvas_size: Vec2) -> Vec2 {
    let ratio = (window_size.x * canvas_size.y) / (window_size.y * canvas_size.x);
    let scale = Vec2::new(ratio.min(1.0), (1.0 / ratio).min(1.0));
    let normalized = position / window_size;
    (((normalized - 0.5) * scale + 0.5) * canvas_size).clamp(Vec2::ZERO, canvas_size)
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

// Pigment data from Curtis et al. "Computer-Generated Watercolor" Figure 5 (a-l)
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
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Watercolor"
    }

    fn initial_window_size() -> (u32, u32) {
        (1024, 768)
    }

    fn render_scale() -> Option<f32> {
        Some(1.0)
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self> {
        // Logical simulation textures; the graph derives which ones need two
        // physical images and rotates them itself
        let mut res = ResourcePlanner::new();
        let r32 = GraphFormat::R32Float;
        let rgba32 = GraphFormat::Rgba32Float;
        let velocity_u = res.texture("velocity_u", CANVAS_WIDTH, CANVAS_HEIGHT, r32);
        let velocity_v = res.texture("velocity_v", CANVAS_WIDTH, CANVAS_HEIGHT, r32);
        let pressure = res.texture("pressure", CANVAS_WIDTH, CANVAS_HEIGHT, r32);
        let pigment_0_3 = res.texture("pigment_0_3", CANVAS_WIDTH, CANVAS_HEIGHT, rgba32);
        let pigment_4_7 = res.texture("pigment_4_7", CANVAS_WIDTH, CANVAS_HEIGHT, rgba32);
        let pigment_8_11 = res.texture("pigment_8_11", CANVAS_WIDTH, CANVAS_HEIGHT, rgba32);
        let saturation = res.texture("saturation", CANVAS_WIDTH, CANVAS_HEIGHT, r32);
        let wet_mask = res.texture("wet_mask", CANVAS_WIDTH, CANVAS_HEIGHT, r32);
        let deposit_0_3 = res.texture("deposit_0_3", CANVAS_WIDTH, CANVAS_HEIGHT, rgba32);
        let deposit_4_7 = res.texture("deposit_4_7", CANVAS_WIDTH, CANVAS_HEIGHT, rgba32);
        let deposit_8_11 = res.texture("deposit_8_11", CANVAS_WIDTH, CANVAS_HEIGHT, rgba32);
        let divergence = res.texture("divergence", CANVAS_WIDTH, CANVAS_HEIGHT, r32);
        let blur_temp = res.texture("blur_temp", CANVAS_WIDTH, CANVAS_HEIGHT, r32);
        let blurred_mask = res.texture("blurred_mask", CANVAS_WIDTH, CANVAS_HEIGHT, r32);

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
        let blur_v_params_buffer =
            renderer.create_uniform_buffer::<wc_gaussian_blur_compute::Params>()?;
        let flow_outward_params_buffer =
            renderer.create_uniform_buffer::<wc_flow_outward_compute::Params>()?;
        let advect_and_transfer_params_buffer =
            renderer.create_uniform_buffer::<wc_advect_and_transfer_pigment_compute::Params>()?;
        let capillary_flow_params_buffer =
            renderer.create_uniform_buffer::<wc_capillary_flow_compute::Params>()?;

        // --- Create pipelines ---
        let brush_pipeline =
            renderer.create_compute_pipeline(shaders.paint_brush_compute.pipeline_config(
                paint_brush_compute::Resources {
                    brush_params_buffer: &brush_params_buffer,
                },
            ))?;

        let update_velocity_pipeline = renderer.create_compute_pipeline(
            shaders.wc_update_velocity_compute.pipeline_config(
                wc_update_velocity_compute::Resources {
                    params_buffer: &update_vel_params_buffer,
                },
            ),
        )?;

        let divergence_pipeline =
            renderer.create_compute_pipeline(shaders.wc_divergence_compute.pipeline_config(
                wc_divergence_compute::Resources {
                    params_buffer: &divergence_params_buffer,
                },
            ))?;

        let pressure_jacobi_pipeline = renderer.create_compute_pipeline(
            shaders.wc_pressure_jacobi_compute.pipeline_config(
                wc_pressure_jacobi_compute::Resources {
                    params_buffer: &pressure_jacobi_params_buffer,
                },
            ),
        )?;

        let project_velocity_pipeline = renderer.create_compute_pipeline(
            shaders.wc_project_velocity_compute.pipeline_config(
                wc_project_velocity_compute::Resources {
                    params_buffer: &project_vel_params_buffer,
                },
            ),
        )?;

        let blur_h_pipeline =
            renderer.create_compute_pipeline(shaders.wc_gaussian_blur_compute.pipeline_config(
                wc_gaussian_blur_compute::Resources {
                    params_buffer: &blur_h_params_buffer,
                },
            ))?;
        let blur_v_pipeline =
            renderer.create_compute_pipeline(shaders.wc_gaussian_blur_compute.pipeline_config(
                wc_gaussian_blur_compute::Resources {
                    params_buffer: &blur_v_params_buffer,
                },
            ))?;

        let flow_outward_pipeline =
            renderer.create_compute_pipeline(shaders.wc_flow_outward_compute.pipeline_config(
                wc_flow_outward_compute::Resources {
                    params_buffer: &flow_outward_params_buffer,
                },
            ))?;

        let advect_and_transfer_pipeline = renderer.create_compute_pipeline(
            shaders
                .wc_advect_and_transfer_pigment_compute
                .pipeline_config(wc_advect_and_transfer_pigment_compute::Resources {
                    params_buffer: &advect_and_transfer_params_buffer,
                }),
        )?;

        let capillary_flow_pipeline =
            renderer.create_compute_pipeline(shaders.wc_capillary_flow_compute.pipeline_config(
                wc_capillary_flow_compute::Resources {
                    params_buffer: &capillary_flow_params_buffer,
                },
            ))?;

        let display_pipeline = renderer.create_pipeline(shaders.paint_display.pipeline_config(
            paint_display::Resources {
                display_params_buffer: &display_params_buffer,
            },
        ))?;

        let paper = paper_height_sampled.bindless_handle();
        let stroke_points = StorageSlot::from(&stroke_points_buffer);

        let graph = RenderGraph::new(
            res,
            (
                // 1. Brush input: in-place stamps into the current versions
                optional((
                    upload(stroke_points),
                    dispatch(
                        &brush_pipeline,
                        &brush_params_buffer,
                        workgroups(paint_brush_compute::WORKGROUP_SIZE),
                    )
                    .with_param_bindings(
                        paint_brush_compute::BrushParamsBindings {
                            wet_mask: wet_mask.mutate(),
                            pressure: pressure.mutate(),
                            pigment_0_3: pigment_0_3.mutate(),
                            pigment_4_7: pigment_4_7.mutate(),
                            pigment_8_11: pigment_8_11.mutate(),
                            saturation: saturation.mutate(),
                            stroke_points: stroke_points.read_addr(),
                        },
                    ),
                )),
                // 2. Update velocity (advection + forces)
                dispatch(
                    &update_velocity_pipeline,
                    &update_vel_params_buffer,
                    workgroups(wc_update_velocity_compute::WORKGROUP_SIZE),
                )
                .with_param_bindings(wc_update_velocity_compute::ParamsBindings {
                    u_in: velocity_u.read(),
                    v_in: velocity_v.read(),
                    pressure: pressure.read(),
                    wet_mask: wet_mask.read(),
                    u_out: velocity_u.write(),
                    v_out: velocity_v.write(),
                    paper_height: paper.into(),
                }),
                // 3. Divergence of the updated velocity
                dispatch(
                    &divergence_pipeline,
                    &divergence_params_buffer,
                    workgroups(wc_divergence_compute::WORKGROUP_SIZE),
                )
                .with_param_bindings(wc_divergence_compute::ParamsBindings {
                    u_in: velocity_u.read(),
                    v_in: velocity_v.read(),
                    divergence: divergence.write(),
                }),
                // 4. Pressure Jacobi iterations; the push block rotates
                //    pressure per iteration, so odd trip counts are legal
                repeat(
                    dispatch(
                        &pressure_jacobi_pipeline,
                        &pressure_jacobi_params_buffer,
                        workgroups(wc_pressure_jacobi_compute::WORKGROUP_SIZE),
                    )
                    .with_param_bindings(wc_pressure_jacobi_compute::ParamsBindings {
                        divergence: divergence.read(),
                    })
                    .with_push_constant(
                        wc_pressure_jacobi_compute::JacobiDispatchInput {
                            pressure_in: pressure.read(),
                            pressure_out: pressure.write(),
                        },
                    ),
                ),
                // 5. Project velocity in place
                dispatch(
                    &project_velocity_pipeline,
                    &project_vel_params_buffer,
                    workgroups(wc_project_velocity_compute::WORKGROUP_SIZE),
                )
                .with_param_bindings(wc_project_velocity_compute::ParamsBindings {
                    u: velocity_u.mutate(),
                    v: velocity_v.mutate(),
                    wet_mask: wet_mask.read(),
                    pressure: pressure.read(),
                }),
                // 6. Gaussian blur H (pre-capillary wet mask -> blur_temp)
                dispatch(
                    &blur_h_pipeline,
                    &blur_h_params_buffer,
                    workgroups(wc_gaussian_blur_compute::WORKGROUP_SIZE),
                )
                .with_push_constant(wc_gaussian_blur_compute::BlurDispatchInput {
                    input_tex: wet_mask.read(),
                    output_tex: blur_temp.write(),
                    direction: Vec2::new(1.0, 0.0),
                }),
                // 7. Gaussian blur V (blur_temp -> blurred_mask)
                dispatch(
                    &blur_v_pipeline,
                    &blur_v_params_buffer,
                    workgroups(wc_gaussian_blur_compute::WORKGROUP_SIZE),
                )
                .with_push_constant(wc_gaussian_blur_compute::BlurDispatchInput {
                    input_tex: blur_temp.read(),
                    output_tex: blurred_mask.write(),
                    direction: Vec2::new(0.0, 1.0),
                }),
                // 8. Flow outward (blurred_mask -> flow formula into pressure
                //    + saturation, in place)
                dispatch(
                    &flow_outward_pipeline,
                    &flow_outward_params_buffer,
                    workgroups(wc_flow_outward_compute::WORKGROUP_SIZE),
                )
                .with_param_bindings(wc_flow_outward_compute::ParamsBindings {
                    wet_mask: wet_mask.read(),
                    saturation: saturation.mutate(),
                    blurred_mask: blurred_mask.read(),
                    pressure: pressure.mutate(),
                }),
                // 9. Advect + transfer pigment; advects by the pre-update
                //    velocity, exactly as the parity code did
                dispatch(
                    &advect_and_transfer_pipeline,
                    &advect_and_transfer_params_buffer,
                    workgroups(wc_advect_and_transfer_pigment_compute::WORKGROUP_SIZE),
                )
                .with_param_bindings(
                    wc_advect_and_transfer_pigment_compute::ParamsBindings {
                        pigment_in_0_3: pigment_0_3.read(),
                        pigment_in_4_7: pigment_4_7.read(),
                        pigment_in_8_11: pigment_8_11.read(),
                        u_in: velocity_u.read_previous(),
                        v_in: velocity_v.read_previous(),
                        wet_mask: wet_mask.read(),
                        pigment_out_0_3: pigment_0_3.write(),
                        pigment_out_4_7: pigment_4_7.write(),
                        pigment_out_8_11: pigment_8_11.write(),
                        deposit_in_0_3: deposit_0_3.read(),
                        deposit_in_4_7: deposit_4_7.read(),
                        deposit_in_8_11: deposit_8_11.read(),
                        deposit_out_0_3: deposit_0_3.write(),
                        deposit_out_4_7: deposit_4_7.write(),
                        deposit_out_8_11: deposit_8_11.write(),
                        paper_height: paper.into(),
                    },
                ),
                // 10. Capillary flow (saturation + wet_mask -> next versions)
                dispatch(
                    &capillary_flow_pipeline,
                    &capillary_flow_params_buffer,
                    workgroups(wc_capillary_flow_compute::WORKGROUP_SIZE),
                )
                .with_param_bindings(wc_capillary_flow_compute::ParamsBindings {
                    saturation_in: saturation.read(),
                    wet_mask_in: wet_mask.read(),
                    saturation_out: saturation.write(),
                    wet_mask_out: wet_mask.write(),
                }),
                // 11. Display; the wet mask is shown pre-capillary, exactly
                //     as the parity code did
                draw_vertex_count(
                    &display_pipeline,
                    &display_params_buffer,
                    3,
                    paint_display::DisplayParamsBindings {
                        deposit_0_3: deposit_0_3.read(),
                        deposit_4_7: deposit_4_7.read(),
                        deposit_8_11: deposit_8_11.read(),
                        paper_height: paper.into(),
                        wet_mask: wet_mask.read_previous(),
                    },
                ),
            ),
        )?
        .prepare(renderer)?;

        Ok(Self {
            graph,

            _paper_height: paper_height,
            _paper_height_sampled: paper_height_sampled,

            _stroke_points_buffer: stroke_points_buffer,
            _brush_params_buffer: brush_params_buffer,
            _display_params_buffer: display_params_buffer,
            _update_vel_params_buffer: update_vel_params_buffer,
            _divergence_params_buffer: divergence_params_buffer,
            _pressure_jacobi_params_buffer: pressure_jacobi_params_buffer,
            _project_vel_params_buffer: project_vel_params_buffer,
            _blur_h_params_buffer: blur_h_params_buffer,
            _blur_v_params_buffer: blur_v_params_buffer,
            _flow_outward_params_buffer: flow_outward_params_buffer,
            _advect_and_transfer_params_buffer: advect_and_transfer_params_buffer,
            _capillary_flow_params_buffer: capillary_flow_params_buffer,

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

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let stroke_points = std::mem::take(&mut self.stroke_points);
        let point_count = stroke_points
            .len()
            .min(MAX_STROKE_POINTS_PER_FRAME as usize) as u32;

        let grid_size = Vec2::new(CANVAS_WIDTH as f32, CANVAS_HEIGHT as f32);
        let texel_size = Vec2::new(1.0 / CANVAS_WIDTH as f32, 1.0 / CANVAS_HEIGHT as f32);
        let window_size = renderer.window_resolution();

        let brush = (point_count > 0).then(|| {
            let gpu_points: Vec<paint_brush_compute::StrokePoint> = stroke_points
                [..point_count as usize]
                .iter()
                .map(|&position| paint_brush_compute::StrokePoint {
                    position: window_to_canvas(position, window_size, grid_size),
                })
                .collect();

            // Pigment color: concentration in the active group/channel
            let mut pigment_color_0_3 = Vec4::ZERO;
            let mut pigment_color_4_7 = Vec4::ZERO;
            let mut pigment_color_8_11 = Vec4::ZERO;
            let group_colors = [
                &mut pigment_color_0_3,
                &mut pigment_color_4_7,
                &mut pigment_color_8_11,
            ];
            group_colors[self.active_pigment.group_index()][self.active_pigment.channel_index()] =
                self.edit_state.brush_concentration.value;

            (
                gpu_points,
                paint_brush_compute::BrushParamsData {
                    point_count,
                    brush_radius: self.brush_radius,
                    brush_opacity: self.brush_opacity,
                    brush_pressure: BRUSH_PRESSURE,
                    pigment_color_0_3,
                    pigment_color_4_7,
                    pigment_color_8_11,
                    canvas_size: grid_size,
                },
            )
        });

        self.graph.execute(
            renderer,
            &(
                brush,
                wc_update_velocity_compute::ParamsData {
                    grid_size,
                    texel_size,
                    dt: DT,
                    mu: MU,
                    kappa: KAPPA,
                    slope_strength: SLOPE_STRENGTH,
                },
                wc_divergence_compute::ParamsData { grid_size },
                (
                    LoopCount(JACOBI_ITERATIONS),
                    wc_pressure_jacobi_compute::ParamsData { grid_size },
                ),
                wc_project_velocity_compute::ParamsData { grid_size },
                wc_gaussian_blur_compute::Params {
                    grid_size,
                    _padding_0: Default::default(),
                },
                wc_gaussian_blur_compute::Params {
                    grid_size,
                    _padding_0: Default::default(),
                },
                wc_flow_outward_compute::ParamsData {
                    grid_size,
                    eta: ETA,
                },
                wc_advect_and_transfer_pigment_compute::ParamsData {
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
                wc_capillary_flow_compute::ParamsData {
                    grid_size,
                    diffuse_rate: DIFFUSE_RATE,
                    capacity: CAPILLARY_CAPACITY,
                    sigma: CAPILLARY_SIGMA,
                    dry_threshold: DRY_THRESHOLD,
                },
                paint_display::DisplayParamsData {
                    texel_size,
                    debug_view: self.edit_state.debug_view,
                    canvas_aspect: grid_size.x / grid_size.y,
                    window_aspect: window_size.x / window_size.y,
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
            ),
        )
    }
}

fn load_paper_height_map(width: u32, height: u32) -> Vec<f32> {
    let path = manifest_path!["textures", "watercolor", "paper_height.png"];
    let img =
        image::open(&path).expect("missing paper texture — run `just watercolor paper-texture`");
    let gray = img.to_luma8();

    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(gray.get_pixel(x, y).0[0] as f32 / 255.0);
        }
    }

    data
}
