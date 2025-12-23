use std::time::Duration;

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
    player: Player,
    enemies: Vec<Enemy>,
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

        let mut sprites = vec![];
        let player_sprite = init_sprite(&mut sprites, sprite_sheet_size, ship_frame);
        let bug_sprite = init_sprite(&mut sprites, sprite_sheet_size, bug_frame);

        let player = Player {
            sprite_id: player_sprite,
            intent: Default::default(),
            speed: 10.0,
            position: Vec2::ZERO,
        };

        let bug = Enemy {
            sprite_id: bug_sprite,
            position: Vec2::new(400.0, 700.0),
            intent: Default::default(),
            timer: Enemy::TURN_TIME,
        };
        let enemies = vec![bug];

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
            player,
            enemies,
        })
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::KeyUp(key) => match key {
                Key::W => self.player.intent.up = false,
                Key::A => self.player.intent.left = false,
                Key::S => self.player.intent.down = false,
                Key::D => self.player.intent.right = false,
                Key::Space => {}
            },

            Input::KeyDown(key) => match key {
                Key::W => self.player.intent.up = true,
                Key::A => self.player.intent.left = true,
                Key::S => self.player.intent.down = true,
                Key::D => self.player.intent.right = true,
                Key::Space => {}
            },
        }
    }

    fn update(&mut self) {
        let player_movement = self.player.intent.movement() * self.player.speed;
        self.player.position.x += player_movement.x;
        self.player.position.y += player_movement.y;

        let elapsed = self.frame_delay();
        for enemy in &mut self.enemies {
            if elapsed >= enemy.timer {
                enemy.timer = Enemy::TURN_TIME;
                enemy.intent = match enemy.intent {
                    EnemyIntent::Up => EnemyIntent::Right,
                    EnemyIntent::Down => EnemyIntent::Left,
                    EnemyIntent::Left => EnemyIntent::Up,
                    EnemyIntent::Right => EnemyIntent::Down,
                }
            } else {
                enemy.timer -= elapsed;
            }

            let enemy_movement = enemy.intent.movement();
            enemy.position.x += enemy_movement.x;
            enemy.position.y += enemy_movement.y;
        }
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        // update sprites
        let player_sprite = &mut self.sprites[self.player.sprite_id];
        player_sprite.position.x = self.player.position.x;
        player_sprite.position.y = self.player.position.y;

        for enemy in &self.enemies {
            let enemy_sprite = &mut self.sprites[enemy.sprite_id];
            enemy_sprite.position.x = enemy.position.x;
            enemy_sprite.position.y = enemy.position.y;
        }

        // make projection matrix
        let (width, height) = renderer.window_size();
        let mut projection_matrix = Mat4::orthographic_lh(0.0, width, height, 0.0, 0.0, -1.0);
        if !COLUMN_MAJOR {
            projection_matrix = projection_matrix.transpose();
        }
        let uniform_data = SpaceInvadersParams { projection_matrix };

        // draw
        renderer.draw_frame(&mut self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.uniform_buffer, uniform_data);
            gpu.write_storage(&mut self.storage_buffer, &self.sprites);
        })
    }
}

struct Player {
    sprite_id: usize,
    intent: PlayerIntent,
    speed: f32,
    position: Vec2,
}

#[derive(Default)]
struct PlayerIntent {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

impl PlayerIntent {
    fn movement(&self) -> Vec2 {
        let mut direction = Vec2::ZERO;
        if self.up {
            direction.y += 1.0;
        }
        if self.down {
            direction.y -= 1.0;
        }
        if self.right {
            direction.x += 1.0;
        }
        if self.left {
            direction.x -= 1.0;
        }

        direction.normalize_or_zero()
    }
}

struct Enemy {
    sprite_id: usize,
    position: Vec2,
    intent: EnemyIntent,
    timer: Duration,
}

impl Enemy {
    const TURN_TIME: Duration = Duration::from_secs(1);
}

#[derive(Default)]
enum EnemyIntent {
    Up,
    #[default]
    Down,
    Left,
    Right,
}

impl EnemyIntent {
    fn movement(&self) -> Vec2 {
        match self {
            EnemyIntent::Up => Vec2::Y,
            EnemyIntent::Down => -Vec2::Y * 1.5,
            EnemyIntent::Left => -Vec2::X,
            EnemyIntent::Right => Vec2::X,
        }
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
    sprites: &mut Vec<Sprite>,
    (sheet_width, sheet_height): (f32, f32),
    frame: SpriteFrameOffsets,
) -> usize {
    let sprite = Sprite {
        scale: Vec2::splat(frame.width * SPRITE_SCALE),
        padding: Vec2::ZERO,

        position: Vec3::ZERO,
        rotation: 0.0,

        tex_u: frame.x / sheet_width,
        tex_v: frame.y / sheet_height,
        tex_w: frame.width / sheet_width,
        tex_h: frame.height / sheet_height,

        color: Vec4::splat(1.0),
    };

    let sprite_id = sprites.len();
    sprites.push(sprite);

    sprite_id
}

fn load_texture(renderer: &mut Renderer, file_name: &str) -> anyhow::Result<TextureHandle> {
    let asset_name = format!("space_invaders/{file_name}");
    let image = load_image(&asset_name)?;

    let texture = renderer.create_texture(asset_name, &image, TextureFilter::Nearest)?;

    Ok(texture)
}
