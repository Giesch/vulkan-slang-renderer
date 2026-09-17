use std::time::Instant;

mod generated;

use mltrs::env_config::EnvConfig;
use mltrs::game::*;
use mltrs::renderer::{
    DrawError, DrawVertexCountNode, FrameRenderer, PreparedRenderGraph, RenderGraph, Renderer,
    ResourcePlanner, UniformBufferHandle, draw_vertex_count,
};
use serde::Deserialize;

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::sdf_2d::*;

use mltrs::manifest_path;
use rodio::MixerDeviceSink;
use std::io::BufReader;

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

type Sdf2dGraph = PreparedRenderGraph<DrawVertexCountNode<SDF2DParams>>;

struct SDF2D {
    start_time: Instant,
    beats: BeatsData,
    graph: Sdf2dGraph,
    /// The graph captured this buffer's slot at build time; the handle stays
    /// here to keep the buffer alive.
    _params_buffer: UniformBufferHandle<SDF2DParams>,
    /// Playback only — the visuals are driven by `beats` plus elapsed time, not
    /// by the audio stream — so this is `None` on a machine with no output
    /// device (containers, CI, `scripts/headless-sweep.sh`) and the example
    /// still renders correctly. Held solely to keep the stream alive; the tuple
    /// drops in declaration order, device sink before player, same as the two
    /// fields it replaced.
    #[expect(unused)]
    audio: Option<(MixerDeviceSink, rodio::Player)>,
}

/// Opens the default output device and starts the track. Fails when there is no
/// audio device at all, which [`SDF2D::setup`] treats as non-fatal.
fn start_audio() -> anyhow::Result<(MixerDeviceSink, rodio::Player)> {
    let config = EnvConfig::from_env();
    if config.headless_sweep {
        anyhow::bail!("headless sweep");
    }

    let mut device_sink = rodio::DeviceSinkBuilder::open_default_sink()?;
    device_sink.log_on_drop(false);
    let mixer = device_sink.mixer();
    let audio_path = manifest_path!["audio", "alias_abandon.flac"];
    let file = std::fs::File::open(&audio_path)?;
    let sink = rodio::play(mixer, BufReader::new(file))?;
    sink.set_volume(0.5);
    Ok((device_sink, sink))
}

impl Game for SDF2D {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "SDF 2D"
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let start_time = Instant::now();

        let beats: BeatsData = serde_json::from_str(&std::fs::read_to_string(manifest_path![
            "audio",
            "alias_abandon.beats.json"
        ])?)?;

        let params_buffer = renderer.create_uniform_buffer::<SDF2DParams>()?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };

        let pipeline_config = shaders.sdf_2d.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        // REVIEW could we modify draw_vertex_count to use a '.with_bindings()' method similar to how we do push constants elsewhere?
        // `SDF2DParams` has no resource fields, so its bindings type is `()`
        // and the whole struct is the per-frame value.
        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            draw_vertex_count(&pipeline, &params_buffer, 3, ()),
        )?
        .prepare(renderer)?;

        // eprintln! rather than log::warn! on purpose: with RUST_LOG unset,
        // env_logger keeps only error!, so a warning here would be invisible
        // exactly on the machines that hit this path.
        let audio = match start_audio() {
            Ok(audio) => Some(audio),
            Err(err) => {
                eprintln!("sdf_2d: no audio ({err}); rendering silently");
                None
            }
        };

        Ok(Self {
            start_time,
            graph,
            _params_buffer: params_buffer,
            beats,
            audio,
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

        self.graph.execute(renderer, &params)
    }
}
