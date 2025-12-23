use glam::{Mat4, Vec2, Vec3, Vec4};

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, FrameRenderer, PipelineHandle, Renderer, StorageBufferHandle, TextureFilter,
    TextureHandle, UniformBufferHandle,
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
        // from 'meta' section of sprite_sheet.json
        let sprite_sheet_size = (480.0, 160.0);
        // from the 'frames' section of sprite_sheet.json
        let ship_frame = SpriteFrameOffsets {
            width: 32.0,
            height: 32.0,
            x: 0.0,
            y: 128.0,
        };
        let bug_frame = SpriteFrameOffsets {
            width: 32.0,
            height: 32.0,
            x: 0.0,
            y: 64.0,
        };

        let player_sprite = init_sprite(sprite_sheet_size, ship_frame, Vec3::ZERO);
        let bug_sprite = init_sprite(sprite_sheet_size, bug_frame, Vec3::new(400.0, 700.0, 0.0));
        let sprites = vec![player_sprite, bug_sprite];

        let uniform_buffer = renderer.create_uniform_buffer::<SpaceInvadersParams>()?;
        let storage_buffer = renderer.create_storage_buffer::<Sprite>(sprites.len() as u32)?;

        let sprite_sheet_texture = load_texture(renderer, "sprite_sheet.png")?;

        let resources = Resources {
            vertex_count: sprites.len() as u32 * 6,
            sprites: &storage_buffer,
            sprite_sheet: &sprite_sheet_texture,
            params_buffer: &uniform_buffer,
        };

        let shader = ShaderAtlas::init().space_invaders;
        let mut pipeline_config = shader.pipeline_config(resources);
        pipeline_config.disable_depth_test = true;
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

        // TODO find a better way to distinguish this?
        let player_sprite = &mut self.sprites[0];
        player_sprite.position.x += player_movement.x;
        player_sprite.position.y += player_movement.y;
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

const SPRITE_SCALE: f32 = 5.0;

struct SpriteFrameOffsets {
    width: f32,
    height: f32,
    x: f32,
    y: f32,
}

fn init_sprite(
    (sheet_width, sheet_height): (f32, f32),
    frame: SpriteFrameOffsets,
    position: Vec3,
) -> Sprite {
    Sprite {
        scale: Vec2::splat(frame.width * SPRITE_SCALE),
        padding: Vec2::ZERO,

        position,
        rotation: 0.0,

        tex_u: frame.x / sheet_width,
        tex_v: frame.y / sheet_height,
        tex_w: frame.width / sheet_width,
        tex_h: frame.height / sheet_height,

        color: Vec4::splat(1.0),
    }
}

fn load_texture(renderer: &mut Renderer, file_name: &str) -> anyhow::Result<TextureHandle> {
    let asset_name = format!("space_invaders/{file_name}");
    let image = load_image(&asset_name)?;

    let texture = renderer.create_texture(asset_name, &image, TextureFilter::Nearest)?;

    Ok(texture)
}
