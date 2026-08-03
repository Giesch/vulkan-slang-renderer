use std::time::Instant;

mod generated;

use facet::Facet;
use mltrs::editor::Slider;
use mltrs::game::*;
use mltrs::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, TextureFilter,
    UniformBufferHandle,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::serenity_crt::*;
use mltrs::manifest_path;
use mltrs::util::load_image;

fn main() -> Result<(), anyhow::Error> {
    SerenityCRT::run()
}

struct SerenityCRT {
    start_time: Instant,
    edit_state: EditState,
    pipeline: PipelineHandle<DrawVertexCount>,
    params_buffer: UniformBufferHandle<SerenityCRTParams>,
}

#[derive(Facet)]
struct EditState {
    scanline_intensity: Slider,
    scanline_count: Slider,
    y_offset: Slider,
    brightness: Slider,
    contrast: Slider,
    saturation: Slider,
    bloom_intensity: Slider,
    bloom_threshold: Slider,
    rgb_shift: Slider,
    adaptive_intensity: Slider,
    vignette_strength: Slider,
    curvature: Slider,
    flicker_strength: Slider,
}

impl Game for SerenityCRT {
    type EditState = EditState;
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Serenity CRT"
    }

    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("Serenity CRT", &mut self.edit_state))
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let image_name = "serenity_crt/castlevania_pixel_art.png";
        let pixel_art_image = load_image(manifest_path!["textures", image_name])?;
        let texture =
            renderer.create_texture(image_name, &pixel_art_image, TextureFilter::Nearest)?;

        let params_buffer = renderer.create_uniform_buffer::<SerenityCRTParams>()?;
        let resources = Resources {
            tex: &texture,
            params_buffer: &params_buffer,
        };

        let pipeline_config = shaders.serenity_crt.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let edit_state = EditState {
            scanline_intensity: Slider::new(0.95, 0.0, 1.0),
            scanline_count: Slider::new(256.0 * 4.0, 0.0, 2000.0),
            y_offset: Slider::new(0.0, -1.0, 1.0),
            brightness: Slider::new(0.9, 0.0, 2.0),
            contrast: Slider::new(1.05, 0.0, 2.0),
            saturation: Slider::new(1.75, 0.0, 3.0),
            bloom_intensity: Slider::new(0.95, 0.0, 2.0),
            bloom_threshold: Slider::new(0.5, 0.0, 1.0),
            rgb_shift: Slider::new(1.0, 0.0, 5.0),
            adaptive_intensity: Slider::new(0.3, 0.0, 1.0),
            vignette_strength: Slider::new(0.3, 0.0, 1.0),
            curvature: Slider::new(0.1, 0.0, 0.5),
            flicker_strength: Slider::new(0.01, 0.0, 0.1),
        };

        Ok(Self {
            start_time: Instant::now(),
            edit_state,
            pipeline,
            params_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let elapsed = (Instant::now() - self.start_time).as_secs_f32();

        let params = SerenityCRTParams {
            resolution: renderer.window_resolution(),
            time: elapsed,

            scanline_intensity: self.edit_state.scanline_intensity.value,
            scanline_count: self.edit_state.scanline_count.value,
            y_offset: self.edit_state.y_offset.value,
            brightness: self.edit_state.brightness.value,
            contrast: self.edit_state.contrast.value,
            saturation: self.edit_state.saturation.value,
            bloom_intensity: self.edit_state.bloom_intensity.value,
            bloom_threshold: self.edit_state.bloom_threshold.value,
            rgb_shift: self.edit_state.rgb_shift.value,
            adaptive_intensity: self.edit_state.adaptive_intensity.value,
            vignette_strength: self.edit_state.vignette_strength.value,
            curvature: self.edit_state.curvature.value,
            flicker_strength: self.edit_state.flicker_strength.value,
        };

        renderer.draw_vertex_count(&self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }
}
