use crate::image_processing::apply_orientation;
use crate::white_balance::WhiteBalance;
use anyhow::{Result, anyhow};
use image::{DynamicImage, ImageBuffer, Rgb};
use rawler::imgop::xyz::Illuminant;
use rawler::{
    decoders::{Orientation, RawDecodeParams},
    imgop::develop::{Intermediate, ProcessingStep, RawDevelop},
    rawimage::{RawImage, RawImageData, RawPhotometricInterpretation},
    rawsource::RawSource,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub fn read_as_shot_white_balance(file_bytes: &[u8]) -> Option<WhiteBalance> {
    let source = RawSource::new_from_slice(file_bytes);
    let decoder = rawler::get_decoder(&source).ok()?;
    let raw_image = decoder
        .raw_image(&source, &RawDecodeParams::default(), true)
        .ok()?;
    if raw_image.cpp == 1 && !matches!(raw_image.photometric, RawPhotometricInterpretation::Cfa(_))
    {
        return None;
    }

    let wb_coeffs =
        crate::multi_exposure::neutralize_wb_if_multiexposure(raw_image.wb_coeffs, file_bytes);
    let neutral = if wb_coeffs[0].is_nan() {
        [1.0; 4]
    } else {
        wb_coeffs.map(|c| 1.0 / c)
    };

    let matrices = &raw_image.color_matrix;
    if let (Some(matrix_a), Some(matrix_d65)) =
        (matrices.get(&Illuminant::A), matrices.get(&Illuminant::D65))
    {
        return WhiteBalance::from_dual_illuminant_camera_neutral(matrix_a, matrix_d65, &neutral);
    }

    let color_matrix = matrices
        .get(&Illuminant::D65)
        .or_else(|| matrices.values().next())?;
    WhiteBalance::from_camera_neutral(color_matrix, &neutral)
}

pub fn develop_raw_image(
    file_bytes: &[u8],
    fast_demosaic: bool,
    highlight_compression: f32,
    linear_mode: String,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
) -> Result<DynamicImage> {
    let (developed_image, orientation) = develop_internal(
        file_bytes,
        fast_demosaic,
        highlight_compression,
        linear_mode,
        cancel_token,
    )?;
    Ok(apply_orientation(developed_image, orientation))
}

fn is_linear_raw_format(raw_image: &RawImage) -> bool {
    matches!(
        raw_image.photometric,
        RawPhotometricInterpretation::LinearRaw
    )
}

// Keep Rawler's decoding, scaling, camera calibration and crop handling. Only
// replace demosaicing for ordinary RGB Bayer sensors; other sensor layouts keep
// Rawler's sensor-specific developer.
fn demosaic_bayer(raw: &mut RawImage) -> Result<bool> {
    let RawPhotometricInterpretation::Cfa(config) = &raw.photometric else {
        return Ok(false);
    };
    if raw.cpp != 1
        || config.cfa.width != 2
        || config.cfa.height != 2
        || !config.cfa.is_rgb()
        || raw.fuji_rotation_width.is_some()
        || raw.width < 32
        || raw.height < 32
    {
        return Ok(false);
    }
    let pattern = std::array::from_fn(|i| config.cfa.color_at(i / 2, i % 2) as u32);
    raw.apply_scaling()?;
    let RawImageData::Float(mut pixels) =
        std::mem::replace(&mut raw.data, RawImageData::Float(Vec::new()))
    else {
        return Err(anyhow!(
            "Rawler did not produce floating-point sensor samples"
        ));
    };
    let rgb = slowraw_rcd::demosaic(&mut pixels, raw.width, raw.height, pattern)
        .map_err(anyhow::Error::msg)?;
    raw.data = RawImageData::Float(rgb);
    raw.cpp = 3;
    raw.photometric = RawPhotometricInterpretation::LinearRaw;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawler::{
        CFA,
        cfa::PlaneColor,
        decoders::Camera,
        imgop::{Dim2, Point, Rect},
        rawimage::CFAConfig,
    };

    #[test]
    fn rcd_uses_sensor_coordinates_and_rawler_applies_the_crop_once() {
        let cfa = CFA::new("RGGB");
        let colors = PlaneColor::new("RGB");
        let camera = Camera {
            cfa: cfa.clone(),
            plane_color: colors.clone(),
            blacklevel: Some(vec![512]),
            whitelevel: Some(vec![4095]),
            ..Default::default()
        };
        let values = [0.25, 0.5, 0.75];
        let pixels = (0..64 * 64)
            .map(|i| 512.0 + 3583.0 * values[cfa.color_at(i / 64, i % 64)])
            .collect();
        let mut raw = RawImage::new_with_data(
            camera,
            RawImageData::Float(pixels),
            64,
            64,
            1,
            [1.0; 4],
            RawPhotometricInterpretation::Cfa(CFAConfig::new(&cfa, &colors)),
            None,
            None,
            false,
        );
        raw.crop_area = Some(Rect::new(Point::new(3, 5), Dim2::new(58, 54)));
        assert!(demosaic_bayer(&mut raw).unwrap());
        assert!(!demosaic_bayer(&mut raw).unwrap());
        let developed = RawDevelop::new_with(&[ProcessingStep::CropDefault])
            .develop_intermediate(&raw)
            .unwrap();
        let Intermediate::ThreeColor(rgb) = developed else {
            panic!("Expected RGB");
        };
        assert_eq!((rgb.dim().w, rgb.dim().h), (58, 54));
        for pixel in rgb.data {
            for c in 0..3 {
                assert!((pixel[c] - values[c]).abs() < 0.0001);
            }
        }
    }
}

#[inline]
fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(3.0)
    }
}

fn develop_internal(
    file_bytes: &[u8],
    fast_demosaic: bool,
    highlight_compression: f32,
    linear_mode: String,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
) -> Result<(DynamicImage, Orientation)> {
    let check_cancel = || -> Result<()> {
        if let Some((tracker, generation)) = &cancel_token
            && tracker.load(Ordering::SeqCst) != *generation
        {
            return Err(anyhow!("Load cancelled"));
        }
        Ok(())
    };

    check_cancel()?;

    let source = RawSource::new_from_slice(file_bytes);
    let decoder = rawler::get_decoder(&source)?;

    check_cancel()?;
    let mut raw_image: RawImage = decoder.raw_image(&source, &RawDecodeParams::default(), false)?;

    let metadata = decoder.raw_metadata(&source, &RawDecodeParams::default())?;
    let orientation = metadata
        .exif
        .orientation
        .map(Orientation::from_u16)
        .unwrap_or(Orientation::Normal);

    let is_linear_format = is_linear_raw_format(&raw_image);

    let (apply_ungamma, apply_calibration) = match linear_mode.as_str() {
        "gamma" => (true, true),
        "skip_calib" => (false, false),
        "gamma_skip_calib" => (true, false),
        _ => (false, true),
    };

    let original_white_level = raw_image
        .whitelevel
        .0
        .first()
        .cloned()
        .unwrap_or(u16::MAX as u32) as f32;
    let original_black_level = raw_image
        .blacklevel
        .levels
        .first()
        .map(|r| r.as_f32())
        .unwrap_or(0.0);

    for level in raw_image.whitelevel.0.iter_mut() {
        *level = u32::MAX;
    }

    let mut developer = RawDevelop::default();

    check_cancel()?;
    if demosaic_bayer(&mut raw_image)? {
        developer
            .steps
            .retain(|&step| step != ProcessingStep::Rescale && step != ProcessingStep::Demosaic);
    }

    if is_linear_format {
        developer.steps.retain(|&step| {
            step != ProcessingStep::SRgb
                && step != ProcessingStep::Demosaic
                && (apply_calibration || step != ProcessingStep::Calibrate)
        });
    } else {
        developer.steps.retain(|&step| step != ProcessingStep::SRgb);
    }

    raw_image.wb_coeffs =
        crate::multi_exposure::neutralize_wb_if_multiexposure(raw_image.wb_coeffs, file_bytes);

    check_cancel()?;
    let mut developed_intermediate = developer.develop_intermediate(&raw_image)?;

    drop(raw_image);

    let denominator = (original_white_level - original_black_level).max(1.0);
    let rescale_factor = (u32::MAX as f32 - original_black_level) / denominator;

    let safe_highlight_compression = highlight_compression.max(1.01);

    let clamp_limit = if fast_demosaic {
        1.0
    } else {
        safe_highlight_compression
    };

    check_cancel()?;

    match &mut developed_intermediate {
        Intermediate::Monochrome(pixels) => {
            pixels.data.iter_mut().for_each(|p| {
                let mut linear_val = *p * rescale_factor;
                if is_linear_format && apply_ungamma {
                    linear_val = srgb_to_linear(linear_val.clamp(0.0, 1.0));
                }
                *p = linear_val.clamp(0.0, clamp_limit);
            });
        }
        Intermediate::ThreeColor(pixels) => {
            pixels.data.iter_mut().for_each(|p| {
                let mut r = (p[0] * rescale_factor).max(0.0);
                let mut g = (p[1] * rescale_factor).max(0.0);
                let mut b = (p[2] * rescale_factor).max(0.0);

                if is_linear_format && apply_ungamma {
                    r = srgb_to_linear(r.clamp(0.0, 1.0));
                    g = srgb_to_linear(g.clamp(0.0, 1.0));
                    b = srgb_to_linear(b.clamp(0.0, 1.0));
                }

                p[0] = r.clamp(-clamp_limit, clamp_limit);
                p[1] = g.clamp(-clamp_limit, clamp_limit);
                p[2] = b.clamp(-clamp_limit, clamp_limit);
            });
        }
        Intermediate::FourColor(pixels) => {
            pixels.data.iter_mut().for_each(|p| {
                p.iter_mut().for_each(|c| {
                    let mut linear_val = *c * rescale_factor;
                    if is_linear_format && apply_ungamma {
                        linear_val = srgb_to_linear(linear_val.clamp(0.0, 1.0));
                    }
                    *c = linear_val.clamp(0.0, clamp_limit);
                });
            });
        }
    }

    let (width, height) = {
        let dim = developed_intermediate.dim();
        (dim.w as u32, dim.h as u32)
    };

    check_cancel()?;

    let dynamic_image = match developed_intermediate {
        Intermediate::ThreeColor(pixels) => {
            let buffer = ImageBuffer::<Rgb<f32>, _>::from_fn(width, height, |x, y| {
                let p = pixels.data[(y * width + x) as usize];
                Rgb([p[0], p[1], p[2]])
            });
            DynamicImage::ImageRgb32F(buffer)
        }
        Intermediate::Monochrome(pixels) => {
            let buffer = ImageBuffer::<Rgb<f32>, _>::from_fn(width, height, |x, y| {
                let p = pixels.data[(y * width + x) as usize];
                Rgb([p, p, p])
            });
            DynamicImage::ImageRgb32F(buffer)
        }
        _ => {
            return Err(anyhow!("Unsupported intermediate format for conversion"));
        }
    };

    Ok((dynamic_image, orientation))
}
