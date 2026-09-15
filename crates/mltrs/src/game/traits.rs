use std::time::Duration;

use facet::Facet;
use sdl3::keyboard::Scancode as SDLScancode;

use crate::app::App;
use crate::env_config::{EnvConfig, exit_code};
use crate::renderer::{self, DrawError, FrameRenderer, Renderer};
use crate::shaders::atlas::ShaderAtlasRoot;

const DEFAULT_FRAME_DELAY: Duration = Duration::from_millis(15); // about 60 fps
const DEFAULT_WINDOW_SIZE: (u32, u32) = (800, 600);
const DEFAULT_WINDOW_TITLE: &str = "Game";

pub use crate::renderer::MaxMSAASamples;

/// This is the only trait from this module to implement directly.
pub trait Game {
    /// The debug state type that will be reflected in egui.
    /// Use `()` if no debug UI is needed.
    type EditState: for<'a> Facet<'a> + 'static;

    /// The generated `ShaderAtlas`.
    type Atlas: ShaderAtlasRoot;

    fn setup(renderer: &mut Renderer, shaders: Self::Atlas) -> anyhow::Result<Self>
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

    /// Override to request a stencil-capable depth buffer.
    /// `Renderer::stencil_support` then returns the token that builds
    /// enabled stencil pipeline state.
    fn needs_stencil() -> bool {
        false
    }

    /// Returns the debug window name and a mutable reference to the debug state for egui rendering.
    /// Return None to disable debug UI for this frame.
    /// Default implementation returns None.
    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        None
    }

    /// Render the contents of the debug window returned by [`Self::editor_ui`].
    ///
    /// The default implementation renders the state's reflected fields. Override
    /// this method to provide custom controls or layout. The runtime creates the
    /// window and calls this hook only when `editor_ui` returns `Some`.
    fn render_editor_ui(ui: &mut egui::Ui, debug_state: &mut Self::EditState) {
        crate::renderer::facet_egui::render_facet_ui(ui, debug_state);
    }

    fn run() -> anyhow::Result<()>
    where
        Self: Sized + 'static,
    {
        pretty_env_logger::init();

        let env = EnvConfig::from_env();
        log::debug!("{env:?}");

        // A sweep of a build with validation compiled out passes everything it
        // is looking for, which is worse than not running: fail before the
        // window exists rather than report a vacuous success.
        if env.headless_sweep && !crate::renderer::ENABLE_VALIDATION {
            eprintln!(
                "VKR_SWEEP is set, but ENABLE_VALIDATION is false. \
                 It is cfg!(debug_assertions), so this is a release build \
                 and nothing would be validated."
            );
            std::process::exit(exit_code::VALIDATION_DISABLED);
        }

        // NOTE: this can cause swapchain starvation, which is why it's not a default
        #[cfg(target_os = "linux")]
        sdl3::hint::set("SDL_VIDEO_DRIVER", "wayland,x11");

        let sdl = sdl3::init()?;
        let video_subsystem = sdl.video()?;
        let window_desc = Self::window_description();
        let window = video_subsystem
            .window(window_desc.title, window_desc.width, window_desc.height)
            .position_centered()
            .resizable()
            .hidden()
            .vulkan()
            .build()?;
        let mut startup_window = window.clone();

        let enable_egui = cfg!(debug_assertions);
        let render_scale = match Self::render_scale() {
            Some(scale_override) => scale_override,
            None => compute_render_scale_for_display(&window),
        };
        let max_msaa_samples = Self::max_msaa_samples();
        let mut renderer = Renderer::init(
            window,
            env.clone(),
            enable_egui,
            render_scale,
            max_msaa_samples,
            Self::needs_stencil(),
            #[cfg(debug_assertions)]
            Self::Atlas::SHADERS_SOURCE_DIR,
        )?;
        let game = Self::setup(&mut renderer, Self::Atlas::init())?;
        let app = App::init(renderer, game)?;

        if !startup_window.show() {
            log::warn!("failed to show window: {}", sdl3::get_error());
        }

        let event_pump = sdl.event_pump()?;
        let result = app.run_loop(event_pump);

        // run_loop consumes the App, so the Renderer is already dropped here.
        // That is deliberate: destroy_device reports leaked objects, and it runs
        // before the debug messenger is destroyed, so teardown is counted too.
        let validation_messages = renderer::debug::validation_message_count();
        let stats = match (result, validation_messages) {
            (Ok(stats), 0) => stats,
            (Ok(_), n) => anyhow::bail!(validation_failure_message(n, &env)),
            (Err(err), 0) => return Err(err),
            (Err(err), n) => {
                return Err(err.context(validation_failure_message(n, &env)));
            }
        };

        // An example that exits cleanly without drawing anything is a pass by
        // every other measure, and covers none of what the sweep is checking.
        if env.headless_sweep && stats.frames == 0 {
            eprintln!("VKR_SWEEP is set, but the run ended without presenting a frame.");
            std::process::exit(exit_code::NO_FRAMES);
        }

        Ok(())
    }

    fn input(&mut self, _input: Input) {}
}

/// The count keys off the severity Vulkan reports, but the *text* of each
/// message still goes through `log`, and with `RUST_LOG` unset env_logger
/// keeps only `error!` — so "see the log above" would point at a log the
/// warning-severity detail never reached. Say so instead.
fn validation_failure_message(count: u64, env: &EnvConfig) -> String {
    match &env.rust_log {
        Some(_) => format!("{count} vulkan validation message(s); see the log above"),
        None => format!(
            "{count} vulkan validation message(s); RUST_LOG is unset, so any \
             warning-severity detail was filtered out — re-run with RUST_LOG=warn"
        ),
    }
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
    R,
    T,
    F,
    Space,
    Num1,
    Num2,
    Num3,
    Num4,
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
            SDLScancode::R => Some(Key::R),
            SDLScancode::T => Some(Key::T),
            SDLScancode::F => Some(Key::F),
            SDLScancode::Space => Some(Key::Space),
            SDLScancode::_1 => Some(Key::Num1),
            SDLScancode::_2 => Some(Key::Num2),
            SDLScancode::_3 => Some(Key::Num3),
            SDLScancode::_4 => Some(Key::Num4),
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
            Self::render_editor_ui(ui, debug_state);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyAtlas;

    impl ShaderAtlasRoot for EmptyAtlas {
        const SHADERS_SOURCE_DIR: &'static str = "";

        fn init() -> Self {
            Self
        }
    }

    #[derive(Facet)]
    struct EditorState {
        reflected_value: crate::editor::Label,
        hook_calls: u32,
    }

    impl Default for EditorState {
        fn default() -> Self {
            Self {
                reflected_value: crate::editor::Label::new("Reflected widget content"),
                hook_calls: 0,
            }
        }
    }

    struct CustomEditorGame {
        state: EditorState,
        show_editor: bool,
    }

    impl Game for CustomEditorGame {
        type EditState = EditorState;
        type Atlas = EmptyAtlas;

        fn setup(_renderer: &mut Renderer, _shaders: Self::Atlas) -> anyhow::Result<Self> {
            unreachable!("CPU editor tests do not initialize a renderer")
        }

        fn draw(&mut self, _renderer: FrameRenderer) -> Result<(), DrawError> {
            unreachable!("CPU editor tests do not draw GPU frames")
        }

        fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
            if !self.show_editor {
                return None;
            }

            Some(("Custom editor window", &mut self.state))
        }

        fn render_editor_ui(ui: &mut egui::Ui, debug_state: &mut Self::EditState) {
            debug_state.hook_calls += 1;
            ui.label("Custom editor content");
        }
    }

    #[derive(Default)]
    struct DefaultEditorGame {
        state: EditorState,
    }

    impl Game for DefaultEditorGame {
        type EditState = EditorState;
        type Atlas = EmptyAtlas;

        fn setup(_renderer: &mut Renderer, _shaders: Self::Atlas) -> anyhow::Result<Self> {
            unreachable!("CPU editor tests do not initialize a renderer")
        }

        fn draw(&mut self, _renderer: FrameRenderer) -> Result<(), DrawError> {
            unreachable!("CPU editor tests do not draw GPU frames")
        }

        fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
            Some(("Reflected editor window", &mut self.state))
        }
    }

    fn editor_output(game: &mut dyn RuntimeGame) -> egui::FullOutput {
        let context = egui::Context::default();
        // Let egui finish the first window sizing pass before inspecting paint.
        let _ = context.run(egui::RawInput::default(), |context| {
            game.draw_edit_ui(context);
        });

        context.run(egui::RawInput::default(), |context| {
            game.draw_edit_ui(context);
        })
    }

    fn painted_text(output: &egui::FullOutput) -> Vec<String> {
        fn collect_text(shape: &egui::epaint::Shape, text: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(shape) => text.push(shape.galley.text().to_owned()),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text(shape, text);
                    }
                }
                _ => {}
            }
        }

        let mut text = Vec::new();
        for clipped_shape in &output.shapes {
            collect_text(&clipped_shape.shape, &mut text);
        }

        text
    }

    #[test]
    fn editor_dispatch_uses_custom_hook() {
        let mut game = CustomEditorGame {
            state: EditorState::default(),
            show_editor: true,
        };
        let output = editor_output(&mut game);
        let text = painted_text(&output);

        assert!(game.state.hook_calls > 0);
        assert!(text.iter().any(|text| text == "Custom editor window"));
        assert!(text.iter().any(|text| text == "Custom editor content"));
        assert!(!text.iter().any(|text| text == "reflected_value"));
    }

    #[test]
    fn default_editor_hook_renders_reflected_fields() {
        let mut game = DefaultEditorGame::default();
        let output = editor_output(&mut game);
        let text = painted_text(&output);

        assert!(text.iter().any(|text| text == "Reflected editor window"));
        assert!(text.iter().any(|text| text == "reflected_value"));
        assert!(text.iter().any(|text| text == "Reflected widget content"));
        assert_eq!(game.state.hook_calls, 0);
    }

    #[test]
    fn absent_editor_state_has_no_window() {
        let mut game = CustomEditorGame {
            state: EditorState::default(),
            show_editor: false,
        };
        let output = editor_output(&mut game);

        assert_eq!(game.state.hook_calls, 0);
        assert!(painted_text(&output).is_empty());
        assert!(output.shapes.is_empty());
    }
}
