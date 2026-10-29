## Why it exists

I tried to find an DAM + raw editor what was not too complex, was free or one time purchase, had a catalog, without generative AI.

But all i found had atleast one of the following:

- Complex editor
- Obligitory subscription
- Folder based
- AI focus or had an goal of adding generative AI
- Abandend

## What This Version Is

This version focuses on the core photographic workflow:

- **Local SQLite catalog** for large photo libraries
- **Simple RAW editing** with a focused, non-destructive workflow
- **Built-in RAW support**
- **Manual sharpening and blur controls**
- **Separately updateable Lensfun profiles**
- **No AI image processing**
- **No cloud-based photo processing**
- **Original files remain under the user's control**

The catalog is an index of the user's files, not a replacement for them. Source photos remain at their normal filesystem paths and catalog data can be rebuilt.

The goal of this version is not to continually add more functionality. The core feature set is intentionally focused, with development concentrated on stability, performance, RAW compatibility, catalog behavior, and the everyday photographic workflow.

## Warning

### Increase of noise and softness

Due to the lack of any pre sharpening and noise reduction, images might appear softer and nosier than other raw developers. But all the detail is still there in the image! But to get comparable results you might have to apply more sharpening and noise reduction then you are used to.

This is intentional to get as close to the raw image as possible, giving the user better control to use better external tools if they wish to do so.

### HDR issue

[vkdt's display documentation](https://github.com/hanatos/vkdt/blob/master/doc/howto/colour-display/readme.md#high-dynamic-range)
reports incorrect image appearance with HDR10/PQ framebuffers on Windows and
therefore disables HDR framebuffers by default on all platforms.

## Origin & Attribution

This project is based on RapidRAW v1.6.3 by CyberTimon (Timon Käch):

https://github.com/CyberTimon/RapidRAW/tree/v1.6.3

The original RapidRAW project provides the foundation for this version.

This version has been modified substantially, especially in the following areas:

* local SQLite catalog architecture
* large-library performance
* catalog filtering, sorting, paging, albums, and smart groups
* import and catalog workflows
* RAW compatibility improvements
* separate Lensfun updates
* desktop-focused build
* local photo processing
* stability and performance improvements

Development and maintenance of these modifications have been made by ChatGPT. I dislike that i had to use ChatGPT but i did not have the knowledge or expetice to do this myself. The changes i did from the original projekt is vibecoded, i hate that. But i decided the direction and scope of the projekt. 

I would have prefered to use someone else not vibecoded projekt or purchase one instead.

See the upstream repository for the original project, its contributors, and its complete historical development record.

## The Idea

This version grew from a simple goal: combine a straightforward RAW editor with a real local catalog that remains practical as a photo library grows.

The original RapidRAW project provided a strong base for the editor and UI. This version adds a stronger catalog-oriented workflow for large local collections.

The catalog was introduced because large libraries can become difficult to manage efficiently when the application relies mainly on filesystem browsing. A database-backed catalog makes filtering, sorting, metadata queries, albums, and large-library navigation much more practical.

The result is intended to stay simple.

## RAW Compatibility and Lens Profiles

Camera support comes from [Rawler](https://github.com/dnglab/dnglab/tree/main/rawler).
New RAW decoder support is delivered with an application update. Lensfun lens
profiles can be updated separately from the application.

## Supported Formats, Lenses & Languages
<details>
<summary><strong>RAW Camera Support</strong></summary>

SlowRAW relies on [dnglab / rawler](https://github.com/dnglab/dnglab/tree/main/rawler) for RAW file decoding. See the **[dnglab Supported Cameras List](https://github.com/dnglab/dnglab/blob/main/SUPPORTED_CAMERAS.md)** for specific camera models.

</details>

<details>
<summary><strong>Lens Correction Support</strong></summary>

SlowRAW reads camera and lens metadata with Phil Harvey's official **[ExifTool](https://exiftool.org/)**. Automatic correction reuses that metadata and prefers supported built-in DNG, Sony, Fujifilm, Olympus/OM System, Panasonic/Leica and Nikon profiles, with **[Lensfun](https://lensfun.github.io/)** as a fallback. Available corrections depend on the camera's data and supported model; the lens panel shows the profile source. Fixed-lens cameras are matched through their camera mount and profile. See [CODEMAP](docs/CODEMAP.md) for model coverage and limitations.

Lens profiles can be updated independently from the main application.

> **Note:** If a required lens profile is missing, check the **[Lensfun repository](https://github.com/lensfun/lensfun/issues)**.

</details>

<details>
<summary><strong>Supported Languages</strong></summary>

SlowRAW currently includes translations for:

* **English**

</details>

## Getting Started

### Download the Latest Release

Download the latest build from this repository's [Releases](../../releases/latest) page.

See [System Requirements](#system-requirements) for supported desktop platforms.

## System Requirements

Editing requires a supported native GPU and a current graphics driver.

| Platform | Requirements | Graphics backend |
| --- | --- | --- |
| Windows | Windows 10 22H2 or Windows 11, 64-bit; Microsoft Edge WebView2 runtime | Vulkan |
| macOS | macOS 13 Ventura or newer; Metal-capable GPU | Vulkan through bundled MoltenVK |
| Linux | Modern 64-bit distribution with Wayland, Vulkan drivers, GTK 3 and WebKitGTK 4.1 | Vulkan |

## Maintainer Documentation

Build and library-update help is below. Read [DEVELOPMENT](docs/DEVELOPMENT.md)
for contributor workflow and documentation responsibilities,
[CODEMAP](docs/CODEMAP.md) for implementation details,
[CHANGELOG](docs/CHANGELOG.md) for completed changes and
[PLANNING](docs/PLANNING.md) for bugs awaiting developer verification.

## Compiling

You'll need to have [Rust](https://www.rust-lang.org/tools/install) and [Node.js](https://nodejs.org/) installed.

### MacOS

Read the [Taure prerequsites](https://v2.tauri.app/start/prerequisites/).

```bash
# Install molten-vk
brew install molten-vk

# Install Perl
brew install perl
```

### Linux

Read the [Taure prerequsites](https://v2.tauri.app/start/prerequisites/) for your distribution. 

```bash
# Install Perl (Debian / Ubuntu)
sudo apt install perl
```

### For all OS

```bash
# 1. Clone the repository
git clone https://github.com/arvid-storck/SlowRAW.git
cd SlowRAW

# 2. Install frontend dependencies
npm install

# 3. Build and run the application
npm start
```

To build the release build:

```bash
# 1. Make the release build
npm run tauri build

# 2. Run the release build
./src-tauri/target/release/SlowRAW
```

### Windows alternative for portable build

```powershell
./scripts/build-portable.ps1 -OutputDirectory portable-artifacts
```

## Developer tools

### Clean up tool

```powershell
npm run clean -- --dry-run
npm run clean
```

```powershell
npm run clean:deep -- --dry-run
npm run clean:deep
```

Both commands remove interrupted ExifTool downloads. `clean:deep` also removes downloaded ExifTool.

### Updating the RAW decoder

```powershell
npm run update:rawler
```

This updates the upstream Rawler revision in src-tauri/Cargo.lock. Keep the
lockfile and rebuild the application. Rawler API changes may require adapting
raw_processing.rs and exif_processing.rs. RAW support is linked into the app.

### RAW Format Issues

If a camera RAW file is unsupported, first check whether it is supported by [dnglab / rawler](https://github.com/dnglab/dnglab/tree/main/rawler).

If support does not exist there, the appropriate upstream RAW-decoder issue is the normal starting point.

## Special Thanks

This project is based on and builds upon the following open-source projects:

* **[RapidRAW](https://github.com/CyberTimon/RapidRAW):** Original application and foundation.
* **[rawler](https://github.com/dnglab/dnglab/tree/main/rawler):** RAW decoding infrastructure.
* **[librtprocess](https://github.com/CarVac/librtprocess):** RCD Bayer demosaicing and border interpolation.
* **[lensfun](https://lensfun.github.io/):** Lens-correction library and profile database.
* **[NegPy](https://github.com/marcinz606/NegPy):** Inspiration for film-negative conversion.
* **[pixls.us](https://discuss.pixls.us/):** Open-source photography community and technical reference.
* **[darktable](https://github.com/darktable-org/darktable):** Reference implementations for parts of the image-processing workflow.
* **[vldk](https://github.com/hanatos/vkdt):** Refrence image to display method.

## License & Philosophy

This project is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.

See the [LICENSE](LICENSE) file for the complete license text.

This repository is based on RapidRAW by CyberTimon and contains substantial modifications.

If there is any issues with a lack of license or its done wrong, please send a message and i will correct it.
