use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3};

use mltrs::game::Game;
use mltrs::ktx::load_ktx2_texture;
use mltrs::renderer::{
    DrawIndexed, PipelineHandle, TextureFilter, TextureHandle, UniformBufferHandle,
};
use mltrs::{Input, manifest_path};

mod generated;
use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::cards::*;

fn main() -> Result<(), anyhow::Error> {
    CardViewer::run()
}

struct CardViewer {
    pipeline: PipelineHandle<DrawIndexed>,
    texture: TextureHandle,
    params_buffer: UniformBufferHandle<CardViewerParams>,
    mouse: Vec2,
}

impl Game for CardViewer {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Card Viewer"
    }

    fn setup(renderer: &mut mltrs::renderer::Renderer, shaders: Self::Atlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        const IMAGE_FILE_NAME: &str = "texture.ktx2";
        let file_path = manifest_path!["textures", IMAGE_FILE_NAME];
        let texture = load_ktx2_texture(renderer, &file_path, TextureFilter::Linear)?;

        let params_buffer = renderer.create_uniform_buffer::<CardViewerParams>()?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };
        let pipeline_config = shaders
            .cards
            .pipeline_config(resources)
            .with_vertices(VERTICES.to_vec(), INDICES.to_vec());
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            pipeline,
            texture,
            params_buffer,
            mouse: Default::default(),
        })
    }

    fn draw(
        &mut self,
        renderer: mltrs::renderer::FrameRenderer,
    ) -> Result<(), mltrs::renderer::DrawError> {
        let window_resolution = renderer.window_resolution();
        let aspect_ratio = renderer.aspect_ratio();

        let mvp = make_mvp_matrices(self.mouse, window_resolution, aspect_ratio);
        let params = CardViewerParams {
            mvp,
            texture: self.texture.bindless_handle(),
            _padding_0: Default::default(),
        };

        renderer.draw_indexed(&self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }

    fn input(&mut self, input: Input) {
        if let Input::MouseMotion { x, y } = input {
            self.mouse = Vec2::new(x, y);
        }
    }
}

// one square in clockwise order
const VERTICES: [Vertex; 4] = [
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
];

// 1 quads of clockwise triangles using the vertices above
const INDICES: [u32; 6] = [0, 1, 2, 2, 3, 0];

fn make_mvp_matrices(mouse: Vec2, window_resolution: Vec2, aspect_ratio: f32) -> MVPMatrices {
    const MAX_TILT_RADIANS: f32 = 15.0_f32.to_radians();

    // mouse offset from screen center, normalized to [-1, 1], y down
    let offset = ((mouse - 0.5 * window_resolution) / (0.5 * window_resolution))
        .clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
    let model = Mat4::from_rotation_y(offset.x * MAX_TILT_RADIANS)
        * Mat4::from_rotation_x(offset.y * MAX_TILT_RADIANS);

    let eye = Vec3::new(0.0, 0.0, 2.0);
    let view = look_at_mat4(eye, Vec3::ZERO, -Vec3::Y);
    const FOV_Y_RADIANS: f32 = 45.0_f32.to_radians();
    let proj = directx::perspective(FOV_Y_RADIANS, aspect_ratio, 0.1, 10.0);

    MVPMatrices { model, view, proj }
}
