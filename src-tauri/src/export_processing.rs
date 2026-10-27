use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use image::ImageEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView, GrayImage, ImageBuffer, ImageFormat, Luma, imageops};
use jxl_encoder::{
    ImageMetadata, LosslessConfig, LossyConfig, PixelLayout,
    api::{calibrated_jxl_quality, quality_to_distance},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Emitter;
use tauri::Manager;

use crate::AppState;
use crate::color_management::ColorSpace;
use crate::exif_processing;
use crate::file_management::{
    generate_filename_from_template, parse_virtual_path, read_file_mapped,
};
use crate::formats::is_raw_file;
use crate::image_loader::load_and_composite;
use crate::image_processing::{
    AllAdjustments, Crop, GpuContext, RenderOutputPrecision, RenderRequest, downscale_f32_image,
    get_all_adjustments_from_json, get_or_init_gpu_context,
    process_and_get_dynamic_image_with_precision, resolve_tonemapper_override_from_handle,
};
use crate::mask_generation::{MaskDefinition, generate_mask_bitmap};
use crate::spot_removal::composite_spot_removals;

use crate::cache_utils::{calculate_full_job_hash, calculate_transform_hash};
use crate::{
    apply_all_transformations, generate_transformed_preview, get_cached_or_generate_mask,
    load_settings, resolve_warped_image_for_masks,
};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(try_from = "u8", into = "u8")]
pub enum TiffBitDepth {
    Eight = 8,
    #[default]
    Sixteen = 16,
}

impl TryFrom<u8> for TiffBitDepth {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            8 => Ok(Self::Eight),
            16 => Ok(Self::Sixteen),
            _ => Err(format!(
                "Invalid TIFF bit depth '{}'; expected 8 or 16.",
                value
            )),
        }
    }
}

impl From<TiffBitDepth> for u8 {
    fn from(value: TiffBitDepth) -> Self {
        value as u8
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ResizeMode {
    LongEdge,
    ShortEdge,
    Width,
    Height,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ResizeOptions {
    pub mode: ResizeMode,
    pub value: u32,
    pub dont_enlarge: bool,
}

fn default_png_bit_depth() -> u8 {
    8
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportSettings {
    #[serde(default)]
    pub encoding: Option<FormatEncoding>,
    #[serde(default)]
    pub keep_author: bool,
    pub jpeg_quality: u8,
    #[serde(default)]
    pub tiff_bit_depth: TiffBitDepth,
    #[serde(default = "default_png_bit_depth")]
    pub png_bit_depth: u8,
    #[serde(default)]
    pub color_space: ColorSpace,
    pub resize: Option<ResizeOptions>,
    pub keep_metadata: bool,
    #[serde(default)]
    pub preserve_timestamps: bool,
    pub strip_gps: bool,
    pub filename_template: Option<String>,
    pub watermark: Option<WatermarkSettings>,
    #[serde(default)]
    pub export_masks: bool,
    #[serde(default)]
    pub preserve_folders: bool,
    #[serde(default)]
    pub destination_type: Option<String>,
    #[serde(default)]
    pub subfolder: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FormatEncoding {
    pub quality: u8,
    pub lossless: bool,
    pub bit_depth: u8,
    pub effort: u8,
}

fn encode_export_image(
    image: &DynamicImage,
    format: &str,
    settings: &ExportSettings,
) -> Result<Vec<u8>, String> {
    let legacy = FormatEncoding {
        quality: settings.jpeg_quality,
        lossless: matches!(format, "png" | "tif" | "tiff")
            || (format == "jxl"
                && (settings.jpeg_quality == 100 || settings.color_space != ColorSpace::Srgb)),
        bit_depth: if format == "jxl" { 16 } else { 8 },
        effort: 7,
    };
    let options = settings.encoding.as_ref().unwrap_or(&legacy);
    if !(1..=100).contains(&options.quality)
        || options.effort > 10
        || (format != "webp" && options.effort == 0)
        || ![8, 16].contains(&options.bit_depth)
    {
        return Err("Invalid export encoding settings".into());
    }
    if matches!(format, "jpeg" | "jpg" | "avif") && options.lossless {
        return Err("This encoder does not support lossless output".into());
    }
    encode_image_with_options(
        image,
        format,
        settings.tiff_bit_depth,
        settings.png_bit_depth,
        settings.color_space,
        options,
    )
}

#[derive(Clone)]
struct ExportEditState {
    active_path: Option<String>,
    active_adjustments: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum WatermarkAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WatermarkSettings {
    pub path: String,
    pub anchor: WatermarkAnchor,
    pub scale: f32,
    pub spacing: f32,
    pub opacity: f32,
}

fn apply_watermark(
    base_image: &mut DynamicImage,
    watermark_settings: &WatermarkSettings,
    color_space: ColorSpace,
) -> Result<(), String> {
    let bytes = fs::read(&watermark_settings.path)
        .map_err(|e| format!("Failed to open watermark image: {}", e))?;
    let mut decoder = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|e| e.to_string())?;
    use image::ImageDecoder;
    let profile = decoder.icc_profile().map_err(|e| e.to_string())?;
    let source = profile
        .as_deref()
        .map(moxcms::ColorProfile::new_from_slice)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(moxcms::ColorProfile::new_srgb);
    let decoded = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    let watermark_img = crate::color_management::convert_image(&decoded, &source, color_space)?;

    let (base_w, base_h) = base_image.dimensions();
    let base_min_dim = base_w.min(base_h) as f32;

    let watermark_scale_factor =
        (base_min_dim * (watermark_settings.scale / 100.0)) / watermark_img.width().max(1) as f32;
    let new_wm_w = (watermark_img.width() as f32 * watermark_scale_factor).round() as u32;
    let new_wm_h = (watermark_img.height() as f32 * watermark_scale_factor).round() as u32;

    if new_wm_w == 0 || new_wm_h == 0 {
        return Ok(());
    }

    let scaled_watermark =
        watermark_img.resize_exact(new_wm_w, new_wm_h, image::imageops::FilterType::Lanczos3);
    let opacity_factor = (watermark_settings.opacity / 100.0).clamp(0.0, 1.0);

    let spacing_pixels = (base_min_dim * (watermark_settings.spacing / 100.0)) as i64;
    let (wm_w, wm_h) = scaled_watermark.dimensions();

    let x = match watermark_settings.anchor {
        WatermarkAnchor::TopLeft | WatermarkAnchor::CenterLeft | WatermarkAnchor::BottomLeft => {
            spacing_pixels
        }
        WatermarkAnchor::TopCenter | WatermarkAnchor::Center | WatermarkAnchor::BottomCenter => {
            (base_w as i64 - wm_w as i64) / 2
        }
        WatermarkAnchor::TopRight | WatermarkAnchor::CenterRight | WatermarkAnchor::BottomRight => {
            base_w as i64 - wm_w as i64 - spacing_pixels
        }
    };

    let y = match watermark_settings.anchor {
        WatermarkAnchor::TopLeft | WatermarkAnchor::TopCenter | WatermarkAnchor::TopRight => {
            spacing_pixels
        }
        WatermarkAnchor::CenterLeft | WatermarkAnchor::Center | WatermarkAnchor::CenterRight => {
            (base_h as i64 - wm_h as i64) / 2
        }
        WatermarkAnchor::BottomLeft
        | WatermarkAnchor::BottomCenter
        | WatermarkAnchor::BottomRight => base_h as i64 - wm_h as i64 - spacing_pixels,
    };

    if matches!(
        base_image,
        DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_)
    ) {
        let mut base_rgba = base_image.to_rgba16();
        let mut watermark_rgba = scaled_watermark.to_rgba16();
        for pixel in watermark_rgba.pixels_mut() {
            pixel[3] = (pixel[3] as f32 * opacity_factor).round() as u16;
        }
        image::imageops::overlay(&mut base_rgba, &watermark_rgba, x, y);
        *base_image = DynamicImage::ImageRgba16(base_rgba);
    } else if matches!(
        base_image,
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_)
    ) {
        let mut base_rgba = base_image.to_rgba32f();
        let mut watermark_rgba = scaled_watermark.to_rgba32f();
        for pixel in watermark_rgba.pixels_mut() {
            pixel[3] *= opacity_factor;
        }
        image::imageops::overlay(&mut base_rgba, &watermark_rgba, x, y);
        *base_image = DynamicImage::ImageRgba32F(base_rgba);
    } else {
        let mut base_rgba = base_image.to_rgba8();
        let mut watermark_rgba = scaled_watermark.to_rgba8();
        for pixel in watermark_rgba.pixels_mut() {
            pixel[3] = (pixel[3] as f32 * opacity_factor).round() as u8;
        }
        image::imageops::overlay(&mut base_rgba, &watermark_rgba, x, y);
        *base_image = DynamicImage::ImageRgba8(base_rgba);
    }

    Ok(())
}

fn calculate_resize_target(
    current_w: u32,
    current_h: u32,
    resize_opts: &ResizeOptions,
) -> (u32, u32) {
    if resize_opts.dont_enlarge {
        let exceeds = match resize_opts.mode {
            ResizeMode::LongEdge => current_w.max(current_h) > resize_opts.value,
            ResizeMode::ShortEdge => current_w.min(current_h) > resize_opts.value,
            ResizeMode::Width => current_w > resize_opts.value,
            ResizeMode::Height => current_h > resize_opts.value,
        };
        if !exceeds {
            return (current_w, current_h);
        }
    }

    let fix_width = match resize_opts.mode {
        ResizeMode::LongEdge => current_w >= current_h,
        ResizeMode::ShortEdge => current_w <= current_h,
        ResizeMode::Width => true,
        ResizeMode::Height => false,
    };

    let value = resize_opts.value;
    if fix_width {
        let h = (value as f32 * (current_h as f32 / current_w as f32)).round() as u32;
        (value, h)
    } else {
        let w = (value as f32 * (current_w as f32 / current_h as f32)).round() as u32;
        (w, value)
    }
}

fn relative_dir_is_safe(rel_dir: &Path) -> bool {
    rel_dir.components().all(|component| {
        matches!(
            component,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    })
}

#[cfg(windows)]
fn component_matches(left: std::path::Component<'_>, right: std::path::Component<'_>) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

#[cfg(not(windows))]
fn component_matches(left: std::path::Component<'_>, right: std::path::Component<'_>) -> bool {
    left == right
}

fn strip_prefix_preserving_source_case(source_path: &Path, base_path: &Path) -> Option<PathBuf> {
    let source_components: Vec<_> = source_path.components().collect();
    let base_components: Vec<_> = base_path.components().collect();

    if base_components.len() > source_components.len() {
        return None;
    }

    if !source_components
        .iter()
        .zip(base_components.iter())
        .all(|(source, base)| component_matches(*source, *base))
    {
        return None;
    }

    Some(source_components[base_components.len()..].iter().collect())
}

fn relative_export_dir_for_preserved_folders(
    source_path: &Path,
    base_origin_folders: &[String],
) -> Option<PathBuf> {
    base_origin_folders
        .iter()
        .filter_map(|base| {
            let base_path = Path::new(base);
            strip_prefix_preserving_source_case(source_path, base_path)
                .map(|rel_path| (base_path.components().count(), rel_path))
        })
        .max_by_key(|(component_count, _)| *component_count)
        .and_then(|(_, rel_path)| {
            let rel_dir = rel_path.parent().unwrap_or_else(|| Path::new(""));
            if relative_dir_is_safe(rel_dir) {
                Some(rel_dir.to_path_buf())
            } else {
                None
            }
        })
}

fn apply_export_resize_and_watermark(
    mut image: DynamicImage,
    export_settings: &ExportSettings,
) -> Result<DynamicImage, String> {
    if let Some(resize_opts) = &export_settings.resize {
        let (current_w, current_h) = image.dimensions();
        let (target_w, target_h) = calculate_resize_target(current_w, current_h, resize_opts);

        if target_w != current_w || target_h != current_h {
            image = image.resize(target_w, target_h, imageops::FilterType::Lanczos3);
        }
    }

    if let Some(watermark_settings) = &export_settings.watermark {
        apply_watermark(&mut image, watermark_settings, export_settings.color_space)?;
    }
    Ok(image)
}

fn ensure_export_not_cancelled(cancellation_token: &AtomicBool) -> Result<(), String> {
    if cancellation_token.load(Ordering::SeqCst) {
        Err("Export cancelled".to_string())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportCancellationRequest {
    Requested,
    AlreadyRequested,
    NoActiveTask,
}

struct ExportTaskGuard {
    task_token: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    cancellation_token: Arc<AtomicBool>,
    app_handle: Option<tauri::AppHandle>,
}

impl ExportTaskGuard {
    fn new(
        task_token: Arc<Mutex<Option<Arc<AtomicBool>>>>,
        cancellation_token: Arc<AtomicBool>,
    ) -> Self {
        Self {
            task_token,
            cancellation_token,
            app_handle: None,
        }
    }

    fn with_app_handle(
        task_token: Arc<Mutex<Option<Arc<AtomicBool>>>>,
        cancellation_token: Arc<AtomicBool>,
        app_handle: tauri::AppHandle,
    ) -> Self {
        let mut guard = Self::new(task_token, cancellation_token);
        guard.app_handle = Some(app_handle);
        guard
    }
}

fn register_export_task(
    task_token: &Mutex<Option<Arc<AtomicBool>>>,
) -> Result<Arc<AtomicBool>, String> {
    let mut active_token = task_token.lock().unwrap();
    if active_token.is_some() {
        return Err("An export is already in progress.".to_string());
    }

    let cancellation_token = Arc::new(AtomicBool::new(false));
    *active_token = Some(Arc::clone(&cancellation_token));
    Ok(cancellation_token)
}

fn request_export_cancellation<F>(
    task_token: &Mutex<Option<Arc<AtomicBool>>>,
    on_requested: F,
) -> ExportCancellationRequest
where
    F: FnOnce(),
{
    let active_token = task_token.lock().unwrap();
    let Some(cancellation_token) = active_token.as_ref() else {
        return ExportCancellationRequest::NoActiveTask;
    };

    if cancellation_token.swap(true, Ordering::SeqCst) {
        ExportCancellationRequest::AlreadyRequested
    } else {
        on_requested();
        ExportCancellationRequest::Requested
    }
}

fn finish_export_task<F>(
    task_token: &Mutex<Option<Arc<AtomicBool>>>,
    cancellation_token: &Arc<AtomicBool>,
    on_finish: F,
) -> bool
where
    F: FnOnce(bool),
{
    let mut active_token = task_token.lock().unwrap();
    let Some(current_token) = active_token.as_ref() else {
        return false;
    };
    if !Arc::ptr_eq(current_token, cancellation_token) {
        return false;
    }

    let cancelled = cancellation_token.load(Ordering::SeqCst);
    *active_token = None;

    on_finish(cancelled);
    true
}

impl Drop for ExportTaskGuard {
    fn drop(&mut self) {
        let app_handle = self.app_handle.clone();
        let _ = finish_export_task(
            &self.task_token,
            &self.cancellation_token,
            |cancelled| match (cancelled, app_handle) {
                (true, Some(app_handle)) => {
                    let _ = app_handle.emit("export-cancelled", ());
                }
                (false, Some(app_handle)) => {
                    let _ = app_handle.emit("export-error", "Export task terminated unexpectedly");
                }
                _ => {}
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn process_image_for_export_pipeline(
    path: &str,
    base_image: &DynamicImage,
    js_adjustments: &Value,
    context: &GpuContext,
    state: &tauri::State<AppState>,
    is_raw: bool,
    debug_tag: &str,
    app_handle: &tauri::AppHandle,
    output_precision: RenderOutputPrecision,
    color_space: ColorSpace,
) -> Result<DynamicImage, String> {
    let (transformed_image, unscaled_crop_offset) =
        apply_all_transformations(Cow::Borrowed(base_image), js_adjustments);
    let (img_w, img_h) = transformed_image.dimensions();

    let mask_definitions: Vec<MaskDefinition> = js_adjustments
        .get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_default();

    let warped_image = resolve_warped_image_for_masks(state, js_adjustments, &mask_definitions);
    let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions
        .iter()
        .filter_map(|def| {
            generate_mask_bitmap(
                def,
                img_w,
                img_h,
                1.0,
                unscaled_crop_offset,
                warped_image.as_deref(),
            )
        })
        .collect();

    let tm_override = resolve_tonemapper_override_from_handle(app_handle, is_raw);
    let mut all_adjustments = get_all_adjustments_from_json(
        js_adjustments,
        is_raw,
        crate::white_balance::as_shot_white_balance(path),
        tm_override,
    );
    all_adjustments.global.show_clipping = 0;
    color_space.configure(&mut all_adjustments);

    let unique_hash = calculate_full_job_hash(path, js_adjustments);

    process_and_get_dynamic_image_with_precision(
        context,
        state,
        transformed_image.as_ref(),
        unique_hash,
        RenderRequest {
            adjustments: all_adjustments,
            mask_bitmaps: &mask_bitmaps,
            roi: None,
        },
        debug_tag,
        output_precision,
    )
}

fn render_output_precision(
    output_format: &str,
    export_settings: &ExportSettings,
) -> RenderOutputPrecision {
    if matches!(output_format.to_lowercase().as_str(), "tif" | "tiff")
        && export_settings.tiff_bit_depth == TiffBitDepth::Sixteen
        || output_format.eq_ignore_ascii_case("png") && export_settings.png_bit_depth == 16
        || output_format.eq_ignore_ascii_case("jxl")
    {
        RenderOutputPrecision::SixteenBit
    } else {
        RenderOutputPrecision::EightBit
    }
}

fn set_timestamps_from_exif(src: &Path, dst: &Path) {
    let capture_dt = exif_processing::get_creation_date_from_path(src);

    if capture_dt.timestamp() <= 0 {
        let now = filetime::FileTime::now();
        if let Err(e) = filetime::set_file_times(dst, now, now) {
            log::warn!("Could not set timestamps on '{}': {}", dst.display(), e);
        }
        return;
    }

    let ft = filetime::FileTime::from_unix_time(
        capture_dt.timestamp(),
        capture_dt.timestamp_subsec_nanos(),
    );
    if let Err(e) = filetime::set_file_times(dst, ft, ft) {
        log::warn!("Could not set timestamps on '{}': {}", dst.display(), e);
    }
}

fn save_image_with_metadata(
    image: &DynamicImage,
    output_path: &std::path::Path,
    source_path_str: &str,
    export_settings: &ExportSettings,
) -> Result<(), String> {
    let extension = output_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let mut image_bytes = encode_export_image(image, &extension, export_settings)?;

    exif_processing::write_image_with_metadata(
        &mut image_bytes,
        source_path_str,
        &extension,
        export_settings.keep_metadata,
        export_settings.strip_gps,
        export_settings.color_space == ColorSpace::Srgb,
    )?;

    crate::export_metadata::preserve_required_metadata(
        &mut image_bytes,
        source_path_str,
        &extension,
        export_settings.keep_author
            && (!export_settings.keep_metadata
                || !matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp")),
        image.width(),
        image.height(),
    )?;

    #[cfg(target_os = "android")]
    {
        let file_name = output_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Missing Android export file name".to_string())?;
        crate::android_integration::save_image_bytes_to_android_gallery(
            file_name,
            mime_type_for_extension(&extension),
            &image_bytes,
        )?;
    }

    #[cfg(not(target_os = "android"))]
    fs::write(output_path, image_bytes)
        .map_err(|e| format!("Failed to write file to '{}': {}", output_path.display(), e))?;

    Ok(())
}

#[cfg(target_os = "android")]
pub fn mime_type_for_extension(extension: &str) -> &'static str {
    match extension {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        "jxl" => "image/jxl",
        _ => "application/octet-stream",
    }
}

#[allow(clippy::too_many_arguments)]
fn process_image_for_export(
    path: &str,
    base_image: &DynamicImage,
    js_adjustments: &Value,
    export_settings: &ExportSettings,
    context: &GpuContext,
    state: &tauri::State<AppState>,
    is_raw: bool,
    app_handle: &tauri::AppHandle,
    output_format: &str,
) -> Result<DynamicImage, String> {
    let processed_image = process_image_for_export_pipeline(
        path,
        base_image,
        js_adjustments,
        context,
        state,
        is_raw,
        "process_image_for_export",
        app_handle,
        render_output_precision(output_format, export_settings),
        export_settings.color_space,
    )?;

    apply_export_resize_and_watermark(processed_image, export_settings)
}

fn build_single_mask_adjustments(all: &AllAdjustments, mask_index: usize) -> AllAdjustments {
    let mut single = *all;
    single.mask_count = 1;
    single.mask_adjustments[0] = all.mask_adjustments[mask_index];
    for i in 1..single.mask_adjustments.len() {
        single.mask_adjustments[i] = Default::default();
    }
    single
}

fn encode_grayscale_to_png(bitmap: &GrayImage) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    bitmap
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

#[cfg(test)]
const SRGB_ICC_PROFILE: &[u8] = include_bytes!("../icc/sRGB-v2-magic.icc");

pub(crate) fn encode_image_to_bytes(
    image: &DynamicImage,
    output_format: &str,
    jpeg_quality: u8,
    tiff_bit_depth: TiffBitDepth,
    png_bit_depth: u8,
    color_space: ColorSpace,
) -> Result<Vec<u8>, String> {
    let options = FormatEncoding {
        quality: jpeg_quality,
        lossless: matches!(output_format, "png" | "tif" | "tiff")
            || (output_format == "jxl" && (jpeg_quality == 100 || color_space != ColorSpace::Srgb)),
        bit_depth: 16,
        effort: 7,
    };
    encode_image_with_options(
        image,
        output_format,
        tiff_bit_depth,
        png_bit_depth,
        color_space,
        &options,
    )
}

fn encode_image_with_options(
    image: &DynamicImage,
    output_format: &str,
    tiff_bit_depth: TiffBitDepth,
    png_bit_depth: u8,
    color_space: ColorSpace,
    options: &FormatEncoding,
) -> Result<Vec<u8>, String> {
    let jpeg_quality = options.quality;
    if color_space != ColorSpace::Srgb
        && matches!(output_format.to_lowercase().as_str(), "webp" | "avif")
    {
        return Err("Wide-gamut export requires JPEG, PNG, TIFF or lossless JPEG XL".to_string());
    }
    let profile = color_space.icc();
    let mut image_bytes = Vec::new();
    let mut cursor = Cursor::new(&mut image_bytes);

    match output_format.to_lowercase().as_str() {
        "jxl" => {
            let (width, height) = image.dimensions();
            let (pixels, layout) = if options.bit_depth == 16 {
                (
                    bytemuck::cast_slice(image.to_rgba16().as_raw()).to_vec(),
                    PixelLayout::Rgba16,
                )
            } else {
                (image.to_rgba8().into_raw(), PixelLayout::Rgba8)
            };
            let metadata = ImageMetadata::new().with_icc_profile(profile);
            return if options.lossless {
                LosslessConfig::new()
                    .with_effort(options.effort)
                    .encode_request(width, height, layout)
                    .with_metadata(&metadata)
                    .encode(&pixels)
            } else {
                // Maximum lossy quality remains lossy; lossless is a separate option.
                let distance =
                    quality_to_distance(calibrated_jxl_quality(jpeg_quality as f32)).max(0.01);
                LossyConfig::new(distance)
                    .with_effort(options.effort)
                    .encode_request(width, height, layout)
                    .with_metadata(&metadata)
                    .encode(&pixels)
            }
            .map_err(|e| e.to_string());
        }
        "webp" => {
            let rgba = image.to_rgba8();
            let encoder = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
            let mut config = webp::WebPConfig::new().map_err(|_| "Invalid WebP configuration")?;
            config.lossless = i32::from(options.lossless);
            config.exact = i32::from(options.lossless);
            config.quality = if options.lossless {
                100.0
            } else {
                jpeg_quality as f32
            };
            config.method = options.effort.min(6) as i32;
            return encoder
                .encode_advanced(&config)
                .map(|data| data.to_vec())
                .map_err(|e| format!("{e:?}"));
        }
        "jpg" | "jpeg" => {
            let rgb_image = image.to_rgb8();
            let mut encoder = JpegEncoder::new_with_quality(&mut cursor, jpeg_quality);
            encoder
                .set_icc_profile(profile.to_vec())
                .map_err(|e| e.to_string())?;
            rgb_image
                .write_with_encoder(encoder)
                .map_err(|e| e.to_string())?;
        }
        "png" => {
            let output = match (png_bit_depth, image.color().has_alpha()) {
                (8, false) => DynamicImage::ImageRgb8(image.to_rgb8()),
                (8, true) => DynamicImage::ImageRgba8(image.to_rgba8()),
                (16, false) => DynamicImage::ImageRgb16(image.to_rgb16()),
                (16, true) => DynamicImage::ImageRgba16(image.to_rgba16()),
                _ => return Err("PNG bit depth must be 8 or 16.".to_string()),
            };
            let compression = match options.effort {
                1..=3 => image::codecs::png::CompressionType::Fast,
                4..=6 => image::codecs::png::CompressionType::Default,
                _ => image::codecs::png::CompressionType::Best,
            };
            let mut encoder = image::codecs::png::PngEncoder::new_with_quality(
                &mut cursor,
                compression,
                image::codecs::png::FilterType::Adaptive,
            );
            encoder
                .set_icc_profile(profile.to_vec())
                .map_err(|e| e.to_string())?;
            output
                .write_with_encoder(encoder)
                .map_err(|e| e.to_string())?;
        }
        "tif" | "tiff" => {
            let image_to_encode = match tiff_bit_depth {
                TiffBitDepth::Eight => DynamicImage::ImageRgb8(image.to_rgb8()),
                TiffBitDepth::Sixteen => DynamicImage::ImageRgb16(image.to_rgb16()),
            };
            let mut encoder = image::codecs::tiff::TiffEncoder::new(&mut cursor);
            encoder
                .set_icc_profile(profile.to_vec())
                .map_err(|e| e.to_string())?;
            image_to_encode
                .write_with_encoder(encoder)
                .map_err(|e| e.to_string())?;
        }
        "avif" => {
            let rgb = image.to_rgba8();
            let encoder = image::codecs::avif::AvifEncoder::new_with_speed_quality(
                &mut cursor,
                11 - options.effort,
                jpeg_quality,
            );
            rgb.write_with_encoder(encoder).map_err(|e| e.to_string())?;
        }
        _ => return Err(format!("Unsupported file format: {}", output_format)),
    };
    Ok(image_bytes)
}

#[allow(clippy::too_many_arguments)]
fn export_masks_for_image(
    base_image: &DynamicImage,
    js_adjustments: &Value,
    export_settings: &ExportSettings,
    output_path_obj: &std::path::Path,
    source_path_str: &str,
    context: &Arc<GpuContext>,
    state: &tauri::State<AppState>,
    is_raw: bool,
    app_handle: &tauri::AppHandle,
    cancellation_token: &AtomicBool,
) -> Result<(), String> {
    ensure_export_not_cancelled(cancellation_token)?;
    let (transformed_image, unscaled_crop_offset) =
        apply_all_transformations(Cow::Borrowed(base_image), js_adjustments);
    ensure_export_not_cancelled(cancellation_token)?;
    let (img_w, img_h) = transformed_image.dimensions();
    let mask_definitions: Vec<MaskDefinition> = js_adjustments
        .get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_default();

    let warped_image = resolve_warped_image_for_masks(state, js_adjustments, &mask_definitions);
    let mut mask_bitmaps = Vec::with_capacity(mask_definitions.len());
    for definition in &mask_definitions {
        ensure_export_not_cancelled(cancellation_token)?;
        if let Some(bitmap) = generate_mask_bitmap(
            definition,
            img_w,
            img_h,
            1.0,
            unscaled_crop_offset,
            warped_image.as_deref(),
        ) {
            mask_bitmaps.push(bitmap);
        }
        ensure_export_not_cancelled(cancellation_token)?;
    }

    if !mask_bitmaps.is_empty() {
        let tm_override = resolve_tonemapper_override_from_handle(app_handle, is_raw);
        let mut all_adjustments = get_all_adjustments_from_json(
            js_adjustments,
            is_raw,
            crate::white_balance::as_shot_white_balance(source_path_str),
            tm_override,
        );
        export_settings.color_space.configure(&mut all_adjustments);
        let unique_hash = calculate_full_job_hash(source_path_str, js_adjustments);
        let output_dir = output_path_obj.parent().unwrap_or(output_path_obj);
        let stem = output_path_obj
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("export");
        let extension = output_path_obj
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("jpg");

        for (i, _) in mask_bitmaps.iter().enumerate() {
            ensure_export_not_cancelled(cancellation_token)?;
            let single_adjustments = build_single_mask_adjustments(&all_adjustments, i);
            let full_white_mask = ImageBuffer::from_fn(img_w, img_h, |_, _| Luma([255u8]));
            let single_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = vec![full_white_mask];

            let processed = process_and_get_dynamic_image_with_precision(
                context,
                state,
                transformed_image.as_ref(),
                unique_hash,
                RenderRequest {
                    adjustments: single_adjustments,
                    mask_bitmaps: &single_bitmaps,
                    roi: None,
                },
                "export_mask_image",
                render_output_precision(extension, export_settings),
            )?;
            ensure_export_not_cancelled(cancellation_token)?;

            let with_options = apply_export_resize_and_watermark(processed, export_settings)?;
            let (out_w, out_h) = with_options.dimensions();

            let alpha_resized = imageops::resize(
                &mask_bitmaps[i],
                out_w,
                out_h,
                imageops::FilterType::Lanczos3,
            );
            ensure_export_not_cancelled(cancellation_token)?;

            let mask_image_path =
                output_dir.join(format!("{}_mask_{}_image.{}", stem, i, extension));
            let mask_alpha_path = output_dir.join(format!("{}_mask_{}_alpha.png", stem, i));

            save_image_with_metadata(
                &with_options,
                &mask_image_path,
                source_path_str,
                export_settings,
            )?;
            ensure_export_not_cancelled(cancellation_token)?;

            if export_settings.preserve_timestamps {
                set_timestamps_from_exif(Path::new(source_path_str), &mask_image_path);
            }

            let alpha_bytes = encode_grayscale_to_png(&alpha_resized)?;
            ensure_export_not_cancelled(cancellation_token)?;
            #[cfg(target_os = "android")]
            {
                let file_name = mask_alpha_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| "Missing Android mask export file name".to_string())?;
                crate::android_integration::save_image_bytes_to_android_gallery(
                    file_name,
                    "image/png",
                    &alpha_bytes,
                )?;
            }

            #[cfg(not(target_os = "android"))]
            fs::write(&mask_alpha_path, alpha_bytes).map_err(|e| e.to_string())?;
            ensure_export_not_cancelled(cancellation_token)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn export_images_impl(
    paths: Vec<String>,
    output_folder_or_file: String,
    is_explicit_file_path: bool,
    base_origin_folders: Vec<String>,
    export_settings: ExportSettings,
    output_format: String,
    adjustments_mode: ExportEditState,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let cancellation_token = register_export_task(&state.export_task_token)?;
    let task_guard = ExportTaskGuard::with_app_handle(
        Arc::clone(&state.export_task_token),
        Arc::clone(&cancellation_token),
        app_handle.clone(),
    );
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    if cancellation_token.load(Ordering::SeqCst) {
        return Ok(());
    }

    let context = match get_or_init_gpu_context(&state, &app_handle) {
        Ok(context) => context,
        Err(_) if cancellation_token.load(Ordering::SeqCst) => return Ok(()),
        Err(error) => return Err(error),
    };

    if cancellation_token.load(Ordering::SeqCst) {
        return Ok(());
    }

    let context = Arc::new(context);
    let progress_counter = Arc::new(AtomicUsize::new(0));

    let available_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let mut sys = sysinfo::System::new();
    sys.refresh_memory();

    let available_ram_gb = sys.available_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let ram_based_limit = (available_ram_gb / 4.0).floor() as usize;

    let num_threads = if paths.len() == 1 {
        1
    } else {
        available_cores.min(ram_based_limit).clamp(1, 4)
    };

    log::info!(
        "Batch Export: {} cores, {:.1} GB free RAM -> {} threads",
        available_cores,
        available_ram_gb,
        num_threads
    );

    let _export_task = tokio::spawn(async move {
        let _task_guard = task_guard;
        let output_folder_path = std::path::Path::new(&output_folder_or_file);
        let total_paths = paths.len();
        let settings = load_settings(app_handle.clone()).unwrap_or_default();

        let mut base_path_counts: HashMap<String, usize> = HashMap::new();
        let mut export_items = Vec::with_capacity(total_paths);

        for (i, path_str) in paths.into_iter().enumerate() {
            let (source_path, _) = parse_virtual_path(&path_str);
            let source_str = source_path.to_string_lossy().to_string();
            let count = base_path_counts.entry(source_str.clone()).or_insert(0);
            *count += 1;

            let mut explicit_vc = None;
            if let Some(idx) = path_str.rfind("vc=") {
                let id_str = path_str[idx + 3..].split('&').next().unwrap_or("");
                if let Ok(id) = id_str.parse::<u32>() {
                    explicit_vc = Some(id);
                }
            }
            if explicit_vc.is_none() {
                let lower = path_str.to_lowercase();
                if let Some(idx) = lower.rfind("_vc") {
                    let id_str: String = lower[idx + 3..]
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    if let Ok(id) = id_str.parse::<u32>() {
                        explicit_vc = Some(id);
                    }
                }
            }
            export_items.push((i, path_str, *count, explicit_vc));
        }

        let used_paths = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let semaphore = Arc::new(tokio::sync::Semaphore::new(num_threads));
        let mut join_handles = Vec::new();

        for (global_index, image_path_str, appearance_count, explicit_vc) in export_items {
            if cancellation_token.load(Ordering::SeqCst) {
                break;
            }
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            if cancellation_token.load(Ordering::SeqCst) {
                drop(permit);
                break;
            }

            let app_handle_clone = app_handle.clone();
            let context_clone = Arc::clone(&context);
            let progress_counter_clone = Arc::clone(&progress_counter);
            let output_folder_path = output_folder_path.to_path_buf();
            let base_origin_folders = base_origin_folders.clone();
            let export_settings = export_settings.clone();
            let output_format = output_format.clone();
            let settings = settings.clone();
            let cancellation_token_clone = Arc::clone(&cancellation_token);
            let adjustments_mode = adjustments_mode.clone();
            let used_paths_clone = Arc::clone(&used_paths);

            let handle = tokio::task::spawn_blocking(move || {
                ensure_export_not_cancelled(&cancellation_token_clone)?;

                let state = app_handle_clone.state::<AppState>();
                let (source_path, sidecar_path) = parse_virtual_path(&image_path_str);
                let source_path_str = source_path.to_string_lossy().to_string();

                let is_current_edit =
                    Some(&source_path_str) == adjustments_mode.active_path.as_ref();
                let js_adjustments = if is_current_edit {
                    adjustments_mode
                        .active_adjustments
                        .clone()
                        .unwrap_or_else(|| {
                            crate::exif_processing::load_sidecar(&sidecar_path).adjustments
                        })
                } else {
                    crate::exif_processing::load_sidecar(&sidecar_path).adjustments
                };
                let is_raw = is_raw_file(&source_path_str);
                let original_path = std::path::Path::new(&source_path_str);
                let file_date = exif_processing::get_creation_date_from_path(original_path);

                let filename_template = export_settings
                    .filename_template
                    .as_deref()
                    .unwrap_or("{original_filename}_edited");

                let mut new_stem = generate_filename_from_template(
                    filename_template,
                    original_path,
                    global_index + 1,
                    total_paths,
                    &file_date,
                );

                if let Some(vc_id) = explicit_vc {
                    new_stem = format!("{}_VC{:02}", new_stem, vc_id);
                } else if appearance_count > 1 {
                    new_stem = format!("{}_VC{:02}", new_stem, appearance_count - 1);
                }

                let new_filename = format!("{}.{}", new_stem, output_format);

                let mut output_path =
                    if export_settings.destination_type.as_deref() == Some("originalFolder") {
                        let mut dir = source_path
                            .parent()
                            .unwrap_or(std::path::Path::new(""))
                            .to_path_buf();
                        if let Some(sub) = &export_settings.subfolder {
                            let mut trimmed = sub.trim();

                            while trimmed.starts_with('/') || trimmed.starts_with('\\') {
                                trimmed = &trimmed[1..];
                            }

                            if !trimmed.is_empty() {
                                dir = dir.join(trimmed);
                            }
                        }

                        if let Err(e) = std::fs::create_dir_all(&dir) {
                            return Err(format!(
                                "Failed to create export subdirectory '{}': {}",
                                dir.display(),
                                e
                            ));
                        }

                        dir.join(&new_filename)
                    } else if is_explicit_file_path && total_paths == 1 {
                        output_folder_path.clone()
                    } else if export_settings.preserve_folders {
                        if let Some(rel_dir) = relative_export_dir_for_preserved_folders(
                            source_path.as_path(),
                            &base_origin_folders,
                        ) {
                            let full_dir = output_folder_path.join(rel_dir);
                            if let Err(e) = std::fs::create_dir_all(&full_dir) {
                                return Err(format!(
                                    "Failed to create export subdirectory '{}': {}",
                                    full_dir.display(),
                                    e
                                ));
                            }
                            full_dir.join(&new_filename)
                        } else {
                            output_folder_path.join(&new_filename)
                        }
                    } else {
                        output_folder_path.join(&new_filename)
                    };

                let extension = output_format.to_lowercase();

                if !(is_explicit_file_path && total_paths == 1) {
                    let mut used = used_paths_clone.lock().unwrap();
                    let parent_dir = output_path
                        .parent()
                        .unwrap_or(std::path::Path::new(""))
                        .to_path_buf();
                    let mut counter = 1;

                    while output_path.exists() || used.contains(&output_path) {
                        let incremented_filename =
                            format!("{}_{}.{}", new_stem, counter, extension);
                        output_path = parent_dir.join(&incremented_filename);
                        counter += 1;
                    }
                    used.insert(output_path.clone());
                }

                let result: Result<(), String> = (|| {
                    let base_image = if is_current_edit {
                        match crate::get_original_image(&state) {
                            Ok((orig_data_arc, _)) => {
                                composite_spot_removals(&orig_data_arc, &js_adjustments)
                            }
                            Err(_) => {
                                let bytes =
                                    fs::read(&source_path_str).map_err(|e| e.to_string())?;
                                load_and_composite(
                                    &bytes,
                                    &source_path_str,
                                    &js_adjustments,
                                    false,
                                    &settings,
                                    None,
                                )
                                .map_err(|e| format!("Failed to load fallback image: {}", e))?
                            }
                        }
                    } else {
                        match read_file_mapped(Path::new(&source_path_str)) {
                            Ok(mmap) => load_and_composite(
                                &mmap,
                                &source_path_str,
                                &js_adjustments,
                                false,
                                &settings,
                                None,
                            )
                            .map_err(|e| format!("Failed to load from mmap: {}", e))?,
                            Err(_) => {
                                let bytes =
                                    fs::read(&source_path_str).map_err(|e| e.to_string())?;
                                load_and_composite(
                                    &bytes,
                                    &source_path_str,
                                    &js_adjustments,
                                    false,
                                    &settings,
                                    None,
                                )
                                .map_err(|e| format!("Failed to load from bytes: {}", e))?
                            }
                        }
                    };
                    ensure_export_not_cancelled(&cancellation_token_clone)?;

                    let mut main_export_adjustments = js_adjustments.clone();
                    if export_settings.export_masks
                        && let Some(obj) = main_export_adjustments.as_object_mut()
                    {
                        obj.insert("masks".to_string(), serde_json::json!([]));
                    }

                    let actual_output_format = output_path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .unwrap_or(output_format.as_str());

                    let final_image = process_image_for_export(
                        &source_path_str,
                        &base_image,
                        &main_export_adjustments,
                        &export_settings,
                        &context_clone,
                        &state,
                        is_raw,
                        &app_handle_clone,
                        actual_output_format,
                    )?;
                    ensure_export_not_cancelled(&cancellation_token_clone)?;
                    save_image_with_metadata(
                        &final_image,
                        &output_path,
                        &source_path_str,
                        &export_settings,
                    )?;
                    ensure_export_not_cancelled(&cancellation_token_clone)?;

                    if export_settings.preserve_timestamps {
                        set_timestamps_from_exif(Path::new(&source_path_str), &output_path);
                    }
                    ensure_export_not_cancelled(&cancellation_token_clone)?;

                    if export_settings.export_masks {
                        export_masks_for_image(
                            &base_image,
                            &js_adjustments,
                            &export_settings,
                            &output_path,
                            &source_path_str,
                            &context_clone,
                            &state,
                            is_raw,
                            &app_handle_clone,
                            &cancellation_token_clone,
                        )?;
                    }

                    Ok(())
                })();

                if !cancellation_token_clone.load(Ordering::SeqCst) {
                    let current_progress =
                        progress_counter_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = app_handle_clone.emit(
                        "batch-export-progress",
                        serde_json::json!({
                            "current": current_progress,
                            "total": total_paths,
                            "path": &image_path_str
                        }),
                    );
                }

                drop(permit);
                if cancellation_token_clone.load(Ordering::SeqCst) {
                    Err("Export cancelled".to_string())
                } else {
                    result
                }
            });

            join_handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in join_handles {
            match handle.await {
                Ok(res) => results.push(res),
                Err(e) => results.push(Err(format!("Thread crashed: {}", e))),
            }
        }

        let errors: Vec<String> = results.into_iter().filter_map(Result::err).collect();
        let error_count = errors.len();
        let export_state = app_handle.state::<AppState>();
        let finalized = finish_export_task(
            &export_state.export_task_token,
            &cancellation_token,
            |cancelled| {
                if cancelled {
                    log::info!("Batch export cancelled and worker cleanup completed");
                    let _ = app_handle.emit("export-cancelled", ());
                    return;
                }

                for error in &errors {
                    log::error!("Export error: {}", error);
                    if total_paths == 1 {
                        let _ = app_handle.emit("export-error", error.clone());
                    }
                }

                if error_count > 0 && total_paths > 1 {
                    let _ = app_handle.emit(
                        "export-error",
                        format!("{error_count} of {total_paths} exports failed"),
                    );
                } else if error_count == 0 {
                    let _ = app_handle.emit(
                        "batch-export-progress",
                        serde_json::json!({ "current": total_paths, "total": total_paths, "path": "" }),
                    );
                    let _ = app_handle.emit("export-complete", ());
                }
            },
        );

        if !finalized {
            log::warn!("Ignoring terminal events from a stale export task");
        }
    });

    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn export_images(
    paths: Vec<String>,
    output_folder_or_file: String,
    is_explicit_file_path: bool,
    base_origin_folders: Vec<String>,
    export_settings: ExportSettings,
    output_format: String,
    current_edit_path: Option<String>,
    current_edit_adjustments: Option<Value>,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    export_images_impl(
        paths,
        output_folder_or_file,
        is_explicit_file_path,
        base_origin_folders,
        export_settings,
        output_format,
        ExportEditState {
            active_path: current_edit_path,
            active_adjustments: current_edit_adjustments,
        },
        state,
        app_handle,
    )
    .await
}

#[tauri::command]
pub fn cancel_export(
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    match request_export_cancellation(&state.export_task_token, || {
        let _ = app_handle.emit("export-cancelling", ());
    }) {
        ExportCancellationRequest::Requested => {
            log::info!("Export cancellation requested; workers will stop at the next checkpoint");
        }
        ExportCancellationRequest::AlreadyRequested => {
            log::info!("Export cancellation was already requested");
        }
        ExportCancellationRequest::NoActiveTask => {
            return Err("No export task is currently running.".to_string());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn estimate_export_sizes(
    paths: Vec<String>,
    export_settings: ExportSettings,
    output_format: String,
    current_edit_path: Option<String>,
    current_edit_adjustments: Option<Value>,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<usize, String> {
    if paths.is_empty() {
        return Ok(0);
    }

    let first_path = &paths[0];
    let (source_path, sidecar_path) = parse_virtual_path(first_path);
    let source_path_str = source_path.to_string_lossy().to_string();

    let context = get_or_init_gpu_context(&state, &app_handle)?;
    let is_current_edit = Some(&source_path_str) == current_edit_path.as_ref();
    let is_raw = is_raw_file(&source_path_str);
    let settings = load_settings(app_handle.clone()).unwrap_or_default();

    let single_image_extrapolated_size: usize = if is_current_edit
        && current_edit_adjustments.is_some()
    {
        let loaded_image = state
            .original_image
            .lock()
            .unwrap()
            .clone()
            .ok_or("No original image loaded")?;
        let adjustments_clone = current_edit_adjustments.clone().unwrap();

        let new_transform_hash = calculate_transform_hash(&adjustments_clone);
        let cached_preview_lock = state.cached_preview.lock().unwrap();
        let preview_dim = settings.editor_preview_resolution.unwrap_or(1920);

        let (preview_image, scale, unscaled_crop_offset) = if let Some(cached) =
            &*cached_preview_lock
        {
            if cached.transform_hash == new_transform_hash && cached.preview_dim == preview_dim {
                let img = Arc::clone(&cached.image);
                let s = cached.scale;
                let offset = cached.unscaled_crop_offset;
                drop(cached_preview_lock);
                let owned_img = Arc::try_unwrap(img).unwrap_or_else(|arc| (*arc).clone());
                (owned_img, s, offset)
            } else {
                drop(cached_preview_lock);
                generate_transformed_preview(
                    &state,
                    &loaded_image,
                    &adjustments_clone,
                    preview_dim,
                )?
            }
        } else {
            drop(cached_preview_lock);
            generate_transformed_preview(&state, &loaded_image, &adjustments_clone, preview_dim)?
        };

        let (img_w, img_h) = preview_image.dimensions();
        let mask_definitions: Vec<MaskDefinition> = adjustments_clone
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();

        let scaled_crop_offset = (
            unscaled_crop_offset.0 * scale,
            unscaled_crop_offset.1 * scale,
        );

        let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions
            .iter()
            .filter_map(|def| {
                get_cached_or_generate_mask(
                    &state,
                    def,
                    img_w,
                    img_h,
                    scale,
                    scaled_crop_offset,
                    &adjustments_clone,
                )
            })
            .collect();

        let tm_override = resolve_tonemapper_override_from_handle(&app_handle, is_raw);
        let mut all_adjustments = get_all_adjustments_from_json(
            &adjustments_clone,
            is_raw,
            loaded_image.as_shot_white_balance,
            tm_override,
        );
        all_adjustments.global.show_clipping = 0;
        export_settings.color_space.configure(&mut all_adjustments);
        let unique_hash =
            calculate_full_job_hash(&loaded_image.path, &adjustments_clone).wrapping_add(1);

        let processed_preview = process_and_get_dynamic_image_with_precision(
            &context,
            &state,
            &preview_image,
            unique_hash,
            RenderRequest {
                adjustments: all_adjustments,
                mask_bitmaps: &mask_bitmaps,
                roi: None,
            },
            "estimate_export_size",
            render_output_precision(&output_format, &export_settings),
        )?;

        let preview_bytes =
            encode_export_image(&processed_preview, &output_format, &export_settings)?;
        let preview_byte_size = preview_bytes.len();

        let (transformed_full_res, _) =
            apply_all_transformations(&loaded_image.image, &adjustments_clone);
        let (full_w, full_h) = transformed_full_res.dimensions();

        let (final_full_w, final_full_h) = if let Some(resize_opts) = &export_settings.resize {
            calculate_resize_target(full_w, full_h, resize_opts)
        } else {
            (full_w, full_h)
        };

        let (processed_preview_w, processed_preview_h) = processed_preview.dimensions();
        let pixel_ratio = if processed_preview_w > 0 && processed_preview_h > 0 {
            (final_full_w as f64 * final_full_h as f64)
                / (processed_preview_w as f64 * processed_preview_h as f64)
        } else {
            1.0
        };

        (preview_byte_size as f64 * pixel_ratio) as usize
    } else {
        let metadata = crate::exif_processing::load_sidecar(&sidecar_path);
        let mut js_adjustments = metadata.adjustments;

        const ESTIMATE_DIM: u32 = 1280;

        let file_slice: Vec<u8>;
        let mmap_guard;
        let file_data: &[u8] = match read_file_mapped(Path::new(&source_path_str)) {
            Ok(mmap) => {
                mmap_guard = Some(mmap);
                mmap_guard.as_ref().unwrap()
            }
            Err(_) => {
                file_slice = fs::read(&source_path_str).map_err(|io_err| io_err.to_string())?;
                &file_slice
            }
        };

        let original_image = load_and_composite(
            file_data,
            &source_path_str,
            &js_adjustments,
            true,
            &settings,
            None,
        )
        .map_err(|e| e.to_string())?;

        let raw_scale_factor = 1.0f32;

        if let Some(crop_val) = js_adjustments.get_mut("crop")
            && let Ok(c) = serde_json::from_value::<Crop>(crop_val.clone())
        {
            *crop_val = serde_json::to_value(Crop {
                x: c.x * raw_scale_factor as f64,
                y: c.y * raw_scale_factor as f64,
                width: c.width * raw_scale_factor as f64,
                height: c.height * raw_scale_factor as f64,
            })
            .unwrap_or(serde_json::Value::Null);
        }

        let (transformed_shrunk_res, unscaled_crop_offset) =
            apply_all_transformations(Cow::Borrowed(&original_image), &js_adjustments);
        let (shrunk_w, shrunk_h) = transformed_shrunk_res.dimensions();

        let preview_base = if shrunk_w > ESTIMATE_DIM || shrunk_h > ESTIMATE_DIM {
            downscale_f32_image(transformed_shrunk_res.as_ref(), ESTIMATE_DIM, ESTIMATE_DIM)
        } else {
            transformed_shrunk_res.into_owned()
        };

        let (preview_w, preview_h) = preview_base.dimensions();
        let gpu_scale = if shrunk_w > 0 {
            preview_w as f32 / shrunk_w as f32
        } else {
            1.0
        };
        let total_scale = gpu_scale * raw_scale_factor;

        let mask_definitions: Vec<MaskDefinition> = js_adjustments
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();
        let scaled_crop_offset = (
            unscaled_crop_offset.0 * gpu_scale,
            unscaled_crop_offset.1 * gpu_scale,
        );

        let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions
            .iter()
            .filter_map(|def| {
                get_cached_or_generate_mask(
                    &state,
                    def,
                    preview_w,
                    preview_h,
                    total_scale,
                    scaled_crop_offset,
                    &js_adjustments,
                )
            })
            .collect();

        let tm_override = resolve_tonemapper_override_from_handle(&app_handle, is_raw);
        let mut all_adjustments = get_all_adjustments_from_json(
            &js_adjustments,
            is_raw,
            crate::white_balance::as_shot_white_balance(&source_path_str),
            tm_override,
        );
        all_adjustments.global.show_clipping = 0;
        export_settings.color_space.configure(&mut all_adjustments);
        let unique_hash =
            calculate_full_job_hash(&source_path_str, &js_adjustments).wrapping_add(1);

        let processed_preview = process_and_get_dynamic_image_with_precision(
            &context,
            &state,
            &preview_base,
            unique_hash,
            RenderRequest {
                adjustments: all_adjustments,
                mask_bitmaps: &mask_bitmaps,
                roi: None,
            },
            "estimate_batch_export_size",
            render_output_precision(&output_format, &export_settings),
        )?;

        let preview_bytes =
            encode_export_image(&processed_preview, &output_format, &export_settings)?;
        let single_image_estimated_size = preview_bytes.len();

        let full_w = (shrunk_w as f32 / raw_scale_factor).round() as u32;
        let full_h = (shrunk_h as f32 / raw_scale_factor).round() as u32;

        let (final_full_w, final_full_h) = if let Some(resize_opts) = &export_settings.resize {
            calculate_resize_target(full_w, full_h, resize_opts)
        } else {
            (full_w, full_h)
        };

        let (processed_preview_w, processed_preview_h) = processed_preview.dimensions();
        let pixel_ratio = if processed_preview_w > 0 && processed_preview_h > 0 {
            (final_full_w as f64 * final_full_h as f64)
                / (processed_preview_w as f64 * processed_preview_h as f64)
        } else {
            1.0
        };

        (single_image_estimated_size as f64 * pixel_ratio) as usize
    };

    Ok(single_image_extrapolated_size * paths.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_jxl_modes_preserve_depth_profile_and_lossless_samples() {
        let source = DynamicImage::ImageRgba16(ImageBuffer::from_fn(32, 32, |x, y| {
            image::Rgba([
                (x * 1711) as u16,
                (y * 1513) as u16,
                ((x + y) * 901) as u16,
                65535,
            ])
        }));
        for lossless in [false, true] {
            for bit_depth in [8, 16] {
                let options = FormatEncoding {
                    quality: 100,
                    lossless,
                    bit_depth,
                    effort: 7,
                };
                let bytes = encode_image_with_options(
                    &source,
                    "jxl",
                    TiffBitDepth::Sixteen,
                    16,
                    ColorSpace::AdobeRgb,
                    &options,
                )
                .unwrap();
                let image = jxl_oxide::JxlImage::read_with_defaults(Cursor::new(&bytes)).unwrap();
                assert_eq!(image.original_icc().unwrap(), ColorSpace::AdobeRgb.icc());
                let decoder = jxl_oxide::integration::JxlDecoder::new(Cursor::new(&bytes)).unwrap();
                let decoded = DynamicImage::from_decoder(decoder).unwrap();
                assert_eq!(decoded.dimensions(), source.dimensions());
                if lossless {
                    if bit_depth == 16 {
                        assert_eq!(decoded.to_rgba16(), source.to_rgba16());
                    } else {
                        assert_eq!(decoded.to_rgba8(), source.to_rgba8());
                    }
                }
            }
        }
    }

    #[test]
    fn webp_lossless_and_avif_quality_are_explicit() {
        let source = DynamicImage::ImageRgba8(ImageBuffer::from_fn(32, 32, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 7) as u8, ((x + y) * 3) as u8, 255])
        }));
        let options = FormatEncoding {
            quality: 5,
            lossless: true,
            bit_depth: 8,
            effort: 6,
        };
        let bytes = encode_image_with_options(
            &source,
            "webp",
            TiffBitDepth::Sixteen,
            8,
            ColorSpace::Srgb,
            &options,
        )
        .unwrap();
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().to_rgba8(),
            source.to_rgba8()
        );
        let low = FormatEncoding {
            quality: 10,
            lossless: false,
            effort: 1,
            ..options.clone()
        };
        let high = FormatEncoding {
            quality: 100,
            lossless: false,
            effort: 1,
            ..options
        };
        let low_bytes = encode_image_with_options(
            &source,
            "avif",
            TiffBitDepth::Sixteen,
            8,
            ColorSpace::Srgb,
            &low,
        )
        .unwrap();
        let high_bytes = encode_image_with_options(
            &source,
            "avif",
            TiffBitDepth::Sixteen,
            8,
            ColorSpace::Srgb,
            &high,
        )
        .unwrap();
        assert_ne!(low_bytes, high_bytes);
    }

    #[test]
    fn export_preset_retains_extended_fields() {
        let mut value =
            serde_json::to_value(crate::app_settings::default_export_presets().remove(0)).unwrap();
        value["colorSpace"] = serde_json::json!("prophoto-rgb");
        value["pngBitDepth"] = serde_json::json!(16);
        value["preserveTimestamps"] = serde_json::json!(true);
        value["destinationType"] = serde_json::json!("originalFolder");
        value["subfolder"] = serde_json::json!("exports");
        value["keepAuthor"] = serde_json::json!(true);
        value["formatEncodings"] = serde_json::json!({"jxl": {"quality": 100, "lossless": false, "bitDepth": 16, "effort": 9}});
        let preset: crate::app_settings::ExportPreset =
            serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(preset).unwrap(), value);
    }

    #[test]
    fn wide_export_metadata_does_not_claim_srgb_or_replace_its_icc() {
        use image::ImageDecoder;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.jpg");
        let image = DynamicImage::new_rgb8(2, 2);
        image.save(&source).unwrap();
        let mut bytes = encode_image_to_bytes(
            &image,
            "jpeg",
            90,
            TiffBitDepth::Eight,
            8,
            ColorSpace::AdobeRgb,
        )
        .unwrap();
        exif_processing::write_image_with_metadata(
            &mut bytes,
            source.to_str().unwrap(),
            "jpeg",
            true,
            true,
            false,
        )
        .unwrap();
        let exif = exif::Reader::new()
            .read_from_container(&mut Cursor::new(&bytes))
            .unwrap();
        assert_eq!(
            exif.get_field(exif::Tag::ColorSpace, exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(65535)
        );
        let mut decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).unwrap();
        assert_eq!(
            decoder.icc_profile().unwrap().unwrap(),
            ColorSpace::AdobeRgb.icc()
        );
    }

    #[test]
    fn wide_exports_embed_the_selected_profile() {
        use image::ImageDecoder;
        let image = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(
            2,
            2,
            image::Rgb([1234, 32769, 65001]),
        ));
        for space in [
            ColorSpace::Srgb,
            ColorSpace::AdobeRgb,
            ColorSpace::DisplayP3,
            ColorSpace::Rec2020,
            ColorSpace::ProphotoRgb,
        ] {
            for format in ["jpeg", "png", "tiff", "jxl"] {
                let bytes =
                    encode_image_to_bytes(&image, format, 100, TiffBitDepth::Sixteen, 16, space)
                        .unwrap();
                let profile = match format {
                    "jpeg" => image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))
                        .unwrap()
                        .icc_profile()
                        .unwrap()
                        .unwrap(),
                    "png" => image::codecs::png::PngDecoder::new(Cursor::new(bytes))
                        .unwrap()
                        .icc_profile()
                        .unwrap()
                        .unwrap(),
                    "tiff" => image::codecs::tiff::TiffDecoder::new(Cursor::new(bytes))
                        .unwrap()
                        .icc_profile()
                        .unwrap()
                        .unwrap(),
                    _ => jxl_oxide::JxlImage::read_with_defaults(Cursor::new(bytes))
                        .unwrap()
                        .original_icc()
                        .unwrap()
                        .to_vec(),
                };
                assert_eq!(profile, space.icc(), "{format} {space:?}");
            }
        }
    }

    #[test]
    fn jpeg_and_png_embed_srgb_profile() {
        use image::ImageDecoder;
        let image = DynamicImage::new_rgb8(4, 4);
        let jpeg =
            encode_image_to_bytes(&image, "jpeg", 90, TiffBitDepth::Eight, 8, ColorSpace::Srgb)
                .unwrap();
        let png =
            encode_image_to_bytes(&image, "png", 90, TiffBitDepth::Eight, 8, ColorSpace::Srgb)
                .unwrap();
        let mut jpeg_decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(jpeg)).unwrap();
        let mut png_decoder = image::codecs::png::PngDecoder::new(Cursor::new(png)).unwrap();
        assert_eq!(
            jpeg_decoder.icc_profile().unwrap().unwrap(),
            SRGB_ICC_PROFILE
        );
        assert_eq!(
            png_decoder.icc_profile().unwrap().unwrap(),
            SRGB_ICC_PROFILE
        );
    }

    #[test]
    fn png_preserves_color_alpha_and_sixteen_bit_samples() {
        let source = DynamicImage::ImageRgba16(ImageBuffer::from_pixel(
            1,
            1,
            image::Rgba([1024, 32768, 65535, 16384]),
        ));
        for depth in [8, 16] {
            let bytes = encode_image_to_bytes(
                &source,
                "png",
                90,
                TiffBitDepth::Eight,
                depth,
                ColorSpace::Srgb,
            )
            .unwrap();
            let decoded =
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
            assert_eq!(decoded.color().bits_per_pixel(), u16::from(depth) * 4);
            if depth == 16 {
                assert_eq!(
                    decoded.to_rgba16().get_pixel(0, 0).0,
                    [1024, 32768, 65535, 16384]
                );
            }
        }
        for depth in [1, 2, 4] {
            assert!(
                encode_image_to_bytes(
                    &source,
                    "png",
                    90,
                    TiffBitDepth::Eight,
                    depth,
                    ColorSpace::Srgb
                )
                .is_err()
            );
        }
    }

    #[test]
    fn tiff_bit_depth_preserves_sixteen_bit_samples() {
        let image = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(
            1,
            1,
            image::Rgb([1024, 32768, 65535]),
        ));
        let bytes = encode_image_to_bytes(
            &image,
            "tiff",
            90,
            TiffBitDepth::Sixteen,
            8,
            ColorSpace::Srgb,
        )
        .unwrap();
        let decoded =
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Tiff).unwrap();
        assert_eq!(decoded.color().bits_per_pixel(), 48);
        assert_eq!(decoded.to_rgb16().get_pixel(0, 0).0, [1024, 32768, 65535]);

        let bytes =
            encode_image_to_bytes(&image, "tiff", 90, TiffBitDepth::Eight, 8, ColorSpace::Srgb)
                .unwrap();
        let decoded =
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Tiff).unwrap();
        assert_eq!(decoded.color().bits_per_pixel(), 24);
    }
}
