use ash::vk;
use egui::{Context, Event, Key, Modifiers, Pos2, RawInput, Vec2};
use sdl3::event::Event as SdlEvent;
use sdl3::event::WindowEvent;
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;

use super::MAX_FRAMES_IN_FLIGHT;

pub struct EguiIntegration {
    start_time: std::time::Instant,
    frame_begun: bool,

    pub ctx: Context,
    renderer: egui_ash_renderer::Renderer,
    raw_input: RawInput,
    // Textures to free on the next frame (per frame-in-flight slot)
    pending_free_textures: [Vec<egui::TextureId>; MAX_FRAMES_IN_FLIGHT],
}

impl EguiIntegration {
    pub fn new(
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        device: ash::Device,
        render_pass: vk::RenderPass,
    ) -> Result<Self, egui_ash_renderer::RendererError> {
        let renderer = egui_ash_renderer::Renderer::with_default_allocator(
            instance,
            physical_device,
            device,
            render_pass,
            egui_ash_renderer::Options {
                in_flight_frames: MAX_FRAMES_IN_FLIGHT,
                ..Default::default()
            },
        )?;

        Ok(Self {
            ctx: Context::default(),
            renderer,
            raw_input: RawInput::default(),
            start_time: std::time::Instant::now(),
            frame_begun: false,
            pending_free_textures: [vec![], vec![]],
        })
    }

    /// Called when render pass is recreated (on resize)
    pub fn set_render_pass(&mut self, render_pass: vk::RenderPass) {
        self.renderer.set_render_pass(render_pass).unwrap();
    }

    /// Free textures that were marked for deletion in a previous frame.
    /// Call this after waiting on the fence for this frame slot.
    pub fn free_pending_textures(&mut self, frame_index: usize) {
        let textures = std::mem::take(&mut self.pending_free_textures[frame_index]);
        if !textures.is_empty() {
            self.renderer.free_textures(&textures).unwrap();
        }
    }

    /// Translate SDL3 event to egui event and accumulate
    pub fn handle_sdl_event(&mut self, event: &SdlEvent) {
        if let Some(egui_event) = translate_sdl_event(event) {
            self.raw_input.events.push(egui_event);
        }
        update_modifiers(&mut self.raw_input.modifiers, event);
    }

    /// Begin egui frame - call at start of frame after handling events.
    /// Idempotent: safe to call multiple times per frame.
    pub fn begin_frame(&mut self, screen_size: [f32; 2]) {
        if self.frame_begun {
            return;
        }
        self.frame_begun = true;

        self.raw_input.time = Some(self.start_time.elapsed().as_secs_f64());
        self.raw_input.screen_rect = Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(screen_size[0], screen_size[1]),
        ));

        self.ctx.begin_pass(self.raw_input.take());
    }

    /// Draw the debug overlay (time display)
    pub fn draw_debug_overlay(&self) {
        egui::Window::new("Debug").show(&self.ctx, |ui| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            let secs = now.as_secs();
            let hours = (secs / 3600) % 24;
            let mins = (secs / 60) % 60;
            let secs_display = secs % 60;
            ui.label(format!(
                "Time: {:02}:{:02}:{:02} UTC",
                hours, mins, secs_display
            ));
        });
    }

    /// End egui frame and record draw commands into the command buffer.
    /// Must be called while render pass is active.
    /// Returns true if egui wants keyboard input (a text field has focus).
    pub fn end_frame_and_draw(
        &mut self,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
        command_buffer: vk::CommandBuffer,
        extent: vk::Extent2D,
        frame_index: usize,
    ) -> bool {
        let wants_keyboard_input = self.ctx.wants_keyboard_input();
        let output = self.ctx.end_pass();

        // Handle texture updates
        self.renderer
            .set_textures(queue, command_pool, output.textures_delta.set.as_slice())
            .unwrap();

        // Tessellate and draw
        let clipped_primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);

        self.renderer
            .cmd_draw(
                command_buffer,
                extent,
                output.pixels_per_point,
                clipped_primitives.as_slice(),
            )
            .unwrap();

        // Defer texture freeing until next frame when we know the GPU is done with this frame slot
        self.pending_free_textures[frame_index].extend(output.textures_delta.free);

        self.frame_begun = false;
        wants_keyboard_input
    }

    /// Get a reference to the egui context for UI building
    pub fn context(&self) -> &Context {
        &self.ctx
    }
}

fn translate_sdl_event(event: &SdlEvent) -> Option<Event> {
    match event {
        SdlEvent::MouseMotion { x, y, .. } => Some(Event::PointerMoved(Pos2::new(*x, *y))),

        SdlEvent::MouseButtonDown {
            mouse_btn, x, y, ..
        } => Some(Event::PointerButton {
            pos: Pos2::new(*x, *y),
            button: translate_mouse_button(*mouse_btn)?,
            pressed: true,
            modifiers: Modifiers::default(),
        }),

        SdlEvent::MouseButtonUp {
            mouse_btn, x, y, ..
        } => Some(Event::PointerButton {
            pos: Pos2::new(*x, *y),
            button: translate_mouse_button(*mouse_btn)?,
            pressed: false,
            modifiers: Modifiers::default(),
        }),

        SdlEvent::MouseWheel { x, y, .. } => Some(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: Vec2::new(*x * 10.0, *y * 10.0),
            modifiers: Modifiers::default(),
        }),

        SdlEvent::KeyDown {
            keycode, repeat, ..
        } => {
            let key = keycode.and_then(translate_keycode)?;
            Some(Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: *repeat,
                modifiers: Modifiers::default(),
            })
        }

        SdlEvent::KeyUp { keycode, .. } => {
            let key = keycode.and_then(translate_keycode)?;
            Some(Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::default(),
            })
        }

        SdlEvent::TextInput { text, .. } => Some(Event::Text(text.clone())),

        SdlEvent::Window { win_event, .. } => match win_event {
            WindowEvent::FocusGained => Some(Event::WindowFocused(true)),
            WindowEvent::FocusLost => Some(Event::WindowFocused(false)),
            WindowEvent::MouseLeave => Some(Event::PointerGone),
            _ => None,
        },

        _ => None,
    }
}

fn translate_mouse_button(btn: MouseButton) -> Option<egui::PointerButton> {
    match btn {
        MouseButton::Left => Some(egui::PointerButton::Primary),
        MouseButton::Right => Some(egui::PointerButton::Secondary),
        MouseButton::Middle => Some(egui::PointerButton::Middle),
        MouseButton::X1 => Some(egui::PointerButton::Extra1),
        MouseButton::X2 => Some(egui::PointerButton::Extra2),
        _ => None,
    }
}

fn translate_keycode(keycode: sdl3::keyboard::Keycode) -> Option<egui::Key> {
    match keycode {
        Keycode::A => Some(Key::A),
        Keycode::B => Some(Key::B),
        Keycode::C => Some(Key::C),
        Keycode::D => Some(Key::D),
        Keycode::E => Some(Key::E),
        Keycode::F => Some(Key::F),
        Keycode::G => Some(Key::G),
        Keycode::H => Some(Key::H),
        Keycode::I => Some(Key::I),
        Keycode::J => Some(Key::J),
        Keycode::K => Some(Key::K),
        Keycode::L => Some(Key::L),
        Keycode::M => Some(Key::M),
        Keycode::N => Some(Key::N),
        Keycode::O => Some(Key::O),
        Keycode::P => Some(Key::P),
        Keycode::Q => Some(Key::Q),
        Keycode::R => Some(Key::R),
        Keycode::S => Some(Key::S),
        Keycode::T => Some(Key::T),
        Keycode::U => Some(Key::U),
        Keycode::V => Some(Key::V),
        Keycode::W => Some(Key::W),
        Keycode::X => Some(Key::X),
        Keycode::Y => Some(Key::Y),
        Keycode::Z => Some(Key::Z),
        Keycode::_0 => Some(Key::Num0),
        Keycode::_1 => Some(Key::Num1),
        Keycode::_2 => Some(Key::Num2),
        Keycode::_3 => Some(Key::Num3),
        Keycode::_4 => Some(Key::Num4),
        Keycode::_5 => Some(Key::Num5),
        Keycode::_6 => Some(Key::Num6),
        Keycode::_7 => Some(Key::Num7),
        Keycode::_8 => Some(Key::Num8),
        Keycode::_9 => Some(Key::Num9),
        Keycode::Escape => Some(Key::Escape),
        Keycode::Tab => Some(Key::Tab),
        Keycode::Backspace => Some(Key::Backspace),
        Keycode::Return => Some(Key::Enter),
        Keycode::Space => Some(Key::Space),
        Keycode::Left => Some(Key::ArrowLeft),
        Keycode::Right => Some(Key::ArrowRight),
        Keycode::Up => Some(Key::ArrowUp),
        Keycode::Down => Some(Key::ArrowDown),
        Keycode::Home => Some(Key::Home),
        Keycode::End => Some(Key::End),
        Keycode::PageUp => Some(Key::PageUp),
        Keycode::PageDown => Some(Key::PageDown),
        Keycode::Insert => Some(Key::Insert),
        Keycode::Delete => Some(Key::Delete),
        _ => None,
    }
}

fn update_modifiers(modifiers: &mut Modifiers, event: &SdlEvent) {
    if let SdlEvent::KeyDown { keymod, .. } | SdlEvent::KeyUp { keymod, .. } = event {
        modifiers.alt =
            keymod.intersects(sdl3::keyboard::Mod::LALTMOD | sdl3::keyboard::Mod::RALTMOD);
        modifiers.ctrl =
            keymod.intersects(sdl3::keyboard::Mod::LCTRLMOD | sdl3::keyboard::Mod::RCTRLMOD);
        modifiers.shift =
            keymod.intersects(sdl3::keyboard::Mod::LSHIFTMOD | sdl3::keyboard::Mod::RSHIFTMOD);
        modifiers.mac_cmd = false;
        modifiers.command = modifiers.ctrl;
    }
}
