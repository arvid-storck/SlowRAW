use crate::{AppState, WindowState};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use tauri::{Emitter, Manager};

static RESOLUTION: AtomicU32 = AtomicU32::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(0);
pub fn resolution() -> u32 {
    RESOLUTION.load(Ordering::Relaxed)
}

fn save(window: &tauri::WebviewWindow) {
    let (Ok(size), Ok(position), Ok(dir)) = (
        window.inner_size(),
        window.outer_position(),
        window.app_handle().path().app_config_dir(),
    ) else {
        return;
    };
    let maximized = window.is_maximized().unwrap_or(false);
    let fullscreen = window.is_fullscreen().unwrap_or(false);
    let mut state = WindowState {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized,
        fullscreen,
    };
    if maximized || fullscreen {
        if let Ok(data) = std::fs::read(dir.join("viewer_window.json")) {
            if let Ok(previous) = serde_json::from_slice::<WindowState>(&data) {
                state.width = previous.width;
                state.height = previous.height;
                state.x = previous.x;
                state.y = previous.y;
            }
        }
    }
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(data) = serde_json::to_vec(&state) {
            let _ = std::fs::write(dir.join("viewer_window.json"), data);
        }
    }
}

#[tauri::command]
pub async fn toggle_viewer_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("viewer") {
        return window.close().map_err(|e| e.to_string());
    }
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let state = app.state::<AppState>();
    let context = crate::gpu_processing::get_or_init_gpu_context(&state, &app)?;
    let mut builder = tauri::WebviewWindowBuilder::new(
        &app,
        "viewer",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("SlowRAW")
    .inner_size(1000.0, 750.0)
    .min_inner_size(320.0, 240.0)
    .transparent(true);
    let monitors = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| crate::MonitorBounds {
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
        })
        .collect::<Vec<_>>();
    let saved = app
        .path()
        .app_config_dir()
        .ok()
        .and_then(|dir| std::fs::read(dir.join("viewer_window.json")).ok())
        .and_then(|data| serde_json::from_slice::<WindowState>(&data).ok())
        .filter(|saved| {
            saved.width >= 320
                && saved.height >= 240
                && (monitors.is_empty()
                    || monitors.iter().any(|m| {
                        (saved.x as i64 + saved.width as i64).min(m.x as i64 + m.width as i64)
                            - (saved.x as i64).max(m.x as i64)
                            >= 100
                            && (saved.y as i64 + saved.height as i64)
                                .min(m.y as i64 + m.height as i64)
                                - (saved.y as i64).max(m.y as i64)
                                >= 100
                    }))
        });
    if let Some(saved) = saved.as_ref() {
        builder = builder
            .fullscreen(saved.fullscreen)
            .maximized(saved.maximized);
    }
    let window = builder.build().map_err(|e| e.to_string())?;
    if let Some(saved) = saved {
        if !saved.fullscreen && !saved.maximized {
            let _ = window.set_size(tauri::PhysicalSize::new(saved.width, saved.height));
            let _ = window.set_position(tauri::PhysicalPosition::new(saved.x, saved.y));
        }
    }
    #[cfg(not(target_os = "linux"))]
    let surface = context
        .instance
        .create_surface(window.clone())
        .map(|s| {
            (
                s,
                window
                    .inner_size()
                    .unwrap_or(tauri::PhysicalSize::new(1000, 750)),
            )
        })
        .map_err(|e| e.to_string());
    #[cfg(target_os = "linux")]
    let surface = {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let target_window = window.clone();
        window
            .run_on_main_thread(move || {
                let _ = tx.send(crate::wayland_display::create(&target_window));
            })
            .map_err(|e| e.to_string())?;
        let target = rx.await.map_err(|e| e.to_string())??;
        let size = target.size();
        context
            .instance
            .create_surface(target)
            .map(|s| (s, size))
            .map_err(|e| e.to_string())
    };
    let result = surface.and_then(|(surface, size)| {
        crate::gpu_processing::create_display(
            surface,
            &context.adapter,
            &context.device,
            &context.queue,
            &window,
            size,
        )
    });
    let display = match result {
        Ok(display) => display,
        Err(error) => {
            let _ = window.close();
            return Err(error);
        }
    };
    *context.viewer_display.lock().unwrap() = Some(display);
    let owned = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => save(&owned),
        tauri::WindowEvent::CloseRequested { .. } => save(&owned),
        tauri::WindowEvent::Destroyed => {
            RESOLUTION.store(0, Ordering::Relaxed);
            let display = context.viewer_display.clone();
            std::thread::spawn(move || {
                let mut display = display.lock().unwrap();
                if GENERATION.load(Ordering::Relaxed) == generation {
                    display.take();
                }
            });
            let _ = owned.app_handle().emit_to("main", "viewer-demand", 0u32);
        }
        _ => {}
    });
    RESOLUTION.store(1920, Ordering::Relaxed);
    let _ = app.emit_to("main", "viewer-demand", 1920u32);
    Ok(())
}

#[tauri::command]
pub async fn viewer_frame(app: tauri::AppHandle) -> Result<Option<[f32; 2]>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let context = state
            .gpu_context
            .lock()
            .unwrap()
            .clone()
            .ok_or("GPU is unavailable")?;
        let frame = context.last_frame.lock().unwrap().clone();
        let Some(frame) = frame else {
            return Ok(None);
        };
        let mut viewer = context.viewer_display.lock().unwrap();
        if let Some(viewer) = viewer.as_mut() {
            if viewer.current_bind_group.is_none() {
                viewer.prepare(
                    &context.device,
                    &context.queue,
                    crate::color_management::PREVIEW_COLOR_SPACE,
                )?;
                viewer.set_frame(
                    &context.device,
                    &context.queue,
                    &frame.view,
                    frame.image_size,
                    frame.texture_size,
                )?;
            }
            return Ok(Some(frame.image_size));
        }
        Ok(None)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn viewer_resolution(app: tauri::AppHandle, resolution: u32) {
    let resolution = resolution.clamp(256, 16384);
    if RESOLUTION.swap(resolution, Ordering::Relaxed) != resolution {
        let _ = app.emit_to("main", "viewer-demand", resolution);
    }
}
