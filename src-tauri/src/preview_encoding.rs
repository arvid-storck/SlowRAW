use crate::color_management::ColorSpace;
use image::{DynamicImage, ImageEncoder};
use jxl_encoder::{ImageMetadata, LosslessConfig, PixelLayout};
use std::borrow::Cow;

// Shared lossless encoding for exports and auxiliary crop/denoise images.
pub fn encode(image: &DynamicImage, space: ColorSpace, jxl: bool) -> Result<Vec<u8>, String> {
    let rgba = match image {
        DynamicImage::ImageRgba16(pixels) => Cow::Borrowed(pixels),
        _ => Cow::Owned(image.to_rgba16()),
    };
    if jxl {
        LosslessConfig::new()
            .with_effort(1)
            .encode_request(rgba.width(), rgba.height(), PixelLayout::Rgba16)
            .with_metadata(&ImageMetadata::new().with_icc_profile(space.icc()))
            .encode(bytemuck::cast_slice(rgba.as_raw()))
            .map_err(|e| e.to_string())
    } else {
        let mut data = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new_with_quality(
            &mut data,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::Adaptive,
        );
        encoder
            .set_icc_profile(space.icc().to_vec())
            .map_err(|e| e.to_string())?;
        encoder
            .write_image(
                bytemuck::cast_slice(rgba.as_raw()),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba16,
            )
            .map_err(|e| e.to_string())?;
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageDecoder;
    use std::io::Cursor;

    #[test]
    fn sixteen_bit_lossless_preview_and_profile_round_trip() {
        let source = DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
            3,
            2,
            image::Rgba([1234, 32769, 65001, 23456]),
        ));
        for jxl in [false, true] {
            let bytes = encode(&source, ColorSpace::DisplayP3, jxl).unwrap();
            let (profile, decoded) = if jxl {
                let original =
                    jxl_oxide::JxlImage::read_with_defaults(Cursor::new(&bytes)).unwrap();
                assert_eq!(
                    original.original_icc().unwrap(),
                    ColorSpace::DisplayP3.icc()
                );
                let mut decoder =
                    jxl_oxide::integration::JxlDecoder::new(Cursor::new(bytes)).unwrap();
                let rendered_profile = decoder.icc_profile().unwrap().unwrap();
                let profile = ColorSpace::DisplayP3.icc().to_vec();
                let decoded = DynamicImage::from_decoder(decoder).unwrap();
                let parsed = moxcms::ColorProfile::new_from_slice(&rendered_profile).unwrap();
                let managed = crate::color_management::convert_image(
                    &decoded,
                    &parsed,
                    ColorSpace::DisplayP3,
                )
                .unwrap()
                .to_rgba32f();
                for (actual, expected) in managed.as_raw().iter().zip(source.to_rgba32f().as_raw())
                {
                    assert!(
                        (actual - expected).abs() < 0.003,
                        "JXL profile changed color: {actual} != {expected}"
                    );
                }
                (profile, decoded)
            } else {
                let mut decoder = image::codecs::png::PngDecoder::new(Cursor::new(bytes)).unwrap();
                let profile = decoder.icc_profile().unwrap().unwrap();
                (profile, DynamicImage::from_decoder(decoder).unwrap())
            };
            assert_eq!(profile, ColorSpace::DisplayP3.icc());
            assert_eq!(decoded.to_rgba16(), source.to_rgba16(), "jxl={jxl}");
        }
    }
}
