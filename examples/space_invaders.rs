use std::time::Duration;

use anyhow::anyhow;
use glam::{Mat4, Vec2, Vec3, Vec4};

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, FrameRenderer, PipelineHandle, Renderer, StorageBufferHandle, TextureFilter,
    TextureHandle, UniformBufferHandle,
};
use vulkan_slang_renderer::shaders::COLUMN_MAJOR;
use vulkan_slang_renderer::util::{load_image, manifest_path};

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
        let sprite_atlas = load_sprite_atlas()?;
        let player_offsets =
            first_frame_matching(&sprite_atlas, |f| f.filename.starts_with("ship"))?;
        let enemy_offsets = first_frame_matching(&sprite_atlas, |f| f.filename.starts_with("bug"))?;

        let mut sprites = vec![];
        let player_sprite = init_sprite(&mut sprites, &sprite_atlas.meta.size, player_offsets);
        let bug_sprite = init_sprite(&mut sprites, &sprite_atlas.meta.size, enemy_offsets);

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
            timer: Enemy::TRAVEL_TIME,
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
        let player_movement = self.player.intent.direction() * self.player.speed;
        self.player.position.x += player_movement.x;
        self.player.position.y += player_movement.y;

        let elapsed = self.frame_delay();
        for enemy in &mut self.enemies {
            if elapsed >= enemy.timer {
                enemy.intent = match enemy.intent {
                    EnemyIntent::Up => EnemyIntent::Right,
                    EnemyIntent::Down => EnemyIntent::Left,
                    EnemyIntent::Left => EnemyIntent::Up,
                    EnemyIntent::Right => EnemyIntent::Down,
                };

                enemy.timer = match enemy.intent {
                    EnemyIntent::Down => Enemy::TRAVEL_TIME,
                    _ => Enemy::TRAVEL_TIME_DOWN,
                };
            } else {
                enemy.timer -= elapsed;
            }

            let enemy_movement = enemy.intent.direction();
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
    fn direction(&self) -> Vec2 {
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
    const TRAVEL_TIME: Duration = Duration::from_secs(1);
    const TRAVEL_TIME_DOWN: Duration = Duration::from_millis(1500);
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
    fn direction(&self) -> Vec2 {
        match self {
            EnemyIntent::Up => Vec2::Y,
            EnemyIntent::Down => -Vec2::Y,
            EnemyIntent::Left => -Vec2::X,
            EnemyIntent::Right => Vec2::X,
        }
    }
}

const SPRITE_SCALE: f32 = 5.0;

fn init_sprite(
    sprites: &mut Vec<Sprite>,
    sprite_atlas_size: &SpriteAtlasSize,
    frame: &SpriteAtlasFrameOffsets,
) -> usize {
    let sheet_width = sprite_atlas_size.w as f32;
    let sheet_height = sprite_atlas_size.h as f32;

    let sprite = Sprite {
        scale: Vec2::splat(frame.w as f32 * SPRITE_SCALE),
        padding: Vec2::ZERO,

        position: Vec3::ZERO,
        rotation: 0.0,

        tex_u: frame.x as f32 / sheet_width,
        tex_v: frame.y as f32 / sheet_height,
        tex_w: frame.w as f32 / sheet_width,
        tex_h: frame.h as f32 / sheet_height,

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

// Aseprite integration

fn load_sprite_atlas() -> anyhow::Result<SpriteAtlas> {
    let sprite_atlas_path = manifest_path(["textures", "space_invaders", "sprite_sheet.json"]);

    let sprite_atlas_json = std::fs::read_to_string(&sprite_atlas_path)?;
    let sprite_atlas: SpriteAtlas = serde_json::from_str(&sprite_atlas_json)?;

    Ok(sprite_atlas)
}

fn first_frame_matching(
    sprite_atlas: &SpriteAtlas,
    condition: impl Fn(&SpriteFrame) -> bool,
) -> anyhow::Result<&SpriteAtlasFrameOffsets> {
    sprite_atlas
        .frames
        .iter()
        .find(|f| condition(f))
        .map(|f| &f.frame)
        .ok_or_else(|| anyhow!("no matching sprite frame found"))
}

#[derive(Debug, serde::Deserialize)]
struct SpriteAtlas {
    meta: SpriteAtlasMeta,
    frames: Vec<SpriteFrame>,
}

#[derive(Debug, serde::Deserialize)]
struct SpriteAtlasMeta {
    size: SpriteAtlasSize,
}

#[derive(Debug, serde::Deserialize)]
struct SpriteAtlasSize {
    w: usize,
    h: usize,
}

#[derive(Debug, serde::Deserialize)]
struct SpriteFrame {
    filename: String,
    frame: SpriteAtlasFrameOffsets,
}

#[derive(Debug, serde::Deserialize)]
struct SpriteAtlasFrameOffsets {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}
