mod generated;

use generated::shader_atlas::readback_compute::{
    ReadbackOutput, ReadbackPush, ReadbackTag, Resources, Shader,
};
use mltrs_renderer::env_config::EnvConfig;
use mltrs_renderer::renderer::{MaxMSAASamples, Renderer, debug};

fn main() -> anyhow::Result<()> {
    anyhow::ensure!(cfg!(debug_assertions), "validation requires a debug build");
    pretty_env_logger::init();
    let sdl = sdl3::init().map_err(anyhow::Error::msg)?;
    let video = sdl.video().map_err(anyhow::Error::msg)?;
    let window = video
        .window("GPU readback diagnostic", 64, 64)
        .vulkan()
        .hidden()
        .build()?;
    let mut renderer = Renderer::init(
        window,
        EnvConfig::from_env(),
        false,
        1.0,
        MaxMSAASamples::Off,
        false,
        #[cfg(debug_assertions)]
        concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source"),
    )?;
    let shader = Shader::init();
    let mut params = renderer
        .create_uniform_buffer::<generated::shader_atlas::readback_compute::ReadbackInput>()?;
    let pipeline = renderer.create_compute_pipeline(shader.pipeline_config(Resources {
        params_buffer: &params,
    }))?;
    let output = renderer.create_gpu_only_buffer::<ReadbackOutput>(2)?;
    let values = renderer.dispatch_readback(&pipeline, &output, [2, 1, 1], |gpu, output| {
        gpu.write_uniform(
            &mut params,
            generated::shader_atlas::readback_compute::ReadbackInput {
                base_marker: glam::UVec4::splat(42),
            },
        );
        ReadbackPush {
            output,
            invalid_tag: 0,
            reserved: 0,
        }
    })?;
    assert_eq!(values.len(), 2);
    for (i, value) in values.iter().enumerate() {
        let f = i as f32;
        let u = i as u32;
        assert_eq!(value.marker, 42 + u);
        // Independently calculated row dot products with (1, 2, 3, 1).
        assert_eq!(
            value.vector.to_array(),
            [30.0 + f, 107.0 + f, 211.0 + f, 321.0 + f]
        );
        assert_eq!(value.nested.xy.to_array(), [-2.5 + f, 8.25 + f]);
        assert_eq!(value.nested.id, 100 + u);
        assert_eq!(
            value.tag,
            if i == 0 {
                ReadbackTag::Ready
            } else {
                ReadbackTag::Done
            }
        );
        // Readback preserves raw upload representation: Slang rows become glam columns.
        let columns = [
            2.0, 3.0, 5.0, 7.0, 11.0, 13.0, 17.0, 19.0, 23.0, 29.0, 31.0, 37.0, 41.0, 43.0, 47.0,
            53.0,
        ];
        println!(
            "element {i}: shader matvec={:?}; decoded columns={:?}",
            value.vector.to_array(),
            value.transform.to_cols_array()
        );
        assert_eq!(
            value.transform.to_cols_array(),
            columns.map(|v| v * (f + 1.0))
        );
        assert_eq!(value.values[0].to_array(), [10 + u, 20 + u, 30 + u, 40 + u]);
        assert_eq!(value.values[1].to_array(), [50 + u, 60 + u, 70 + u, 80 + u]);
    }
    let error = renderer
        .dispatch_readback(&pipeline, &output, [2, 1, 1], |_, output| ReadbackPush {
            output,
            invalid_tag: 1,
            reserved: 0,
        })
        .expect_err("GPU enum discriminant 99 must be rejected");
    let message = format!("{error:#}");
    anyhow::ensure!(
        message.contains("ReadbackTag") && message.contains("99"),
        "unexpected decode error: {message}"
    );
    drop(renderer);
    anyhow::ensure!(
        debug::validation_message_count() == 0,
        "Vulkan validation reported {} messages",
        debug::validation_message_count()
    );
    println!("GPU_READBACK_OK: 2 elements; invalid enum rejected; validation=0; teardown complete");

    Ok(())
}
