use image::{GrayImage, Luma};
use vulkan_slang_renderer::util::manifest_path;

const SIZE: u32 = 2048;

#[allow(dead_code)]
enum WorleyMode {
    F1,
    F2,
    F2MinusF1,
}

#[allow(dead_code)]
enum CombineMode {
    Additive,
    Multiplicative,
}

#[allow(dead_code)]
struct DomainWarpSettings {
    freq: f32,
    amp: f32,
}

#[allow(dead_code)]
enum TransferCurve {
    None,
    SmoothstepPlateau,
}

struct PaperParams {
    scale: f32,
    stretch_x: f32,
    stretch_y: f32,
    // fBm
    octaves: u32,
    gain: f32,
    lacunarity: f32,
    perlin_amp: f32,
    // Worley
    worley_freq: f32,
    worley_amp: f32,
    worley_mode: WorleyMode,
    jitter: f32,
    #[allow(dead_code)]
    domain_warp: Option<DomainWarpSettings>,
    // Micro grain
    grain_freq: f32,
    grain_amp: f32,
    // Combine & transfer
    combine: CombineMode,
    transfer: TransferCurve,
}

impl PaperParams {
    fn smooth_bond() -> Self {
        Self {
            scale: 8.0,
            stretch_x: 1.0,
            stretch_y: 1.0,
            octaves: 6,
            gain: 0.5,
            lacunarity: 2.0,
            perlin_amp: 0.7,
            worley_freq: 8.0,            // was 6.0
            worley_amp: 0.15,            // was 0.3
            worley_mode: WorleyMode::F1, // was F2MinusF1
            jitter: 1.0,                 // was 0.25
            domain_warp: None,
            grain_freq: 40.0, // was 64.0
            grain_amp: 0.08,  // was 0.05
            combine: CombineMode::Additive,
            transfer: TransferCurve::SmoothstepPlateau,
        }
    }
}

fn main() {
    let data = generate_paper_height_map(SIZE, SIZE);

    let mut img = GrayImage::new(SIZE, SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let v = data[(y * SIZE + x) as usize];
            img.put_pixel(x, y, Luma([(v * 255.0) as u8]));
        }
    }

    let path = manifest_path(["textures", "watercolor", "paper_height.png"]);
    img.save(&path).expect("failed to save paper texture");
    println!("saved paper texture to {}", path.display());
}

fn hash(n: f32) -> f32 {
    let s = (n * 127.1).sin() * 43758.546;
    s - s.floor()
}

fn hash2(x: f32, y: f32) -> (f32, f32) {
    let a = hash(x * 127.1 + y * 311.7);
    let b = hash(x * 269.5 + y * 183.3);
    (a, b)
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn grad(ix: i32, iy: i32, fx: f32, fy: f32) -> f32 {
    let (gx, gy) = hash2(ix as f32, iy as f32);
    let gx = gx * 2.0 - 1.0;
    let gy = gy * 2.0 - 1.0;
    gx * fx + gy * fy
}

fn perlin(x: f32, y: f32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();

    let ux = smoothstep(fx);
    let uy = smoothstep(fy);

    let n00 = grad(ix, iy, fx, fy);
    let n10 = grad(ix + 1, iy, fx - 1.0, fy);
    let n01 = grad(ix, iy + 1, fx, fy - 1.0);
    let n11 = grad(ix + 1, iy + 1, fx - 1.0, fy - 1.0);

    let nx0 = n00 + ux * (n10 - n00);
    let nx1 = n01 + ux * (n11 - n01);
    nx0 + uy * (nx1 - nx0)
}

fn perlin_fbm(x: f32, y: f32, octaves: u32, gain: f32, lacunarity: f32) -> f32 {
    let mut value = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut max_amplitude = 0.0;
    for _ in 0..octaves {
        value += amplitude * perlin(x * frequency, y * frequency);
        max_amplitude += amplitude;
        amplitude *= gain;
        frequency *= lacunarity;
    }
    value / max_amplitude
}

fn worley(x: f32, y: f32, jitter: f32) -> (f32, f32) {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();

    let mut f1 = f32::MAX;
    let mut f2 = f32::MAX;

    for dy in -1..=1 {
        for dx in -1..=1 {
            let (px, py) = hash2((ix + dx) as f32, (iy + dy) as f32);
            let px = 0.5 + jitter * (px - 0.5);
            let py = 0.5 + jitter * (py - 0.5);
            let vx = dx as f32 + px - fx;
            let vy = dy as f32 + py - fy;
            let d = vx * vx + vy * vy;
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }

    (f1.sqrt(), f2.sqrt())
}

fn generate_paper_height_map(width: u32, height: u32) -> Vec<f32> {
    let params = PaperParams::smooth_bond();

    let mut data = Vec::with_capacity((width * height) as usize);

    for py in 0..height {
        for px in 0..width {
            // World-space coords
            let x = (px as f32 / width as f32) * params.scale;
            let y = (py as f32 / height as f32) * params.scale;

            // Apply stretch
            let sx = x * params.stretch_x;
            let sy = y * params.stretch_y;

            // Perlin fBm (normalized to ~[-1,1])
            let p = perlin_fbm(sx, sy, params.octaves, params.gain, params.lacunarity)
                * params.perlin_amp;

            // Worley
            let wx = sx * params.worley_freq;
            let wy = sy * params.worley_freq;
            let (f1, f2) = worley(wx, wy, params.jitter);
            let w_raw = match params.worley_mode {
                WorleyMode::F1 => f1,
                WorleyMode::F2 => f2,
                WorleyMode::F2MinusF1 => f2 - f1,
            };
            let w = w_raw * params.worley_amp;

            // Micro grain
            let grain = perlin(sx * params.grain_freq, sy * params.grain_freq) * params.grain_amp;

            // Combine
            let h = match params.combine {
                CombineMode::Additive => p + w + grain,
                CombineMode::Multiplicative => p * w + grain,
            };

            data.push(h);
        }
    }

    // Normalize to [0,1]
    let min = data.iter().copied().fold(f32::MAX, f32::min);
    let max = data.iter().copied().fold(f32::MIN, f32::max);
    let range = max - min;
    if range > 0.0 {
        for v in &mut data {
            *v = (*v - min) / range;
        }
    }

    // Transfer curve
    match params.transfer {
        TransferCurve::None => {}
        TransferCurve::SmoothstepPlateau => {
            for v in &mut data {
                *v = smoothstep(smoothstep(*v));
            }
        }
    }

    data
}
