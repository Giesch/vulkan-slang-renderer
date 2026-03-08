use image::{GrayImage, Luma};
use vulkan_slang_renderer::util::manifest_path;

const SIZE: u32 = 2048;

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

fn perlin_fbm(x: f32, y: f32) -> f32 {
    let mut value = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    for _ in 0..3 {
        value += amplitude * perlin(x * frequency, y * frequency);
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    value
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
    let perlin_period = 128.0; // pixels per perlin cycle (was 1024/8)
    let worley_period = 21.333; // pixels per worley cell (was 1024/48)
    let jitter = 0.25;

    let mut data = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            let px = x as f32 / perlin_period;
            let py = y as f32 / perlin_period;

            let p = perlin_fbm(px, py);

            let (f1a, f2a) = worley(x as f32 / worley_period, y as f32 / worley_period, jitter);
            let w = (0.5 * (f2a - f1a)).sqrt();

            let h = 0.7 * p + 0.3 * w;
            data.push(h);
        }
    }

    let min = data.iter().copied().fold(f32::MAX, f32::min);
    let max = data.iter().copied().fold(f32::MIN, f32::max);
    let range = max - min;
    if range > 0.0 {
        for v in &mut data {
            *v = (*v - min) / range;
        }
    }

    data
}
