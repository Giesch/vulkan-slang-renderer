use std::path::PathBuf;
use std::time::{Duration, Instant};

use glam::{Mat4, Vec2, Vec3};

use vulkan_slang_renderer::game::Game;
use vulkan_slang_renderer::renderer::{
    PipelineHandle, Renderer, TextureHandle, UniformBufferHandle,
};
use vulkan_slang_renderer::shaders::COLUMN_MAJOR;
use vulkan_slang_renderer::util::load_image;

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::depth_texture::*;

fn main() -> Result<(), anyhow::Error> {
    VikingRoom::run()
}

#[allow(unused)]
pub struct VikingRoom {
    start_time: Instant,
    pipeline: PipelineHandle,
    texture: TextureHandle,
    uniform_buffer: UniformBufferHandle<DepthTexture>,
}

impl VikingRoom {
    // From unknownue's rust version of the vulkan tutorial
    // https://github.com/unknownue/vulkan-tutorial-rust/blob/master/src/tutorials/27_model_loading.rs
    fn load_vertices() -> anyhow::Result<(Vec<Vertex>, Vec<u32>)> {
        let file_path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "models", "viking_room.obj"]
            .iter()
            .collect();

        let (mut models, _materials) = tobj::load_obj(file_path, &tobj::GPU_LOAD_OPTIONS)?;

        debug_assert!(models.len() == 1);
        let model = models.remove(0);

        let mut vertices = vec![];
        let mesh = model.mesh;
        let vertices_count = mesh.positions.len() / 3;
        for i in 0..vertices_count {
            let position = {
                let offset = i * 3;
                Vec3::new(
                    mesh.positions[offset],
                    mesh.positions[offset + 1],
                    mesh.positions[offset + 2],
                )
            };

            let tex_coord = {
                let offset = i * 2;
                let u = mesh.texcoords[offset];
                // in obj, 0 is the bottom, in vulkan, 0 is the top
                // (for texture coordinates)
                let v = 1.0 - mesh.texcoords[offset + 1];
                Vec2::new(u, v)
            };

            let vertex = Vertex {
                position,
                color: Vec3::splat(1.0),
                tex_coord,
            };

            vertices.push(vertex);
        }

        Ok((vertices, mesh.indices))
    }
}

impl Game for VikingRoom {
    fn window_title() -> &'static str {
        "Viking Room"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let (vertices, indices) = Self::load_vertices()?;

        const IMAGE_FILE_NAME: &str = "viking_room.png";
        let image = load_image(IMAGE_FILE_NAME)?;

        let shader_atlas = ShaderAtlas::init();
        let shader = shader_atlas.depth_texture;

        let texture = renderer.create_texture(IMAGE_FILE_NAME, &image)?;
        let uniform_buffer = renderer.create_uniform_buffer::<DepthTexture>()?;
        let resources = Resources {
            vertices,
            indices,
            texture: &texture,
            depth_texture_buffer: &uniform_buffer,
        };
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let start_time = Instant::now();

        Ok(Self {
            start_time,
            pipeline,
            texture,
            uniform_buffer,
        })
    }

    fn draw_frame(&mut self, renderer: &mut Renderer) -> anyhow::Result<()> {
        let aspect_ratio = renderer.aspect_ratio();
        renderer.draw_frame(&self.pipeline, |gpu| {
            let elapsed = Instant::now() - self.start_time;
            let mvp = make_mvp_matrices(elapsed, aspect_ratio);
            gpu.write_uniform(&mut self.uniform_buffer, DepthTexture { mvp });
        })
    }
}

fn make_mvp_matrices(elapsed: Duration, aspect_ratio: f32) -> MVPMatrices {
    const TURN_DEGREES_PER_SECOND: f32 = 5.0;
    const STARTING_ANGLE_DEGREES: f32 = 45.0;

    let turn_radians = elapsed.as_secs_f32() * TURN_DEGREES_PER_SECOND.to_radians();

    let model = Mat4::from_rotation_z(turn_radians);
    let eye = Vec3::splat(2.0);
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Z);
    let fov_y_radians = STARTING_ANGLE_DEGREES.to_radians();
    let proj = Mat4::perspective_rh(fov_y_radians, aspect_ratio, 0.1, 10.0);

    normalize_mvp(MVPMatrices { model, view, proj })
}

fn normalize_mvp(mut mvp: MVPMatrices) -> MVPMatrices {
    // "GLM was originally designed for OpenGL,
    // where the Y coordinate of the clip coordinates is inverted.
    // The easiest way to compensate for that is to flip the sign
    // on the scaling factor of the Y axis in the projection matrix.
    // If you don’t do this, then the image will be rendered upside down."
    // https://docs.vulkan.org/tutorial/latest/05_Uniform_buffers/00_Descriptor_set_layout_and_buffer.html
    mvp.proj.y_axis.y *= -1.0;

    // GLM & glam use column-major matrices, but D3D12 and Slang use row-major by default
    // it's also possible to avoid the transpose by reversing the mul() calls in shaders
    // https://discord.com/channels/1303735196696445038/1395879559827816458/1396913440584634499
    if !COLUMN_MAJOR {
        mvp.model = mvp.model.transpose();
        mvp.view = mvp.view.transpose();
        mvp.proj = mvp.proj.transpose();
    }

    mvp
}
