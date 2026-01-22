use sdl3::EventPump;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;
use sdl3::sys::timer::SDL_DelayPrecise;

use crate::game::traits::RuntimeGame;
use crate::renderer::{FrameRenderer, Renderer};
use crate::traits::{Input, Key, MouseButton};

pub struct App {
    renderer: Renderer,
    pub game: Box<dyn RuntimeGame>,
    pub minimized: bool,
    pub quit: bool,
}

impl App {
    pub fn init(renderer: Renderer, game: impl RuntimeGame + 'static) -> anyhow::Result<App> {
        Ok(Self {
            renderer,
            game: Box::new(game),
            minimized: false,
            quit: false,
        })
    }

    pub fn run_loop(mut self, mut event_pump: EventPump) -> anyhow::Result<()> {
        loop {
            let Ok(()) = self.handle_events(&mut event_pump) else {
                break;
            };
            if self.quit {
                break;
            }

            if !self.minimized {
                self.game.update();

                self.renderer.begin_egui_frame();
                if let Some(ctx) = self.renderer.egui_context() {
                    self.game.draw_edit_ui(&ctx);
                }

                let frame_renderer = FrameRenderer::new(&mut self.renderer);
                self.game.draw_frame(frame_renderer)?;
            }

            let frame_delay = self.game.frame_delay().as_nanos() as u64;
            unsafe { SDL_DelayPrecise(frame_delay) };
        }

        self.renderer.drain_gpu()?;

        Ok(())
    }

    // https://wiki.libsdl.org/SDL3/SDL_EventType
    pub fn handle_events(&mut self, event_pump: &mut EventPump) -> anyhow::Result<()> {
        for event in event_pump.poll_iter() {
            if let Some(egui) = self.renderer.egui() {
                egui.handle_sdl_event(&event);
            }

            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    self.quit = true;
                    return Ok(());
                }

                Event::Window { win_event, .. } => match win_event {
                    WindowEvent::Resized(_new_width, _new_height) => {
                        // we take the new dimensions off the renderer's window ref
                        self.renderer.on_resize()?;
                    }
                    WindowEvent::Minimized => {
                        self.minimized = true;
                    }
                    WindowEvent::Maximized => {
                        self.minimized = false;
                    }
                    WindowEvent::Restored => {
                        self.minimized = false;
                    }

                    WindowEvent::Exposed => {
                        // Window has been exposed and should be redrawn,
                        // and can be redrawn directly from event watchers for this event
                    }
                    WindowEvent::PixelSizeChanged(_, _) => {
                        // vulkan: update display scale
                    }
                    WindowEvent::FocusLost => {
                        // pause in-game?
                    }
                    WindowEvent::DisplayChanged(_) => {
                        // vulkan: update whatever is necessary for new surface
                        // ie, display scale
                    }
                    WindowEvent::Shown => {}
                    WindowEvent::Hidden => {
                        // what do these two mean? minimized to task bar?
                    }
                    WindowEvent::CloseRequested => {
                        // handle same as quit?
                    }

                    WindowEvent::Moved(_, _) => {}
                    WindowEvent::MouseEnter => {}
                    WindowEvent::MouseLeave => {}
                    WindowEvent::FocusGained => {}
                    WindowEvent::HitTest(_, _) => {}
                    WindowEvent::ICCProfChanged => {}

                    WindowEvent::None => {}
                },

                Event::KeyDown { scancode, .. } => {
                    let Some(key) = scancode.and_then(Key::from_sdl_scancode) else {
                        continue;
                    };
                    let input = Input::KeyDown(key);
                    self.game.input(input);
                }

                Event::KeyUp { scancode, .. } => {
                    let Some(key) = scancode.and_then(Key::from_sdl_scancode) else {
                        continue;
                    };
                    let input = Input::KeyUp(key);
                    self.game.input(input);
                }

                Event::MouseMotion { x, y, .. } => {
                    let input = Input::MouseMotion { x, y };
                    self.game.input(input);
                }

                Event::MouseButtonDown {
                    mouse_btn, x, y, ..
                } => {
                    let button = match mouse_btn {
                        sdl3::mouse::MouseButton::Left => MouseButton::Left,
                        sdl3::mouse::MouseButton::Middle => MouseButton::Middle,
                        sdl3::mouse::MouseButton::Right => MouseButton::Right,
                        _ => MouseButton::Unknown,
                    };

                    let input = Input::MouseDown { button, x, y };

                    self.game.input(input);
                }

                Event::MouseButtonUp {
                    mouse_btn, x, y, ..
                } => {
                    let button = match mouse_btn {
                        sdl3::mouse::MouseButton::Left => MouseButton::Left,
                        sdl3::mouse::MouseButton::Middle => MouseButton::Middle,
                        sdl3::mouse::MouseButton::Right => MouseButton::Right,
                        _ => MouseButton::Unknown,
                    };

                    let input = Input::MouseUp { button, x, y };

                    self.game.input(input);
                }

                _ => {}
            }
        }

        Ok(())
    }
}
