//! A Vulkan-presentable child surface behind GTK's transparent webview.
//! GTK keeps ownership of its display and parent surface; only the child is ours.
use gtk::{glib, prelude::*};
use std::{
    ffi::c_void,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use tauri::{Emitter, Manager};
use wayland_client::{
    Connection, Dispatch, Proxy,
    backend::{Backend, ObjectId},
    delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry,
        wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface,
    },
};
use wgpu::rwh::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};

#[link(name = "gdk-3")]
unsafe extern "C" {
    fn gdk_wayland_display_get_wl_display(display: *mut c_void) -> *mut c_void;
    fn gdk_wayland_window_get_wl_surface(window: *mut c_void) -> *mut c_void;
}

struct Events;
impl Dispatch<WlRegistry, GlobalListContents> for Events {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(Events: ignore WlCompositor);
delegate_noop!(Events: ignore WlSubcompositor);
delegate_noop!(Events: ignore WlSurface);
delegate_noop!(Events: ignore WlSubsurface);
delegate_noop!(Events: ignore WlRegion);

pub struct WaylandDisplay {
    connection: Connection,
    surface: WlSurface,
    subsurface: WlSubsurface,
    width: AtomicU32,
    height: AtomicU32,
    // Keep the Tauri window/runtime alive for the borrowed native display.
    _window: tauri::WebviewWindow,
}

impl WaylandDisplay {
    fn disable(&self) {
        let state = self._window.state::<crate::AppState>();
        if let Some(context) = state.gpu_context.lock().unwrap().as_ref() {
            context.display.lock().unwrap().take();
        }
        self.surface.attach(None, 0, 0);
        self.surface.commit();
        let _ = self.connection.flush();
        let _ = self._window.emit("wgpu-display-unavailable", ());
    }

    pub fn size(&self) -> tauri::PhysicalSize<u32> {
        tauri::PhysicalSize::new(
            self.width.load(Ordering::Relaxed),
            self.height.load(Ordering::Relaxed),
        )
    }

    fn resize(&self, viewport: &gtk::Box, window: &gtk::ApplicationWindow) {
        let scale = window.scale_factor().max(1);
        let width = (viewport.allocated_width().max(1) as u32).saturating_mul(scale as u32);
        let height = (viewport.allocated_height().max(1) as u32).saturating_mul(scale as u32);
        self.width.store(width, Ordering::Relaxed);
        self.height.store(height, Ordering::Relaxed);
        self.surface.set_buffer_scale(scale);
        if let Some((x, y)) = viewport.translate_coordinates(window, 0, 0) {
            self.subsurface.set_position(x, y);
        }
        let state = self._window.state::<crate::AppState>();
        if let Ok(context) = state.gpu_context.try_lock()
            && let Some(context) = context.as_ref()
            && let Ok(mut display) = context.display.try_lock()
            && let Some(display) = display.as_mut()
        {
            display.config.width = width;
            display.config.height = height;
            display.surface.configure(&context.device, &display.config);
            display.render(&context.device, &context.queue);
        }
        window.queue_draw();
    }
}

impl HasDisplayHandle for WaylandDisplay {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.connection.backend().display_ptr().cast())
            .ok_or(HandleError::Unavailable)?;
        // SAFETY: the guest connection and its GTK owner outlive this borrow.
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
                pointer,
            )))
        })
    }
}

impl HasWindowHandle for WaylandDisplay {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer =
            NonNull::new(self.surface.id().as_ptr().cast()).ok_or(HandleError::Unavailable)?;
        // SAFETY: wgpu retains an Arc<Self> until its native surface is released.
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Wayland(WaylandWindowHandle::new(pointer)))
        })
    }
}

impl Drop for WaylandDisplay {
    fn drop(&mut self) {
        self.subsurface.destroy();
        self.surface.destroy();
        let _ = self.connection.flush();
    }
}

/// Called on GTK's main thread, before the shared GPU context is initialized.
pub fn create(window: &tauri::WebviewWindow) -> Result<Arc<WaylandDisplay>, String> {
    let gtk_window = window.gtk_window().map_err(|e| e.to_string())?;
    let display = gtk::prelude::WidgetExt::display(&gtk_window);
    if display.type_().name() != "GdkWaylandDisplay" {
        return Err("Native Linux presentation requires a Wayland session".into());
    }
    gtk_window.realize();
    let gdk_window = gtk_window.window().ok_or("GTK window is not realized")?;
    // SAFETY: both GObjects belong to the live GTK window, on GTK's main thread.
    let (display_ptr, parent_ptr) = unsafe {
        (
            gdk_wayland_display_get_wl_display(display.as_ptr().cast()),
            gdk_wayland_window_get_wl_surface(gdk_window.as_ptr().cast()),
        )
    };
    if display_ptr.is_null() || parent_ptr.is_null() {
        return Err("GTK did not expose a Wayland display/surface".into());
    }
    // Guest mode does not disconnect GTK's display or take over its event queue.
    let connection =
        Connection::from_backend(unsafe { Backend::from_foreign_display(display_ptr.cast()) });
    let parent_id = unsafe { ObjectId::from_ptr(WlSurface::interface(), parent_ptr.cast()) }
        .map_err(|e| e.to_string())?;
    let parent = WlSurface::from_id(&connection, parent_id).map_err(|e| e.to_string())?;
    let (globals, mut events) =
        registry_queue_init::<Events>(&connection).map_err(|e| e.to_string())?;
    let qh = events.handle();
    let compositor: WlCompositor = globals.bind(&qh, 4..=4, ()).map_err(|e| e.to_string())?;
    let subcompositor: WlSubcompositor = globals.bind(&qh, 1..=1, ()).map_err(|e| e.to_string())?;
    let viewport = window.default_vbox().map_err(|e| e.to_string())?;
    let surface = compositor.create_surface(&qh, ());
    let subsurface = subcompositor.get_subsurface(&surface, &parent, &qh, ());
    // Independent commits let pan/zoom present without waiting for a webview repaint.
    subsurface.set_desync();
    subsurface.place_below(&parent);
    let (x, y) = viewport
        .translate_coordinates(&gtk_window, 0, 0)
        .unwrap_or((0, 0));
    subsurface.set_position(x, y);
    surface.set_buffer_scale(gtk_window.scale_factor().max(1));
    let input = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&input));
    input.destroy(); // An empty input region leaves all interaction with the webview.
    surface.commit();
    let target = Arc::new(WaylandDisplay {
        connection,
        surface,
        subsurface,
        width: AtomicU32::new(1),
        height: AtomicU32::new(1),
        _window: window.clone(),
    });

    let weak_target = Arc::downgrade(&target);
    let weak_window = gtk_window.downgrade();
    viewport.connect_size_allocate(move |viewport, _| {
        if let (Some(target), Some(window)) = (weak_target.upgrade(), weak_window.upgrade()) {
            target.resize(viewport, &window);
        }
    });
    let weak_target = Arc::downgrade(&target);
    gtk_window.connect_unmap(move |_| {
        // GTK can destroy/recreate its wl_surface when hidden. Do not keep
        // presenting to a child of the old parent after it is shown again.
        if let Some(target) = weak_target.upgrade() {
            target.disable();
        }
    });
    let weak_target = Arc::downgrade(&target);
    glib::timeout_add_local(Duration::from_millis(32), move || {
        let Some(target) = weak_target.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let state = target._window.state::<crate::AppState>();
        if let Some(context) = state.gpu_context.lock().unwrap().as_ref()
            && context.display.lock().unwrap().is_none()
        {
            target.surface.attach(None, 0, 0);
            target.surface.commit();
            let _ = target.connection.flush();
            return glib::ControlFlow::Break;
        }
        // GTK reads the shared socket. Dispatch only our queue, never GTK's events.
        if target.connection.backend().dispatch_inner_queue().is_err()
            || events.dispatch_pending(&mut Events).is_err()
        {
            target.disable();
            return glib::ControlFlow::Break;
        }
        let _ = target.connection.flush();
        glib::ControlFlow::Continue
    });
    target.resize(&viewport, &gtk_window);
    Ok(target)
}
