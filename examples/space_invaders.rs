use std::cmp::Ordering;
use std::time::Duration;

use glam::{Mat4, Vec2, Vec3, Vec4};

use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, StorageBufferHandle,
    TextureFilter, TextureHandle, UniformBufferHandle,
};
use vulkan_slang_renderer::util::{load_image, manifest_path};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::space_invaders::*;

fn main() -> Result<(), anyhow::Error> {
    SpaceInvaders::run()
}

struct SpaceInvaders {
    frame_counter: usize,
    pipeline: PipelineHandle<DrawVertexCount>,
    params_buffer: UniformBufferHandle<SpaceInvadersParams>,
    sprites_buffer: StorageBufferHandle<Sprite>,
    debug_boxes_buffer: StorageBufferHandle<DebugBox>,
    sprites: Vec<Sprite>,
    debug_boxes: Vec<DebugBox>,
    player: Player,
    enemies: Vec<Enemy>,
    sprite_atlas_size: SpriteAtlasSize,
    player_animation_frames: Vec<SpriteFrame>,
    enemy_animation_frames: Vec<SpriteFrame>,
    game_screen: GameScreen,
    game_lost_sprite: usize,
    you_win_sprite: usize,
    bullets: Vec<Bullet>,
}

const INITIAL_WINDOW_WIDTH_PIXELS: u32 = 160;
const INITIAL_WINDOW_HEIGHT_PIXELS: u32 = 180;

const ENABLE_DEBUG_BOXES: bool = false;
const MAX_DEBUG_BOXES: u32 = 100;

impl Game for SpaceInvaders {
    fn window_title() -> &'static str {
        "Space Invaders"
    }

    fn initial_window_size() -> (u32, u32) {
        (
            INITIAL_WINDOW_WIDTH_PIXELS * SPRITE_SCALE as u32,
            INITIAL_WINDOW_HEIGHT_PIXELS * SPRITE_SCALE as u32,
        )
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let sprite_atlas = load_sprite_atlas()?;

        let player_animation_frames = get_animation_frames(&sprite_atlas, "ship");
        assert!(!player_animation_frames.is_empty());
        let enemy_animation_frames = get_animation_frames(&sprite_atlas, "bug");
        assert!(!enemy_animation_frames.is_empty());
        let bullet_animation_frames = get_animation_frames(&sprite_atlas, "bullet");
        assert!(!bullet_animation_frames.is_empty());

        let mut sprites = vec![];

        let game_over_frames = get_animation_frames(&sprite_atlas, "game_over");
        let game_over_frame = &game_over_frames[0].frame;
        let game_lost_sprite = Sprite::init(&mut sprites, &sprite_atlas.meta.size, game_over_frame);
        {
            let game_over_sprite = &mut sprites[game_lost_sprite];
            game_over_sprite.flags |= SPRITE_FLAG_UI;
            game_over_sprite.flags &= !SPRITE_FLAG_VISIBLE;
        }
        let you_win_frames = get_animation_frames(&sprite_atlas, "you_win");
        let you_win_frame = &you_win_frames[0].frame;
        let you_win_sprite = Sprite::init(&mut sprites, &sprite_atlas.meta.size, you_win_frame);
        {
            let you_win_sprite = &mut sprites[you_win_sprite];
            you_win_sprite.flags |= SPRITE_FLAG_UI;
            you_win_sprite.flags &= !SPRITE_FLAG_VISIBLE;
        }

        let player_sprite = Sprite::init(
            &mut sprites,
            &sprite_atlas.meta.size,
            &player_animation_frames[0].frame,
        );
        let enemy_sprite = Sprite::init(
            &mut sprites,
            &sprite_atlas.meta.size,
            &enemy_animation_frames[0].frame,
        );

        let player_frame = &player_animation_frames[0].frame;
        let player = Player {
            sprite_id: player_sprite,
            intent: Default::default(),
            animation: Animation::from_frames(&player_animation_frames),
            bounding_box: BoundingBox {
                x: 0.0,
                y: 0.0,
                w: player_frame.w as f32,
                h: player_frame.h as f32,
            },
        };

        let enemy_frame = &enemy_animation_frames[0].frame;
        let enemies = vec![Enemy {
            sprite_id: enemy_sprite,
            bounding_box: BoundingBox {
                x: 80.0,
                y: 140.0,
                w: enemy_frame.w as f32,
                h: enemy_frame.h as f32,
            },
            intent: EnemyIntent::Right,
            animation: Animation::from_frames(&enemy_animation_frames),
            health: 50,
            movement_script: EnemyMovementScript::new(vec![
                (EnemyIntent::Right, 100),
                (EnemyIntent::Down, 200),
                (EnemyIntent::Left, 100),
                (EnemyIntent::Up, 100),
            ]),
        }];

        let bullet_frame = &bullet_animation_frames[0].frame;
        let bullets = {
            let mut bullets = vec![];

            for _ in 0..Bullet::MAX_BULLETS {
                let bullet = Bullet::new(&mut sprites, &sprite_atlas.meta.size, bullet_frame);
                bullets.push(bullet);
            }

            bullets
        };

        let mut debug_boxes: Vec<DebugBox> = Vec::with_capacity(MAX_DEBUG_BOXES as usize);
        if ENABLE_DEBUG_BOXES {
            setup_debug_boxes(&mut debug_boxes, &mut sprites, &player);
        }

        let params_buffer = renderer.create_uniform_buffer::<SpaceInvadersParams>()?;
        let debug_boxes_buffer = renderer.create_storage_buffer::<DebugBox>(MAX_DEBUG_BOXES)?;
        let sprites_buffer = renderer.create_storage_buffer::<Sprite>(sprites.len() as u32)?;

        let sprite_sheet_texture = load_texture(renderer, "sprite_sheet.png")?;

        let resources = Resources {
            sprites: &sprites_buffer,
            debug_boxes: &debug_boxes_buffer,
            sprite_sheet: &sprite_sheet_texture,
            params_buffer: &params_buffer,
        };

        let shader = ShaderAtlas::init().space_invaders;
        let mut pipeline_config = shader.pipeline_config(resources);
        pipeline_config.disable_depth_test = true;
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let sprite_atlas_size = sprite_atlas.meta.size;

        Ok(Self {
            frame_counter: 0,
            pipeline,
            params_buffer,
            sprites_buffer,
            debug_boxes_buffer,
            sprites,
            debug_boxes,
            player,
            enemies,
            sprite_atlas_size,
            player_animation_frames,
            enemy_animation_frames,
            game_screen: Default::default(),
            game_lost_sprite,
            you_win_sprite,
            bullets,
        })
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::KeyDown(key) => match key {
                Key::W => self.player.intent.up = true,
                Key::A => self.player.intent.left = true,
                Key::S => self.player.intent.down = true,
                Key::D => self.player.intent.right = true,
                Key::Space => self.player.intent.fire = true,
                _ => {}
            },

            Input::KeyUp(key) => match key {
                Key::W => self.player.intent.up = false,
                Key::A => self.player.intent.left = false,
                Key::S => self.player.intent.down = false,
                Key::D => self.player.intent.right = false,
                Key::Space => self.player.intent.fire = false,
                _ => {}
            },
        }
    }

    fn update(&mut self) {
        if self.game_screen.game_over() {
            return;
        }

        // timers
        self.frame_counter += 1;
        let elapsed = self.frame_delay();
        self.player.animation.tick(elapsed);
        for enemy in &mut self.enemies {
            if !enemy.is_alive() {
                continue;
            }

            enemy.animation.tick(elapsed);
            enemy.movement_script.tick();
        }

        // player movement
        let player_movement = self.player.intent.direction() * Player::SPEED;
        self.player.bounding_box.x += player_movement.x;
        self.player.bounding_box.y += player_movement.y;

        // player fire
        if self.player.intent.fire {
            if let Some(free_bullet_id) = self.bullets.iter().position(|b| !b.active) {
                let bullet = &mut self.bullets[free_bullet_id];
                let position = Vec2::new(self.player.bounding_box.x, self.player.bounding_box.y);
                bullet.spawn(&mut self.sprites, position);
            };
        }

        // bullet movement
        for bullet in &mut self.bullets {
            if !bullet.active {
                continue;
            }

            bullet.step_forward();

            let probably_offscreen =
                bullet.bounding_box.y > 1.5 * INITIAL_WINDOW_HEIGHT_PIXELS as f32;
            if probably_offscreen {
                bullet.active = false;
                let bullet_sprite = &mut self.sprites[bullet.sprite_id];
                bullet_sprite.flags &= !SPRITE_FLAG_VISIBLE;
            }
        }

        // enemy movement
        for enemy in &mut self.enemies {
            if !enemy.is_alive() {
                continue;
            }

            enemy.intent = enemy.movement_script.intent();

            let enemy_movement = enemy.intent.direction() * Enemy::SPEED;
            enemy.bounding_box.x += enemy_movement.x;
            enemy.bounding_box.y += enemy_movement.y;
        }

        // bullet-enemy collisions
        for bullet in &mut self.bullets {
            for enemy in &mut self.enemies {
                let bullet_hit_box = bullet.hit_box();

                if !enemy.is_alive()
                    || !bullet.active
                    || !bullet_hit_box.overlaps(&enemy.bounding_box)
                {
                    continue;
                }

                bullet.despawn(&mut self.sprites);
                enemy.health -= 1;
            }
        }

        // despawn dead enemies
        for enemy in &mut self.enemies {
            if enemy.is_alive() {
                continue;
            }

            enemy.despawn(&mut self.sprites);
        }

        // game over check
        for enemy in &self.enemies {
            if !enemy.is_alive() {
                continue;
            }

            if enemy.bounding_box.y <= 0.0 {
                self.game_screen = GameScreen::Lost;
            }

            if enemy.bounding_box.overlaps(&self.player.bounding_box) {
                self.game_screen = GameScreen::Lost;
            }
        }

        let all_enemies_defeated = self.enemies.iter().filter(|e| e.is_alive()).count() == 0;
        if all_enemies_defeated {
            self.game_screen = GameScreen::Won;
        }
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let resolution = renderer.window_resolution();
        let width = resolution.x;
        let height = resolution.y;

        // player sprite
        let player_sprite = &mut self.sprites[self.player.sprite_id];
        player_sprite.set_position(&self.player.bounding_box);
        if player_sprite.debug_box_id != u32::MAX {
            // let debug_box = &mut self.debug_boxes[player_sprite.debug_box_id as usize];
            // dbg!(player_sprite.debug_box_id, debug_box);
        }

        let player_frame = self.player.animation.frame(&self.player_animation_frames);
        player_sprite.set_frame(player_frame, &self.sprite_atlas_size);

        // bullet sprites
        for bullet in &self.bullets {
            let bullet_sprite = &mut self.sprites[bullet.sprite_id];
            bullet_sprite.set_position(&bullet.bounding_box);
        }

        // enemy sprites
        for enemy in &self.enemies {
            let enemy_sprite = &mut self.sprites[enemy.sprite_id];
            enemy_sprite.set_position(&enemy.bounding_box);

            let enemy_frame = enemy.animation.frame(&self.enemy_animation_frames);
            enemy_sprite.set_frame(enemy_frame, &self.sprite_atlas_size);
        }

        // game over screens
        let game_over_sprite_id = match &self.game_screen {
            GameScreen::Playing => None,
            GameScreen::Lost => Some(self.game_lost_sprite),
            GameScreen::Won => Some(self.you_win_sprite),
        };
        if let Some(game_over_sprite_id) = game_over_sprite_id {
            let game_over_sprite = &mut self.sprites[game_over_sprite_id];
            game_over_sprite.flags |= SPRITE_FLAG_VISIBLE;
            game_over_sprite.scale = Vec2::new(400.0, 64.0);
            game_over_sprite.position.x = (width - game_over_sprite.scale.x) / 2.0;
            game_over_sprite.position.y = 400.0;
        }

        // make projection matrix
        let projection_matrix = Mat4::orthographic_lh(0.0, width, height, 0.0, 0.0, -1.0);
        let params = SpaceInvadersParams { projection_matrix };

        // draw
        let visible_sprites = self
            .sprites
            .iter()
            .filter(|sprite| flag_enabled(sprite, SPRITE_FLAG_VISIBLE))
            .count();
        let vertex_count = visible_sprites as u32 * 6;

        renderer.draw_vertex_count(&mut self.pipeline, vertex_count, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);

            // TODO FIXME need a separate draw call?
            //   or find a way to match these with their sprites
            // dbg!(&self.debug_boxes);
            gpu.write_storage(&mut self.debug_boxes_buffer, &self.debug_boxes);

            gpu.write_storage(&mut self.sprites_buffer, &self.sprites);
            gpu.sort_storage_by(&mut self.sprites_buffer, sprite_draw_order);
        })
    }
}

#[derive(Debug, Default)]
enum GameScreen {
    #[default]
    Playing,
    Lost,
    Won,
}

impl GameScreen {
    fn game_over(&self) -> bool {
        match self {
            GameScreen::Playing => false,
            GameScreen::Lost => true,
            GameScreen::Won => true,
        }
    }
}

fn sprite_draw_order(a: &Sprite, b: &Sprite) -> Ordering {
    let a_visible: bool = flag_enabled(a, SPRITE_FLAG_VISIBLE);
    let a_ui: bool = flag_enabled(a, SPRITE_FLAG_UI);
    let a_y = a.position.y;

    let b_visible: bool = flag_enabled(b, SPRITE_FLAG_VISIBLE);
    let b_ui: bool = flag_enabled(b, SPRITE_FLAG_UI);
    let b_y = b.position.y;

    // invisible sprites are last
    // so that we can leave them out of the draw call
    let invisible_last = a_visible.cmp(&b_visible).reverse();

    // visible sprites are drawn back-to-front
    // using the painter's algorithm
    let ui_on_top = a_ui.cmp(&b_ui);
    let y_descending = a_y.total_cmp(&b_y).reverse();

    invisible_last.then(ui_on_top).then(y_descending)
}

struct Player {
    sprite_id: usize,
    intent: PlayerIntent,
    animation: Animation,
    bounding_box: BoundingBox,
}

impl Player {
    const SPEED: f32 = 2.0;
}

#[derive(Default)]
struct PlayerIntent {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    fire: bool,
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
    animation: Animation,
    bounding_box: BoundingBox,
    health: i32,
    movement_script: EnemyMovementScript,
}

impl Enemy {
    const SPEED: f32 = 2.0 / 5.0;

    fn is_alive(&self) -> bool {
        self.health > 0
    }

    fn despawn(&mut self, sprites: &mut [Sprite]) {
        let sprite = &mut sprites[self.sprite_id];
        sprite.flags &= !SPRITE_FLAG_VISIBLE;
    }
}

#[derive(Debug, Clone, Copy)]
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

struct Bullet {
    sprite_id: usize,
    active: bool,
    bounding_box: BoundingBox,
}

impl Bullet {
    const SPEED: f32 = 3.0;
    const MAX_BULLETS: usize = 100;

    const HIT_BOX_OFFSET: f32 = 15.0;
    const HIT_BOX_SIDE: f32 = 2.0;

    fn new(
        sprites: &mut Vec<Sprite>,
        atlas_size: &SpriteAtlasSize,
        offsets: &SpriteAtlasFrameOffsets,
    ) -> Self {
        let sprite_id = Sprite::init(sprites, atlas_size, offsets);

        let bullet_sprite = &mut sprites[sprite_id];
        bullet_sprite.flags &= !SPRITE_FLAG_VISIBLE;

        let bounding_box = BoundingBox {
            x: 0.0,
            y: 0.0,
            w: offsets.w as f32,
            h: offsets.h as f32,
        };

        Bullet {
            active: false,
            sprite_id,
            bounding_box,
        }
    }

    fn hit_box(&self) -> BoundingBox {
        BoundingBox {
            x: self.bounding_box.x + Self::HIT_BOX_OFFSET,
            y: self.bounding_box.y + Self::HIT_BOX_OFFSET,
            w: Self::HIT_BOX_SIDE,
            h: Self::HIT_BOX_SIDE,
        }
    }

    fn step_forward(&mut self) {
        self.bounding_box.y += Bullet::SPEED;
    }

    fn spawn(&mut self, sprites: &mut [Sprite], position: Vec2) {
        self.active = true;

        self.bounding_box.x = position.x;
        self.bounding_box.y = position.y;

        let sprite = &mut sprites[self.sprite_id];
        sprite.flags |= SPRITE_FLAG_VISIBLE;
    }

    fn despawn(&mut self, sprites: &mut [Sprite]) {
        self.active = false;
        let bullet_sprite = &mut sprites[self.sprite_id];
        bullet_sprite.flags &= !SPRITE_FLAG_VISIBLE;
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

const SPRITE_FLAG_UI: u32 = 1 << 0;
const SPRITE_FLAG_VISIBLE: u32 = 1 << 1;

fn flag_enabled(sprite: &Sprite, flag: u32) -> bool {
    (sprite.flags & flag) == flag
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
            Some((title, _frame_index)) => title == name,
            None => f.filename == name,
        })
        .cloned()
        .collect()
}

struct Animation {
    /// the index of the current frame in the animation
    current_frame: usize,
    /// the remaining millis of the current frame
    frame_millis: usize,
    /// the time within the full animation;
    /// always less than total_duration
    timer: Duration,
    /// the sum total duration of all animation frames
    total_duration: Duration,
    /// the individual durations of each frame, in milliseconds
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

        let mut next_current_frame = self.current_frame;
        self.frame_millis += elapsed.as_millis() as usize;
        loop {
            let current_frame_duration = self.frame_durations[next_current_frame] as usize;

            if self.frame_millis >= current_frame_duration {
                self.frame_millis %= current_frame_duration;
                next_current_frame += 1;
                next_current_frame %= self.frame_durations.len();
            } else {
                break;
            }
        }

        self.current_frame = next_current_frame;
    }

    fn frame<'f>(&self, frames: &'f [SpriteFrame]) -> &'f SpriteFrame {
        &frames[self.current_frame % frames.len()]
    }
}

fn mod_duration(timer: Duration, limit: Duration) -> Duration {
    let millis = timer.as_millis() % limit.as_millis();
    Duration::from_millis(millis as u64)
}

struct EnemyMovementScript {
    steps: Vec<(EnemyIntent, usize)>,
    total_frames: usize,
    frame_counter: usize,
}

impl EnemyMovementScript {
    fn new(steps: Vec<(EnemyIntent, usize)>) -> Self {
        assert!(!steps.is_empty());

        let total_frames = steps.iter().map(|(_intent, frames)| frames).sum();

        Self {
            steps,
            total_frames,
            frame_counter: 0,
        }
    }

    fn tick(&mut self) {
        self.frame_counter += 1;
        self.frame_counter %= self.total_frames;
    }

    fn intent(&self) -> EnemyIntent {
        let mut remaining_frames = self.frame_counter;

        for &(intent, step_frames) in &self.steps {
            if step_frames >= remaining_frames {
                return intent;
            }

            remaining_frames -= step_frames
        }

        debug_assert!(false, "invalid enemy movement script");

        return self.steps[0].0;
    }
}

const SPRITE_SCALE: f32 = 5.0;

trait CPUSprite {
    fn init(
        sprites: &mut Vec<Sprite>,
        sprite_atlas_size: &SpriteAtlasSize,
        frame: &SpriteAtlasFrameOffsets,
    ) -> usize;

    fn set_frame(&mut self, sprite_frame: &SpriteFrame, sprite_atlas_size: &SpriteAtlasSize);

    fn set_position(&mut self, bounding_box: &BoundingBox);
}

impl CPUSprite for Sprite {
    fn init(
        sprites: &mut Vec<Sprite>,
        sprite_atlas_size: &SpriteAtlasSize,
        frame: &SpriteAtlasFrameOffsets,
    ) -> usize {
        let sheet_width = sprite_atlas_size.w as f32;
        let sheet_height = sprite_atlas_size.h as f32;

        let sprite = Sprite {
            scale: Vec2::new(frame.w as f32 * SPRITE_SCALE, frame.h as f32 * SPRITE_SCALE),
            flags: SPRITE_FLAG_VISIBLE,
            debug_box_id: u32::MAX,

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

    fn set_frame(&mut self, sprite_frame: &SpriteFrame, sprite_atlas_size: &SpriteAtlasSize) {
        let sheet_width = sprite_atlas_size.w as f32;
        let sheet_height = sprite_atlas_size.h as f32;
        let frame = &sprite_frame.frame;

        self.tex_u = frame.x as f32 / sheet_width;
        self.tex_v = frame.y as f32 / sheet_height;
        self.tex_w = frame.w as f32 / sheet_width;
        self.tex_h = frame.h as f32 / sheet_height;
    }

    fn set_position(&mut self, bounding_box: &BoundingBox) {
        self.position.x = bounding_box.x * SPRITE_SCALE;
        self.position.y = bounding_box.y * SPRITE_SCALE;
    }
}

fn setup_debug_boxes(debug_boxes: &mut Vec<DebugBox>, sprites: &mut [Sprite], player: &Player) {
    let player_box = DebugBox {
        color: Vec4::new(0.0, 1.0, 0.0, 1.0),
        position: Vec2::ZERO,
        size: Vec2::splat(1.0),
    };

    assign_debug_box_to_sprite(player.sprite_id, player_box, debug_boxes, sprites);
}

fn assign_debug_box_to_sprite(
    sprite_id: usize,
    debug_box: DebugBox,
    debug_boxes: &mut Vec<DebugBox>,
    sprites: &mut [Sprite],
) {
    let player_sprite = &mut sprites[sprite_id];
    player_sprite.debug_box_id = debug_boxes.len() as u32;
    debug_boxes.push(debug_box);
}
