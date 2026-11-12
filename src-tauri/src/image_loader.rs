use crate::Cursor;
use crate::app_settings::{AppSettings, load_settings};
use crate::app_state::{AppState, LoadedImage};
use crate::exif_processing;
use crate::file_management::{parse_virtual_path, read_file_mapped};
use crate::formats::is_raw_file;
use crate::image_processing::ImageMetadata;
use crate::image_processing::{apply_orientation, reduce_raw_color_noise};
use crate::white_balance::WhiteBalance;
use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, GenericImageView, ImageReader};
use rawler::Orientation;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;
use tauri::Manager;

#[derive(serde::Serialize)]
pub struct LoadImageResult {
    pub width: u32,
    pub height: u32,
    pub metadata: ImageMetadata,
    pub exif: HashMap<String, String>,
    pub is_raw: bool,
    pub as_shot_white_balance: WhiteBalance,
}

pub fn load_and_composite(
    base_image: &[u8],
    path: &str,
    adjustments: &Value,
    use_fast_raw_dev: bool,
    settings: &AppSettings,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
) -> Result<DynamicImage> {
    let base_image =
        load_base_image_from_bytes(base_image, path, use_fast_raw_dev, settings, cancel_token)?;
    Ok(crate::spot_removal::composite_spot_removals(
        &base_image,
        adjustments,
    ))
}

pub fn load_base_image_from_bytes(
    bytes: &[u8],
    path_for_ext_check: &str,
    use_fast_raw_dev: bool,
    settings: &AppSettings,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
) -> Result<DynamicImage> {
    let highlight_compression = if settings.raw_highlight_recovery_enabled.unwrap_or(true) {
        settings.raw_highlight_compression.unwrap_or(2.5)
    } else {
        0.0
    };
    let color_nr_setting = settings.raw_preprocessing_color_nr.unwrap_or(0.0);
    let color_nr_amount = if color_nr_setting <= 0.0 {
        0.0
    } else {
        let x = color_nr_setting.clamp(0.01, 1.0);
        (12.0 / x - 10.0).max(0.1)
    };
    let apply_to_non_raws = settings.apply_preprocessing_to_non_raws.unwrap_or(false);

    if is_raw_file(path_for_ext_check) {
        let mut image = std::panic::catch_unwind(|| {
            crate::raw_processing::develop_raw_image(
                bytes,
                use_fast_raw_dev,
                1000.0,
                settings.linear_raw_mode.clone(),
                cancel_token,
                highlight_compression,
            )
        })
        .map_err(|panic| {
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("decoder panicked");
            anyhow!("RAW decoder failed for '{path_for_ext_check}': {message}")
        })?
        .with_context(|| format!("RAW development failed for '{path_for_ext_check}'"))?;
        if !use_fast_raw_dev && color_nr_amount > 0.0 {
            let start = Instant::now();
            reduce_raw_color_noise(&mut image, color_nr_amount);
            log::info!(
                "RAW enhancement for '{}' took {:?}",
                path_for_ext_check,
                start.elapsed()
            );
        }
        Ok(image)
    } else {
        let mut image = load_image_with_orientation(bytes, path_for_ext_check, cancel_token)?;

        if apply_to_non_raws && !use_fast_raw_dev && color_nr_amount > 0.0 {
            let start = Instant::now();
            reduce_raw_color_noise(&mut image, color_nr_amount);
            let duration = start.elapsed();
            log::info!(
                "Enhancing non-RAW '{}' took {:?}",
                path_for_ext_check,
                duration
            );
        }

        Ok(image)
    }
}

pub fn load_image_with_orientation(
    bytes: &[u8],
    source_path: &str,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
) -> Result<DynamicImage> {
    let check_cancel = || -> Result<()> {
        if let Some((tracker, generation)) = &cancel_token
            && tracker.load(Ordering::SeqCst) != *generation
        {
            return Err(anyhow!("Load cancelled"));
        }
        Ok(())
    };

    let cursor = Cursor::new(bytes);
    let mut reader = ImageReader::new(cursor.clone())
        .with_guessed_format()
        .context("Failed to guess image format")?;

    reader.no_limits();

    check_cancel()?;

    use image::ImageDecoder;
    let format = reader.format();
    let mut decoder = reader
        .into_decoder()
        .context("Failed to create image decoder")?;
    let mut profile = decoder
        .icc_profile()
        .context("Failed to read input ICC profile")?;
    // image's TIFF pixel-allocation limit can also block ICC tags larger than
    // a tiny image's pixel buffer. Read metadata without that pixel-sized limit.
    if profile.is_none() && format == Some(image::ImageFormat::Tiff) {
        profile = image::codecs::tiff::TiffDecoder::new(cursor.clone())?
            .icc_profile()
            .context("Failed to read TIFF ICC profile")?;
    }
    let mut image = DynamicImage::from_decoder(decoder).context("Failed to decode image")?;
    if let Some(profile) = profile {
        let source =
            moxcms::ColorProfile::new_from_slice(&profile).context("Invalid input ICC profile")?;
        image = crate::color_management::convert_image(
            &image,
            &source,
            crate::color_management::ColorSpace::Srgb,
        )
        .map_err(|error| anyhow!("Input color conversion failed: {error}"))?;
    }
    check_cancel()?;

    let orientation = exif_processing::read_exif_data(source_path, bytes)
        .get("Orientation")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(1);
    check_cancel()?;
    let oriented_image = apply_orientation(
        image,
        Orientation::from_u16(orientation).unwrap_or(Orientation::Normal),
    );

    Ok(DynamicImage::ImageRgb32F(oriented_image.to_rgb32f()))
}

#[tauri::command]
pub fn is_image_cached(path: String, state: tauri::State<'_, AppState>) -> bool {
    let (source_path, _) = parse_virtual_path(&path);
    let source_path_str = source_path.to_string_lossy().to_string();
    state
        .decoded_image_cache
        .lock()
        .unwrap()
        .get(&source_path_str)
        .is_some()
}

pub(crate) fn load_cached_source(
    source: &Path,
    settings: &AppSettings,
    state: &AppState,
    cancel_token: Option<(Arc<AtomicUsize>, usize)>,
) -> Result<(Arc<DynamicImage>, HashMap<String, String>), String> {
    let check_cancelled = || {
        if let Some((tracker, generation)) = &cancel_token
            && tracker.load(Ordering::SeqCst) != *generation
        {
            return Err("Load cancelled".to_owned());
        }
        Ok(())
    };
    check_cancelled()?;
    let path = source.to_string_lossy();
    if let Some(cached) = state.decoded_image_cache.lock().unwrap().get(&path) {
        return Ok(cached);
    }
    if crate::file_management::is_cloud_placeholder(source) {
        return Err(format!(
            "'{path}' is stored in iCloud and hasn't been downloaded yet."
        ));
    }

    let decode = |bytes: &[u8]| {
        check_cancelled()?;
        let image = load_base_image_from_bytes(bytes, &path, false, settings, cancel_token.clone())
            .map_err(|error| format!("{error:#}"))?;
        check_cancelled()?;
        let exif = exif_processing::read_exif_data(&path, bytes);
        Ok::<_, String>((Arc::new(image), exif))
    };
    let (image, exif) = match read_file_mapped(source) {
        Ok(mapped) => decode(&mapped)?,
        Err(error) => {
            log::warn!("Failed to map '{path}': {error}. Falling back to standard read.");
            let bytes =
                fs::read(source).map_err(|error| format!("Could not read '{path}': {error}"))?;
            decode(&bytes)?
        }
    };
    check_cancelled()?;
    state.decoded_image_cache.lock().unwrap().insert(
        path.into_owned(),
        image.clone(),
        exif.clone(),
    );
    Ok((image, exif))
}

#[tauri::command]
pub async fn load_image(
    path: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<LoadImageResult, String> {
    let my_generation = state.load_image_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let generation_tracker = state.load_image_generation.clone();
    let cancel_token = Some((generation_tracker.clone(), my_generation));

    {
        *state
            .original_image
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *state
            .gpu_image_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *state
            .full_warped_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;

        state
            .mask_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();

        *state
            .denoise_result
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *state.hdr_result.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *state
            .panorama_result
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    let (source_path, sidecar_path) = parse_virtual_path(&path);
    let source_path_str = source_path.to_string_lossy().to_string();

    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let mut metadata = crate::xmp::load_metadata(&source_path, &sidecar_path);
    crate::file_management::apply_raw_lens_default(
        &mut metadata.adjustments,
        &source_path,
        &settings,
    );

    let app = app_handle.clone();
    let decode_path = source_path.clone();
    let (pristine_arc, exif_data) = tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        load_cached_source(&decode_path, &settings, &state, cancel_token)
    })
    .await
    .map_err(|error| error.to_string())??;

    if state.load_image_generation.load(Ordering::SeqCst) != my_generation {
        return Err("Load cancelled".into());
    }
    let is_raw = is_raw_file(&source_path);

    let mut exif_data = exif_data;
    exif_data.extend(crate::exif_processing::read_rrexif_sidecar(&source_path).unwrap_or_default());
    exif_data.extend(metadata.exif.clone().unwrap_or_default());
    let (orig_width, orig_height) = pristine_arc.dimensions();
    let lens_db = state.lens_db.lock().map_err(|e| e.to_string())?.clone();
    let source_for_lens = source_path.clone();
    let exif_for_lens = Some(exif_data.clone());
    let metadata = tokio::task::spawn_blocking(move || {
        let mut metadata = metadata;
        if metadata.adjustments["lensCorrectionMode"].as_str() == Some("auto") {
            crate::file_management::resolve_lens_params_in_adjustments(
                &mut metadata.adjustments,
                &exif_for_lens,
                lens_db.as_deref(),
                &source_for_lens,
            );
        }
        metadata
    })
    .await
    .map_err(|e| e.to_string())?;
    let as_shot_white_balance = tokio::task::spawn_blocking(move || {
        crate::white_balance::as_shot_white_balance(&source_path_str)
    })
    .await
    .map_err(|e| format!("White balance metadata failed: {e}"))?;
    if state.load_image_generation.load(Ordering::SeqCst) != my_generation {
        return Err("Load cancelled".to_string());
    }

    *state.original_image.lock().unwrap() = Some(LoadedImage {
        path,
        image: pristine_arc,
        is_raw,
        as_shot_white_balance,
    });

    Ok(LoadImageResult {
        width: orig_width,
        height: orig_height,
        metadata,
        exif: exif_data,
        is_raw,
        as_shot_white_balance,
    })
}
