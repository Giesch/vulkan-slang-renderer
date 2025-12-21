use sdl3::EventPump;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;
use sdl3::sys::timer::SDL_DelayPrecise;

use crate::game::traits::RuntimeGame;
use crate::renderer::Renderer;
use crate::traits::{Input, Key};

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
                self.game.draw_frame(&mut self.renderer)?;
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

                _ => {}
            }
        }

        Ok(())
    }
}
