use std::time::{Duration, Instant};

use glam::{Mat4, Vec2, Vec3};

use vulkan_slang_renderer::game::Game;
use vulkan_slang_renderer::ktx::load_ktx2;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawIndexed, FrameRenderer, PipelineHandle, Renderer, TextureFilter, TextureHandle,
    UniformBufferHandle,
};
use vulkan_slang_renderer::util::manifest_path;

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::suzanne::*;

fn main() -> Result<(), anyhow::Error> {
    Suzanne::run()
}

#[allow(unused)]
pub struct Suzanne {
    start_time: Instant,
    pipeline: PipelineHandle<DrawIndexed>,
    textures: Vec<TextureHandle>,
    params_buffer: UniformBufferHandle<SuzanneParams>,
}

impl Suzanne {
    fn load_vertices() -> anyhow::Result<(Vec<Vertex>, Vec<u32>)> {
        let file_path = manifest_path(["models", "suzanne", "suzanne.obj"]);

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

impl Game for Suzanne {
    type EditState = ();

    fn window_title() -> &'static str {
        "Suzanne"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let (vertices, indices) = Self::load_vertices()?;

        // converted from KTX1 with: ktx2ktx2 -f models/suzanne/suzanne{0,1,2}.ktx
        let mut textures = Vec::new();
        for i in 0..3 {
            let file_name = format!("suzanne{i}.ktx2");
            let file_path = manifest_path(["models", "suzanne", &file_name]);
            let ktx = load_ktx2(&file_path)?;
            let texture = renderer.create_texture_with_mips(
                &ktx.source_file_name,
                ktx.format,
                ktx.extent,
                &ktx.mip_slices(),
                TextureFilter::Linear,
            )?;
            textures.push(texture);
        }

        let shader_atlas = ShaderAtlas::init();
        let shader = shader_atlas.suzanne;

        let params_buffer = renderer.create_uniform_buffer::<SuzanneParams>()?;
        let resources = Resources {
            vertices,
            indices,
            texture0: &textures[0],
            texture1: &textures[1],
            texture2: &textures[2],
            params_buffer: &params_buffer,
        };
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let start_time = Instant::now();

        Ok(Self {
            start_time,
            pipeline,
            textures,
            params_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = Instant::now() - self.start_time;
        let aspect_ratio = renderer.aspect_ratio();
        let mvp = make_mvp_matrices(elapsed, aspect_ratio);
        let params = SuzanneParams {
            mvp,
            time: elapsed.as_secs_f32(),
            _padding_0: Default::default(),
        };

        renderer.draw_indexed(&self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }
}

fn make_mvp_matrices(elapsed: Duration, aspect_ratio: f32) -> MVPMatrices {
    const TURN_DEGREES_PER_SECOND: f32 = 20.0;
    const FOV_DEGREES: f32 = 45.0;

    let turn_radians = elapsed.as_secs_f32() * TURN_DEGREES_PER_SECOND.to_radians();

    // Blender's monkey faces +Z in obj coordinates; spin it around +Y
    let model = Mat4::from_rotation_y(turn_radians);
    let eye = Vec3::new(0.0, 0.5, 3.0);
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let fov_y_radians = FOV_DEGREES.to_radians();
    let proj = Mat4::perspective_rh(fov_y_radians, aspect_ratio, 0.1, 10.0);

    MVPMatrices { model, view, proj }
}
