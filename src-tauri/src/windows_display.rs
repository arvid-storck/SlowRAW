//! Select the window's monitor profile only when Windows is not color-managing it.
use std::{path::PathBuf, time::SystemTime};
use tauri::WebviewWindow;
use windows::{
    Win32::{Devices::Display::*, Graphics::Gdi::*, UI::ColorSystem::GetICMProfileW},
    core::{PCWSTR, PWSTR, w},
};

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub device: Vec<u16>,
    pub profile: Option<PathBuf>,
    pub modified: Option<SystemTime>,
}

// Windows SDK wingdi.h: GET_ADVANCED_COLOR_INFO_2 (request 15). windows-rs
// 0.62 does not expose it yet. Prefer it to distinguish SDR, WCG and HDR on
// newer Windows; older systems reject the request and use the original query.
#[repr(C)]
#[derive(Default)]
struct AdvancedColorInfo2 {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    flags: u32,
    color_encoding: i32,
    bits_per_channel: u32,
    active_color_mode: u32,
}

// A failed query is not evidence that color management is off. In that case use
// a declared compositor color space rather than risk applying the profile twice.
unsafe fn advanced_color(device: &[u16]) -> Option<bool> {
    unsafe {
        for _ in 0..3 {
            let (mut paths_len, mut modes_len) = (0, 0);
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths_len, &mut modes_len)
                .ok()
                .ok()?;
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); paths_len as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); modes_len as usize];
            let result = QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut paths_len,
                paths.as_mut_ptr(),
                &mut modes_len,
                modes.as_mut_ptr(),
                None,
            );
            if result == windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            result.ok().ok()?;
            let mut enabled = None;
            for path in paths.iter().take(paths_len as usize) {
                let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
                source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
                source.header.size = std::mem::size_of_val(&source) as u32;
                source.header.adapterId = path.sourceInfo.adapterId;
                source.header.id = path.sourceInfo.id;
                if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
                    continue;
                }
                let name = &source.viewGdiDeviceName;
                if name.split(|c| *c == 0).next() != device.split(|c| *c == 0).next() {
                    continue;
                }
                let mut modern = AdvancedColorInfo2::default();
                modern.header.r#type = DISPLAYCONFIG_DEVICE_INFO_TYPE(15);
                modern.header.size = std::mem::size_of_val(&modern) as u32;
                modern.header.adapterId = path.targetInfo.adapterId;
                modern.header.id = path.targetInfo.id;
                let result = DisplayConfigGetDeviceInfo(&mut modern.header);
                if result == 0 {
                    enabled = Some(
                        enabled.unwrap_or(false)
                            || modern.active_color_mode != 0
                            || modern.flags & 2 != 0,
                    );
                    continue;
                }
                if result != 87 && result != 50 {
                    return None;
                } // Unsupported on older Windows.
                let mut info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
                info.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
                info.header.size = std::mem::size_of_val(&info) as u32;
                info.header.adapterId = path.targetInfo.adapterId;
                info.header.id = path.targetInfo.id;
                if DisplayConfigGetDeviceInfo(&mut info.header) != 0 {
                    return None;
                }
                // advancedColorEnabled, not merely advancedColorSupported. Cloned
                // outputs are considered managed if any target is managed.
                enabled = Some(enabled.unwrap_or(false) || info.Anonymous.value & 6 != 0);
            }
            return enabled;
        }
        None
    }
}

pub fn monitor(window: &WebviewWindow) -> Result<Monitor, String> {
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    monitor_handle(unsafe {
        MonitorFromWindow(
            windows::Win32::Foundation::HWND(hwnd.0),
            MONITOR_DEFAULTTONEAREST,
        )
    })
}

fn monitor_handle(handle: HMONITOR) -> Result<Monitor, String> {
    unsafe {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of_val(&info) as u32;
        GetMonitorInfoW(handle, &mut info.monitorInfo)
            .ok()
            .map_err(|e| e.to_string())?;
        let device = info.szDevice.to_vec();
        if advanced_color(&device) != Some(false) {
            return Ok(Monitor {
                device,
                profile: None,
                modified: None,
            });
        }
        let dc = CreateDCW(w!("DISPLAY"), PCWSTR(device.as_ptr()), PCWSTR::null(), None);
        if dc.is_invalid() {
            return Err("NATIVE_DISPLAY_PROFILE_INVALID".into());
        }
        let mut len = 0;
        let _ = GetICMProfileW(dc, &mut len, None);
        let mut buffer = vec![0u16; (len as usize).min(32768)];
        let found = len > 0
            && len <= 32768
            && GetICMProfileW(dc, &mut len, Some(PWSTR(buffer.as_mut_ptr()))).as_bool();
        let _ = DeleteDC(dc);
        if !found {
            return Err("NATIVE_DISPLAY_PROFILE_INVALID".into());
        }
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        let profile = PathBuf::from(String::from_utf16_lossy(&buffer[..end]));
        let modified = profile.metadata().and_then(|m| m.modified()).ok();
        Ok(Monitor {
            device,
            profile: Some(profile),
            modified,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modern_display_config_matches_sdk_layout() {
        assert_eq!(std::mem::size_of::<AdvancedColorInfo2>(), 36);
        assert_eq!(
            std::mem::offset_of!(AdvancedColorInfo2, active_color_mode),
            32
        );
    }
    #[test]
    #[ignore = "Reads the live desktop monitor configuration"]
    fn desktop_profile_is_readable() {
        let handle = unsafe { MonitorFromPoint(Default::default(), MONITOR_DEFAULTTOPRIMARY) };
        let monitor = monitor_handle(handle).unwrap();
        println!("Advanced Color active: {:?}", unsafe {
            advanced_color(&monitor.device)
        });
        println!("Monitor profile: {:?}", monitor.profile);
        if let Some(path) = monitor.profile {
            let data = std::fs::read(path).unwrap();
            let profile = moxcms::ColorProfile::new_from_slice(&data).unwrap();
            crate::native_display::MonitorProfile::new(profile).unwrap();
        }
    }
}
