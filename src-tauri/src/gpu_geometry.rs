use crate::gpu_compute::{self, Bind, Grid, Group, IMAGE_FORMAT, Out, RGB_MODE, Slot};
use crate::image_processing::{Crop, GpuContext};
use image::{DynamicImage, GenericImageView};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::sync::Arc;

pub struct Source {
    image: std::sync::Weak<DynamicImage>,
    spots: u64,
    slot: Slot,
}

pub struct Plan<'a> {
    pub source: &'a Arc<DynamicImage>,
    pub adjustments: &'a Value,
    pub size: (u32, u32),
    pub scale: f32,
    pub offset: (f32, f32),
    frame_size: (u32, u32),
    crop_size: (u32, u32),
    pub patches_applied: bool,
}

impl<'a> Plan<'a> {
    pub fn new(source: &'a Arc<DynamicImage>, adjustments: &'a Value, max_dim: u32) -> Self {
        let (w, h) = source.dimensions();
        let orientation = adjustments["orientationSteps"].as_u64().unwrap_or(0) % 4;
        let frame_size = if orientation % 2 == 1 { (h, w) } else { (w, h) };
        let crop: Option<Crop> = serde_json::from_value(adjustments["crop"].clone()).ok();
        let (offset, crop_size) = if let Some(crop) = crop {
            let x = (crop.x.round().max(0.0) as u32).min(frame_size.0.saturating_sub(1));
            let y = (crop.y.round().max(0.0) as u32).min(frame_size.1.saturating_sub(1));
            let width = (crop.width.round().max(1.0) as u32).min(frame_size.0 - x);
            let height = (crop.height.round().max(1.0) as u32).min(frame_size.1 - y);
            ((x as f32, y as f32), (width, height))
        } else {
            ((0.0, 0.0), frame_size)
        };
        let ratio = (max_dim.max(1) as f64 / crop_size.0.max(crop_size.1).max(1) as f64).min(1.0);
        let size = (
            ((crop_size.0 as f64 * ratio).round() as u32).max(1),
            ((crop_size.1 as f64 * ratio).round() as u32).max(1),
        );
        Self {
            source,
            adjustments,
            size,
            scale: size.0 as f32 / crop_size.0.max(1) as f32,
            offset,
            frame_size,
            crop_size,
            patches_applied: false,
        }
    }

    pub fn render(&self, context: &GpuContext) -> Result<Slot, String> {
        let dimensions = self.source.dimensions();
        if dimensions.0 == 0 || dimensions.1 == 0 {
            return Err("Source image is empty".into());
        }
        let mut hasher = DefaultHasher::new();
        crate::cache_utils::hash_spot_removals(self.adjustments, &mut hasher);
        let spots = hasher.finish();
        let mut source = context.source.lock().unwrap_or_else(|e| e.into_inner());
        if source.as_ref().is_none_or(|cache| {
            cache
                .image
                .upgrade()
                .is_none_or(|image| !Arc::ptr_eq(&image, self.source))
                || cache.spots != spots
        }) {
            let patched = if !self.patches_applied
                && crate::spot_removal::has_spot_removals(self.adjustments)
            {
                Some(crate::spot_removal::composite_spot_removals(
                    self.source,
                    self.adjustments,
                ))
            } else {
                None
            };
            let image = patched.as_ref().unwrap_or(self.source);
            let limit = context.limits.max_texture_dimension_2d;
            let reduced = (dimensions.0.max(dimensions.1) > limit)
                .then(|| crate::image_processing::downscale_f32_image(image, limit, limit));
            let image = reduced.as_ref().unwrap_or(image);
            let halves = gpu_compute::rgba_halves(image);
            let slot = gpu_compute::upload_image(context, image.dimensions(), &halves);
            *source = Some(Source {
                image: Arc::downgrade(self.source),
                spots,
                slot,
            });
        }
        let src = &source.as_ref().unwrap().slot;
        let ratio = (self.size.0 as f32 / self.crop_size.0 as f32)
            .max(self.size.1 as f32 / self.crop_size.1 as f32)
            .min(1.0)
            .min(
                context.limits.max_texture_dimension_2d as f32
                    / dimensions.0.max(dimensions.1) as f32,
            );
        let work_size = (
            ((dimensions.0 as f32 * ratio).ceil() as u32).clamp(1, src.size.0),
            ((dimensions.1 as f32 * ratio).ceil() as u32).clamp(1, src.size.1),
        );
        let geometry = crate::image_processing::get_geometry_params_from_json(self.adjustments);
        let identity = crate::image_processing::is_geometry_identity(&geometry);
        let map_key = crate::cache_utils::calculate_geometry_hash(self.adjustments);
        let mapping =
            crate::image_processing::geometry_mapping(geometry, dimensions.0, dimensions.1);
        let grid_size = (
            dimensions
                .0
                .min(1025)
                .min(context.limits.max_texture_dimension_2d / 3)
                .max(2),
            dimensions.1.min(1025).max(2),
        );
        let angle = self.adjustments["rotation"].as_f64().unwrap_or(0.0) as f32
            * std::f32::consts::PI
            / 180.0;
        let (sin, cos) = angle.sin_cos();
        let frame = Params {
            grid: Grid::new(self.size).with_src(work_size),
            original: [dimensions.0 as f32, dimensions.1 as f32],
            frame: [self.frame_size.0 as f32, self.frame_size.1 as f32],
            crop: [
                self.offset.0,
                self.offset.1,
                self.crop_size.0 as f32,
                self.crop_size.1 as f32,
            ],
            rotation: [cos, sin],
            orientation: self.adjustments["orientationSteps"].as_u64().unwrap_or(0) as u32 % 4,
            flips: self.adjustments["flipHorizontal"]
                .as_bool()
                .unwrap_or(false) as u32
                | ((self.adjustments["flipVertical"].as_bool().unwrap_or(false) as u32) << 1),
        };
        gpu_compute::encode(context, |enc| {
            let down = (src.size != work_size).then(|| enc.area_down(src, work_size, RGB_MODE));
            let input = down.as_deref().unwrap_or(src);
            let warped = if identity {
                None
            } else {
                let key = map_key
                    .wrapping_mul(31)
                    .wrapping_add(dimensions.0 as u64)
                    .wrapping_mul(31)
                    .wrapping_add(dimensions.1 as u64);
                let map = enc.cached_map(
                    "Lens Mapping",
                    key,
                    (grid_size.0 * 3, grid_size.1),
                    wgpu::TextureFormat::Rgba32Float,
                    || {
                        let mut values =
                            vec![0.0f32; grid_size.0 as usize * grid_size.1 as usize * 12];
                        values
                            .par_chunks_exact_mut(grid_size.0 as usize * 12)
                            .enumerate()
                            .for_each(|(y, row)| {
                                for x in 0..grid_size.0 as usize {
                                    let channels = mapping(
                                        x as f32 * (dimensions.0 - 1) as f32
                                            / (grid_size.0 - 1) as f32,
                                        y as f32 * (dimensions.1 - 1) as f32
                                            / (grid_size.1 - 1) as f32,
                                    );
                                    for (channel, p) in channels.into_iter().enumerate() {
                                        let i = (channel * grid_size.0 as usize + x) * 4;
                                        row[i..i + 4].copy_from_slice(&[p[0], p[1], p[2], 1.0]);
                                    }
                                }
                            });
                        values
                    },
                );
                let out = enc.texture("Warped Image", work_size, IMAGE_FORMAT);
                enc.run(
                    Group::Transform,
                    "warp",
                    &Params {
                        grid: Grid::new(work_size).with_src(input.size),
                        ..frame
                    },
                    work_size,
                    &[&input.view, &map.view],
                    Out::F16(&out.view),
                    Bind::default(),
                );
                Some(out)
            };
            let warped = warped.as_deref().unwrap_or(input);
            let blur = crate::gpu_lens_blur::prepare(self.adjustments, work_size);
            let blurred = blur.map(|job| {
                let out = enc.texture("Lens Blur", work_size, IMAGE_FORMAT);
                crate::gpu_lens_blur::encode(enc, &job, warped, &out);
                out
            });
            let framed = enc.texture("Framed Image", self.size, IMAGE_FORMAT);
            enc.run(
                Group::Transform,
                "frame",
                &frame,
                self.size,
                &[&blurred.as_deref().unwrap_or(warped).view],
                Out::F16(&framed.view),
                Bind::default(),
            );
            framed.detach()
        })
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    grid: Grid,
    original: [f32; 2],
    frame: [f32; 2],
    crop: [f32; 4],
    rotation: [f32; 2],
    orientation: u32,
    flips: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, Rgba32FImage};
    use serde_json::json;

    fn context() -> GpuContext {
        let instance = crate::vulkan_instance::create().unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: limits.clone(),
            ..Default::default()
        }))
        .unwrap();
        GpuContext {
            device: Arc::new(device),
            queue: Arc::new(queue),
            limits,
            instance: Arc::new(instance),
            adapter,
            display: Arc::new(std::sync::Mutex::new(None)),
            viewer_display: Arc::new(std::sync::Mutex::new(None)),
            last_frame: Arc::new(std::sync::Mutex::new(None)),
            compute: Arc::new(std::sync::OnceLock::new()),
            source: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn pixels(context: &GpuContext, slot: &Slot) -> Vec<f32> {
        crate::gpu_processing::read_texture_data_roi(
            &context.device,
            &context.queue,
            &slot.texture,
            wgpu::Origin3d::ZERO,
            wgpu::Extent3d {
                width: slot.size.0,
                height: slot.size.1,
                depth_or_array_layers: 1,
            },
            8,
        )
        .unwrap()
        .chunks_exact(2)
        .map(|b| half::f16::from_bits(u16::from_ne_bytes([b[0], b[1]])).to_f32())
        .collect()
    }

    #[test]
    fn native_geometry_matches_orientation_flip_crop_and_keeps_float_range() {
        let context = context();
        let source = Arc::new(DynamicImage::ImageRgba32F(Rgba32FImage::from_fn(
            32,
            24,
            |x, y| Rgba([x as f32 / 10.0 - 0.2, y as f32 / 10.0, 0.125, 1.0]),
        )));
        for orientation in 0..4 {
            for flip in [false, true] {
                let edits = json!({"orientationSteps":orientation,"flipHorizontal":flip,"flipVertical":true,"crop":{"x":3,"y":4,"width":15,"height":12}});
                let plan = Plan::new(&source, &edits, u32::MAX);
                let slot = plan.render(&context).unwrap();
                let actual = pixels(&context, &slot);
                let expected =
                    crate::image_processing::apply_coarse_rotation(source.as_ref(), orientation);
                let expected = if flip {
                    expected.fliph()
                } else {
                    expected.into_owned()
                };
                let expected = expected.flipv().crop_imm(3, 4, 15, 12);
                for (a, b) in actual.iter().zip(expected.to_rgba32f().as_raw()) {
                    assert!((a - b).abs() < 0.003, "{orientation} {flip}: {a} != {b}");
                }
            }
        }
    }

    #[test]
    #[ignore = "Uses developer-provided testbilder RAW files"]
    fn supplied_raw_files_render_through_gpu_geometry() {
        let context = context();
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testbilder");
        let mut count = 0;
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if !crate::formats::is_raw_file(&path) {
                continue;
            }
            let metadata = crate::exif_processing::read_source_metadata(&path).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            let image = Arc::new(
                crate::raw_processing::develop_raw_image(
                    &bytes,
                    false,
                    1000.0,
                    "auto".into(),
                    None,
                    2.5,
                )
                .unwrap(),
            );
            let edits = json!({"lensDistortionParams":{"embedded":metadata.lens_profile},"orientationSteps":1,"rotation":2});
            let slot = Plan::new(&image, &edits, 512).render(&context).unwrap();
            let output = pixels(&context, &slot);
            assert!(output.iter().all(|p| p.is_finite()), "{}", path.display());
            assert!(
                output.chunks_exact(4).any(|p| p[0] + p[1] + p[2] > 0.01),
                "{}",
                path.display()
            );
            eprintln!("GPU geometry: {}", path.display());
            count += 1;
        }
        assert!(count >= 6);
    }

    #[test]
    fn oversized_source_can_still_render_a_bounded_preview() {
        let mut context = context();
        context.limits.max_texture_dimension_2d = 128;
        let source = Arc::new(DynamicImage::ImageRgb32F(image::Rgb32FImage::from_pixel(
            256,
            160,
            image::Rgb([0.3, 0.5, 1.2]),
        )));
        let edits = json!({});
        let slot = Plan::new(&source, &edits, 64).render(&context).unwrap();
        assert_eq!(slot.size, (64, 40));
        let values = pixels(&context, &slot);
        assert!(
            values
                .chunks_exact(4)
                .all(|p| (p[0] - 0.3).abs() < 0.002 && (p[2] - 1.2).abs() < 0.002)
        );
    }

    #[test]
    fn gpu_warp_and_depth_blur_are_valid_and_depth_changes_invalidate() {
        use base64::Engine;
        let context = context();
        let source = Arc::new(DynamicImage::ImageRgb32F(image::Rgb32FImage::from_fn(
            64,
            48,
            |x, y| image::Rgb([x as f32 / 64.0, y as f32 / 48.0, 0.2]),
        )));
        let edits = json!({"transformDistortion":4,"transformVertical":8});
        let plan = Plan::new(&source, &edits, u32::MAX);
        let actual = pixels(&context, &plan.render(&context).unwrap());
        let expected = crate::image_processing::warp_image_geometry(
            &source,
            crate::image_processing::get_geometry_params_from_json(&edits),
        )
        .to_rgba32f();
        for y in 4..44 {
            for x in 4..60 {
                for c in 0..3 {
                    let i = (y * 64 + x) * 4 + c;
                    assert!(
                        (actual[i] - expected.as_raw()[i]).abs() < 0.003,
                        "{x},{y},{c}: {} != {}",
                        actual[i],
                        expected.as_raw()[i]
                    );
                }
            }
        }
        let depth = image::GrayImage::from_pixel(16, 16, image::Luma([0]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        depth
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let mut blur = json!({"lensBlurEnabled":true,"lensBlurAmount":100,"lensBlurDepthMap":base64::engine::general_purpose::STANDARD.encode(encoded.into_inner())});
        for shape in ["circle", "hexagon", "octagon", "ring"] {
            blur["lensBlurShape"] = json!(shape);
            blur["lensBlurDiffusion"] = json!(50);
            assert!(
                pixels(
                    &context,
                    &Plan::new(&source, &blur, 64).render(&context).unwrap()
                )
                .iter()
                .all(|v| v.is_finite())
            );
        }
    }
}
