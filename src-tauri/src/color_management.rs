use moxcms::ColorProfile;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u32)]
pub enum ColorSpace {
    #[default]
    Srgb,
    AdobeRgb,
    DisplayP3,
    Rec2020,
    ProphotoRgb,
}

impl ColorSpace {
    pub fn profile(self) -> ColorProfile {
        let mut profile = match self {
            Self::Srgb => ColorProfile::new_srgb(),
            Self::AdobeRgb => ColorProfile::new_adobe_rgb(),
            Self::DisplayP3 => ColorProfile::new_display_p3(),
            Self::Rec2020 => ColorProfile::new_bt2020(),
            Self::ProphotoRgb => ColorProfile::new_pro_photo_rgb(),
        };
        profile.cicp = None;
        profile.media_white_point = Some(profile.white_point);
        // Matrix colorants are already D50-adapted. ICC's chad tag must contain
        // the white-point adaptation, not the Bradford cone-response matrix.
        profile.chromatic_adaptation = if self == Self::ProphotoRgb {
            None
        } else {
            use glam::{DMat3, DVec3};
            let bradford = DMat3::from_cols_array(&[
                0.8951, -0.7502, 0.0389, 0.2664, 1.7135, -0.0685, -0.1614, 0.0367, 1.0296,
            ]);
            let d65 = DVec3::new(0.3127 / 0.3290, 1.0, (1.0 - 0.3127 - 0.3290) / 0.3290);
            let d50 = DVec3::new(
                profile.white_point.x,
                profile.white_point.y,
                profile.white_point.z,
            );
            let adaptation = bradford.inverse()
                * DMat3::from_diagonal((bradford * d50) / (bradford * d65))
                * bradford;
            let cols = adaptation.to_cols_array();
            Some(moxcms::Matrix3d {
                v: std::array::from_fn(|r| std::array::from_fn(|c| cols[c * 3 + r])),
            })
        };
        profile
    }

    pub fn icc(self) -> &'static [u8] {
        if self == Self::Srgb {
            return include_bytes!("../icc/sRGB-v2-magic.icc");
        }
        static PROFILES: OnceLock<[Vec<u8>; 5]> = OnceLock::new();
        &PROFILES.get_or_init(|| {
            [
                Self::Srgb,
                Self::AdobeRgb,
                Self::DisplayP3,
                Self::Rec2020,
                Self::ProphotoRgb,
            ]
            .map(|space| {
                let mut profile = space.profile();
                // ICC is authoritative, including P3's D65 white and TRC.
                profile.cicp = None;
                profile.media_white_point = Some(profile.white_point);
                profile.encode().expect("Built-in RGB profile")
            })
        })[self as usize]
    }

    pub fn configure(self, adjustments: &mut crate::image_processing::AllAdjustments) {
        let working = ColorProfile::new_pro_photo_rgb();
        let gpu_matrix = |matrix: moxcms::Matrix3d| {
            let cols = std::array::from_fn(|i| matrix.v[i % 3][i / 3] as f32);
            crate::image_processing::mat3_to_gpu_mat3(glam::Mat3::from_cols_array(&cols))
        };
        adjustments.working_matrix =
            gpu_matrix(ColorProfile::new_srgb().transform_matrix(&working));
        adjustments.output_matrix = gpu_matrix(working.transform_matrix(&self.profile()));
        adjustments.output_transfer = self as u32;
    }
}

// Preview transport is independent of monitor and export profiles.
pub const PREVIEW_COLOR_SPACE: ColorSpace = ColorSpace::Rec2020;

pub fn convert_image(
    image: &image::DynamicImage,
    source: &ColorProfile,
    target: ColorSpace,
) -> Result<image::DynamicImage, String> {
    let target = target.profile();
    let rgba = image.to_rgba32f();
    let transform = source
        .create_transform_f32(
            moxcms::Layout::Rgba,
            &target,
            moxcms::Layout::Rgba,
            moxcms::TransformOptions {
                allow_extended_range_rgb_xyz: true,
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
    let mut output = vec![0.0; rgba.as_raw().len()];
    transform
        .transform(rgba.as_raw(), &mut output)
        .map_err(|e| e.to_string())?;
    Ok(image::DynamicImage::ImageRgba32F(
        image::ImageBuffer::from_raw(rgba.width(), rgba.height(), output)
            .ok_or("Invalid color transform dimensions")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_input_keeps_extended_srgb_and_round_trips() {
        let source = image::DynamicImage::ImageRgba32F(image::ImageBuffer::from_pixel(
            1,
            1,
            image::Rgba([0.0, 0.8, 0.0, 0.5]),
        ));
        let converted =
            convert_image(&source, &ColorSpace::DisplayP3.profile(), ColorSpace::Srgb).unwrap();
        let pixels = converted.to_rgba32f();
        assert!(
            pixels.get_pixel(0, 0)[0] < -0.01,
            "{:?}",
            pixels.get_pixel(0, 0)
        );
        let restored = convert_image(
            &converted,
            &ColorSpace::Srgb.profile(),
            ColorSpace::DisplayP3,
        )
        .unwrap()
        .to_rgba32f();
        for (actual, expected) in restored.as_raw().iter().zip(source.to_rgba32f().as_raw()) {
            assert!((actual - expected).abs() < 0.003, "{actual} != {expected}");
        }
    }
    #[test]
    fn all_profiles_round_trip_and_conversion_preserves_white() {
        for space in [
            ColorSpace::Srgb,
            ColorSpace::AdobeRgb,
            ColorSpace::DisplayP3,
            ColorSpace::Rec2020,
            ColorSpace::ProphotoRgb,
        ] {
            let parsed = ColorProfile::new_from_slice(space.icc()).unwrap();
            let matrix = ColorProfile::new_srgb().transform_matrix(&parsed).v;
            for row in matrix {
                assert!((row.iter().sum::<f64>() - 1.0).abs() < 0.002);
            }
            assert!(space.icc().len() > 128);
        }
    }
}
