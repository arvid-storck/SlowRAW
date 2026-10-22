//! Shared Vulkan display transform: image RGB -> linear light -> display matrix/TRCs.
use crate::color_management::ColorSpace;
use moxcms::ColorProfile;

pub const CURVE_SAMPLES: usize = 4096;

/// Formats are presentation transports, not the image's working color space.
pub fn surface_format(
    caps: &wgpu::SurfaceCapabilities,
) -> Option<(wgpu::TextureFormat, wgpu::SurfaceColorSpace)> {
    use wgpu::{SurfaceColorSpace as S, TextureFormat as F};
    for (format, space) in [
        (F::Rgba16Float, S::ExtendedSrgbLinear),
        (F::Rgba16Float, S::ExtendedSrgb),
        (F::Rgb10a2Unorm, S::Bt2100Pq),
        (F::Rgba16Float, S::Bt2100Pq),
        (F::Rgba16Float, S::DisplayP3),
        (F::Rgb10a2Unorm, S::DisplayP3),
        (F::Bgra8Unorm, S::DisplayP3),
        (F::Rgba8Unorm, S::DisplayP3),
    ] {
        if supports(caps, format, space) {
            return Some((format, space));
        }
    }
    sdr_surface_format(caps)
}

fn supports(
    caps: &wgpu::SurfaceCapabilities,
    format: wgpu::TextureFormat,
    space: wgpu::SurfaceColorSpace,
) -> bool {
    caps.format_capabilities.iter().any(|cap| {
        cap.format == format
            && space
                .to_color_spaces()
                .is_some_and(|flag| cap.color_spaces.contains(flag))
    })
}

pub fn sdr_surface_format(
    caps: &wgpu::SurfaceCapabilities,
) -> Option<(wgpu::TextureFormat, wgpu::SurfaceColorSpace)> {
    use wgpu::{SurfaceColorSpace as S, TextureFormat as F};
    [F::Rgb10a2Unorm, F::Bgra8Unorm, F::Rgba8Unorm]
        .into_iter()
        .find(|format| supports(caps, *format, S::Srgb))
        .map(|format| (format, S::Srgb))
}

/// A matrix/TRC monitor transform, inspired by vkdt's display-only transform.
/// Sampling the actual inverse TRCs also handles ordinary sampled ICC curves.
pub struct MonitorProfile {
    pub profile: ColorProfile,
    pub curves: Vec<[f32; 4]>,
}

impl MonitorProfile {
    pub fn new(profile: ColorProfile) -> Result<Self, String> {
        let invalid = || "NATIVE_DISPLAY_PROFILE_INVALID".to_string();
        if profile.color_space != moxcms::DataColorSpace::Rgb
            || profile.lut_a_to_b_perceptual.is_some()
            || profile.lut_a_to_b_colorimetric.is_some()
            || profile.lut_a_to_b_saturation.is_some()
            || profile.lut_b_to_a_perceptual.is_some()
            || profile.lut_b_to_a_colorimetric.is_some()
            || profile.lut_b_to_a_saturation.is_some()
        {
            return Err(invalid());
        }
        let matrix = ColorSpace::Rec2020.profile().transform_matrix(&profile).v;
        if matrix.iter().flatten().any(|v| !v.is_finite())
            || glam::DMat3::from_cols_array(&std::array::from_fn(|i| matrix[i % 3][i / 3]))
                .determinant()
                .abs()
                < 1e-10
        {
            return Err(invalid());
        }
        let evaluators = [&profile.red_trc, &profile.green_trc, &profile.blue_trc].map(|curve| {
            curve
                .as_ref()
                .ok_or_else(invalid)?
                .make_gamma_evaluator()
                .map_err(|_| invalid())
        });
        let [r, g, b] = evaluators;
        let evaluators = [r?, g?, b?];
        let mut curves: Vec<[f32; 4]> = Vec::with_capacity(CURVE_SAMPLES);
        for i in 0..CURVE_SAMPLES {
            // More samples near black, where encoding curves change fastest.
            let x = (i as f32 / (CURVE_SAMPLES - 1) as f32).powi(2);
            let rgb = evaluators.each_ref().map(|e| e.evaluate_value(x));
            if rgb.iter().enumerate().any(|(channel, value)| {
                !value.is_finite()
                    || !(0.0..=1.001).contains(value)
                    || curves.last().is_some_and(|p| *value + 1e-5 < p[channel])
            }) {
                return Err(invalid());
            }
            curves.push([rgb[0], rgb[1], rgb[2], 1.0]);
        }
        Ok(Self { profile, curves })
    }

    pub fn encode(&self, value: f32, channel: usize) -> f32 {
        let x = value.clamp(0.0, 1.0).sqrt() * (CURVE_SAMPLES - 1) as f32;
        let lo = x.floor() as usize;
        let hi = (lo + 1).min(CURVE_SAMPLES - 1);
        self.curves[lo][channel] * (1.0 - x.fract()) + self.curves[hi][channel] * x.fract()
    }
}

fn matrix(source: ColorSpace, target: &ColorProfile) -> [[f32; 4]; 3] {
    let m = source.profile().transform_matrix(target).v;
    std::array::from_fn(|c| [m[0][c] as f32, m[1][c] as f32, m[2][c] as f32, 0.0])
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DisplayColor {
    pub image_matrix: [[f32; 4]; 3],
    pub background_matrix: [[f32; 4]; 3],
    pub input_transfer: u32,
    // 0: sRGB/P3 encoding, 1: linear, 2: signed extended sRGB, 3: PQ.
    pub output_encoding: u32,
    pub _pad: [u32; 2],
}

impl DisplayColor {
    pub fn new(
        image_space: ColorSpace,
        surface: wgpu::SurfaceColorSpace,
        format: wgpu::TextureFormat,
    ) -> Self {
        let target = match surface {
            wgpu::SurfaceColorSpace::DisplayP3 => ColorSpace::DisplayP3,
            wgpu::SurfaceColorSpace::Bt2100Pq => ColorSpace::Rec2020,
            _ => ColorSpace::Srgb,
        };
        Self {
            image_matrix: matrix(image_space, &target.profile()),
            background_matrix: matrix(ColorSpace::Srgb, &target.profile()),
            input_transfer: image_space as u32,
            output_encoding: match surface {
                wgpu::SurfaceColorSpace::ExtendedSrgbLinear => 1,
                wgpu::SurfaceColorSpace::ExtendedSrgb => 2,
                wgpu::SurfaceColorSpace::Bt2100Pq => 3,
                _ => u32::from(format.is_srgb()),
            },
            _pad: [0; 2],
        }
    }

    pub fn for_monitor(image_space: ColorSpace, profile: &ColorProfile) -> Self {
        Self {
            image_matrix: matrix(image_space, profile),
            background_matrix: matrix(ColorSpace::Srgb, profile),
            input_transfer: image_space as u32,
            output_encoding: 4,
            _pad: [0; 2],
        }
    }

    pub(crate) fn encode_component(&self, v: f32) -> f32 {
        match self.output_encoding {
            1 => v,
            3 => {
                // Match scRGB reference white: 1.0 = 80 cd/m²; PQ uses 10000.
                let y = (v.max(0.0) * (80.0 / 10000.0)).powf(2610.0 / 16384.0);
                ((3424.0 / 4096.0 + (2413.0 / 128.0) * y) / (1.0 + (2392.0 / 128.0) * y))
                    .powf(2523.0 / 32.0)
            }
            2 if v.abs() > 0.0031308 => v.signum() * (1.055 * v.abs().powf(1.0 / 2.4) - 0.055),
            _ if v <= 0.0031308 => 12.92 * v,
            _ => 1.055 * v.powf(1.0 / 2.4) - 0.055,
        }
    }

    pub fn clear_color(&self, rgba: [f32; 4], profile: Option<&MonitorProfile>) -> wgpu::Color {
        let linear: [f32; 3] = std::array::from_fn(|i| {
            let v = rgba[i];
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        let rgb: [f64; 3] = std::array::from_fn(|row| {
            let v: f32 = (0..3)
                .map(|col| self.background_matrix[col][row] * linear[col])
                .sum();
            let v = profile.map_or_else(|| self.encode_component(v), |p| p.encode(v, row));
            (v * rgba[3]) as f64
        });
        wgpu::Color {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
            a: rgba[3] as f64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::{SurfaceColorSpace as S, SurfaceColorSpaces as C, TextureFormat as F};

    fn caps(pairs: &[(F, C)]) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats: pairs.iter().map(|p| p.0).collect(),
            format_capabilities: pairs
                .iter()
                .map(|p| wgpu::SurfaceFormatCapabilities {
                    format: p.0,
                    color_spaces: p.1,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn surface_selection_uses_advertised_transports_and_prefers_ten_bit_sdr() {
        let sdr = caps(&[(F::Bgra8Unorm, C::SRGB), (F::Rgb10a2Unorm, C::SRGB)]);
        assert_eq!(surface_format(&sdr), Some((F::Rgb10a2Unorm, S::Srgb)));
        let wide = caps(&[
            (F::Rgba16Float, C::EXTENDED_SRGB_LINEAR),
            (F::Rgb10a2Unorm, C::SRGB),
        ]);
        assert_eq!(
            surface_format(&wide),
            Some((F::Rgba16Float, S::ExtendedSrgbLinear))
        );
        assert_eq!(sdr_surface_format(&wide), Some((F::Rgb10a2Unorm, S::Srgb)));
        assert_eq!(
            surface_format(&caps(&[(F::Rgba16Float, C::DISPLAY_P3)])),
            Some((F::Rgba16Float, S::DisplayP3))
        );
        assert_eq!(
            surface_format(&caps(&[(F::Rgb10a2Unorm, C::BT2100_PQ)])),
            Some((F::Rgb10a2Unorm, S::Bt2100Pq))
        );
        assert_eq!(
            surface_format(&caps(&[(F::Bgra8Unorm, C::BT2100_PQ)])),
            None
        );
        assert_eq!(surface_format(&caps(&[])), None);
    }

    #[test]
    fn monitor_curves_round_trip_and_use_each_channels_actual_trc() {
        for space in [
            ColorSpace::Srgb,
            ColorSpace::AdobeRgb,
            ColorSpace::DisplayP3,
            ColorSpace::Rec2020,
        ] {
            let mut profile = space.profile();
            profile.green_trc = Some(moxcms::ToneReprCurve::Lut(vec![512])); // gamma 2
            let monitor = MonitorProfile::new(profile).unwrap();
            for channel in 0..3 {
                let curve = [
                    &monitor.profile.red_trc,
                    &monitor.profile.green_trc,
                    &monitor.profile.blue_trc,
                ][channel]
                    .as_ref()
                    .unwrap()
                    .make_linear_evaluator()
                    .unwrap();
                for encoded in [0.0, 0.001, 0.02, 0.1, 0.5, 0.9, 1.0] {
                    assert!(
                        (monitor.encode(curve.evaluate_value(encoded), channel) - encoded).abs()
                            < 0.0003
                    );
                }
            }
            assert!(
                MonitorProfile::new(ColorProfile::new_from_slice(space.icc()).unwrap()).is_ok()
            );
        }
        let mut invalid = ColorProfile::default();
        invalid.color_space = moxcms::DataColorSpace::Cmyk;
        assert!(MonitorProfile::new(invalid).is_err());
    }

    #[test]
    fn sdr_monitor_matrix_maps_wide_colors_without_srgb_clipping() {
        let monitor = MonitorProfile::new(ColorSpace::AdobeRgb.profile()).unwrap();
        let color = DisplayColor::for_monitor(ColorSpace::Rec2020, &monitor.profile);
        let source = ColorSpace::AdobeRgb
            .profile()
            .transform_matrix(&ColorSpace::Rec2020.profile())
            .v;
        // Adobe green is outside sRGB but within this display. Matrix multiplication
        // must recover green rather than clamp it to sRGB before the screen transform.
        let rgb: [f32; 3] = std::array::from_fn(|r| {
            (0..3)
                .map(|c| color.image_matrix[c][r] * source[c][1] as f32)
                .sum()
        });
        assert!(rgb[0].abs() < 0.0001 && (rgb[1] - 1.0).abs() < 0.0001 && rgb[2].abs() < 0.0001);
        let clear = color.clear_color([0.5, 0.5, 0.5, 1.0], Some(&monitor));
        assert!((clear.r - clear.g).abs() < 0.001 && (clear.g - clear.b).abs() < 0.001);
    }

    #[test]
    fn managed_transport_retains_extended_values_and_pq_reference_white() {
        let linear = DisplayColor::new(ColorSpace::Rec2020, S::ExtendedSrgbLinear, F::Rgba16Float);
        assert!(linear.image_matrix[1][0] < 0.0);
        assert_eq!(linear.encode_component(-0.5), -0.5);
        let extended = DisplayColor::new(ColorSpace::Rec2020, S::ExtendedSrgb, F::Rgba16Float);
        assert!((extended.encode_component(-0.5) + extended.encode_component(0.5)).abs() < 0.00001);
        let pq = DisplayColor::new(ColorSpace::Rec2020, S::Bt2100Pq, F::Rgb10a2Unorm);
        assert!((pq.encode_component(1.0) - 0.4858568).abs() < 0.00002);
    }
}
