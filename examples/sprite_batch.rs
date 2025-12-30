//! Based on the official SDL_gpu example here:
//! https://github.com/TheSpydog/SDL_gpu_examples/blob/main/Examples/PullSpriteBatch.c
//! https://github.com/TheSpydog/SDL_gpu_examples/blob/main/Content/Shaders/Source/PullSpriteBatch.vert.hlsl
//! https://github.com/TheSpydog/SDL_gpu_examples/blob/main/Content/Shaders/Source/TexturedQuad.frag.hlsl
//!
//! which uses the method described in this blog post:
//! https://moonside.games/posts/sdl-gpu-sprite-batcher/

use std::f32::consts::TAU;
use std::time::Duration;

use glam::{Mat4, Vec2, Vec3, Vec4};
use sdl3::sys::everything::{SDL_rand, SDL_randf, SDL_srand};

use vulkan_slang_renderer::game::Game;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, StorageBufferHandle,
    TextureFilter, UniformBufferHandle,
};
use vulkan_slang_renderer::util::load_image;

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::sprite_batch::*;

fn main() -> Result<(), anyhow::Error> {
    SpriteBatch::run()
}

pub struct SpriteBatch {
    pipeline: PipelineHandle<DrawVertexCount>,
    params_buffer: UniformBufferHandle<SpriteBatchParams>,
    sprites_buffer: StorageBufferHandle<Sprite>,
    sprites: Vec<Sprite>,
}

const SPRITE_COUNT: usize = 8192;

impl Game for SpriteBatch {
    fn window_title() -> &'static str {
        "Sprite Batch"
    }

    fn frame_delay(&self) -> Duration {
        Duration::from_nanos(10)
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let mut sprites = Vec::with_capacity(SPRITE_COUNT);
        for _ in 0..SPRITE_COUNT {
            let sprite = init_sprite();
            sprites.push(sprite);
        }

        unsafe { SDL_srand(0) };

        let params_buffer = renderer.create_uniform_buffer::<SpriteBatchParams>()?;
        let sprites_buffer = renderer.create_storage_buffer::<Sprite>(sprites.len() as u32)?;

        let image_file_name = "ravioli_atlas.bmp";
        let image = load_image(image_file_name)?;
        let texture = renderer.create_texture(image_file_name, &image, TextureFilter::Nearest)?;

        let resources = Resources {
            sprites: &sprites_buffer,
            params_buffer: &params_buffer,
            texture: &texture,
        };

        let shader = ShaderAtlas::init().sprite_batch;
        let mut pipeline_config = shader.pipeline_config(resources);
        pipeline_config.disable_depth_test = true;
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            pipeline,
            params_buffer,
            sprites_buffer,
            sprites,
        })
    }

    fn update(&mut self) {
        let window_size = Self::initial_window_size();

        for sprite in &mut self.sprites {
            randomize_sprite(sprite, window_size);
        }
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let (width, height) = Self::initial_window_size();
        let projection_matrix =
            Mat4::orthographic_lh(0.0, width as f32, height as f32, 0.0, 0.0, -1.0);
        let params = SpriteBatchParams { projection_matrix };
        // 6 = the corners in 2 triangles to make a quad
        let vertex_count = self.sprites.len() as u32 * 6;

        renderer.draw_vertex_count(&self.pipeline, vertex_count, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
            gpu.write_storage(&mut self.sprites_buffer, &self.sprites);
        })
    }
}

fn randomize_sprite(sprite: &mut Sprite, (width, height): (u32, u32)) {
    // the U and V offsets into the sprite sheet for the 4 sprites
    const U_COORDS: [f32; 4] = [0.0, 0.5, 0.0, 0.5];
    const V_COORDS: [f32; 4] = [0.0, 0.0, 0.5, 0.5];

    sprite.position.x = unsafe { SDL_rand(width as i32) } as f32;
    sprite.position.y = unsafe { SDL_rand(height as i32) } as f32;

    sprite.rotation = unsafe { SDL_randf() } * TAU;

    let sprite_index = unsafe { SDL_rand(4) } as usize;
    sprite.tex_u = U_COORDS[sprite_index];
    sprite.tex_v = V_COORDS[sprite_index];
}

fn init_sprite() -> Sprite {
    Sprite {
        // to be overwritten by 'randomize_sprite'
        position: Vec3::ZERO,
        rotation: 0.0,
        tex_u: 0.0,
        tex_v: 0.0,

        // constants
        scale: Vec2::splat(32.0),
        padding: Vec2::ZERO,
        tex_w: 0.5,
        tex_h: 0.5,
        color: Vec4::splat(1.0),
    }
}
