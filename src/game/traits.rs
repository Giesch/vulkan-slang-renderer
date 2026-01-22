use std::time::Duration;

use facet::Facet;
use sdl3::keyboard::Scancode as SDLScancode;

use crate::app::App;
use crate::renderer::{DrawError, FrameRenderer, Renderer};

const DEFAULT_FRAME_DELAY: Duration = Duration::from_millis(15); // about 60 fps
const DEFAULT_WINDOW_SIZE: (u32, u32) = (800, 600);
const DEFAULT_WINDOW_TITLE: &str = "Game";

/// Maximum MSAA sample count to use.
/// The renderer will use the best supported sample count up to this limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaxMSAASamples {
    #[default]
    Max8,
    Max4,
    Max2,
}

/// This is the only trait from this module to implement directly.
pub trait Game {
    /// The debug state type that will be reflected in egui.
    /// Use `()` if no debug UI is needed.
    type EditState: for<'a> Facet<'a> + 'static;

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

    /// Override to set the render scale.
    /// The default is based on the user's display, with larger displays getting a smaller scale.
    /// Valid range: 0.25 to 1.0. Lower values improve performance at cost of image quality.
    fn render_scale() -> Option<f32> {
        None
    }

    /// Override to limit the maximum MSAA sample count.
    /// Default is Max8 (use best available up to 8x).
    fn max_msaa_samples() -> MaxMSAASamples {
        MaxMSAASamples::default()
    }

    /// Returns the debug window name and a mutable reference to the debug state for egui rendering.
    /// Return None to disable debug UI for this frame.
    /// Default implementation returns None.
    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        None
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

        let enable_egui = cfg!(debug_assertions);
        let render_scale = match Self::render_scale() {
            Some(scale_override) => scale_override,
            None => compute_render_scale_for_display(&window),
        };
        let max_msaa_samples = Self::max_msaa_samples();
        let mut renderer = Renderer::init(window, enable_egui, render_scale, max_msaa_samples)?;
        let game = Self::setup(&mut renderer)?;
        let app = App::init(renderer, game)?;

        let event_pump = sdl.event_pump()?;
        app.run_loop(event_pump)
    }

    fn input(&mut self, _input: Input) {}
}

/// Compute render scale based on display resolution.
/// Returns lower scale for high-resolution displays to improve performance.
fn compute_render_scale_for_display(window: &sdl3::video::Window) -> f32 {
    let Ok(display) = window.get_display() else {
        return 1.0;
    };
    let Ok(bounds) = display.get_bounds() else {
        return 1.0;
    };

    let pixel_count = bounds.w as u64 * bounds.h as u64;

    // Scale based on total pixels:
    // - 4K+ (3840x2160 = 8.3M pixels): 0.5
    // - 2K/1440p (2560x1440 = 3.7M pixels): 0.75
    // - 1080p and below: 1.0
    if pixel_count >= 8_000_000 {
        0.5
    } else if pixel_count >= 3_500_000 {
        0.75
    } else {
        1.0
    }
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

    /// Draw debug UI using egui. Called by the renderer during egui pass.
    fn draw_edit_ui(&mut self, ctx: &egui::Context);
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Input {
    KeyUp(Key),
    KeyDown(Key),
    MouseMotion { x: f32, y: f32 },
    MouseDown { button: MouseButton, x: f32, y: f32 },
    MouseUp { button: MouseButton, x: f32, y: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Unknown,
    Left,
    Middle,
    Right,
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

    fn draw_edit_ui(&mut self, ctx: &egui::Context) {
        let Some((window_name, debug_state)) = Game::editor_ui(self) else {
            return;
        };

        egui::Window::new(window_name).show(ctx, |ui| {
            crate::renderer::facet_egui::render_facet_ui(ui, debug_state);
        });
    }
}
