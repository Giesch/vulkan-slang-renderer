use std::time::Instant;

use serde::Deserialize;
use vulkan_slang_renderer::game::*;
use vulkan_slang_renderer::renderer::{
    DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer, UniformBufferHandle,
};

use vulkan_slang_renderer::generated::shader_atlas::ShaderAtlas;
use vulkan_slang_renderer::generated::shader_atlas::sdf_2d::*;

use rodio::MixerDeviceSink;
use std::io::BufReader;
use vulkan_slang_renderer::util::manifest_path;

#[derive(Debug, Deserialize)]
struct BeatsData {
    // bpm: f64,
    // beats_confidence: f64,
    beats: Vec<f64>,
    // beats_intervals: Vec<f64>,
}

fn main() -> Result<(), anyhow::Error> {
    SDF2D::run()
}

struct SDF2D {
    start_time: Instant,
    beats: BeatsData,

    pipeline: PipelineHandle<DrawVertexCount>,
    params_buffer: UniformBufferHandle<SDF2DParams>,

    #[expect(unused)]
    device_sink: MixerDeviceSink,
    #[expect(unused)]
    sink: rodio::Player,
}

impl Game for SDF2D {
    type EditState = ();

    fn window_title() -> &'static str {
        "SDF 2D"
    }

    fn setup(renderer: &mut Renderer) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let start_time = Instant::now();

        let beats: BeatsData =
            serde_json::from_str(&std::fs::read_to_string("audio/alias_abandon.beats.json")?)?;

        let params_buffer = renderer.create_uniform_buffer::<SDF2DParams>()?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };

        let shader = ShaderAtlas::init().sdf_2d;
        let pipeline_config = shader.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let mut device_sink = rodio::DeviceSinkBuilder::open_default_sink()?;
        device_sink.log_on_drop(false);
        let mixer = device_sink.mixer();
        let audio_path = manifest_path(["audio", "alias_abandon.flac"]);
        let file = std::fs::File::open(&audio_path)?;
        let sink = rodio::play(mixer, BufReader::new(file))?;
        sink.set_volume(0.5);

        Ok(Self {
            start_time,
            pipeline,
            params_buffer,
            beats,
            device_sink,
            sink,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let time = (Instant::now() - self.start_time).as_secs_f32();
        let resolution = renderer.window_resolution();

        // Find the closest beat timestamp and compute proximity (1.0 = on beat, 0.0 = far from beat)
        let time_f64 = time as f64;
        let idx = self
            .beats
            .beats
            .binary_search_by(|b| b.partial_cmp(&time_f64).unwrap())
            .unwrap_or_else(|i| i);
        let mut min_dist = f32::MAX;
        if idx > 0 {
            min_dist = min_dist.min((self.beats.beats[idx - 1] - time_f64).abs() as f32);
        }
        if idx < self.beats.beats.len() {
            min_dist = min_dist.min((self.beats.beats[idx] - time_f64).abs() as f32);
        }
        // Map distance to a 0..1 proximity: peaks at 1.0 on a beat, decays over ~0.15s
        let beat_proximity = (-min_dist / 0.15).exp();

        let params = SDF2DParams {
            time,
            resolution,
            beat_proximity,
        };

        renderer.draw_vertex_count(&self.pipeline, 3, |gpu| {
            gpu.write_uniform(&mut self.params_buffer, params);
        })
    }
}
