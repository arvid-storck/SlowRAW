use image::{DynamicImage, Rgba32FImage};
use serde_json::Value;
use std::borrow::Cow;

fn number(value: Option<&Value>, default: f32) -> f32 {
    value
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

fn point(value: &Value) -> Option<(f32, f32)> {
    let x = number(value.get("x"), f32::NAN);
    let y = number(value.get("y"), f32::NAN);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

fn sample(image: &Rgba32FImage, x: f32, y: f32) -> Option<[f32; 3]> {
    if x < 0.0 || y < 0.0 || x >= image.width() as f32 || y >= image.height() as f32 {
        return None;
    }
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = (
        (x0 + 1).min(image.width() - 1),
        (y0 + 1).min(image.height() - 1),
    );
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (a, b, c, d) = (
        image.get_pixel(x0, y0),
        image.get_pixel(x1, y0),
        image.get_pixel(x0, y1),
        image.get_pixel(x1, y1),
    );
    Some(
        [0, 1, 2]
            .map(|i| (a[i] + (b[i] - a[i]) * fx) * (1.0 - fy) + (c[i] + (d[i] - c[i]) * fx) * fy),
    )
}

fn stamp(
    mask: &mut [f32],
    bounds: (u32, u32, u32, u32),
    x: f32,
    y: f32,
    radius: f32,
    feather: f32,
) {
    let (left, top, width, height) = bounds;
    if x + radius < left as f32
        || y + radius < top as f32
        || x - radius > (left + width - 1) as f32
        || y - radius > (top + height - 1) as f32
    {
        return;
    }
    let x0 = ((x - radius).floor() as i64).max(left as i64) as u32;
    let y0 = ((y - radius).floor() as i64).max(top as i64) as u32;
    let x1 = ((x + radius).ceil() as i64).clamp(left as i64, (left + width - 1) as i64) as u32;
    let y1 = ((y + radius).ceil() as i64).clamp(top as i64, (top + height - 1) as i64) as u32;
    let inner = radius * (1.0 - feather);
    for py in y0..=y1 {
        for px in x0..=x1 {
            let distance = ((px as f32 - x).powi(2) + (py as f32 - y).powi(2)).sqrt();
            if distance > radius {
                continue;
            }
            let alpha = if distance <= inner || feather <= 0.0 {
                1.0
            } else {
                let t = ((distance - inner) / (radius - inner)).clamp(0.0, 1.0);
                1.0 - t * t * (3.0 - 2.0 * t)
            };
            let index = ((py - top) * width + px - left) as usize;
            mask[index] = mask[index].max(alpha);
        }
    }
}

fn rasterize(lines: &[Value], bounds: (u32, u32, u32, u32), scale: f32) -> Vec<f32> {
    let mut mask = vec![0.0; bounds.2 as usize * bounds.3 as usize];
    let mut coverage = vec![0.0; mask.len()];
    for line in lines {
        coverage.fill(0.0);
        let points: Vec<_> = line["points"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(point)
            .map(|(x, y)| (x * scale, y * scale))
            .collect();
        let radius = (number(line.get("brushSize"), 20.0) * scale / 2.0).clamp(0.75, 512.0);
        let feather = number(line.get("feather"), 0.25).clamp(0.0, 1.0);
        for &(x, y) in &points {
            stamp(&mut coverage, bounds, x, y, radius, feather);
        }
        for segment in points.windows(2) {
            let (start, end) = (segment[0], segment[1]);
            let n = ((end.0 - start.0).hypot(end.1 - start.1) / (radius * 0.5).max(0.5))
                .ceil()
                .max(1.0) as u32;
            for i in 1..n {
                let t = i as f32 / n as f32;
                stamp(
                    &mut coverage,
                    bounds,
                    start.0 + (end.0 - start.0) * t,
                    start.1 + (end.1 - start.1) * t,
                    radius,
                    feather,
                );
            }
        }
        let flow = number(line.get("flow"), 1.0).clamp(0.0, 1.0);
        let erase = line["tool"].as_str() == Some("eraser");
        for (mask, coverage) in mask.iter_mut().zip(&coverage) {
            let alpha = coverage * flow;
            *mask = if erase {
                *mask * (1.0 - alpha)
            } else {
                *mask + alpha - *mask * alpha
            };
        }
    }
    mask
}

fn paint_spot(
    output: &mut Rgba32FImage,
    source: &Rgba32FImage,
    lines: &[Value],
    offset: (f32, f32),
    heal: bool,
    opacity: f32,
) {
    if output.width() == 0 || output.height() == 0 {
        return;
    }
    let mut rect = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for line in lines {
        if line["tool"].as_str() == Some("eraser") {
            continue;
        }
        let radius = (number(line.get("brushSize"), 20.0) / 2.0).clamp(0.75, 512.0);
        for (x, y) in line["points"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(point)
        {
            rect = (
                rect.0.min(x - radius),
                rect.1.min(y - radius),
                rect.2.max(x + radius),
                rect.3.max(y + radius),
            );
        }
    }
    if !rect.0.is_finite()
        || rect.0 >= output.width() as f32
        || rect.1 >= output.height() as f32
        || rect.2 < 0.0
        || rect.3 < 0.0
    {
        return;
    }
    let (left, top) = (
        rect.0.floor().max(0.0) as u32,
        rect.1.floor().max(0.0) as u32,
    );
    let (right, bottom) = (
        rect.2.ceil().min(output.width() as f32 - 1.0) as u32,
        rect.3.ceil().min(output.height() as f32 - 1.0) as u32,
    );
    let (width, height) = (right - left + 1, bottom - top + 1);
    let mask = rasterize(lines, (left, top, width, height), 1.0);
    let healed = if heal {
        let padded_left = left.saturating_sub(1);
        let padded_top = top.saturating_sub(1);
        let patch_w = (right + 2).min(output.width()) - padded_left;
        let patch_h = (bottom + 2).min(output.height()) - padded_top;
        let mut patch = image::Rgb32FImage::new(patch_w, patch_h);
        let mut patch_mask = image::GrayImage::new(patch_w, patch_h);
        for y in 0..patch_h {
            for x in 0..patch_w {
                let px = x + padded_left;
                let py = y + padded_top;
                let p = output.get_pixel(px, py);
                patch.put_pixel(x, y, image::Rgb([p[0], p[1], p[2]]));
                if px >= left
                    && px <= right
                    && py >= top
                    && py <= bottom
                    && mask[((py - top) * width + px - left) as usize] > 0.0
                {
                    patch_mask.put_pixel(x, y, image::Luma([255]));
                }
            }
        }
        let source_patch = image::Rgb32FImage::from_fn(patch_w, patch_h, |x, y| {
            image::Rgb(
                sample(
                    source,
                    (x + padded_left) as f32 + offset.0,
                    (y + padded_top) as f32 + offset.1,
                )
                .unwrap_or(patch.get_pixel(x, y).0),
            )
        });
        Some((
            crate::healing::heal_difference(&patch, &source_patch, &patch_mask),
            padded_left,
            padded_top,
        ))
    } else {
        None
    };

    for py in top..=bottom {
        for px in left..=right {
            let alpha = mask[((py - top) * width + px - left) as usize] * opacity;
            if alpha <= 0.0 {
                continue;
            }
            let Some(sampled) = sample(source, px as f32 + offset.0, py as f32 + offset.1) else {
                continue;
            };
            let target = output.get_pixel_mut(px, py);
            for channel in 0..3 {
                let replacement = healed
                    .as_ref()
                    .map_or(sampled[channel], |(patch, left, top)| {
                        patch.get_pixel(px - left, py - top)[channel]
                    });
                target[channel] = target[channel] * (1.0 - alpha) + replacement * alpha;
            }
        }
    }
}

fn display_mapping(
    parameters: &Value,
    width: u32,
    height: u32,
) -> impl Fn(f32, f32) -> (f32, f32) + Sync {
    let frame = parameters
        .get("coordinateFrame")
        .cloned()
        .unwrap_or(Value::Null);
    let orientation = frame["orientationSteps"].as_u64().unwrap_or(0) % 4;
    let (ow, oh) = if orientation % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let (sin, cos) = (number(frame.get("rotation"), 0.0) * std::f32::consts::PI / 180.0).sin_cos();
    let mapping = crate::image_processing::geometry_mapping(
        crate::image_processing::get_geometry_params_from_json(&frame),
        width,
        height,
    );
    move |x, y| {
        let (dx, dy) = (x - ow as f32 * 0.5, y - oh as f32 * 0.5);
        let mut x = cos * dx + sin * dy + ow as f32 * 0.5;
        let mut y = -sin * dx + cos * dy + oh as f32 * 0.5;
        if frame["flipHorizontal"].as_bool().unwrap_or(false) {
            x = ow as f32 - 1.0 - x;
        }
        if frame["flipVertical"].as_bool().unwrap_or(false) {
            y = oh as f32 - 1.0 - y;
        }
        let (x, y) = match orientation {
            1 => (y, height as f32 - 1.0 - x),
            2 => (width as f32 - 1.0 - x, height as f32 - 1.0 - y),
            3 => (width as f32 - 1.0 - y, x),
            _ => (x, y),
        };
        let p = mapping(x, y)[1];
        (p[0], p[1])
    }
}

fn image_parameters(parameters: &Value, width: u32, height: u32) -> Value {
    if !parameters["coordinateFrame"].is_object() {
        return parameters.clone();
    }
    let mapping = display_mapping(parameters, width, height);
    let mut projected = parameters.clone();
    if let (Some(x), Some(y)) = (
        parameters["sourceX"].as_f64(),
        parameters["sourceY"].as_f64(),
    ) {
        let (x, y) = mapping(x as f32, y as f32);
        projected["sourceX"] = serde_json::json!(x);
        projected["sourceY"] = serde_json::json!(y);
    }
    if let Some(lines) = projected["lines"].as_array_mut() {
        for line in lines {
            if let Some(anchor) = line["points"]
                .as_array()
                .and_then(|p| p.first())
                .and_then(point)
            {
                let p = mapping(anchor.0, anchor.1);
                let px = mapping(anchor.0 + 1.0, anchor.1);
                let py = mapping(anchor.0, anchor.1 + 1.0);
                let scale = (((px.0 - p.0).hypot(px.1 - p.1) + (py.0 - p.0).hypot(py.1 - p.1))
                    * 0.5)
                    .clamp(0.01, 100.0);
                line["brushSize"] = serde_json::json!(number(line.get("brushSize"), 20.0) * scale);
            }
            if let Some(points) = line["points"].as_array_mut() {
                for p in points {
                    if let Some((x, y)) = point(p) {
                        let (x, y) = mapping(x, y);
                        p["x"] = serde_json::json!(x);
                        p["y"] = serde_json::json!(y);
                    }
                }
            }
        }
    }
    projected
}

fn display_source(parameters: &Value, size: (u32, u32), target: (f32, f32)) -> Option<(f32, f32)> {
    if !parameters["coordinateFrame"].is_object() {
        return Some(target);
    }
    let mapping = display_mapping(parameters, size.0, size.1);
    let anchor = parameters["lines"].as_array()?.first()?["points"]
        .as_array()?
        .first()
        .and_then(point)?;
    let mut p = anchor;
    for _ in 0..16 {
        let q = mapping(p.0, p.1);
        let error = (target.0 - q.0, target.1 - q.1);
        if error.0.hypot(error.1) < 0.02 {
            return Some(p);
        }
        let dx = mapping(p.0 + 1.0, p.1);
        let dy = mapping(p.0, p.1 + 1.0);
        let (a, b, c, d) = (dx.0 - q.0, dy.0 - q.0, dx.1 - q.1, dy.1 - q.1);
        let determinant = a * d - b * c;
        if determinant.abs() < 1e-8 {
            return None;
        }
        p.0 += (d * error.0 - b * error.1) / determinant;
        p.1 += (a * error.1 - c * error.0) / determinant;
        if !p.0.is_finite() || !p.1.is_finite() {
            return None;
        }
    }
    None
}

pub fn composite_spot_removals(base: &DynamicImage, adjustments: &Value) -> DynamicImage {
    let Some(containers) = adjustments.get("masks").and_then(Value::as_array) else {
        return base.clone();
    };
    let mut result: Option<Rgba32FImage> = None;
    let mut source: Option<Cow<'_, Rgba32FImage>> = None;
    for container in containers {
        if container.get("visible").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let container_opacity = (number(container.get("opacity"), 100.0) / 100.0).clamp(0.0, 1.0);
        let Some(sub_masks) = container.get("subMasks").and_then(Value::as_array) else {
            continue;
        };
        for sub_mask in sub_masks {
            let kind = sub_mask.get("type").and_then(Value::as_str);
            if !matches!(kind, Some("clone" | "heal"))
                || sub_mask.get("visible").and_then(Value::as_bool) == Some(false)
            {
                continue;
            }
            let Some(parameters) = sub_mask.get("parameters") else {
                continue;
            };
            let parameters = image_parameters(parameters, base.width(), base.height());
            let (source_x, source_y) = (
                number(parameters.get("sourceX"), f32::NAN),
                number(parameters.get("sourceY"), f32::NAN),
            );
            if !source_x.is_finite() || !source_y.is_finite() {
                continue;
            }
            let Some(lines) = parameters.get("lines").and_then(Value::as_array) else {
                continue;
            };
            let Some(anchor) = lines
                .iter()
                .filter(|line| line.get("tool").and_then(Value::as_str) != Some("eraser"))
                .filter_map(|line| {
                    line.get("points")
                        .and_then(Value::as_array)?
                        .first()
                        .and_then(point)
                })
                .next()
            else {
                continue;
            };
            let source_image = source.get_or_insert_with(|| match base {
                DynamicImage::ImageRgba32F(pixels) => Cow::Borrowed(pixels),
                _ => Cow::Owned(base.to_rgba32f()),
            });
            let output = result.get_or_insert_with(|| source_image.as_ref().clone());
            let opacity = container_opacity
                * (number(sub_mask.get("opacity"), 100.0) / 100.0).clamp(0.0, 1.0);
            let offset = (source_x - anchor.0, source_y - anchor.1);
            paint_spot(
                output,
                source_image.as_ref(),
                lines,
                offset,
                kind == Some("heal"),
                opacity,
            );
        }
    }
    result
        .map(DynamicImage::ImageRgba32F)
        .unwrap_or_else(|| base.clone())
}

pub fn has_spot_removals(adjustments: &Value) -> bool {
    adjustments
        .get("masks")
        .and_then(Value::as_array)
        .is_some_and(|containers| {
            containers.iter().any(|container| {
                if container.get("visible").and_then(Value::as_bool) == Some(false) {
                    return false;
                }
                container
                    .get("subMasks")
                    .and_then(Value::as_array)
                    .is_some_and(|sub_masks| {
                        sub_masks.iter().any(|sub_mask| {
                            sub_mask.get("visible").and_then(Value::as_bool) != Some(false)
                                && matches!(
                                    sub_mask.get("type").and_then(Value::as_str),
                                    Some("clone" | "heal")
                                )
                                && sub_mask.get("parameters").is_some_and(|parameters| {
                                    number(parameters.get("sourceX"), f32::NAN).is_finite()
                                        && number(parameters.get("sourceY"), f32::NAN).is_finite()
                                        && parameters
                                            .get("lines")
                                            .and_then(Value::as_array)
                                            .is_some_and(|lines| !lines.is_empty())
                                })
                        })
                    })
            })
        })
}

type SourcePreview = (
    std::sync::Weak<DynamicImage>,
    std::sync::Arc<image::Rgb32FImage>,
);
static SOURCE_PREVIEW: std::sync::Mutex<Option<SourcePreview>> = std::sync::Mutex::new(None);

fn source_preview(base: &std::sync::Arc<DynamicImage>) -> std::sync::Arc<image::Rgb32FImage> {
    let mut cache = SOURCE_PREVIEW.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((original, preview)) = cache.as_ref()
        && original
            .upgrade()
            .is_some_and(|image| std::sync::Arc::ptr_eq(&image, base))
    {
        return preview.clone();
    }
    let preview = std::sync::Arc::new(
        crate::image_processing::downscale_f32_image(base, 1536, 1536).to_rgb32f(),
    );
    *cache = Some((std::sync::Arc::downgrade(base), preview.clone()));
    preview
}

#[tauri::command]
pub async fn find_spot_source(
    state: tauri::State<'_, crate::AppState>,
    path: String,
    parameters: Value,
    heal: bool,
    variant: usize,
) -> Result<Option<(f32, f32)>, String> {
    let base = {
        let guard = state.original_image.lock().unwrap();
        let loaded = guard.as_ref().ok_or("No image loaded")?;
        if loaded.path != path {
            return Err("Image changed".into());
        }
        loaded.image.clone()
    };
    tauri::async_runtime::spawn_blocking(move || {
        let image = source_preview(&base);
        let scale = image.width() as f32 / base.width() as f32;
        let projected = image_parameters(&parameters, base.width(), base.height());
        let lines = projected.get("lines")?.as_array()?;
        let anchor = lines
            .iter()
            .find(|line| line["tool"].as_str() != Some("eraser"))?["points"]
            .as_array()?
            .first()
            .and_then(point)?;
        let anchor = (anchor.0 * scale, anchor.1 * scale);
        let coverage = rasterize(lines, (0, 0, image.width(), image.height()), scale);
        let mask = image::GrayImage::from_raw(
            image.width(),
            image.height(),
            coverage
                .into_iter()
                .map(|v| if v > 0.0 { 255 } else { 0 })
                .collect(),
        )?;
        let mut bounds = (usize::MAX, 0, usize::MAX, 0);
        for (x, y, p) in mask.enumerate_pixels() {
            if p[0] > 0 {
                bounds = (
                    bounds.0.min(x as usize),
                    bounds.1.max(x as usize),
                    bounds.2.min(y as usize),
                    bounds.3.max(y as usize),
                );
            }
        }
        if bounds.0 == usize::MAX {
            return None;
        }
        let offset = crate::healing::find_source_offset(
            &image,
            &mask,
            bounds,
            (0, 0, image.width() as i64, image.height() as i64),
            heal,
            variant.min(1000),
        )?;
        display_source(
            &parameters,
            (base.width(), base.height()),
            (
                (anchor.0 + offset.0 as f32) / scale,
                (anchor.1 + offset.1 as f32) / scale,
            ),
        )
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use serde_json::json;

    #[test]
    fn clone_uses_selected_source_and_preserves_original() {
        let mut image = Rgba32FImage::from_pixel(12, 8, Rgba([0.1, 0.1, 0.1, 1.0]));
        image.put_pixel(2, 2, Rgba([0.8, 0.4, 0.2, 1.0]));
        let base = DynamicImage::ImageRgba32F(image);
        let edits = json!({"masks": [{"visible": true, "opacity": 100, "subMasks": [{"type": "clone", "visible": true, "opacity": 100, "parameters": {"sourceX": 2, "sourceY": 2, "lines": [{"brushSize": 2, "feather": 0, "points": [{"x": 7, "y": 2}]}]}}]}]});
        let result = composite_spot_removals(&base, &edits).to_rgba32f();
        assert_eq!(result.get_pixel(7, 2)[0], 0.8);
        assert_eq!(base.to_rgba32f().get_pixel(7, 2)[0], 0.1);
    }

    #[test]
    fn heal_replaces_blemish_using_source_texture() {
        let mut image = Rgba32FImage::from_pixel(12, 8, Rgba([0.2, 0.2, 0.2, 1.0]));
        image.put_pixel(2, 2, Rgba([0.4, 0.4, 0.4, 1.0]));
        image.put_pixel(7, 2, Rgba([0.9, 0.9, 0.9, 1.0]));
        let base = DynamicImage::ImageRgba32F(image);
        let edits = json!({"masks": [{"subMasks": [{"type": "heal", "parameters": {"sourceX": 2, "sourceY": 2, "lines": [{"brushSize": 2, "feather": 0, "points": [{"x": 7, "y": 2}]}]}}]}]});
        let result = composite_spot_removals(&base, &edits).to_rgba32f();
        assert!((result.get_pixel(7, 2)[0] - 0.4).abs() < 0.001);
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use image::Rgba;
    use serde_json::json;

    #[test]
    fn new_spots_map_rotated_coordinates_and_source_roundtrips() {
        for orientation in 0..4 {
            let parameters = json!({"coordinateFrame":{"orientationSteps":orientation,"flipHorizontal":true,"rotation":5},"lines":[{"points":[{"x":20,"y":15}]}]});
            let map = display_mapping(&parameters, 64, 48);
            let original = map(31.5, 19.25);
            let display = display_source(&parameters, (64, 48), original).unwrap();
            assert!((display.0 - 31.5).abs() < 0.03 && (display.1 - 19.25).abs() < 0.03);
        }
    }

    #[test]
    fn erasing_one_spot_preserves_underlying_spots_and_float_colors() {
        let mut pixels = Rgba32FImage::from_pixel(24, 16, Rgba([0.1, 0.1, 0.1, 1.0]));
        pixels.put_pixel(2, 4, Rgba([1.5, -0.1, 0.25, 1.0]));
        pixels.put_pixel(4, 4, Rgba([0.8, 0.4, 0.2, 1.0]));
        let base = DynamicImage::ImageRgba32F(pixels);
        let edits = json!({"masks":[{"subMasks":[
            {"type":"clone","parameters":{"sourceX":2,"sourceY":4,"lines":[{"brushSize":2,"feather":0,"points":[{"x":12,"y":8}]}]}},
            {"type":"clone","parameters":{"sourceX":4,"sourceY":4,"lines":[
                {"brushSize":2,"feather":0,"points":[{"x":12,"y":8}]},
                {"brushSize":4,"feather":0,"tool":"eraser","points":[{"x":12,"y":8}]}
            ]}}
        ]}]});
        let result = composite_spot_removals(&base, &edits).to_rgba32f();
        assert_eq!(result.get_pixel(12, 8).0, [1.5, -0.1, 0.25, 1.0]);
        assert_eq!(base.to_rgba32f().get_pixel(12, 8)[0], 0.1);
    }
}
