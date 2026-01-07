use std::time::Duration;

use sdl3::keyboard::Scancode as SDLScancode;

use crate::app::App;
use crate::renderer::{DrawError, FrameRenderer, Renderer};

const DEFAULT_FRAME_DELAY: Duration = Duration::from_millis(15); // about 60 fps
const DEFAULT_WINDOW_SIZE: (u32, u32) = (800, 600);
const DEFAULT_WINDOW_TITLE: &str = "Game";

/// This is the only trait from this module to implement directly.
pub trait Game {
    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized;

    fn update(&mut self) {}

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError>;

    fn window_title() -> &'static str {
        DEFAULT_WINDOW_TITLE
    }

    fn initial_window_size() -> (u32, u32) {
        DEFAULT_WINDOW_SIZE
    }

    fn window_description() -> WindowDescription {
        let title = Self::window_title();
        let (width, height) = Self::initial_window_size();

        WindowDescription {
            title,
            width,
            height,
        }
    }

    fn frame_delay(&self) -> Duration {
        DEFAULT_FRAME_DELAY
    }

    fn run() -> anyhow::Result<()>
    where
        Self: Sized + 'static,
    {
        pretty_env_logger::init();

        let sdl = sdl3::init()?;
        let video_subsystem = sdl.video()?;
        let window_desc = Self::window_description();
        let window = video_subsystem
            .window(window_desc.title, window_desc.width, window_desc.height)
            .position_centered()
            .resizable()
            .vulkan()
            .build()?;

        let mut renderer = Renderer::init(window)?;
        let game = Self::setup(&mut renderer)?;
        let app = App::init(renderer, game)?;

        let event_pump = sdl.event_pump()?;
        app.run_loop(event_pump)
    }

    fn input(&mut self, _input: Input) {}
}

/// parameters passed through to SDL to create a window
pub struct WindowDescription {
    pub title: &'static str,
    pub width: u32,
    pub height: u32,
}

/// methods used after initialization
/// this trait needs to be object-safe
pub trait RuntimeGame {
    fn update(&mut self);

    fn draw_frame(&mut self, renderer: FrameRenderer) -> Result<(), DrawError>;

    fn frame_delay(&self) -> Duration;

    fn input(&mut self, input: Input);
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Input {
    KeyUp(Key),
    KeyDown(Key),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Key {
    W,
    A,
    S,
    D,
    Q,
    E,
    Space,
}

impl Key {
    pub fn from_sdl_scancode(scancode: SDLScancode) -> Option<Self> {
        match scancode {
            SDLScancode::W => Some(Key::W),
            SDLScancode::A => Some(Key::A),
            SDLScancode::S => Some(Key::S),
            SDLScancode::D => Some(Key::D),
            SDLScancode::Q => Some(Key::Q),
            SDLScancode::E => Some(Key::E),
            SDLScancode::Space => Some(Key::Space),
            _ => None,
        }
    }
}

impl<G> RuntimeGame for G
where
    G: Game,
{
    fn update(&mut self) {
        self.update()
    }

    fn draw_frame(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        self.draw(renderer)
    }

    fn frame_delay(&self) -> Duration {
        self.frame_delay()
    }

    fn input(&mut self, input: Input) {
        self.input(input);
    }
}
