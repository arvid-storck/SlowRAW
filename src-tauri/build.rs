fn main() {
    prepare_exiftool();
    // Tauri embeds its application manifest only in binaries. Unit-test harnesses
    // also import TaskDialogIndirect via the dialog plugin and need Common Controls 6.
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
    #[cfg(target_os = "macos")]
    prepare_moltenvk();
    #[cfg(target_os = "windows")]
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
    )
    .expect("Build Windows resources");
    #[cfg(not(target_os = "windows"))]
    tauri_build::build();
}

fn prepare_exiftool() {
    use std::{env, path::PathBuf, process::Command};

    let platform = env::var("CARGO_CFG_TARGET_OS").unwrap();
    assert!(
        matches!(platform.as_str(), "windows" | "macos" | "linux"),
        "SlowRAW supports Windows, macOS and Linux"
    );
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let runtime = manifest.join("resources/exiftool");
    let updater = manifest.join("../scripts/update-exiftool.mjs");
    let launcher = runtime.join(if platform == "windows" {
        "exiftool.exe"
    } else {
        "exiftool"
    });
    let library = runtime.join(if platform == "windows" {
        "exiftool_files/lib/Image/ExifTool.pm"
    } else {
        "lib/Image/ExifTool.pm"
    });
    println!("cargo:rerun-if-changed={}", updater.display());
    for file in [&launcher, &library] {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    if launcher.is_file() && library.is_file() {
        return;
    }

    println!("cargo:warning=Preparing official ExifTool for the {platform} build");
    let mut command = Command::new("node");
    command
        .arg(updater)
        .args(["--ensure", "--platform", &platform]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let status = command
        .status()
        .expect("Run scripts/update-exiftool.mjs (Node.js is required)");
    assert!(
        status.success(),
        "Could not prepare official ExifTool; check the download error above. For offline builds, stage it first with npm run update:exiftool."
    );
}

#[cfg(target_os = "macos")]
fn prepare_moltenvk() {
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };
    println!("cargo:rerun-if-env-changed=SLOWRAW_MOLTENVK_PATH");
    println!("cargo:rerun-if-env-changed=SLOWRAW_MOLTENVK_NOTICES");
    let library = env::var_os("SLOWRAW_MOLTENVK_PATH").map(PathBuf::from)
        .or_else(|| ["/opt/homebrew/opt/molten-vk/lib/libMoltenVK.dylib", "/usr/local/opt/molten-vk/lib/libMoltenVK.dylib"]
            .into_iter().map(PathBuf::from).find(|p| p.is_file()))
        .expect("Install MoltenVK for the target architecture (brew install molten-vk), or set SLOWRAW_MOLTENVK_PATH and SLOWRAW_MOLTENVK_NOTICES.");
    let library = library.canonicalize().expect("Resolve MoltenVK library");
    println!("cargo:rerun-if-changed={}", library.display());
    let architecture = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => panic!("Unsupported macOS architecture"),
    };
    assert!(
        std::process::Command::new("lipo")
            .args(["-verify_arch", architecture])
            .arg(&library)
            .status()
            .expect("Run lipo")
            .success(),
        "MoltenVK does not contain the target architecture"
    );
    let source_root = library
        .parent()
        .and_then(Path::parent)
        .expect("MoltenVK prefix");
    let notices = env::var_os("SLOWRAW_MOLTENVK_NOTICES")
        .map(PathBuf::from)
        .unwrap_or_else(|| source_root.to_path_buf());
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let target = manifest.join("resources/macos");
    let runtime = manifest.join("target/moltenvk");
    fs::create_dir_all(&runtime).expect("Create MoltenVK runtime directory");
    fs::create_dir_all(target.join("licenses")).expect("Create MoltenVK resource directory");
    // Keep the upstream library unmodified. Tauri embeds/signs it in Frameworks.
    fs::copy(&library, runtime.join("libMoltenVK.dylib")).expect("Stage MoltenVK runtime");
    let mut count = 0;
    fn copy_notices(source: &Path, root: &Path, target: &Path, count: &mut usize) {
        for entry in std::fs::read_dir(source).expect("Read MoltenVK notices") {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                copy_notices(&path, root, target, count);
            } else if kind.is_file() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if ["license", "licence", "notice", "copying", "copyright"]
                    .iter()
                    .any(|p| name.starts_with(p))
                {
                    let dest = target.join(path.strip_prefix(root).unwrap());
                    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
                    std::fs::copy(path, dest).unwrap();
                    *count += 1;
                }
            }
        }
    }
    copy_notices(&notices, &notices, &target.join("licenses"), &mut count);
    assert!(
        count > 0,
        "MoltenVK redistribution notices are required; set SLOWRAW_MOLTENVK_NOTICES to the distribution's license directory"
    );
}
