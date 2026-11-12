use crate::image_processing::apply_orientation;
use crate::white_balance::WhiteBalance;
use anyhow::{Result, anyhow};
use image::{DynamicImage, ImageBuffer, Rgb};
use rawler::imgop::xyz::Illuminant;
use rawler::{
    decoders::{Orientation, RawDecodeParams},
    imgop::render::{
        DemosaicQuality, ProcessingStep, RenderOptions, WhiteBalancePlacement, render,
    },
    rawimage::{RawImage, RawImageData, RawPhotometricInterpretation},
    rawsource::RawSource,
};
use rayon::prelude::*;
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

    let (a, d65, fallback) = camera_color_matrices(&raw_image);
    if let (Some(matrix_a), Some(matrix_d65)) = (a, d65) {
        return WhiteBalance::from_dual_illuminant_camera_neutral(matrix_a, matrix_d65, &neutral);
    }

    WhiteBalance::from_camera_neutral(d65.or(fallback)?, &neutral)
}

fn camera_color_matrices(raw: &RawImage) -> (Option<&[f32]>, Option<&[f32]>, Option<&[f32]>) {
    let Some(profile) = raw.as_shot_camera_profile() else {
        return (None, None, None);
    };
    let matrix = |illuminant| {
        profile
            .slot_of(illuminant)
            .and_then(|slot| profile.color_matrix[slot].as_deref())
    };
    (
        matrix(Illuminant::A),
        matrix(Illuminant::D65),
        raw.best_color_matrix().map(|(_, matrix)| matrix.as_slice()),
    )
}

pub fn develop_raw_image(
    file_bytes: &[u8],
    fast_demosaic: bool,
    highlight_compression: f32,
    linear_mode: String,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
    recovery: f32,
) -> Result<DynamicImage> {
    let (developed_image, orientation) = develop_internal(
        file_bytes,
        fast_demosaic,
        highlight_compression,
        linear_mode,
        cancel_token,
        recovery,
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

    #[test]
    #[ignore = "Requires a Nikon HE or HE* sample file"]
    fn nikon_high_efficiency_nef_develops() {
        let path =
            std::env::var_os("SLOWRAW_NIKON_HE_SAMPLE").expect("Set SLOWRAW_NIKON_HE_SAMPLE");
        let bytes = std::fs::read(path).unwrap();
        let image = develop_raw_image(&bytes, false, 1000.0, "auto".into(), None, 2.5).unwrap();
        let buffer = image.as_rgb32f().unwrap();
        let (width, height) = buffer.dimensions();
        assert!(
            width.min(height) >= 2000,
            "Unexpected Nikon image dimensions: {width}x{height}"
        );
        assert!(buffer.as_raw().iter().all(|sample| sample.is_finite()));
        assert!(buffer.as_raw().iter().any(|sample| *sample > 0.01));
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
    recovery: f32,
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
        .and_then(Orientation::from_u16)
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

    check_cancel()?;
    let demosaiced = demosaic_bayer(&mut raw_image)?;

    raw_image.wb_coeffs =
        crate::multi_exposure::neutralize_wb_if_multiexposure(raw_image.wb_coeffs, file_bytes);

    let camera_matrix = if (!is_linear_format || apply_calibration)
        && matches!(
            raw_image.photometric,
            RawPhotometricInterpretation::Cfa(_) | RawPhotometricInterpretation::LinearRaw
        ) {
        let neutral = raw_image.wb_coeffs.map(|v| {
            if v.is_finite() && v > 0.0 {
                1.0 / v
            } else {
                1.0
            }
        });
        let (a, d65, fallback) = camera_color_matrices(&raw_image);
        let matrix = if let (Some(a), Some(d65)) = (a, d65) {
            WhiteBalance::from_dual_illuminant_camera_neutral(a, d65, &neutral).map(|white| {
                let weight = ((1.0 / white.temperature as f64 - 1.0 / 6504.0)
                    / (1.0 / 2856.0 - 1.0 / 6504.0))
                    .clamp(0.0, 1.0) as f32;
                a.iter()
                    .zip(d65)
                    .map(|(a, d)| weight * a + (1.0 - weight) * d)
                    .collect::<Vec<_>>()
            })
        } else {
            d65.or(fallback).map(<[f32]>::to_vec)
        };
        matrix
            .filter(|m| m.len() == 9)
            .and_then(|m| crate::white_balance::camera_to_srgb(&m, &neutral))
    } else {
        None
    };
    let mut steps = vec![ProcessingStep::CropActiveArea, ProcessingStep::CropDefault];
    if !demosaiced {
        steps.push(ProcessingStep::Rescale);
        if !is_linear_format {
            steps.extend([ProcessingStep::Demosaic, ProcessingStep::FujiRotate]);
        }
    }
    if camera_matrix.is_none() && (!is_linear_format || apply_calibration) {
        steps.extend([ProcessingStep::WhiteBalance, ProcessingStep::Calibrate]);
    }
    let options = RenderOptions {
        steps: steps.into_iter().collect(),
        white_balance: WhiteBalancePlacement::AfterDemosaic,
        demosaic: DemosaicQuality::Fast,
        ..Default::default()
    };

    check_cancel()?;
    let mut developed = render(raw_image, &options)?;

    let denominator = (original_white_level - original_black_level).max(1.0);
    let rescale_factor = (u32::MAX as f32 - original_black_level) / denominator;

    let safe_highlight_compression = highlight_compression.max(1.01);

    let clamp_limit = if fast_demosaic {
        1.0
    } else {
        safe_highlight_compression
    };

    check_cancel()?;

    let reconstructs_camera_highlights = camera_matrix.is_some() && !is_linear_format;
    if developed.channels == 3
        && let Some(matrix) = camera_matrix
    {
        let pixels = developed.data.as_chunks_mut::<3>().0;
        if !is_linear_format && recovery > 0.0 {
            let region = crate::highlight_reconstruction::Region {
                stride: developed.width,
                x: 0,
                y: 0,
                width: developed.width,
                height: developed.height,
            };
            let clips =
                crate::highlight_reconstruction::detect_clip_levels(pixels, region, rescale_factor)
                    .map(|v| v / rescale_factor);
            crate::highlight_reconstruction::reconstruct_highlights(
                pixels, region, clips, recovery,
            );
        }
        pixels.par_iter_mut().for_each(|pixel| {
            let input = *pixel;
            *pixel = std::array::from_fn(|c| {
                matrix
                    .iter()
                    .zip(input)
                    .map(|(column, value)| column[c] * value)
                    .sum()
            });
        });
    }

    let clamp_min = match developed.channels {
        1 => 0.0,
        3 => -clamp_limit,
        _ => return Err(anyhow!("Unsupported rendered channel count")),
    };
    developed.data.iter_mut().for_each(|sample| {
        let mut value = *sample * rescale_factor;
        if is_linear_format && apply_ungamma {
            value = srgb_to_linear(value.clamp(0.0, 1.0));
        }
        *sample = value.clamp(clamp_min, clamp_limit);
    });

    check_cancel()?;

    let (width, height) = (developed.width as u32, developed.height as u32);
    let buffer = if developed.channels == 1 {
        ImageBuffer::<Rgb<f32>, _>::from_fn(width, height, |x, y| {
            let value = developed.data[(y * width + x) as usize];
            Rgb([value; 3])
        })
    } else {
        ImageBuffer::from_raw(width, height, developed.data)
            .ok_or_else(|| anyhow!("Invalid developed RGB dimensions"))?
    };
    let mut dynamic_image = DynamicImage::ImageRgb32F(buffer);

    if !reconstructs_camera_highlights {
        crate::highlight_recovery::apply_highlight_recovery(&mut dynamic_image, recovery);
    }
    Ok((dynamic_image, orientation))
}
