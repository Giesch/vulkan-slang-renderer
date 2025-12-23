use std::time::{Duration, Instant};

use glam::{Mat4, Vec2, Vec3};

use vulkan_slang_renderer::game::Game;
use vulkan_slang_renderer::renderer::{
    DrawError, FrameRenderer, PipelineHandle, Renderer, TextureFilter, TextureHandle,
    UniformBufferHandle,
};
use vulkan_slang_renderer::shaders::COLUMN_MAJOR;
use vulkan_slang_renderer::util::load_image;

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::depth_texture::*;

fn main() -> Result<(), anyhow::Error> {
    DepthTextureGame::run()
}

#[allow(unused)]
pub struct DepthTextureGame {
    start_time: Instant,
    pipeline: PipelineHandle,
    texture: TextureHandle,
    uniform_buffer: UniformBufferHandle<DepthTexture>,
}

const VERTICES: [Vertex; 8] = [
    Vertex {
        position: Vec3::new(-0.5, -0.5, 0.0),
        color: Vec3::new(1.0, 0.0, 0.0),
        tex_coord: Vec2::new(1.0, 0.0),
    },
    Vertex {
        position: Vec3::new(0.5, -0.5, 0.0),
        color: Vec3::new(0.0, 1.0, 0.0),
        tex_coord: Vec2::new(0.0, 0.0),
    },
    Vertex {
        position: Vec3::new(0.5, 0.5, 0.0),
        color: Vec3::new(0.0, 0.0, 1.0),
        tex_coord: Vec2::new(0.0, 1.0),
    },
    Vertex {
        position: Vec3::new(-0.5, 0.5, 0.0),
        color: Vec3::new(1.0, 1.0, 1.0),
        tex_coord: Vec2::new(1.0, 1.0),
    },
    Vertex {
        position: Vec3::new(-0.5, -0.5, -0.5),
        color: Vec3::new(1.0, 0.0, 0.0),
        tex_coord: Vec2::new(1.0, 0.0),
    },
    Vertex {
        position: Vec3::new(0.5, -0.5, -0.5),
        color: Vec3::new(0.0, 1.0, 0.0),
        tex_coord: Vec2::new(0.0, 0.0),
    },
    Vertex {
        position: Vec3::new(0.5, 0.5, -0.5),
        color: Vec3::new(0.0, 0.0, 1.0),
        tex_coord: Vec2::new(0.0, 1.0),
    },
    Vertex {
        position: Vec3::new(-0.5, 0.5, -0.5),
        color: Vec3::new(1.0, 1.0, 1.0),
        tex_coord: Vec2::new(1.0, 1.0),
    },
];

#[rustfmt::skip]
const INDICES: [u32; 12] = [
    0, 1, 2, 2, 3, 0,
    4, 5, 6, 6, 7, 4,
];

impl Game for DepthTextureGame {
    fn window_title() -> &'static str {
        "Depth Texture"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        const IMAGE_FILE_NAME: &str = "texture.jpg";
        let image = load_image(IMAGE_FILE_NAME)?;

        let shader_atlas = ShaderAtlas::init();
        let shader = shader_atlas.depth_texture;

        let texture = renderer.create_texture(IMAGE_FILE_NAME, &image, TextureFilter::Linear)?;
        let uniform_buffer = renderer.create_uniform_buffer::<DepthTexture>()?;
        let resources = Resources {
            vertices: VERTICES.to_vec(),
            indices: INDICES.to_vec(),
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

    fn draw_frame(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let aspect_ratio = renderer.aspect_ratio();
        let elapsed = Instant::now() - self.start_time;
        let mvp = make_mvp_matrices(elapsed, aspect_ratio, COLUMN_MAJOR);
        let uniform_data = DepthTexture { mvp };

        renderer.draw_frame(&self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.uniform_buffer, uniform_data);
        })
    }
}

fn make_mvp_matrices(elapsed: Duration, aspect_ratio: f32, column_major: bool) -> MVPMatrices {
    const TURN_DEGREES_PER_SECOND: f32 = 5.0;
    const STARTING_ANGLE_DEGREES: f32 = 45.0;

    let turn_radians = elapsed.as_secs_f32() * TURN_DEGREES_PER_SECOND.to_radians();

    let model = Mat4::from_rotation_z(turn_radians);
    let eye = Vec3::splat(2.0);
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Z);
    let fov_y_radians = STARTING_ANGLE_DEGREES.to_radians();
    let proj = Mat4::perspective_rh(fov_y_radians, aspect_ratio, 0.1, 10.0);

    let mut mvp = MVPMatrices { model, view, proj };

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
    if !column_major {
        mvp.model = mvp.model.transpose();
        mvp.view = mvp.view.transpose();
        mvp.proj = mvp.proj.transpose();
    }

    mvp
}
