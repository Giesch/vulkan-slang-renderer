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
    frame_counter: usize,
    pipeline: PipelineHandle,
    params_buffer: UniformBufferHandle<SpaceInvadersParams>,
    sprites_buffer: StorageBufferHandle<Sprite>,
    sprites: Vec<Sprite>,
    player: Player,
    enemies: Vec<Enemy>,
    sprite_atlas_size: SpriteAtlasSize,
    player_animation_frames: Vec<SpriteFrame>,
    enemy_animation_frames: Vec<SpriteFrame>,
    game_over: bool,
}

impl Game for SpaceInvaders {
    fn window_title() -> &'static str {
        "Space Invaders"
    }

    fn initial_window_size() -> (u32, u32) {
        (800, 900)
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
        assert!(!player_animation_frames.is_empty());
        let enemy_animation_frames = get_animation_frames(&sprite_atlas, "bug");
        assert!(!enemy_animation_frames.is_empty());

        let mut sprites = vec![];
        let player_sprite = init_sprite(&mut sprites, &sprite_atlas.meta.size, player_offsets);
        let enemy_sprite = init_sprite(&mut sprites, &sprite_atlas.meta.size, enemy_offsets);
        let sprite_atlas_size = sprite_atlas.meta.size;

        let player_frame = &player_animation_frames[0].frame;
        let player = Player {
            sprite_id: player_sprite,
            intent: Default::default(),
            speed: 10.0,
            animation: Animation::from_frames(&player_animation_frames),
            bounding_box: BoundingBox {
                x: 0.0,
                y: 0.0,
                w: player_frame.w as f32 * SPRITE_SCALE,
                h: player_frame.h as f32 * SPRITE_SCALE,
            },
        };

        let enemy_frame = &enemy_animation_frames[0].frame;
        let enemies = vec![
            //
            Enemy {
                sprite_id: enemy_sprite,
                bounding_box: BoundingBox {
                    x: 400.0,
                    y: 700.0,
                    w: enemy_frame.w as f32 * SPRITE_SCALE,
                    h: enemy_frame.h as f32 * SPRITE_SCALE,
                },
                intent: EnemyIntent::Right,
                movement_timer: 0,
                animation: Animation::from_frames(&enemy_animation_frames),
            },
        ];

        let params_buffer = renderer.create_uniform_buffer::<SpaceInvadersParams>()?;
        let sprites_buffer = renderer.create_storage_buffer::<Sprite>(sprites.len() as u32)?;

        let sprite_sheet_texture = load_texture(renderer, "sprite_sheet.png")?;

        let resources = Resources {
            vertex_count: sprites.len() as u32 * 6,
            sprites: &sprites_buffer,
            sprite_sheet: &sprite_sheet_texture,
            params_buffer: &params_buffer,
        };

        let shader = ShaderAtlas::init().space_invaders;
        let mut pipeline_config = shader.pipeline_config(resources);
        pipeline_config.disable_depth_test = true;
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        Ok(Self {
            frame_counter: 0,
            pipeline,
            params_buffer,
            sprites_buffer,
            sprites,
            player,
            enemies,
            sprite_atlas_size,
            player_animation_frames,
            enemy_animation_frames,
            game_over: false,
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
        self.frame_counter += 1;
        let elapsed = self.frame_delay();
        self.player.animation.tick(elapsed);
        for enemy in &mut self.enemies {
            enemy.animation.tick(elapsed);
        }

        if self.game_over {
            return;
        }

        // player movement
        let player_movement = self.player.intent.direction() * self.player.speed;
        self.player.bounding_box.x += player_movement.x;
        self.player.bounding_box.y += player_movement.y;

        // enemy movement
        for enemy in &mut self.enemies {
            enemy.movement_timer += 1;

            let travel_time = match enemy.intent {
                EnemyIntent::Down => Enemy::TRAVEL_TIME_DOWN,
                _ => Enemy::TRAVEL_TIME,
            };

            if enemy.movement_timer >= travel_time {
                enemy.movement_timer = enemy.movement_timer % travel_time;

                enemy.intent = match enemy.intent {
                    EnemyIntent::Up => EnemyIntent::Right,
                    EnemyIntent::Down => EnemyIntent::Left,
                    EnemyIntent::Left => EnemyIntent::Up,
                    EnemyIntent::Right => EnemyIntent::Down,
                };
            }

            let enemy_movement = enemy.intent.direction();
            enemy.bounding_box.x += enemy_movement.x;
            enemy.bounding_box.y += enemy_movement.y;
        }

        for enemy in &self.enemies {
            if enemy.bounding_box.y <= 0.0 {
                self.game_over = true;
            }

            if enemy.bounding_box.overlaps(&self.player.bounding_box) {
                self.game_over = true;
            }
        }

        if self.game_over {
            println!("Game OVER!");
        }
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        // update sprites
        let player_sprite = &mut self.sprites[self.player.sprite_id];
        player_sprite.position.x = self.player.bounding_box.x;
        player_sprite.position.y = self.player.bounding_box.y;

        let player_frame = self.player.animation.frame(&self.player_animation_frames);
        set_sprite_frame(player_sprite, player_frame, &self.sprite_atlas_size);

        for enemy in &self.enemies {
            let enemy_sprite = &mut self.sprites[enemy.sprite_id];
            enemy_sprite.position.x = enemy.bounding_box.x;
            enemy_sprite.position.y = enemy.bounding_box.y;

            let enemy_frame = enemy.animation.frame(&self.enemy_animation_frames);
            set_sprite_frame(enemy_sprite, enemy_frame, &self.sprite_atlas_size);
        }

        // make projection matrix
        let (width, height) = renderer.window_size();
        let mut projection_matrix = Mat4::orthographic_lh(0.0, width, height, 0.0, 0.0, -1.0);
        if !COLUMN_MAJOR {
            projection_matrix = projection_matrix.transpose();
        }
        let params = SpaceInvadersParams { projection_matrix };

        // draw
        renderer.draw_frame(&mut self.pipeline, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
            gpu.write_storage(&mut self.sprites_buffer, &self.sprites);

            gpu.sort_storage_by(&mut self.sprites_buffer, |a, b| {
                b.position.y.total_cmp(&a.position.y)
            });
        })
    }
}

struct Player {
    sprite_id: usize,
    intent: PlayerIntent,
    speed: f32,
    animation: Animation,
    bounding_box: BoundingBox,
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
    intent: EnemyIntent,
    movement_timer: usize,
    animation: Animation,
    bounding_box: BoundingBox,
}

impl Enemy {
    const TRAVEL_TIME: usize = 100;
    const TRAVEL_TIME_DOWN: usize = Self::TRAVEL_TIME * 2;
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

#[derive(Debug)]
struct BoundingBox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl BoundingBox {
    fn overlaps(&self, other: &BoundingBox) -> bool {
        let our_bottom = self.y;
        let our_top = self.y + self.h;
        let our_left = self.x;
        let our_right = self.x + self.w;

        let their_bottom = other.y;
        let their_top = other.y + other.h;
        let their_left = other.x;
        let their_right = other.x + other.w;

        let vert_overlap = (our_bottom < their_top && our_bottom > their_bottom)
            || (our_top > their_bottom && our_top < their_top);
        let horz_overlap = (our_left < their_right && our_left > their_left)
            || (our_right > their_left && our_right < their_right);

        vert_overlap && horz_overlap
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
