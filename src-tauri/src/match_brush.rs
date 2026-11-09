use image::DynamicImage;
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

pub struct Analysis {
    pub width: usize,
    pub height: usize,
    pub features: Vec<[f32; 4]>,
}

static CACHE: Mutex<Option<(blake3::Hash, Arc<Analysis>)>> = Mutex::new(None);

fn perceptual(rgb: [f32; 3]) -> [f32; 3] {
    let scale = 1.0 / (1.0 + rgb.iter().copied().fold(0.0f32, f32::max));
    let [r, g, b] = rgb.map(|v| if v.is_finite() { v * scale } else { 0.0 });
    let l = (0.4122215 * r + 0.5363325 * g + 0.051446 * b).cbrt();
    let m = (0.2119035 * r + 0.6806995 * g + 0.107397 * b).cbrt();
    let s = (0.0883025 * r + 0.2817188 * g + 0.6299787 * b).cbrt();
    [
        0.2104543 * l + 0.7936178 * m - 0.004072 * s,
        (1.9779985 * l - 2.4285922 * m + 0.4505937 * s) * 1.5,
        (0.025904 * l + 0.7827718 * m - 0.8086758 * s) * 1.5,
    ]
}

pub fn analyze(image: &DynamicImage) -> Arc<Analysis> {
    let mut hash = blake3::Hasher::new();
    hash.update(&image.width().to_le_bytes());
    hash.update(&image.height().to_le_bytes());
    hash.update(image.as_bytes());
    let key = hash.finalize();
    if let Some((_, analysis)) = CACHE.lock().unwrap().as_ref().filter(|(k, _)| *k == key) {
        return analysis.clone();
    }
    let small = image.thumbnail(1536, 1536).to_rgb32f();
    let (width, height) = (small.width() as usize, small.height() as usize);
    let lab: Vec<_> = small
        .as_raw()
        .par_chunks_exact(3)
        .map(|p| perceptual([p[0], p[1], p[2]]))
        .collect();
    let features = (0..lab.len())
        .into_par_iter()
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let mut sum = [0.0; 3];
            let (mut squared, mut count) = (0.0, 0.0);
            for yy in y.saturating_sub(2)..=(y + 2).min(height - 1) {
                for xx in x.saturating_sub(2)..=(x + 2).min(width - 1) {
                    let value = lab[yy * width + xx];
                    for c in 0..3 {
                        sum[c] += value[c];
                    }
                    squared += value[0] * value[0];
                    count += 1.0;
                }
            }
            let mean = sum.map(|v| v / count);
            [
                mean[0],
                mean[1],
                mean[2],
                (squared / count - mean[0] * mean[0]).max(0.0).sqrt() * 2.0,
            ]
        })
        .collect();
    let analysis = Arc::new(Analysis {
        width,
        height,
        features,
    });
    *CACHE.lock().unwrap() = Some((key, analysis.clone()));
    analysis
}

impl Analysis {
    pub fn sample(&self, x: f32, y: f32, size: (u32, u32)) -> [f32; 4] {
        let x =
            (x * self.width as f32 / size.0 as f32).clamp(0.0, (self.width - 1) as f32) as usize;
        let y =
            (y * self.height as f32 / size.1 as f32).clamp(0.0, (self.height - 1) as f32) as usize;
        self.features[y * self.width + x]
    }
}
