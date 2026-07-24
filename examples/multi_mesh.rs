//! Demonstrates the multi-draw queue, shared meshes, per-pipeline raster state
//! and per-texture options.
//!
//! The foreground is the original multi-draw scene: three shapes concatenated
//! into one mesh, each baked centered at the origin and placed by its own model
//! matrix. Behind it sit two rows of test panels, each one existing to make a
//! single renderer knob visually unmistakable — see `pipeline_specs()`.
//!
//! Uniform buffers are per-pipeline, so every draw sharing a pipeline must
//! share a model matrix: distinctly-moving shapes need their own pipelines,
//! one per (shape, color) pair here. That per-object pipeline growth is a
//! consequence of this renderer's no-per-draw-uniform design, not a general
//! rule — a single articulated model (like the planned toon_link example)
//! shares one transform across all of its material pipelines.
//!
//! NOTE the order of `DRAWS` is semantically load-bearing now, not incidental:
//! the translucent panel must be queued after its backdrop, and the
//! depth-write-off panel before the panel it fails to occlude. Reordering them
//! silently destroys what they test. That is the same two-pass rule (opaque
//! before translucent) the Link example will follow.

use std::time::Instant;

use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};
use image::{DynamicImage, Rgba, RgbaImage};

use vulkan_slang_renderer::game::Game;
use vulkan_slang_renderer::renderer::{
    BlendMode, CullMode, DrawError, DrawIndexed, FrameRenderer, MeshHandle, PipelineHandle,
    RasterState, Renderer, TextureColorSpace, TextureFilter, TextureHandle, TextureOptions,
    TextureWrap, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::multi_mesh::*;

fn main() -> Result<(), anyhow::Error> {
    MultiMesh::run()
}

const CUBE: usize = 0;
const PYRAMID: usize = 1;
const DISC: usize = 2;

const RED: Vec4 = Vec4::new(0.9, 0.2, 0.15, 1.0);
const GREEN: Vec4 = Vec4::new(0.2, 0.85, 0.3, 1.0);
const BLUE: Vec4 = Vec4::new(0.25, 0.4, 0.95, 1.0);

const WHITE: Vec4 = Vec4::new(1.0, 1.0, 1.0, 1.0);
const ORANGE: Vec4 = Vec4::new(0.95, 0.55, 0.15, 1.0);
const SLATE: Vec4 = Vec4::new(0.25, 0.3, 0.45, 1.0);
/// deliberately half-transparent; two panels carry this exact tint and differ
/// only in their BlendMode
const HALF_YELLOW: Vec4 = Vec4::new(0.95, 0.9, 0.2, 0.5);
const CYAN: Vec4 = Vec4::new(0.2, 0.8, 0.85, 1.0);
const MAGENTA: Vec4 = Vec4::new(0.85, 0.25, 0.7, 1.0);

// --- textures, in creation order ---

const T_WHITE: usize = 0;
const T_CHECK_CLAMP_LINEAR: usize = 1;
const T_CHECK_REPEAT_LINEAR: usize = 2;
const T_CHECK_CLAMP_NEAREST: usize = 3;
const T_CHECK_REPEAT_NEAREST: usize = 4;
const T_GRAY_SRGB: usize = 5;
const T_GRAY_UNORM: usize = 6;

// --- pipelines, in creation order; DRAWS indexes into these ---

const P_CUBE: usize = 0;
const P_PYRAMID: usize = 1;
const P_DISC_RED: usize = 2;
const P_DISC_GREEN: usize = 3;
const P_DISC_BLUE: usize = 4;
const P_CULL_FRONT: usize = 5;
const P_BACKDROP: usize = 6;
const P_ALPHA: usize = 7;
const P_OPAQUE: usize = 8;
const P_DEPTH_WRITE_OFF: usize = 9;
const P_DEPTH_WRITE_ON: usize = 10;
const P_GRID_CLAMP_LINEAR: usize = 11;
const P_GRID_REPEAT_LINEAR: usize = 12;
const P_GRID_CLAMP_NEAREST: usize = 13;
const P_GRID_REPEAT_NEAREST: usize = 14;
const P_GRAY_SRGB: usize = 15;
const P_GRAY_UNORM: usize = 16;

/// A test panel pinned to a fixed spot on screen.
///
/// Panels are positioned in *view* space rather than world space, so the
/// layout is immune to the camera's orbit, its downward tilt and the window's
/// aspect ratio — and, sitting nearer than any shape, no panel is ever
/// occluded. A panel you can only read for part of an orbit is a panel you
/// cannot check.
///
/// `pos` and `half` are both in units of the viewport's half-height, y up, so
/// equal half extents look square and the whole layout scales together. The
/// row geometry below fits any viewport at least as wide as ~0.85 of its
/// height; wider windows just leave margins at the sides.
struct Panel {
    pos: Vec2,
    /// distance in front of the camera; the shapes never come nearer than
    /// about 2.8, and within a group it also orders the panels
    depth: f32,
    half: Vec2,
}

/// world placement of a pipeline's draws
enum Placement {
    /// one of the three original animated shapes, by index
    Shape(usize),
    Panel(Panel),
}

struct PipelineSpec {
    tint: Vec4,
    placement: Placement,
    raster: RasterState,
    /// index into the texture list built by `create_textures`
    texture: usize,
}

const PANEL_TOP: f32 = 0.78;
const PANEL_BOTTOM: f32 = -0.78;
const PANEL_DEPTH: f32 = 2.5;

/// One entry per pipeline, in `P_*` order. Built at runtime rather than as a
/// const so `..RasterState::default()` reads as "today's behavior, except".
fn pipeline_specs() -> Vec<PipelineSpec> {
    let shape = |tint, shape| PipelineSpec {
        tint,
        placement: Placement::Shape(shape),
        raster: RasterState::default(),
        texture: T_WHITE,
    };

    let panel = |tint, panel: Panel, raster, texture| PipelineSpec {
        tint,
        placement: Placement::Panel(panel),
        raster,
        texture,
    };

    // the checkerboard panels vary only in sampler options, never in state
    let grid = |x: f32, texture: usize| {
        panel(
            WHITE,
            Panel {
                pos: Vec2::new(x, PANEL_BOTTOM),
                depth: PANEL_DEPTH,
                half: Vec2::splat(0.12),
            },
            RasterState::default(),
            texture,
        )
    };

    vec![
        // the original scene: unchanged geometry, tints and default state, so
        // it doubles as the proof that RasterState::default() and the white
        // texture are true no-ops
        shape(RED, CUBE),
        shape(GREEN, PYRAMID),
        shape(RED, DISC),
        shape(GREEN, DISC),
        shape(BLUE, DISC),
        // cull: a closed cube rendered front-face-culled shows its interior
        // back faces — inside-out
        panel(
            ORANGE,
            Panel {
                pos: Vec2::new(-0.58, PANEL_TOP),
                depth: PANEL_DEPTH,
                half: Vec2::splat(0.12),
            },
            RasterState {
                cull: CullMode::Front,
                ..Default::default()
            },
            T_WHITE,
        ),
        // blend: a wide opaque backdrop, queued first, that the next two
        // panels are read against (translucency is invisible over nothing)
        panel(
            SLATE,
            Panel {
                pos: Vec2::new(0.10, PANEL_TOP),
                depth: PANEL_DEPTH + 0.1,
                half: Vec2::new(0.23, 0.15),
            },
            RasterState {
                blend: BlendMode::Opaque,
                ..Default::default()
            },
            T_WHITE,
        ),
        // ... the same half-transparent tint, twice: Alpha lets the backdrop
        // through, Opaque discards the alpha entirely
        panel(
            HALF_YELLOW,
            Panel {
                pos: Vec2::new(-0.01, PANEL_TOP),
                depth: PANEL_DEPTH,
                half: Vec2::splat(0.095),
            },
            RasterState::default(), // BlendMode::Alpha
            T_WHITE,
        ),
        panel(
            HALF_YELLOW,
            Panel {
                pos: Vec2::new(0.21, PANEL_TOP),
                depth: PANEL_DEPTH,
                half: Vec2::splat(0.095),
            },
            RasterState {
                blend: BlendMode::Opaque,
                ..Default::default()
            },
            T_WHITE,
        ),
        // depth_write: this panel is nearer the camera and queued first, but
        // writes no depth, so the farther panel below draws straight over it
        // where they overlap
        panel(
            CYAN,
            Panel {
                pos: Vec2::new(0.55, PANEL_TOP),
                depth: PANEL_DEPTH - 0.2,
                half: Vec2::splat(0.12),
            },
            RasterState {
                depth_write: false,
                ..Default::default()
            },
            T_WHITE,
        ),
        panel(
            MAGENTA,
            Panel {
                pos: Vec2::new(0.68, PANEL_TOP),
                depth: PANEL_DEPTH + 0.2,
                half: Vec2::splat(0.12),
            },
            RasterState::default(),
            T_WHITE,
        ),
        // wrap x filter: four panels, same checkerboard image and same UVs
        // (which run past the [0, 1] edge in both directions), differing only
        // in sampler options
        grid(-0.70, T_CHECK_CLAMP_LINEAR),
        grid(-0.43, T_CHECK_REPEAT_LINEAR),
        grid(-0.16, T_CHECK_CLAMP_NEAREST),
        grid(0.11, T_CHECK_REPEAT_NEAREST),
        // color space: one mid-gray image sampled two ways. Srgb applies the
        // sRGB->linear transfer on read and the sRGB swapchain undoes it, so
        // that panel lands back on the authored gray; Unorm hands the raw
        // 0.502 to the shader and the swapchain encodes it, so that panel is
        // the visibly lighter one. They are placed edge to edge because the
        // difference is a brightness step, not a pattern.
        panel(
            WHITE,
            Panel {
                pos: Vec2::new(0.45, PANEL_BOTTOM),
                depth: PANEL_DEPTH,
                half: Vec2::splat(0.12),
            },
            RasterState::default(),
            T_GRAY_SRGB,
        ),
        panel(
            WHITE,
            Panel {
                pos: Vec2::new(0.70, PANEL_BOTTOM),
                depth: PANEL_DEPTH,
                half: Vec2::splat(0.12),
            },
            RasterState::default(),
            T_GRAY_UNORM,
        ),
    ]
}

/// index counts into the shared mesh, and which pipeline draws each; each
/// draw starts where the previous one ended (a running sum at queue time),
/// so the ranges are contiguous and disjoint by construction. The order must
/// match the order `build_mesh` appends geometry.
const DRAWS: [(u32, usize); 18] = [
    (18, P_CUBE),       // cube faces 0-2 [0, 18)
    (18, P_CUBE),       // cube faces 3-5 [18, 36)  (the same pipeline, queued twice)
    (18, P_PYRAMID),    // pyramid        [36, 54)
    (18, P_DISC_RED),   // disc sector 1  [54, 72)
    (18, P_DISC_GREEN), // disc sector 2  [72, 90)
    (18, P_DISC_BLUE),  // disc sector 3  [90, 108)
    (36, P_CULL_FRONT), // second cube, front-face culled
    (6, P_BACKDROP),    // queued before the two panels drawn over it
    (6, P_ALPHA),
    (6, P_OPAQUE),
    (6, P_DEPTH_WRITE_OFF), // queued before the panel it fails to occlude
    (6, P_DEPTH_WRITE_ON),
    (6, P_GRID_CLAMP_LINEAR),
    (6, P_GRID_REPEAT_LINEAR),
    (6, P_GRID_CLAMP_NEAREST),
    (6, P_GRID_REPEAT_NEAREST),
    (6, P_GRAY_SRGB),
    (6, P_GRAY_UNORM),
];

/// cube (36) + pyramid (18) + disc (54) + cull cube (36) + 11 panel quads (66);
/// build_mesh must produce exactly this many indices, checked in setup
const INDEX_COUNT: u32 = 210;

const fn draws_total(draws: &[(u32, usize)]) -> u32 {
    let mut sum = 0;
    let mut i = 0;
    while i < draws.len() {
        sum += draws[i].0;
        i += 1;
    }
    sum
}
// with the derived starts, full coverage means the draws tile the index
// buffer exactly
const _: () = assert!(draws_total(&DRAWS) == INDEX_COUNT);

pub struct MultiMesh {
    start_time: Instant,
    #[allow(unused)]
    mesh: MeshHandle<Vertex>,
    specs: Vec<PipelineSpec>,
    pipelines: Vec<(
        PipelineHandle<DrawIndexed>,
        UniformBufferHandle<MultiMeshParams>,
    )>,
}

impl Game for MultiMesh {
    type EditState = ();

    fn window_title() -> &'static str {
        "Multi Mesh"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let (vertices, indices) = build_mesh();

        // ties INDEX_COUNT (which DRAWS provably covers) to the actual mesh
        assert_eq!(indices.len(), INDEX_COUNT as usize);

        // one shared mesh; each pipeline gets empty vertex/index vecs in its
        // Resources and is pointed at the mesh with .with_shared_mesh()
        let mesh = renderer.create_mesh(&vertices, &indices)?;

        let textures = create_textures(renderer)?;

        let specs = pipeline_specs();
        assert_eq!(specs.len(), DRAWS.iter().map(|(_, p)| p + 1).max().unwrap());

        let mut pipelines = vec![];
        for spec in &specs {
            let params_buffer = renderer.create_uniform_buffer::<MultiMeshParams>()?;
            let resources = Resources {
                vertices: vec![],
                indices: vec![],
                texture: &textures[spec.texture],
                params_buffer: &params_buffer,
            };
            let pipeline_config = Shader::init()
                .pipeline_config(resources)
                .with_shared_mesh(&mesh)
                .with_raster_state(spec.raster);
            let pipeline = renderer.create_pipeline(pipeline_config)?;
            pipelines.push((pipeline, params_buffer));
        }

        Ok(Self {
            start_time: Instant::now(),
            mesh,
            specs,
            pipelines,
        })
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = (Instant::now() - self.start_time).as_secs_f32();
        let orbit = orbit_angle(elapsed);
        let aspect_ratio = renderer.aspect_ratio();
        let (view, proj) = camera(orbit, aspect_ratio);
        let inverse_view = view.inverse();
        let models = shape_models(elapsed);

        let mut first_index = 0;
        for (index_count, pipeline_index) in DRAWS {
            let (pipeline, _) = &self.pipelines[pipeline_index];
            renderer.queue_draw_index_range(pipeline, first_index, index_count);
            first_index += index_count;
        }

        renderer.submit_draws(|gpu| {
            for ((_, params_buffer), spec) in self.pipelines.iter_mut().zip(&self.specs) {
                let model = match &spec.placement {
                    Placement::Shape(shape) => models[*shape],
                    Placement::Panel(panel) => panel_model(inverse_view, aspect_ratio, panel),
                };
                let mvp = MVPMatrices { model, view, proj };
                gpu.write_uniform(
                    params_buffer,
                    MultiMeshParams {
                        mvp,
                        tint: spec.tint,
                    },
                );
            }
        })
    }
}

// --- textures ---

/// All test images are procedural, so the example needs no asset files.
fn create_textures(renderer: &mut Renderer) -> anyhow::Result<Vec<TextureHandle>> {
    // reproduces the un-textured look of the original scene exactly
    let white = solid_image(255, 255, 255);
    // 2x2 checkerboard of 4x4 texel cells: big enough features that Nearest
    // vs Linear and ClampToEdge vs Repeat are both unmissable at panel size
    let checker = checker_image();
    // the color-space probe; 128 is far enough from both endpoints that the
    // sRGB transfer moves it a lot
    let gray = solid_image(128, 128, 128);

    // a full mip chain would average an 8x8 checkerboard to flat gray at
    // distance and destroy the wrap/filter test; the Link work needs this
    // path anyway
    let unmipped = |wrap: TextureWrap, filter: TextureFilter| TextureOptions {
        filter,
        wrap_u: wrap,
        wrap_v: wrap,
        mipmaps: false,
        ..Default::default()
    };

    let mut textures =
        vec![renderer.create_texture_with_options("white", &white, TextureOptions::default())?];

    for (name, wrap, filter) in [
        (
            "checker clamp linear",
            TextureWrap::ClampToEdge,
            TextureFilter::Linear,
        ),
        (
            "checker repeat linear",
            TextureWrap::Repeat,
            TextureFilter::Linear,
        ),
        (
            "checker clamp nearest",
            TextureWrap::ClampToEdge,
            TextureFilter::Nearest,
        ),
        (
            "checker repeat nearest",
            TextureWrap::Repeat,
            TextureFilter::Nearest,
        ),
    ] {
        textures.push(renderer.create_texture_with_options(
            name,
            &checker,
            unmipped(wrap, filter),
        )?);
    }

    // identical in every respect except the color space
    for (name, color_space) in [
        ("gray srgb", TextureColorSpace::Srgb),
        ("gray unorm", TextureColorSpace::Unorm),
    ] {
        textures.push(renderer.create_texture_with_options(
            name,
            &gray,
            TextureOptions {
                mipmaps: false,
                color_space,
                ..Default::default()
            },
        )?);
    }

    Ok(textures)
}

const TEXTURE_SIZE: u32 = 8;

fn solid_image(r: u8, g: u8, b: u8) -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        TEXTURE_SIZE,
        TEXTURE_SIZE,
        Rgba([r, g, b, 255]),
    ))
}

fn checker_image() -> DynamicImage {
    const CELL: u32 = TEXTURE_SIZE / 2;

    let image = RgbaImage::from_fn(TEXTURE_SIZE, TEXTURE_SIZE, |x, y| {
        if (x / CELL + y / CELL) % 2 == 0 {
            Rgba([245, 245, 245, 255])
        } else {
            Rgba([20, 20, 60, 255])
        }
    });

    DynamicImage::ImageRgba8(image)
}

// --- camera and placement ---

const ORBIT_DEGREES_PER_SECOND: f32 = 20.0;
const FOV_Y_DEGREES: f32 = 45.0;

fn orbit_angle(elapsed: f32) -> f32 {
    elapsed * ORBIT_DEGREES_PER_SECOND.to_radians()
}

fn camera(orbit: f32, aspect_ratio: f32) -> (Mat4, Mat4) {
    let eye = Mat3::from_rotation_y(orbit) * Vec3::new(0.0, 2.2, 5.5);
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let proj = Mat4::perspective_rh(FOV_Y_DEGREES.to_radians(), aspect_ratio, 0.1, 20.0);

    (view, proj)
}

/// Places a unit-sized panel at its fixed spot on screen: build the placement
/// in view space (where the frustum's half extents at a given depth are just
/// trigonometry) and then lift it into world space with the inverse view. The
/// scale is uniform in both axes so a panel with equal half extents renders
/// square whatever the window's aspect ratio.
fn panel_model(inverse_view: Mat4, aspect_ratio: f32, panel: &Panel) -> Mat4 {
    let half_height = panel.depth * (FOV_Y_DEGREES.to_radians() * 0.5).tan();
    let half_width = half_height * aspect_ratio;

    let center = Vec3::new(
        panel.pos.x * half_height,
        panel.pos.y * half_height,
        -panel.depth,
    );
    // guard against a panel drifting off a very narrow window
    debug_assert!(
        (panel.pos.x.abs() + panel.half.x) * half_height <= half_width,
        "panel is off screen horizontally at this aspect ratio"
    );
    // depth scales with the height too, so the one panel that is a solid
    // (the cull-front cube) stays a cube instead of a box stretched at the
    // camera; the flat quads sit at z == 0 and don't care
    let scale = Vec3::new(
        panel.half.x * half_height,
        panel.half.y * half_height,
        panel.half.y * half_height,
    );

    inverse_view * Mat4::from_translation(center) * Mat4::from_scale(scale)
}

/// world placement and animation live here, not in the vertex data; the
/// shader rotates normals by the model matrix so lighting follows along
fn shape_models(elapsed: f32) -> [Mat4; 3] {
    let cube = Mat4::from_translation(Vec3::new(-2.2, 0.0, 0.0))
        * Mat4::from_rotation_y(elapsed * 40.0f32.to_radians());

    let pyramid = Mat4::from_translation(Vec3::new(2.2, 0.0, 0.0))
        * Mat4::from_rotation_y(elapsed * -60.0f32.to_radians());

    // spins in its own plane, then tilts so the orbiting camera never sees
    // it edge-on
    let disc = Mat4::from_rotation_x(20.0f32.to_radians())
        * Mat4::from_rotation_y(elapsed * 30.0f32.to_radians());

    [cube, pyramid, disc]
}

// --- geometry ---

/// Everything is concatenated into one vertex/index buffer, each object baked
/// centered at the origin in model space, CCW winding, flat per-face normals.
/// The append order must match `DRAWS`, which derives each draw's first index
/// as a running sum.
fn build_mesh() -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = vec![];
    let mut indices = vec![];

    build_cube(&mut vertices, &mut indices, 0.75);
    build_pyramid(&mut vertices, &mut indices);
    build_disc(&mut vertices, &mut indices);

    // the cull-front cube; a unit cube, scaled into place like the panels
    build_cube(&mut vertices, &mut indices, 1.0);

    // Every panel is the same unit quad — its on-screen size lives in the
    // model matrix (see `panel_model`), not here. Only the UVs differ.
    let unit_uv = (Vec2::ZERO, Vec2::ONE);
    // UVs that run a half tile past the edge in every direction, so clamp and
    // repeat cannot look the same
    let overhang_uv = (Vec2::splat(-0.5), Vec2::splat(1.5));

    // blend: wide backdrop, then the alpha and opaque panels over it
    build_panel(&mut vertices, &mut indices, unit_uv);
    build_panel(&mut vertices, &mut indices, unit_uv);
    build_panel(&mut vertices, &mut indices, unit_uv);

    // depth_write: two overlapping panels
    build_panel(&mut vertices, &mut indices, unit_uv);
    build_panel(&mut vertices, &mut indices, unit_uv);

    // wrap x filter row
    for _ in 0..4 {
        build_panel(&mut vertices, &mut indices, overhang_uv);
    }

    // color space pair
    build_panel(&mut vertices, &mut indices, unit_uv);
    build_panel(&mut vertices, &mut indices, unit_uv);

    (vertices, indices)
}

/// quad [p0, p1, p2, p3] in CCW order viewed from outside, with matching UVs
fn push_quad(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    corners: [Vec3; 4],
    normal: Vec3,
    uvs: [Vec2; 4],
) {
    let base = vertices.len() as u32;
    for (position, uv0) in corners.into_iter().zip(uvs) {
        vertices.push(Vertex {
            position,
            normal,
            uv0,
        });
    }
    indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn push_triangle(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, corners: [Vec3; 3]) {
    let normal = (corners[1] - corners[0])
        .cross(corners[2] - corners[0])
        .normalize();
    let base = vertices.len() as u32;
    for position in corners {
        vertices.push(Vertex {
            position,
            normal,
            uv0: Vec2::ZERO,
        });
    }
    indices.extend([base, base + 1, base + 2]);
}

/// The unit quad every test panel is drawn from: centered at the origin in
/// the xy plane facing +Z, corners at +/-1, 6 indices. `panel_model` does the
/// scaling and placing. `uv` spans the quad from its top-left to its
/// bottom-right, so values outside [0, 1] reach the sampler unmodified.
fn build_panel(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, (uv_min, uv_max): (Vec2, Vec2)) {
    let corners = [
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(-1.0, 1.0, 0.0),
    ];
    // v grows downward in image space, so the image's top edge lands on the
    // quad's top edge
    let uvs = [
        Vec2::new(uv_min.x, uv_max.y),
        Vec2::new(uv_max.x, uv_max.y),
        Vec2::new(uv_max.x, uv_min.y),
        Vec2::new(uv_min.x, uv_min.y),
    ];

    push_quad(vertices, indices, corners, Vec3::Z, uvs);
}

/// 24 vertices, 36 indices
fn build_cube(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, h: f32) {
    let corner = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * h;

    let faces: [([Vec3; 4], Vec3); 6] = [
        // +Z
        (
            [
                corner(-1.0, -1.0, 1.0),
                corner(1.0, -1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
            ],
            Vec3::Z,
        ),
        // -Z
        (
            [
                corner(1.0, -1.0, -1.0),
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
                corner(1.0, 1.0, -1.0),
            ],
            Vec3::NEG_Z,
        ),
        // +X
        (
            [
                corner(1.0, -1.0, 1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
            ],
            Vec3::X,
        ),
        // -X
        (
            [
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, -1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
                corner(-1.0, 1.0, -1.0),
            ],
            Vec3::NEG_X,
        ),
        // +Y
        (
            [
                corner(-1.0, 1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(1.0, 1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
            ],
            Vec3::Y,
        ),
        // -Y
        (
            [
                corner(-1.0, -1.0, -1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, -1.0, 1.0),
                corner(-1.0, -1.0, 1.0),
            ],
            Vec3::NEG_Y,
        ),
    ];

    for (corners, normal) in faces {
        push_quad(vertices, indices, corners, normal, [Vec2::ZERO; 4]);
    }
}

/// 16 vertices, 18 indices (4 sides + square base)
fn build_pyramid(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>) {
    let apex = Vec3::new(0.0, 0.9, 0.0);
    let base = [
        Vec3::new(-0.75, -0.6, 0.75),
        Vec3::new(0.75, -0.6, 0.75),
        Vec3::new(0.75, -0.6, -0.75),
        Vec3::new(-0.75, -0.6, -0.75),
    ];

    for i in 0..4 {
        push_triangle(vertices, indices, [base[i], base[(i + 1) % 4], apex]);
    }
    push_quad(
        vertices,
        indices,
        [base[0], base[3], base[2], base[1]],
        Vec3::NEG_Y,
        [Vec2::ZERO; 4],
    );
}

/// 20 vertices (center + 19 rim), 54 indices: 18 wedges laid out wedge-major
/// so the three 120° sectors are contiguous index ranges; flat in the xz
/// plane facing +Y (the tilt lives in the disc's model matrix)
fn build_disc(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>) {
    const WEDGES: u32 = 18;
    const RADIUS: f32 = 1.2;

    let normal = Vec3::Y;

    let base = vertices.len() as u32;
    vertices.push(Vertex {
        position: Vec3::ZERO,
        normal,
        uv0: Vec2::ZERO,
    });
    // 19 rim vertices: rim 18 duplicates rim 0's position so wedge 17 doesn't
    // need to wrap around
    for i in 0..=WEDGES {
        let angle = i as f32 / WEDGES as f32 * std::f32::consts::TAU;
        let position = Vec3::new(angle.cos(), 0.0, angle.sin()) * RADIUS;
        vertices.push(Vertex {
            position,
            normal,
            uv0: Vec2::ZERO,
        });
    }

    for i in 0..WEDGES {
        // (center, rim i+1, rim i) is CCW viewed from above (+Y)
        indices.extend([base, base + 1 + i + 1, base + 1 + i]);
    }
}
