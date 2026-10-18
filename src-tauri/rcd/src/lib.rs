unsafe extern "C" {
    fn slowraw_rcd(
        width: i32,
        height: i32,
        input: *const f32,
        output: *mut f32,
        pattern: *const u32,
    ) -> i32;
}

/// Full-resolution RCD, without sharpening or noise reduction. The input scratch
/// buffer is rescaled to the upstream range; output retains its original units.
pub fn demosaic(
    input: &mut [f32],
    width: usize,
    height: usize,
    pattern: [u32; 4],
) -> Result<Vec<f32>, String> {
    let count = width
        .checked_mul(height)
        .ok_or("Image dimensions overflow")?;
    if width < 32
        || height < 32
        || width > i32::MAX as usize
        || height > i32::MAX as usize
        || input.len() != count
    {
        return Err("RCD requires a complete Bayer image of at least 32x32 pixels".into());
    }
    if pattern.iter().filter(|&&c| c == 0).count() != 1
        || pattern.iter().filter(|&&c| c == 1).count() != 2
        || pattern.iter().filter(|&&c| c == 2).count() != 1
        || (pattern[0] & 1) != (pattern[3] & 1)
    {
        return Err("RCD requires a standard RGB Bayer pattern".into());
    }
    let peak = input.iter().try_fold(0.0_f32, |peak, &v| {
        if v.is_finite() {
            Ok(peak.max(v))
        } else {
            Err("Non-finite RAW sample")
        }
    })?;
    let scale = 65536.0_f64 / f64::from(peak.max(f32::MIN_POSITIVE));
    for value in input.iter_mut() {
        *value = (f64::from(value.max(0.0)) * scale) as f32;
    }
    let mut output = vec![0.0; count.checked_mul(3).ok_or("RGB dimensions overflow")?];
    // SAFETY: lengths, dimensions and CFA are checked; C++ catches exceptions and
    // consumes these buffers synchronously without retaining any pointers.
    let error = unsafe {
        slowraw_rcd(
            width as i32,
            height as i32,
            input.as_ptr(),
            output.as_mut_ptr(),
            pattern.as_ptr(),
        )
    };
    if error != 0 {
        return Err(format!("librtprocess RCD failed ({error})"));
    }
    for value in &mut output {
        *value = (f64::from(*value) / scale) as f32;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_bayer_patterns_preserve_flat_color_and_highlight_headroom() {
        for pattern in [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]] {
            let color = [0.25, 0.5, 1.5];
            let mut input: Vec<f32> = (0..64 * 64)
                .map(|i| color[pattern[(i / 64 % 2) * 2 + i % 2] as usize])
                .collect();
            let output = demosaic(&mut input, 64, 64, pattern).unwrap();
            for pixel in output.chunks_exact(3) {
                for c in 0..3 {
                    assert!((pixel[c] - color[c]).abs() < 0.0001, "{pixel:?}");
                }
            }
        }
    }

    #[test]
    fn rejects_invalid_input_and_handles_black() {
        assert!(demosaic(&mut [0.0; 4], 2, 2, [0, 1, 1, 2]).is_err());
        assert!(demosaic(&mut vec![0.0; 1024], 32, 32, [0, 0, 0, 0]).is_err());
        assert!(demosaic(&mut vec![f32::NAN; 1024], 32, 32, [0, 1, 1, 2]).is_err());
        assert!(
            demosaic(&mut vec![0.0; 1024], 32, 32, [0, 1, 1, 2])
                .unwrap()
                .iter()
                .all(|&v| v == 0.0)
        );
    }

    #[test]
    fn measured_samples_are_not_sharpened_even_across_tile_boundaries() {
        let (width, height) = (384, 208);
        let pattern = [0, 1, 1, 2];
        let mut input: Vec<f32> = (0..width * height)
            .map(|i| {
                let (x, y) = (i % width, i / width);
                0.1 + 1.4 * ((x / 7 + y / 11) % 2) as f32 + (x % 17) as f32 * 0.001
            })
            .collect();
        let original = input.clone();
        let output = demosaic(&mut input, width, height, pattern).unwrap();
        assert!(output.iter().all(|v| v.is_finite()));
        for i in 0..width * height {
            let channel = pattern[((i / width) % 2) * 2 + i % 2] as usize;
            assert!(
                (output[3 * i + channel] - original[i]).abs() < 0.00001,
                "sample {i}"
            );
        }
    }
}
