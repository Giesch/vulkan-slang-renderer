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
    sprite_atlas_size: SpriteAtlasSize,
    player_animation_frames: Vec<SpriteFrame>,
    enemy_animation_frames: Vec<SpriteFrame>,
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

        let player_animation_frames = get_animation_frames(&sprite_atlas, "ship");
        let enemy_animation_frames = get_animation_frames(&sprite_atlas, "bug");

        let mut sprites = vec![];
        let player_sprite = init_sprite(&mut sprites, &sprite_atlas.meta.size, player_offsets);
        let enemy_sprite = init_sprite(&mut sprites, &sprite_atlas.meta.size, enemy_offsets);
        let sprite_atlas_size = sprite_atlas.meta.size;

        let player = Player {
            sprite_id: player_sprite,
            intent: Default::default(),
            speed: 10.0,
            position: Vec2::ZERO,
            animation: Animation::from_frames(&player_animation_frames),
        };

        let enemies = vec![
            //
            Enemy {
                sprite_id: enemy_sprite,
                position: Vec2::new(400.0, 700.0),
                intent: EnemyIntent::Right,
                movement_timer: Duration::ZERO,
                animation: Animation::from_frames(&enemy_animation_frames),
            },
        ];

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
            sprite_atlas_size,
            player_animation_frames,
            enemy_animation_frames,
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
        // timers
        let elapsed = self.frame_delay();
        self.player.animation.tick(elapsed);
        for enemy in &mut self.enemies {
            enemy.animation.tick(elapsed);
        }

        // player movement
        let player_movement = self.player.intent.direction() * self.player.speed;
        self.player.position.x += player_movement.x;
        self.player.position.y += player_movement.y;

        // enemy movement
        for enemy in &mut self.enemies {
            enemy.movement_timer += elapsed;

            let travel_time = match enemy.intent {
                EnemyIntent::Down => Enemy::TRAVEL_TIME_DOWN,
                _ => Enemy::TRAVEL_TIME,
            };

            if enemy.movement_timer >= travel_time {
                enemy.movement_timer = mod_duration(enemy.movement_timer, travel_time);

                enemy.intent = match enemy.intent {
                    EnemyIntent::Up => EnemyIntent::Right,
                    EnemyIntent::Down => EnemyIntent::Left,
                    EnemyIntent::Left => EnemyIntent::Up,
                    EnemyIntent::Right => EnemyIntent::Down,
                };
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

        let player_frame = self.player.animation.frame(&self.player_animation_frames);
        set_sprite_frame(player_sprite, player_frame, &self.sprite_atlas_size);

        for enemy in &self.enemies {
            let enemy_sprite = &mut self.sprites[enemy.sprite_id];
            enemy_sprite.position.x = enemy.position.x;
            enemy_sprite.position.y = enemy.position.y;

            let enemy_frame = enemy.animation.frame(&self.enemy_animation_frames);
            set_sprite_frame(enemy_sprite, enemy_frame, &self.sprite_atlas_size);
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
    animation: Animation,
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
    movement_timer: Duration,
    animation: Animation,
}

impl Enemy {
    const TRAVEL_TIME: Duration = Duration::from_secs(1);
    const TRAVEL_TIME_DOWN: Duration = Duration::from_millis(1500);
}

enum EnemyIntent {
    Up,
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

#[derive(Debug, serde::Deserialize, Clone)]
struct SpriteFrame {
    filename: String,
    frame: SpriteAtlasFrameOffsets,
    duration: u64,
}

#[derive(Debug, serde::Deserialize, Clone)]
struct SpriteAtlasFrameOffsets {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

fn get_animation_frames(sprite_atlas: &SpriteAtlas, name: &str) -> Vec<SpriteFrame> {
    sprite_atlas
        .frames
        .iter()
        .filter(|f| match f.filename.rsplit_once(" ") {
            Some((title, _)) => title == name,
            None => f.filename == name,
        })
        .cloned()
        .collect()
}

struct Animation {
    current_frame: usize,
    frame_millis: usize,
    timer: Duration,
    total_duration: Duration,
    frame_durations: Vec<u64>,
}

impl Animation {
    fn from_frames(frames: &[SpriteFrame]) -> Self {
        let frame_durations: Vec<_> = frames.iter().map(|f| f.duration).collect();
        let total_duration = Duration::from_millis(frame_durations.iter().sum());

        Self {
            current_frame: 0,
            frame_millis: 0,
            timer: Duration::ZERO,
            total_duration,
            frame_durations,
        }
    }

    fn tick(&mut self, elapsed: Duration) {
        self.timer += elapsed;
        self.timer = mod_duration(self.timer, self.total_duration);

        self.frame_millis += elapsed.as_millis() as usize;
        let mut current_frame = self.current_frame;
        loop {
            let current_frame_duration = self.frame_durations[current_frame] as usize;

            if self.frame_millis >= current_frame_duration {
                self.frame_millis %= current_frame_duration;
                current_frame += 1;
                current_frame %= self.frame_durations.len();
            } else {
                break;
            }
        }

        self.current_frame = current_frame;
    }

    fn frame<'f>(&self, frames: &'f [SpriteFrame]) -> &'f SpriteFrame {
        &frames[self.current_frame % frames.len()]
    }
}

fn mod_duration(timer: Duration, limit: Duration) -> Duration {
    let millis = timer.as_millis() % limit.as_millis();
    Duration::from_millis(millis as u64)
}

fn set_sprite_frame(
    sprite: &mut Sprite,
    sprite_frame: &SpriteFrame,
    sprite_atlas_size: &SpriteAtlasSize,
) {
    let sheet_width = sprite_atlas_size.w as f32;
    let sheet_height = sprite_atlas_size.h as f32;
    let frame = &sprite_frame.frame;

    sprite.tex_u = frame.x as f32 / sheet_width;
    sprite.tex_v = frame.y as f32 / sheet_height;
    sprite.tex_w = frame.w as f32 / sheet_width;
    sprite.tex_h = frame.h as f32 / sheet_height;
}
