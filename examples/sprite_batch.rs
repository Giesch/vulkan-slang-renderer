//! Based on the official example in C here:
//! https://github.com/TheSpydog/SDL_gpu_examples/blob/main/Examples/PullSpriteBatch.c
//! https://github.com/TheSpydog/SDL_gpu_examples/blob/main/Content/Shaders/Source/TexturedQuad.frag.hlsl
//! https://github.com/TheSpydog/SDL_gpu_examples/blob/main/Content/Shaders/Source/TexturedQuad.frag.hlsl
//!
//! which uses the method described in this blog post:
//! https://moonside.games/posts/sdl-gpu-sprite-batcher/

use glam::{Mat4, Vec2, Vec3, Vec4};

use vulkan_slang_renderer::game::Game;
use vulkan_slang_renderer::renderer::{
    PipelineHandle, Renderer, StorageBufferHandle, UniformBufferHandle,
};
use vulkan_slang_renderer::util::load_image;

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::sprite_batch::*;

fn main() -> Result<(), anyhow::Error> {
    SpriteBatch::run()
}

pub struct SpriteBatch {
    pipeline: PipelineHandle,
    uniform_buffer: UniformBufferHandle<SpriteBatchParams>,
    storage_buffer: StorageBufferHandle<Sprite>,
    sprites: Vec<Sprite>,
}

const SPRITE_COUNT: usize = 8192;

impl Game for SpriteBatch {
    fn window_title() -> &'static str {
        "Sprite Batch"
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

        let uniform_buffer = renderer.create_uniform_buffer::<SpriteBatchParams>()?;
        let storage_buffer = renderer.create_storage_buffer::<Sprite>(sprites.len() as u32)?;

        const IMAGE_FILE_NAME: &str = "ravioli_atlas.bmp";
        let image = load_image(IMAGE_FILE_NAME)?;
        let texture = renderer.create_texture(IMAGE_FILE_NAME, &image)?;

        let resources = Resources {
            vertex_count: sprites.len() as u32 * 6,
            sprites: &storage_buffer,
            params_buffer: &uniform_buffer,
            texture: &texture,
        };

        let shader = ShaderAtlas::init().sprite_batch;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            pipeline,
            uniform_buffer,
            storage_buffer,
            sprites,
        })
    }

    fn update(&mut self) {
        let window_size = Self::window_size();

        for sprite in &mut self.sprites {
            randomize_sprite(sprite, window_size);
        }
    }

    fn draw_frame(&mut self, renderer: &mut Renderer) -> anyhow::Result<()> {
        let (width, height) = Self::window_size();
        let projection_matrix =
            Mat4::orthographic_lh(0.0, width as f32, height as f32, 0.0, 0.0, -1.0);
        let uniform_data = SpriteBatchParams { projection_matrix };

        renderer.draw_frame(&self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.uniform_buffer, uniform_data);
            gpu.write_storage(&mut self.storage_buffer, &self.sprites);
        })
    }
}

fn randomize_sprite(sprite: &mut Sprite, (width, height): (u32, u32)) {
    use sdl3::sys::everything::{SDL_rand, SDL_randf};

    // the U and V offsets into the sprite sheet for the 4 sprites
    const U_COORDS: [f32; 4] = [0.0, 0.5, 0.0, 0.5];
    const V_COORDS: [f32; 4] = [0.0, 0.0, 0.5, 0.5];

    sprite.position.x = unsafe { SDL_rand(width as i32) } as f32;
    sprite.position.y = unsafe { SDL_rand(height as i32) } as f32;

    sprite.rotation = unsafe { SDL_randf() } * std::f32::consts::TAU;

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
