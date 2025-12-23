use glam::{Mat4, Vec2, Vec3, Vec4};

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, FrameRenderer, PipelineHandle, Renderer, StorageBufferHandle, TextureFilter,
    UniformBufferHandle,
};
use vulkan_slang_renderer::shaders::COLUMN_MAJOR;
use vulkan_slang_renderer::util::load_image;

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::space_invaders::*;

fn main() -> Result<(), anyhow::Error> {
    SpaceInvaders::run()
}

struct SpaceInvaders {
    pipeline: PipelineHandle,
    uniform_buffer: UniformBufferHandle<SpaceInvadersParams>,
    storage_buffer: StorageBufferHandle<Sprite>,
    sprites: Vec<Sprite>,
    player_intent: PlayerIntent,
}

impl Game for SpaceInvaders {
    fn window_title() -> &'static str {
        "Space Invaders"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        // json["meta"]["size"]["w"]
        let player_sheet_width = 128.0;
        // json["frames"][0]["sourceSize"]["w"]
        let player_frame_width = 32.0;

        let player_sprite = Sprite {
            scale: Vec2::splat(player_frame_width * 5.0),
            texture_id: 0,
            padding: 0,

            position: Vec3::ZERO,
            rotation: 0.0,

            tex_u: 0.0,
            tex_v: 0.0,
            tex_w: player_frame_width / player_sheet_width,
            tex_h: 1.0,

            color: Vec4::splat(1.0),
        };
        let sprites = vec![player_sprite];

        let uniform_buffer = renderer.create_uniform_buffer::<SpaceInvadersParams>()?;
        let storage_buffer = renderer.create_storage_buffer::<Sprite>(sprites.len() as u32)?;

        let image_file_name = "space_invaders/space_invaders_player.png";
        let image = load_image(image_file_name)?;
        let player_texture =
            renderer.create_texture(image_file_name, &image, TextureFilter::Nearest)?;

        let resources = Resources {
            vertex_count: sprites.len() as u32 * 6,
            sprites: &storage_buffer,
            player_texture: &player_texture,
            params_buffer: &uniform_buffer,
        };

        let shader = ShaderAtlas::init().space_invaders;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            pipeline,
            uniform_buffer,
            storage_buffer,
            sprites,
            player_intent: Default::default(),
        })
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::KeyUp(key) => match key {
                Key::W => self.player_intent.up = false,
                Key::A => self.player_intent.left = false,
                Key::S => self.player_intent.down = false,
                Key::D => self.player_intent.right = false,
                Key::Space => {}
            },

            Input::KeyDown(key) => match key {
                Key::W => self.player_intent.up = true,
                Key::A => self.player_intent.left = true,
                Key::S => self.player_intent.down = true,
                Key::D => self.player_intent.right = true,
                Key::Space => {}
            },
        }
    }

    fn update(&mut self) {
        let player_movement = self.player_intent.to_vec();
        for sprite in &mut self.sprites {
            if sprite.texture_id != 0 {
                continue;
            }

            sprite.position.x += player_movement.x;
            sprite.position.y += player_movement.y;
        }
    }

    fn draw_frame(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let (width, height) = renderer.window_size();
        let mut projection_matrix = Mat4::orthographic_lh(0.0, width, height, 0.0, 0.0, -1.0);
        if !COLUMN_MAJOR {
            projection_matrix = projection_matrix.transpose();
        }
        let uniform_data = SpaceInvadersParams { projection_matrix };

        renderer.draw_frame(&mut self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.uniform_buffer, uniform_data);
            gpu.write_storage(&mut self.storage_buffer, &self.sprites);
        })
    }
}

#[derive(Default)]
struct PlayerIntent {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

impl PlayerIntent {
    fn to_vec(&self) -> Vec2 {
        let mut player_intent = Vec2::ZERO;
        if self.up {
            player_intent.y += 1.0;
        }
        if self.down {
            player_intent.y -= 1.0;
        }
        if self.right {
            player_intent.x += 1.0;
        }
        if self.left {
            player_intent.x -= 1.0;
        }

        let player_speed = 10.0;

        player_intent.normalize_or_zero() * player_speed
    }
}
