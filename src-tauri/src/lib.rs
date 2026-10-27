#[cfg(not(all(target_os = "windows", target_arch = "aarch64")))]
use mimalloc::MiMalloc;

#[cfg(not(all(target_os = "windows", target_arch = "aarch64")))]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod adjustment_utils;
mod app_settings;
mod app_state;
mod cache_utils;
mod catalog;
mod color_management;
mod component_updates;
mod culling;
mod denoising;
mod exif_processing;
mod export_metadata;
mod export_processing;
mod file_management;
mod focus_stacking;
mod formats;
mod gpu_processing;
mod guided_perspective;
mod hdr_deghosting;
mod highlight_recovery;
mod image_loader;
mod image_processing;
mod launch_request;
mod lens_blur;
mod lens_correction;
#[cfg(debug_assertions)]
mod logging;
mod mask_generation;
mod multi_exposure;
mod native_display;
mod negative_conversion;
mod panorama_stitching;
mod panorama_utils;
mod portable_paths;
mod preview_encoding;
mod raw_processing;
mod spot_removal;
mod tagging;
mod vulkan_instance;
#[cfg(target_os = "linux")]
mod wayland_display;
mod white_balance;
mod window_customizer;
#[cfg(target_os = "windows")]
mod windows_display;

// Fail a release build if dependency features ever re-enable native log calls.
#[cfg(not(debug_assertions))]
const _: () = assert!(matches!(log::STATIC_MAX_LEVEL, log::LevelFilter::Off));

use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageFormat, Luma};
use image_hdr::hdr_merge_images;
use image_hdr::input::HDRInput;
use mozjpeg_rs::{Encoder, Preset};

use serde_json::Value;
use tauri::{Emitter, Manager, ipc::Response};

use crate::cache_utils::{
    DecodedImageCache, calculate_full_job_hash, calculate_geometry_hash, calculate_transform_hash,
    calculate_visual_hash,
};
use crate::file_management::{parse_virtual_path, read_file_mapped};
use crate::formats::is_raw_file;
use crate::hdr_deghosting::{align_hdr_frames, assert_uniform_dimensions, load_hdr_frames};
use crate::image_loader::load_and_composite;
use crate::image_processing::{
    RenderRequest, apply_coarse_rotation, apply_cpu_default_raw_processing, apply_flip,
    apply_geometry_warp, apply_linear_to_srgb, downscale_f32_image, get_all_adjustments_from_json,
    get_or_init_gpu_context, process_and_get_dynamic_image, resolve_tonemapper_override,
    resolve_tonemapper_override_from_handle, warp_image_geometry,
};
use crate::mask_generation::{
    MaskDefinition, generate_mask_bitmap, get_cached_or_generate_mask,
    resolve_warped_image_for_masks,
};
use crate::spot_removal::composite_spot_removals;
use crate::window_customizer::PinchZoomDisablePlugin;
pub use adjustment_utils::*;
pub use app_settings::*;
pub use app_state::*;
pub use launch_request::*;

#[cfg(target_os = "macos")]
extern "C" fn force_exit(_signal: libc::c_int) {
    unsafe {
        libc::_exit(0);
    }
}

#[cfg(target_os = "macos")]
pub fn register_exit_handler() {
    unsafe {
        libc::signal(libc::SIGABRT, force_exit as *const () as libc::sighandler_t);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn register_exit_handler() {}

#[derive(serde::Serialize)]
struct ImageDimensions {
    width: u32,
    height: u32,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WgpuTransformPayload {
    pub window_width: f32,
    pub window_height: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub clip_x: f32,
    pub clip_y: f32,
    pub clip_width: f32,
    pub clip_height: f32,
    pub bg_primary: [f32; 4],
    pub bg_secondary: [f32; 4],
    pub pixelated: bool,
}

pub fn generate_transformed_preview(
    state: &tauri::State<AppState>,
    loaded_image: &LoadedImage,
    adjustments: &serde_json::Value,
    preview_dim: u32,
) -> Result<(DynamicImage, f32, (f32, f32)), String> {
    let transform_hash = calculate_transform_hash(adjustments);

    let (transformed_full_res, unscaled_crop_offset) = {
        let mut cache_lock = state
            .full_transformed_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some((hash, img, offset)) = cache_lock.as_ref() {
            if *hash == transform_hash {
                (Arc::clone(img), *offset)
            } else {
                let (arc_img, offset) = compute_full_transformed_res(loaded_image, adjustments)?;
                *cache_lock = Some((transform_hash, Arc::clone(&arc_img), offset));
                (arc_img, offset)
            }
        } else {
            let (arc_img, offset) = compute_full_transformed_res(loaded_image, adjustments)?;
            *cache_lock = Some((transform_hash, Arc::clone(&arc_img), offset));
            (arc_img, offset)
        }
    };

    let (full_res_w, full_res_h) = transformed_full_res.dimensions();

    let final_preview_base = if full_res_w > preview_dim || full_res_h > preview_dim {
        downscale_f32_image(&transformed_full_res, preview_dim, preview_dim)
    } else {
        (*transformed_full_res).clone()
    };

    let scale_for_gpu = if full_res_w > 0 {
        final_preview_base.width() as f32 / full_res_w as f32
    } else {
        1.0
    };

    Ok((final_preview_base, scale_for_gpu, unscaled_crop_offset))
}

fn compute_full_transformed_res(
    loaded_image: &LoadedImage,
    adjustments: &serde_json::Value,
) -> Result<(Arc<DynamicImage>, (f32, f32)), String> {
    let has_patches = crate::spot_removal::has_spot_removals(adjustments);
    let patched_original_image = if has_patches {
        Cow::Owned(composite_spot_removals(&loaded_image.image, adjustments))
    } else {
        Cow::Borrowed(loaded_image.image.as_ref())
    };

    let (transformed_img, offset) = apply_all_transformations(patched_original_image, adjustments);
    Ok((Arc::new(transformed_img.into_owned()), offset))
}

#[tauri::command]
fn get_image_dimensions(path: String) -> Result<ImageDimensions, String> {
    let (source_path, _) = parse_virtual_path(&path);
    image::image_dimensions(&source_path)
        .map(|(width, height)| ImageDimensions { width, height })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_thumbnail_generation(
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    state
        .thumbnail_cancellation_token
        .store(true, Ordering::SeqCst);

    let mut tracker = state.thumbnail_progress.lock().unwrap();
    tracker.total = 0;
    tracker.completed = 0;
    drop(tracker);

    let _ = app_handle.emit(
        "thumbnail-progress",
        serde_json::json!({ "current": 0, "total": 0 }),
    );
    Ok(())
}

pub fn get_cached_full_warped_image(
    state: &tauri::State<AppState>,
    js_adjustments: &serde_json::Value,
) -> Result<Arc<DynamicImage>, String> {
    let geo_hash = calculate_geometry_hash(js_adjustments);

    {
        let cache_lock = state
            .full_warped_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some((hash, img)) = cache_lock.as_ref()
            && *hash == geo_hash
        {
            return Ok(Arc::clone(img));
        }
    }

    let (base_arc, is_raw) = get_original_image(state)?;
    let mut cow_image = Cow::Borrowed(base_arc.as_ref());

    if is_raw {
        apply_cpu_default_raw_processing(cow_image.to_mut());
    }

    let warped_image = apply_geometry_warp(cow_image, js_adjustments).into_owned();
    let warped_arc = Arc::new(warped_image);

    {
        let mut cache_lock = state
            .full_warped_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *cache_lock = Some((geo_hash, Arc::clone(&warped_arc)));
    }

    Ok(warped_arc)
}

#[tauri::command]
async fn update_wgpu_transform(
    payload: WgpuTransformPayload,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let context = match state
        .gpu_context
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        Some(c) => c.clone(),
        None => return Ok(()),
    };

    tokio::task::spawn_blocking(move || {
        let mut display_lock = context.display.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(display) = display_lock.as_mut() {
            display.latest_transform.rect = [payload.x, payload.y, payload.width, payload.height];
            display.latest_transform.clip = [
                payload.clip_x,
                payload.clip_y,
                payload.clip_width,
                payload.clip_height,
            ];
            display.latest_transform.window = [payload.window_width, payload.window_height];
            display.latest_transform.bg_primary = payload.bg_primary;
            display.latest_transform.bg_secondary = payload.bg_secondary;
            display.latest_transform.pixelated = if payload.pixelated { 1.0 } else { 0.0 };

            context.queue.write_buffer(
                &display.transform_buffer,
                0,
                bytemuck::bytes_of(&display.latest_transform),
            );
            if let Err(error) = display.render(&context.device, &context.queue) {
                if error == "NATIVE_DISPLAY_LOST" {
                    display_lock.take();
                }
                let _ = app_handle.emit("wgpu-display-unavailable", error);
            }
        }
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_preview_job(
    app_handle: &tauri::AppHandle,
    state: tauri::State<AppState>,
    adjustments_json: serde_json::Value,
    is_interactive: bool,
    target_resolution: Option<u32>,
    roi: Option<(f32, f32, f32, f32)>,
    request_analytics: bool,
    compute_waveform: bool,
    active_waveform_channel: Option<&str>,
) -> Result<(), String> {
    let context = get_or_init_gpu_context(&state, app_handle)?;
    let adjustments_clone = adjustments_json;

    let loaded_image_guard = state.original_image.lock().unwrap();
    let loaded_image = loaded_image_guard
        .as_ref()
        .ok_or("No original image loaded")?
        .clone();
    drop(loaded_image_guard);

    let new_transform_hash = calculate_transform_hash(&adjustments_clone);
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let live_quality = settings.live_preview_quality.as_deref().unwrap_or("high");

    let default_preview_dim = settings.editor_preview_resolution.unwrap_or(1920);
    let preview_dim = target_resolution.unwrap_or(default_preview_dim);
    {
        let mut display = context.display.lock().unwrap_or_else(|e| e.into_inner());
        let display = display.as_mut().ok_or("NATIVE_DISPLAY_UNAVAILABLE")?;
        display.prepare(
            &context.device,
            &context.queue,
            color_management::PREVIEW_COLOR_SPACE,
        )?;
    }

    let has_roi = roi.is_some();
    let interactive_divisor = match live_quality {
        "full" => 1.0_f32,
        "performance" => {
            if has_roi {
                1.8_f32
            } else {
                1.5_f32
            }
        }
        _ => {
            if has_roi {
                1.4_f32
            } else {
                1.0_f32
            }
        }
    };

    let mut cached_preview_lock = state
        .cached_preview
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let base_valid = cached_preview_lock
        .as_ref()
        .is_some_and(|c| c.transform_hash == new_transform_hash && c.preview_dim == preview_dim);
    let small_valid = base_valid
        && cached_preview_lock
            .as_ref()
            .is_some_and(|c| c.interactive_divisor == interactive_divisor);

    let (final_preview_base, scale_for_gpu, unscaled_crop_offset) = if base_valid {
        let cached = cached_preview_lock.as_ref().unwrap();
        (
            Arc::clone(&cached.image),
            cached.scale,
            cached.unscaled_crop_offset,
        )
    } else {
        *state
            .gpu_image_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;

        let (base, scale, offset) =
            generate_transformed_preview(&state, &loaded_image, &adjustments_clone, preview_dim)?;
        (Arc::new(base), scale, offset)
    };

    let small_preview_base = if small_valid {
        Arc::clone(&cached_preview_lock.as_ref().unwrap().small_image)
    } else {
        let small = if interactive_divisor > 1.0 {
            let target_size = (preview_dim as f32 / interactive_divisor) as u32;
            let (w, h) = final_preview_base.dimensions();
            let (small_w, small_h) = if w > h {
                let ratio = h as f32 / w as f32;
                (target_size, (target_size as f32 * ratio) as u32)
            } else {
                let ratio = w as f32 / h as f32;
                ((target_size as f32 * ratio) as u32, target_size)
            };
            Arc::new(image_processing::downscale_f32_image(
                &final_preview_base,
                small_w,
                small_h,
            ))
        } else {
            Arc::clone(&final_preview_base)
        };

        if is_interactive && base_valid {
            *state
                .gpu_image_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
        }

        small
    };

    *cached_preview_lock = Some(CachedPreview {
        image: Arc::clone(&final_preview_base),
        small_image: Arc::clone(&small_preview_base),
        transform_hash: new_transform_hash,
        scale: scale_for_gpu,
        unscaled_crop_offset,
        preview_dim,
        interactive_divisor,
    });

    drop(cached_preview_lock);

    let (processing_image, effective_scale) = if is_interactive {
        let orig_w = final_preview_base.width() as f32;
        let small_w = small_preview_base.width() as f32;
        let scale_factor = if orig_w > 0.0 { small_w / orig_w } else { 1.0 };
        let new_scale = scale_for_gpu * scale_factor;
        (small_preview_base, new_scale)
    } else {
        (final_preview_base, scale_for_gpu)
    };

    let (preview_width, preview_height) = processing_image.dimensions();

    let pixel_roi = if is_interactive {
        roi.map(|(nx, ny, nw, nh)| crate::gpu_processing::Roi {
            x: (nx * preview_width as f32).round() as u32,
            y: (ny * preview_height as f32).round() as u32,
            width: (nw * preview_width as f32).round() as u32,
            height: (nh * preview_height as f32).round() as u32,
        })
    } else {
        None
    };

    let mask_definitions: Vec<MaskDefinition> = adjustments_clone
        .get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_default();

    let scaled_crop_offset = (
        unscaled_crop_offset.0 * effective_scale,
        unscaled_crop_offset.1 * effective_scale,
    );

    let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions
        .iter()
        .filter_map(|def| {
            get_cached_or_generate_mask(
                &state,
                def,
                preview_width,
                preview_height,
                effective_scale,
                scaled_crop_offset,
                &adjustments_clone,
            )
        })
        .collect();

    let is_raw = loaded_image.is_raw;
    let tm_override = resolve_tonemapper_override_from_handle(app_handle, is_raw);
    let mut final_adjustments = get_all_adjustments_from_json(
        &adjustments_clone,
        is_raw,
        loaded_image.as_shot_white_balance,
        tm_override,
    );

    let wants_analytics = !(is_interactive && pixel_roi.is_some()) && request_analytics;
    let channel_filter = if is_interactive {
        active_waveform_channel.map(|s| s.to_string())
    } else {
        None
    };

    let analytics_config = if wants_analytics {
        state
            .analytics_worker_tx
            .lock()
            .unwrap()
            .clone()
            .map(|tx| crate::AnalyticsConfig {
                path: loaded_image.path.clone(),
                compute_waveform,
                active_waveform_channel: channel_filter,
                sender: tx,
            })
    } else {
        None
    };

    let output_space = color_management::PREVIEW_COLOR_SPACE;
    output_space.configure(&mut final_adjustments);
    crate::image_processing::process_and_get_dynamic_image_with_analytics(
        &context,
        &state,
        &processing_image,
        new_transform_hash,
        RenderRequest {
            adjustments: final_adjustments,
            mask_bitmaps: &mask_bitmaps,
            roi: pixel_roi,
        },
        "apply_adjustments",
        true,
        analytics_config,
    )?;
    if context
        .display
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_none()
    {
        return Err("NATIVE_DISPLAY_LOST".into());
    }
    let _ = app_handle.emit(
        "wgpu-frame-ready",
        serde_json::json!({ "path": loaded_image.path }),
    );
    Ok(())
}

fn start_analytics_worker(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    let (tx, rx): (Sender<AnalyticsJob>, Receiver<AnalyticsJob>) = mpsc::channel();
    *state.analytics_worker_tx.lock().unwrap() = Some(tx);

    std::thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            while let Ok(latest) = rx.try_recv() {
                job = latest;
            }

            let histogram_data = image_processing::calculate_histogram_from_image(&job.image).ok();

            let waveform_data = if job.compute_waveform {
                image_processing::calculate_waveform_from_image(
                    &job.image,
                    job.active_waveform_channel.as_deref(),
                )
                .ok()
            } else {
                None
            };

            if histogram_data.is_some() || waveform_data.is_some() {
                let _ = app_handle.emit(
                    "analytics-update",
                    serde_json::json!({
                        "path": job.path,
                        "histogram": histogram_data,
                        "waveform": waveform_data,
                    }),
                );
            }
        }
    });
}

fn start_preview_worker(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    let (tx, rx): (Sender<PreviewJob>, Receiver<PreviewJob>) = mpsc::channel();

    *state.preview_worker_tx.lock().unwrap() = Some(tx);

    std::thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            while let Ok(latest_job) = rx.try_recv() {
                job = latest_job;
            }

            let state = app_handle.state::<AppState>();
            let responder = job.responder;
            let result = process_preview_job(
                &app_handle,
                state,
                job.adjustments,
                job.is_interactive,
                job.target_resolution,
                job.roi,
                job.request_analytics,
                job.compute_waveform,
                job.active_waveform_channel.as_deref(),
            );
            let _ = responder.send(result);
        }
    });
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn apply_adjustments(
    js_adjustments: serde_json::Value,
    is_interactive: bool,
    target_resolution: Option<u32>,
    roi: Option<(f32, f32, f32, f32)>,
    request_analytics: bool,
    compute_waveform: bool,
    active_waveform_channel: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();

    {
        let tx_guard = state.preview_worker_tx.lock().unwrap();
        if let Some(worker_tx) = tx_guard.as_ref() {
            let job = PreviewJob {
                adjustments: js_adjustments,
                is_interactive,
                target_resolution,
                roi,
                request_analytics,
                compute_waveform,
                active_waveform_channel,
                responder: tx,
            };
            worker_tx
                .send(job)
                .map_err(|e| format!("Failed to send to preview worker: {}", e))?;
        } else {
            return Err("Preview worker not running".to_string());
        }
    }

    match rx.await {
        Ok(result) => result,
        Err(_) => Err("Superseded or worker failed".to_string()),
    }
}

#[tauri::command]
async fn generate_uncropped_preview(
    js_adjustments: serde_json::Value,
    target_resolution: Option<u32>,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let context = get_or_init_gpu_context(&state, &app_handle)?;
    let adjustments_clone = js_adjustments.clone();

    let loaded_image = state
        .original_image
        .lock()
        .unwrap()
        .clone()
        .ok_or("No original image loaded")?;

    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let path = loaded_image.path.clone();
        let is_raw = loaded_image.is_raw;
        let settings = load_settings(app_handle.clone()).unwrap_or_default();
        let target_dim = target_resolution
            .unwrap_or(settings.editor_preview_resolution.unwrap_or(1920))
            .clamp(
                512,
                loaded_image
                    .image
                    .width()
                    .max(loaded_image.image.height())
                    .max(512),
            );
        let mut geometry_hasher = std::collections::hash_map::DefaultHasher::new();
        calculate_visual_hash(&path, &adjustments_clone).hash(&mut geometry_hasher);
        target_dim.hash(&mut geometry_hasher);
        let visual_hash = geometry_hasher.finish();

        let pre_geometry_base = {
            let mut cache = state.geometry_cache.lock().unwrap();
            if let Some(cached) = cache.get(&visual_hash).cloned() {
                cached
            } else {
                let has_patches = crate::spot_removal::has_spot_removals(&adjustments_clone);
                let patched_image = if has_patches {
                    Cow::Owned(composite_spot_removals(
                        &loaded_image.image,
                        &adjustments_clone,
                    ))
                } else {
                    Cow::Borrowed(loaded_image.image.as_ref())
                };

                let blurred_image =
                    crate::lens_blur::apply_lens_blur(patched_image, &adjustments_clone);

                let downscaled = downscale_f32_image(&blurred_image, target_dim, target_dim);

                if cache.len() > 5 {
                    cache.clear();
                }
                cache.insert(visual_hash, downscaled.clone());
                downscaled
            }
        };

        let scale_for_gpu = if loaded_image.image.width() > 0 {
            pre_geometry_base.width() as f32 / loaded_image.image.width() as f32
        } else {
            1.0
        };

        let params = crate::image_processing::get_geometry_params_from_json(&adjustments_clone);
        let mut adjusted_params = params;
        adjusted_params.lens_vignette_amount *= if is_raw { 0.4 } else { 0.8 };

        let warped_image = warp_image_geometry(&pre_geometry_base, adjusted_params);
        let orientation_steps = adjustments_clone["orientationSteps"].as_u64().unwrap_or(0) as u8;
        let flipped_image = apply_flip(
            apply_coarse_rotation(Cow::Owned(warped_image), orientation_steps),
            adjustments_clone["flipHorizontal"]
                .as_bool()
                .unwrap_or(false),
            adjustments_clone["flipVertical"].as_bool().unwrap_or(false),
        )
        .into_owned();

        let (preview_width, preview_height) = flipped_image.dimensions();

        let mask_definitions: Vec<MaskDefinition> = adjustments_clone
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();

        let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions
            .iter()
            .filter_map(|def| {
                get_cached_or_generate_mask(
                    &state,
                    def,
                    preview_width,
                    preview_height,
                    scale_for_gpu,
                    (0.0, 0.0),
                    &adjustments_clone,
                )
            })
            .collect();

        let tm_override = resolve_tonemapper_override_from_handle(&app_handle, is_raw);
        let mut uncropped_adjustments = get_all_adjustments_from_json(
            &adjustments_clone,
            is_raw,
            loaded_image.as_shot_white_balance,
            tm_override,
        );
        uncropped_adjustments.global.show_clipping = 0;
        let output_space = color_management::PREVIEW_COLOR_SPACE;
        output_space.configure(&mut uncropped_adjustments);

        let mut render_hasher = std::collections::hash_map::DefaultHasher::new();
        calculate_full_job_hash(&path, &adjustments_clone).hash(&mut render_hasher);
        target_dim.hash(&mut render_hasher);
        let full_hash = render_hasher.finish();

        if let Ok(processed_image) = image_processing::process_and_get_dynamic_image_with_precision(
            &context,
            &state,
            &flipped_image,
            full_hash,
            RenderRequest {
                adjustments: uncropped_adjustments,
                mask_bitmaps: &mask_bitmaps,
                roi: None,
            },
            "generate_uncropped_preview",
            image_processing::RenderOutputPrecision::SixteenBit,
        ) {
            let bytes = preview_encoding::encode(&processed_image, output_space, false)?;
            return Ok(format!(
                "data:image/png;base64,{}",
                general_purpose::STANDARD.encode(&bytes)
            ));
        }

        Err("Failed to process uncropped preview".to_string())
    })
    .await
    .map_err(|e| format!("Task execution failed: {}", e))?
}

pub fn get_original_image(
    state: &tauri::State<AppState>,
) -> Result<(std::sync::Arc<image::DynamicImage>, bool), String> {
    let original_image_lock = state.original_image.lock().unwrap();
    let loaded_image = original_image_lock
        .as_ref()
        .ok_or("No original image loaded")?;
    Ok((
        std::sync::Arc::clone(&loaded_image.image),
        loaded_image.is_raw,
    ))
}

#[tauri::command]
async fn merge_hdr(
    paths: Vec<String>,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if paths.len() < 2 {
        return Err("Please select at least two images to merge.".to_string());
    }

    let hdr_result_handle = state.hdr_result.clone();
    let settings = load_settings(app_handle.clone()).unwrap_or_default();

    let mut frames = load_hdr_frames(&paths, &app_handle, &settings)?;
    assert_uniform_dimensions(&frames)?;
    align_hdr_frames(&mut frames, &app_handle);

    let images: Vec<HDRInput> = frames
        .iter()
        .map(|(path, img, exposure, gains)| {
            HDRInput::with_image(img, *exposure, *gains)
                .map_err(|e| format!("Failed to prepare HDR input for {}: {}", path, e))
        })
        .collect::<Result<Vec<HDRInput>, String>>()?;

    log::info!("Starting HDR merge of {} images", images.len());
    let mut hdr_merged = hdr_merge_images(&mut images.into()).map_err(|e| e.to_string())?;
    hdr_merged =
        image_hdr::stretch::apply_histogram_stretch(&hdr_merged).map_err(|e| e.to_string())?;
    hdr_merged = apply_linear_to_srgb(hdr_merged);
    log::info!("HDR merge completed");

    let mut buf = Cursor::new(Vec::new());
    if let Err(e) = hdr_merged.to_rgb8().write_to(&mut buf, ImageFormat::Png) {
        return Err(format!("Failed to encode hdr preview: {}", e));
    }

    let base64_str = general_purpose::STANDARD.encode(buf.get_ref());
    let final_base64 = format!("data:image/png;base64,{}", base64_str);

    let _ = app_handle.emit("hdr-progress", "Creating preview...");

    *hdr_result_handle.lock().unwrap() = Some(hdr_merged);

    let _ = app_handle.emit(
        "hdr-complete",
        serde_json::json!({
            "base64": final_base64,
        }),
    );
    Ok(())
}

#[tauri::command]
async fn save_hdr(
    first_path_str: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let hdr_image = state.hdr_result.lock().unwrap().take().ok_or_else(|| {
        "No hdr image found in memory to save. It might have already been saved.".to_string()
    })?;

    let (first_path, _) = parse_virtual_path(&first_path_str);
    let parent_dir = first_path
        .parent()
        .ok_or_else(|| "Could not determine parent directory of the first image.".to_string())?;
    let stem = first_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("hdr");

    let (output_filename, image_to_save): (String, DynamicImage) = if hdr_image.color().has_alpha()
    {
        (
            format!("{}_Hdr.png", stem),
            DynamicImage::ImageRgba8(hdr_image.to_rgba8()),
        )
    } else if hdr_image.as_rgb32f().is_some() {
        (format!("{}_Hdr.tiff", stem), hdr_image)
    } else {
        (
            format!("{}_Hdr.png", stem),
            DynamicImage::ImageRgb8(hdr_image.to_rgb8()),
        )
    };

    let output_path = parent_dir.join(output_filename);

    image_to_save
        .save(&output_path)
        .map_err(|e| format!("Failed to save hdr image: {}", e))?;

    let (real_path, _) = crate::file_management::parse_virtual_path(&first_path_str);
    let _ =
        crate::exif_processing::write_rrexif_sidecar(&real_path.to_string_lossy(), &output_path);

    Ok(output_path.to_string_lossy().to_string())
}

#[tauri::command]
async fn save_collage(base64_data: String, first_path_str: String) -> Result<String, String> {
    let data_url_prefix = "data:image/png;base64,";
    if !base64_data.starts_with(data_url_prefix) {
        return Err("Invalid base64 data format".to_string());
    }
    let encoded_data = &base64_data[data_url_prefix.len()..];

    let decoded_bytes = general_purpose::STANDARD
        .decode(encoded_data)
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

    let (first_path, _) = parse_virtual_path(&first_path_str);
    let parent_dir = first_path
        .parent()
        .ok_or_else(|| "Could not determine parent directory of the first image.".to_string())?;
    let stem = first_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("collage");

    let output_filename = format!("{}_Collage.png", stem);
    let output_path = parent_dir.join(output_filename);

    fs::write(&output_path, &decoded_bytes)
        .map_err(|e| format!("Failed to save collage image: {}", e))?;

    Ok(output_path.to_string_lossy().to_string())
}

#[tauri::command]
async fn generate_preview_for_path(
    path: String,
    js_adjustments: Value,
    app_handle: tauri::AppHandle,
) -> Result<Response, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let context = get_or_init_gpu_context(&state, &app_handle)?;
        let (source_path, _) = parse_virtual_path(&path);
        let source_path_str = source_path.to_string_lossy().to_string();
        let is_raw = is_raw_file(&source_path_str);
        let settings = load_settings(app_handle.clone()).unwrap_or_default();

        let base_image = match read_file_mapped(&source_path) {
            Ok(mmap) => load_and_composite(
                &mmap,
                &source_path_str,
                &js_adjustments,
                false,
                &settings,
                None,
            )
            .map_err(|e| e.to_string())?,
            Err(e) => {
                log::warn!(
                    "Failed to memory-map file '{}': {}. Falling back to standard read.",
                    source_path_str,
                    e
                );
                let bytes = fs::read(&source_path).map_err(|io_err| io_err.to_string())?;
                load_and_composite(
                    &bytes,
                    &source_path_str,
                    &js_adjustments,
                    false,
                    &settings,
                    None,
                )
                .map_err(|e| e.to_string())?
            }
        };

        let (transformed_image, unscaled_crop_offset) =
            apply_all_transformations(Cow::Borrowed(&base_image), &js_adjustments);
        let (img_w, img_h) = transformed_image.dimensions();
        let mask_definitions: Vec<MaskDefinition> = js_adjustments
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();

        let warped_image =
            resolve_warped_image_for_masks(&state, &js_adjustments, &mask_definitions);
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

        let tm_override = resolve_tonemapper_override(&settings, is_raw);
        let mut all_adjustments = get_all_adjustments_from_json(
            &js_adjustments,
            is_raw,
            crate::white_balance::as_shot_white_balance(&source_path_str),
            tm_override,
        );
        all_adjustments.global.show_clipping = 0;
        let unique_hash = calculate_full_job_hash(&source_path_str, &js_adjustments);

        let final_image = process_and_get_dynamic_image(
            &context,
            &state,
            transformed_image.as_ref(),
            unique_hash,
            RenderRequest {
                adjustments: all_adjustments,
                mask_bitmaps: &mask_bitmaps,
                roi: None,
            },
            "generate_preview_for_path",
        )?;

        let (width, height) = final_image.dimensions();
        let rgb_pixels = final_image.to_rgb8().into_vec();

        let bytes = Encoder::new(Preset::BaselineFastest)
            .quality(92)
            .encode_rgb(&rgb_pixels, width, height)
            .map_err(|e| format!("Failed to encode with mozjpeg-rs: {}", e))?;

        Ok(Response::new(bytes))
    })
    .await
    .map_err(|e| format!("Task execution failed: {}", e))?
}

#[derive(Clone, Copy, Debug)]
struct MonitorBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn saved_window_state_is_usable(state: &WindowState, monitors: &[MonitorBounds]) -> bool {
    if state.width < 800 || state.height < 600 {
        return false;
    }

    if monitors.is_empty() {
        return true;
    }

    let window_left = state.x as i64;
    let window_top = state.y as i64;
    let window_right = window_left + state.width as i64;
    let window_bottom = window_top + state.height as i64;

    monitors.iter().any(|monitor| {
        let monitor_left = monitor.x as i64;
        let monitor_top = monitor.y as i64;
        let monitor_right = monitor_left + monitor.width as i64;
        let monitor_bottom = monitor_top + monitor.height as i64;

        let overlap_width = window_right.min(monitor_right) - window_left.max(monitor_left);
        let overlap_height = window_bottom.min(monitor_bottom) - window_top.max(monitor_top);

        overlap_width >= 100 && overlap_height >= 100
    })
}

#[cfg(not(target_os = "android"))]
fn available_monitor_bounds(window: &tauri::WebviewWindow) -> Vec<MonitorBounds> {
    window
        .available_monitors()
        .map(|monitors| {
            monitors
                .into_iter()
                .map(|monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    MonitorBounds {
                        x: position.x,
                        y: position.y,
                        width: size.width,
                        height: size.height,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "android")]
fn available_monitor_bounds(_window: &tauri::WebviewWindow) -> Vec<MonitorBounds> {
    Vec::new()
}

#[tauri::command]
fn frontend_ready(
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    state: tauri::State<AppState>,
) -> Result<LaunchPayload, String> {
    let is_first_run = !state
        .window_setup_complete
        .swap(true, std::sync::atomic::Ordering::Relaxed);
    #[cfg(target_os = "android")]
    let _ = (is_first_run, &window, &app_handle);

    #[cfg(not(target_os = "android"))]
    {
        #[cfg(any(windows, target_os = "linux"))]
        let mut should_maximize = false;
        #[cfg(any(windows, target_os = "linux"))]
        let mut should_fullscreen = false;
        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = (&app_handle, is_first_run);

        #[cfg(any(windows, target_os = "linux"))]
        if is_first_run && let Ok(config_dir) = portable_paths::app_config_dir(&app_handle) {
            let path = config_dir.join("window_state.json");

            if let Ok(contents) = std::fs::read_to_string(&path)
                && let Ok(saved_state) = serde_json::from_str::<WindowState>(&contents)
            {
                #[cfg(any(windows, target_os = "linux"))]
                {
                    should_maximize = saved_state.maximized;
                    should_fullscreen = saved_state.fullscreen;
                }

                if (should_maximize || should_fullscreen)
                    && let Some(monitor) = window
                        .current_monitor()
                        .ok()
                        .flatten()
                        .or_else(|| window.primary_monitor().ok().flatten())
                        .or_else(|| {
                            window
                                .available_monitors()
                                .ok()
                                .and_then(|m| m.into_iter().next())
                        })
                {
                    let monitor_size = monitor.size();
                    let monitor_pos = monitor.position();
                    let default_width = 1280i32;
                    let default_height = 720i32;
                    let center_x = monitor_pos.x + (monitor_size.width as i32 - default_width) / 2;
                    let center_y =
                        monitor_pos.y + (monitor_size.height as i32 - default_height) / 2;

                    let _ = window.set_size(tauri::PhysicalSize::new(
                        default_width as u32,
                        default_height as u32,
                    ));
                    let _ = window.set_position(tauri::PhysicalPosition::new(center_x, center_y));
                }
            }
        }

        if let Err(e) = window.show() {
            log::error!("Failed to show window: {}", e);
        }
        if let Err(e) = window.set_focus() {
            log::error!("Failed to focus window: {}", e);
        }
        #[cfg(any(windows, target_os = "linux"))]
        if is_first_run {
            if should_maximize {
                let _ = window.maximize();
            }
            if should_fullscreen {
                let _ = window.set_fullscreen(true);
            }
        }
    }

    let open_with_file = state.initial_file_path.lock().unwrap().take();
    let edit_session = state.pending_edit_session.lock().unwrap().take();
    if let Some(path) = &open_with_file {
        log::info!("Frontend is ready, returning initial path: {}", path);
    }
    if let Some(session) = &edit_session {
        log::info!(
            "Frontend is ready, returning external edit session for: {}",
            session.source
        );
    }
    Ok(LaunchPayload {
        open_with_file,
        edit_session,
    })
}

pub fn run() {
    let _ = rayon::ThreadPoolBuilder::new()
        .stack_size(8 * 1024 * 1024)
        .build_global();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let launch_req = parse_launch_args(&args);

    let mut builder = tauri::Builder::default();

    #[cfg(target_os = "linux")]
    {
        builder = builder.plugin(tauri_plugin_wayland_nvidia_quirk::init());
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            log::info!(
                "New instance launched with args: {:?}. Focusing main window.",
                argv
            );
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = window.unminimize() {
                    log::error!("Failed to unminimize window: {}", e);
                }
                if let Err(e) = window.set_focus() {
                    log::error!("Failed to set focus on window: {}", e);
                }
            }

            let forwarded_args = argv.get(1..).unwrap_or(&[]);
            emit_launch_request(app, parse_launch_args(forwarded_args));
        }));
    }

    builder
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(PinchZoomDisablePlugin)
        .on_window_event(|window, event| {
            if !matches!(event, tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) | tauri::WindowEvent::ScaleFactorChanged { .. }) { return; }
            let Ok(size) = window.inner_size() else { return; };
            let state = window.state::<AppState>();
            if let Some(ctx) = state.gpu_context.lock().unwrap().as_ref()
                && let Ok(mut display_lock) = ctx.display.try_lock()
                    && let Some(display) = display_lock.as_mut() {
                        #[cfg(target_os = "linux")]
                        let size = state.wayland_display.lock().unwrap().as_ref().map(|target| target.size()).unwrap_or(size);
                        display.invalidate();
                        display.config.width = size.width.max(1);
                        display.config.height = size.height.max(1);
                        display.surface.configure(&ctx.device, &display.config);
                        if let Err(error) = display.render(&ctx.device, &ctx.queue) {
                            if error == "NATIVE_DISPLAY_LOST" { display_lock.take(); }
                            let _ = window.emit("wgpu-display-unavailable", error);
                        }
                    }
        })
        .setup(move |app| {
            let state = app.state::<AppState>();

            #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
            {
                match launch_req.clone() {
                    LaunchRequest::EditSession(session) => {
                        log::info!("Initial launch with external edit session for: {}", session.source);
                        *state.pending_edit_session.lock().unwrap() = Some(session);
                    }
                    LaunchRequest::OpenFile(path) => {
                        log::info!("Initial open: Storing path {} for later.", path);
                        *state.initial_file_path.lock().unwrap() = Some(path);
                    }
                    _ => {}
                }
            }

            let app_handle = app.handle().clone();
            portable_paths::migrate_legacy_app_data(&app_handle)
                .map_err(std::io::Error::other)?;
            component_updates::initialize_component_environment(&app_handle);

            if let Ok(cache_dir) = app_handle.path().app_cache_dir() {
                crate::exif_processing::initialize_cache_dir(cache_dir);
            }

            {
                let disks_app_handle = app_handle.clone();
                std::thread::spawn(move || {
                    let disks = sysinfo::Disks::new_with_refreshed_list();
                    let state = disks_app_handle.state::<AppState>();
                    *state.disks_cache.lock().unwrap() = Some(disks);
                });
            }

            let config_dir = portable_paths::app_config_dir(&app_handle).expect("Failed to get config dir");
            let crash_flag_path = config_dir.join(".gpu_init_crash_flag");

            {
                let state = app.state::<AppState>();
                *state.gpu_crash_flag_path.lock().unwrap() = Some(crash_flag_path.clone());
            }

            let settings: AppSettings = load_settings(app_handle.clone()).unwrap_or_default();

            {
                let state = app.state::<AppState>();
                let cache_size = settings.image_cache_size.unwrap_or(5) as usize;
                state.decoded_image_cache.lock().unwrap().set_capacity(cache_size);
            }

            if crash_flag_path.exists() {
                log::warn!("GPU initialization failed on the previous run; retrying Vulkan.");
                let _ = std::fs::remove_file(&crash_flag_path);
            }

            let lens_db = lens_correction::load_lensfun_db(&app_handle);
            {
                let state = app.state::<AppState>();
                *state.lens_db.lock().unwrap() = Some(Arc::new(lens_db));
            }

            #[cfg(debug_assertions)]
            logging::initialize();

            #[cfg(target_os = "linux")]
            log::info!("Wayland Nvidia quirk status: {:?}", tauri_plugin_wayland_nvidia_quirk::status());

            start_preview_worker(app_handle.clone());
            start_analytics_worker(app_handle.clone());
            file_management::start_thumbnail_workers(app_handle.clone());
            file_management::start_metadata_workers(app_handle.clone());
            jxl_oxide::integration::register_image_decoding_hook();

            let window_cfg = app.config().app.windows.first().unwrap().clone();
            let decorations = settings.decorations.unwrap_or(window_cfg.decorations);
            #[cfg(target_os = "android")]
            let _ = decorations;

            let main_window_cfg = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .expect("Main window config not found")
                .clone();

            let mut window_builder =
                tauri::WebviewWindowBuilder::from_config(app.handle(), &main_window_cfg)
                    .unwrap();

            #[cfg(not(target_os = "android"))]
            {
                window_builder = window_builder.decorations(decorations).visible(false);
            }

            let window = window_builder.build().expect("Failed to build window");

            #[cfg(target_os = "linux")]
            {
                match wayland_display::create(&window) {
                    Ok(display) => *state.wayland_display.lock().unwrap() = Some(display),
                    Err(error) => log::warn!("Native editor display unavailable: {error}"),
                }
            }

            #[cfg(target_os = "android")]
            android_integration::initialize_android(&window);

            #[cfg(not(target_os = "android"))]
            {
                let app_state = app.state::<AppState>();
                if let Err(error) = get_or_init_gpu_context(&app_state, app.handle()) {
                    log::warn!(
                        "GPU pre-initialization failed (editing and thumbnails may be degraded): {}",
                        error
                    );
                }

                if let Ok(config_dir) = portable_paths::app_config_dir(&app.handle()) {
                    let path = config_dir.join("window_state.json");
                    if let Ok(contents) = std::fs::read_to_string(&path) {
                        if let Ok(state) = serde_json::from_str::<WindowState>(&contents) {
                            let monitor_bounds = available_monitor_bounds(&window);
                            if saved_window_state_is_usable(&state, &monitor_bounds) {
                                let _ = window.set_size(tauri::Size::Physical(
                                    tauri::PhysicalSize::new(state.width, state.height),
                                ));
                                let _ = window.set_position(tauri::Position::Physical(
                                    tauri::PhysicalPosition::new(state.x, state.y),
                                ));
                            } else {
                                log::warn!(
                                    "Saved window state was unusable ({}x{} at {},{}), centering instead.",
                                    state.width,
                                    state.height,
                                    state.x,
                                    state.y
                                );
                                let _ = window.center();
                            }
                        } else {
                            let _ = window.center();
                        }
                    } else {
                        let _ = window.center();
                    }
                } else {
                    let _ = window.center();
                }

                let window_failsafe = window.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    if let Ok(false) = window_failsafe.is_visible() {
                        log::warn!(
                            "Frontend failed to report ready within timeout. Forcing window visibility."
                        );
                        let _ = window_failsafe.show();
                        let _ = window_failsafe.set_focus();
                    }
                });

                let pending_window_state = Arc::new(Mutex::new(None::<WindowState>));
                let pending_state_for_saver = pending_window_state.clone();
                let app_handle_for_saver = app.handle().clone();

                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(500)).await;

                        let state_to_save = {
                            let mut lock = pending_state_for_saver.lock().unwrap();
                            lock.take()
                        };

                        if let Some(state) = state_to_save
                            && let Ok(config_dir) =
                                portable_paths::app_config_dir(&app_handle_for_saver)
                        {
                            let path = config_dir.join("window_state.json");
                            let _ = std::fs::create_dir_all(&config_dir);
                            if let Ok(json) = serde_json::to_string(&state) {
                                let _ = std::fs::write(&path, json);
                            }
                        }
                    }
                });

                let window_for_handler = window.clone();
                let pending_state_for_handler = pending_window_state.clone();

                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                        #[cfg(any(windows, target_os = "linux"))]
                        let maximized = window_for_handler.is_maximized().unwrap_or(false);
                        #[cfg(not(any(windows, target_os = "linux")))]
                        let maximized = false;

                        #[cfg(any(windows, target_os = "linux"))]
                        let fullscreen = window_for_handler.is_fullscreen().unwrap_or(false);
                        #[cfg(not(any(windows, target_os = "linux")))]
                        let fullscreen = false;

                        if window_for_handler.is_minimized().unwrap_or(false) {
                            return;
                        }

                        let mut state = WindowState {
                            width: 1280,
                            height: 720,
                            x: 0,
                            y: 0,
                            maximized,
                            fullscreen,
                        };

                        if let Ok(position) = window_for_handler.outer_position() {
                            state.x = position.x;
                            state.y = position.y;
                        }

                        if !maximized
                            && !fullscreen
                            && let Ok(size) = window_for_handler.outer_size()
                            && size.width >= 800
                            && size.height >= 600
                        {
                            state.width = size.width;
                            state.height = size.height;
                        }

                        *pending_state_for_handler.lock().unwrap() = Some(state);
                    }
                    _ => {}
                });
            }

            crate::register_exit_handler();
            Ok(())
        })
        .manage(AppState {
            window_setup_complete: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            wayland_display: Mutex::new(None),
            gpu_crash_flag_path: Mutex::new(None),
            original_image: Mutex::new(None),
            cached_preview: Mutex::new(None),
            gpu_context: Mutex::new(None),
            gpu_image_cache: Mutex::new(None),
            gpu_processor: Mutex::new(None),
            export_task_token: Arc::new(Mutex::new(None)),
            hdr_result: Arc::new(Mutex::new(None)),
            panorama_result: Arc::new(Mutex::new(None)),
            focus_stack_result: Arc::new(Mutex::new(None)),
            denoise_result: Arc::new(Mutex::new(None)),
            indexing_task_handle: Mutex::new(None),
            initial_file_path: Mutex::new(None),
            pending_edit_session: Mutex::new(None),
            thumbnail_cancellation_token: Arc::new(AtomicBool::new(false)),
            thumbnail_progress: Mutex::new(ThumbnailProgressTracker { total: 0, completed: 0 }),
            preview_worker_tx: Mutex::new(None),
            analytics_worker_tx: Mutex::new(None),
            mask_cache: Mutex::new(HashMap::new()),
            geometry_cache: Mutex::new(HashMap::new()),
            thumbnail_geometry_cache: Mutex::new(HashMap::new()),
            lens_db: Mutex::new(None),
            load_image_generation: Arc::new(AtomicUsize::new(0)),
            full_warped_cache: Mutex::new(None),
            full_transformed_cache: Mutex::new(None),
            decoded_image_cache: Mutex::new(DecodedImageCache::new(5)),
            thumbnail_manager: ThumbnailManager::new(),
            metadata_manager: MetadataManager::new(),
            disks_cache: Mutex::new(None),
            disks_cache_refreshing: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            apply_adjustments,
            generate_preview_for_path,
            generate_uncropped_preview,
            save_collage,
            merge_hdr,
            save_hdr,
            get_image_dimensions,
            frontend_ready,
            cancel_thumbnail_generation,
            update_wgpu_transform,
            cache_utils::clear_session_caches,
            cache_utils::clear_image_caches,
            catalog::catalog_status,
            catalog::catalog_add_folder,
            catalog::catalog_remove_folder,
            catalog::catalog_remove_images,
            catalog::catalog_refresh_paths,
            catalog::catalog_rebuild,
            catalog::catalog_query_page,
            catalog::open_video_in_system,
            catalog::catalog_facets,
            component_updates::update_lensfun,
            app_settings::load_settings,
            app_settings::save_settings,
            denoising::apply_denoising,
            denoising::batch_denoise_images,
            denoising::save_denoised_image,
            focus_stacking::stitch_focus_stack,
            focus_stacking::save_focus_stack,
            image_loader::load_image,
            image_loader::is_image_cached,
            panorama_stitching::stitch_panorama,
            panorama_stitching::save_panorama,
            export_processing::export_images,
            export_processing::cancel_export,
            export_processing::estimate_export_sizes,
            image_processing::calculate_auto_adjustments,
            image_processing::sample_white_balance,
            mask_generation::generate_mask_overlay,
            file_management::update_exif_fields,
            file_management::get_supported_file_types,
            file_management::read_exif_for_paths,
            file_management::list_images_in_dir,
            file_management::list_images_recursive,
            file_management::update_thumbnail_queue,
            file_management::copy_files,
            file_management::move_files,
            file_management::rename_files,
            file_management::duplicate_file,
            file_management::show_in_finder,
            file_management::delete_files_from_disk,
            file_management::delete_files_with_associated,
            file_management::save_metadata_and_update_thumbnail,
            file_management::apply_adjustments_to_paths,
            file_management::load_metadata,
            file_management::reset_adjustments_for_paths,
            file_management::apply_auto_lens_correction_to_paths,
            file_management::apply_auto_adjustments_to_paths,
            file_management::clear_all_sidecars,
            file_management::clear_thumbnail_cache,
            file_management::set_color_label_for_paths,
            file_management::set_rating_for_paths,
            tagging::set_flag_for_paths,
            file_management::import_files,
            file_management::create_virtual_copy,
            file_management::get_albums,
            file_management::save_albums,
            file_management::add_to_album,
            file_management::get_album_images,
            tagging::add_tag_for_paths,
            tagging::remove_tag_for_paths,
            culling::cull_images,
            lens_correction::get_lensfun_makers,
            lens_correction::get_lensfun_lenses_for_maker,
            lens_correction::autodetect_lens,
            lens_correction::get_lens_distortion_params,
            negative_conversion::preview_negative_conversion,
            negative_conversion::convert_negatives,
            guided_perspective::calculate_guided_perspective,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(#[allow(unused_variables)] |app_handle, event| {
            match event {
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Opened { urls } => {
                    if let Some(url) = urls.first()
                        && let Ok(path) = url.to_file_path()
                        && let Some(path_str) = path.to_str()
                    {
                        let state = app_handle.state::<AppState>();
                        *state.initial_file_path.lock().unwrap() = Some(path_str.to_string());
                        log::info!("macOS initial open: Stored path {} for later.", path_str);
                    }
                }
                tauri::RunEvent::ExitRequested { api, .. } => {
                    api.prevent_exit();

                    #[cfg(target_os = "macos")]
                    unsafe { libc::_exit(0); }

                    #[cfg(not(target_os = "macos"))]
                    std::process::exit(0);
                }
                tauri::RunEvent::Exit => {
                    #[cfg(target_os = "macos")]
                    unsafe { libc::_exit(0); }

                    #[cfg(not(target_os = "macos"))]
                    std::process::exit(0);
                }
                _ => {}
            }
        });
}
