//! One Vulkan backend on every desktop. macOS loads our bundled MoltenVK directly.
pub fn create() -> Result<wgpu::Instance, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = wgpu::Backends::VULKAN;
        Ok(wgpu::Instance::new(desc))
    }
    #[cfg(target_os = "macos")]
    {
        use ash::{khr, vk};
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let directory = executable.parent().ok_or("Missing executable directory")?;
        let bundled = directory.join("../Frameworks/libMoltenVK.dylib");
        let development = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/moltenvk/libMoltenVK.dylib");
        let in_bundle = directory.file_name().is_some_and(|n| n == "MacOS")
            && directory
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|n| n == "Contents");
        let path = if in_bundle { bundled } else { development };
        // SAFETY: ash retains the library; wgpu-hal owns the instance and destroys
        // it before unloading its entry. All requested HAL extensions are enabled.
        unsafe {
            let entry = ash::Entry::load_from(&path)
                .map_err(|e| format!("MoltenVK {}: {e}", path.display()))?;
            let version = entry
                .try_enumerate_instance_version()
                .map_err(|e| e.to_string())?
                .unwrap_or(vk::API_VERSION_1_0)
                .min(vk::API_VERSION_1_3);
            let flags = wgpu::InstanceFlags::empty();
            let extensions =
                wgpu::hal::vulkan::Instance::desired_extensions(&entry, version, flags)
                    .map_err(|e| e.to_string())?;
            if !extensions.contains(&khr::portability_enumeration::NAME) {
                return Err("MoltenVK portability enumeration is unavailable".into());
            }
            let names: Vec<_> = extensions.iter().map(|s| s.as_ptr()).collect();
            let app = vk::ApplicationInfo::default()
                .application_name(c"SlowRAW")
                .api_version(version);
            let raw = entry
                .create_instance(
                    &vk::InstanceCreateInfo::default()
                        .application_info(&app)
                        .enabled_extension_names(&names)
                        .flags(vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR),
                    None,
                )
                .map_err(|e| e.to_string())?;
            let hal = wgpu::hal::vulkan::Instance::from_raw(
                entry,
                raw,
                version,
                0,
                None,
                extensions,
                flags,
                Default::default(),
                false,
                None,
            )
            .map_err(|e| e.to_string())?;
            Ok(wgpu::Instance::from_hal::<wgpu::hal::api::Vulkan>(hal))
        }
    }
}
