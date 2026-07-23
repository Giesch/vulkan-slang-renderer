//! Demonstrates the multi-draw queue and shared meshes: three shapes are
//! concatenated into one mesh, each baked centered at the origin, and each
//! placed and animated by its own model matrix. Six queued index-range draws
//! tile the index buffer exactly.
//!
//! Uniform buffers are per-pipeline, so every draw sharing a pipeline must
//! share a model matrix: distinctly-moving shapes need their own pipelines,
//! one per (shape, color) pair here. That per-object pipeline growth is a
//! consequence of this renderer's no-per-draw-uniform design, not a general
//! rule — a single articulated model (like the planned toon_link example)
//! shares one transform across all of its material pipelines.

use std::time::Instant;

use glam::{Mat3, Mat4, Vec3, Vec4};

use vulkan_slang_renderer::game::Game;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawIndexed, FrameRenderer, MeshHandle, PipelineHandle, Renderer,
    UniformBufferHandle,
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

/// (tint, shape) per pipeline; the shape determines which model matrix the
/// pipeline's uniform carries each frame
const PIPELINES: [(Vec4, usize); 5] = [
    (RED, CUBE),
    (GREEN, PYRAMID),
    (RED, DISC),
    (GREEN, DISC),
    (BLUE, DISC),
];

/// index counts into the shared mesh, and which pipeline draws each; each
/// draw starts where the previous one ended (a running sum at queue time),
/// so the ranges are contiguous and disjoint by construction
const DRAWS: [(u32, usize); 6] = [
    (18, 0), // cube faces 0-2 [0, 18)    -> red
    (18, 0), // cube faces 3-5 [18, 36)   (the same pipeline, queued twice)
    (18, 1), // pyramid        [36, 54)   -> green
    (18, 2), // disc sector 1  [54, 72)   -> red
    (18, 3), // disc sector 2  [72, 90)   -> green
    (18, 4), // disc sector 3  [90, 108)  -> blue
];

/// cube (36) + pyramid (18) + disc (54); build_mesh must produce exactly this
/// many indices, checked in setup
const INDEX_COUNT: u32 = 108;

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

        let mut pipelines = vec![];
        for _ in PIPELINES {
            let params_buffer = renderer.create_uniform_buffer::<MultiMeshParams>()?;
            let resources = Resources {
                vertices: vec![],
                indices: vec![],
                params_buffer: &params_buffer,
            };
            let pipeline_config = Shader::init()
                .pipeline_config(resources)
                .with_shared_mesh(&mesh);
            let pipeline = renderer.create_pipeline(pipeline_config)?;
            pipelines.push((pipeline, params_buffer));
        }

        Ok(Self {
            start_time: Instant::now(),
            mesh,
            pipelines,
        })
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = (Instant::now() - self.start_time).as_secs_f32();
        let (view, proj) = camera(elapsed, renderer.aspect_ratio());
        let models = shape_models(elapsed);

        let mut first_index = 0;
        for (index_count, pipeline_index) in DRAWS {
            let (pipeline, _) = &self.pipelines[pipeline_index];
            renderer.queue_draw_index_range(pipeline, first_index, index_count);
            first_index += index_count;
        }

        renderer.submit_draws(|gpu| {
            for ((_, params_buffer), (tint, shape)) in self.pipelines.iter_mut().zip(PIPELINES) {
                let mvp = MVPMatrices {
                    model: models[shape],
                    view,
                    proj,
                };
                gpu.write_uniform(params_buffer, MultiMeshParams { mvp, tint });
            }
        })
    }
}

fn camera(elapsed: f32, aspect_ratio: f32) -> (Mat4, Mat4) {
    const ORBIT_DEGREES_PER_SECOND: f32 = 20.0;

    let orbit = elapsed * ORBIT_DEGREES_PER_SECOND.to_radians();
    let eye = Mat3::from_rotation_y(orbit) * Vec3::new(0.0, 2.2, 5.5);
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let proj = Mat4::perspective_rh(45.0f32.to_radians(), aspect_ratio, 0.1, 20.0);

    (view, proj)
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

/// cube + pyramid + disc concatenated into one vertex/index buffer, each
/// centered at the origin in model space, CCW winding, flat per-face normals
fn build_mesh() -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = vec![];
    let mut indices = vec![];

    build_cube(&mut vertices, &mut indices, 0.75);
    build_pyramid(&mut vertices, &mut indices);
    build_disc(&mut vertices, &mut indices);

    (vertices, indices)
}

/// quad [p0, p1, p2, p3] in CCW order viewed from outside
fn push_quad(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, corners: [Vec3; 4], normal: Vec3) {
    let base = vertices.len() as u32;
    for position in corners {
        vertices.push(Vertex { position, normal });
    }
    indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn push_triangle(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, corners: [Vec3; 3]) {
    let normal = (corners[1] - corners[0])
        .cross(corners[2] - corners[0])
        .normalize();
    let base = vertices.len() as u32;
    for position in corners {
        vertices.push(Vertex { position, normal });
    }
    indices.extend([base, base + 1, base + 2]);
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
        push_quad(vertices, indices, corners, normal);
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
    });
    // 19 rim vertices: rim 18 duplicates rim 0's position so wedge 17 doesn't
    // need to wrap around
    for i in 0..=WEDGES {
        let angle = i as f32 / WEDGES as f32 * std::f32::consts::TAU;
        let position = Vec3::new(angle.cos(), 0.0, angle.sin()) * RADIUS;
        vertices.push(Vertex { position, normal });
    }

    for i in 0..WEDGES {
        // (center, rim i+1, rim i) is CCW viewed from above (+Y)
        indices.extend([base, base + 1 + i + 1, base + 1 + i]);
    }
}
