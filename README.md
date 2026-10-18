[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge\&logo=rust\&logoColor=white)](https://www.rust-lang.org/)
[![wgpu](https://img.shields.io/badge/wgpu-%23282C34.svg?style=for-the-badge\&logo=webgpu\&logoColor=white)](https://wgpu.rs/)
[![React](https://img.shields.io/badge/react-%2320232a.svg?style=for-the-badge\&logo=react\&logoColor=%2361DAFB)](https://react.dev/)
[![Tauri](https://img.shields.io/badge/Tauri-24C8DB?style=for-the-badge\&logo=tauri\&logoColor=white)](https://tauri.app/)
[![AGPL-3.0](https://img.shields.io/badge/License-AGPL_v3-blue.svg?style=for-the-badge)](https://opensource.org/licenses/AGPL-3.0)

</div>

## Why it exists

I tried to find an DAM + raw editor what was not too complex, was free or one time purchase, had a catalog, without generative AI.

But all i found had atleast one of the following:

- Complex editor
- Obligitory subscription
- Folder based
- AI focus or had an goal of adding generative AI
- Abandend

## What This Version Is

Author: Arvid Storck, chatgpt.

This version focuses on the core photographic workflow:

- **Local SQLite catalog** for large photo libraries
- **Simple RAW editing** with a focused, non-destructive workflow
- **Local processing** of photos, metadata, thumbnails, and exports
- **Built-in RAW engine** updated through the source dependency and application rebuild
- **RCD Bayer demosaicing** with manual sharpening and blur controls
- **Separately updateable Lensfun profiles**
- **Local AI/ML for traditional processing**, such as noise reduction
- **No generative AI image editing**
- **No cloud-based photo processing**
- **Original files remain under the user's control**

The catalog is an index of the user's files, not a replacement for them. Source photos remain at their normal filesystem paths and catalog data can be rebuilt.

The goal of this version is not to continually add more functionality. The core feature set is intentionally focused, with development concentrated on stability, performance, RAW compatibility, catalog behavior, and the everyday photographic workflow.

## Origin & Attribution

This project is based on RapidRAW v1.6.3 by CyberTimon (Timon Käch):

https://github.com/CyberTimon/RapidRAW/tree/v1.6.3

The original RapidRAW project established the main React, Tauri, Rust, WGPU, RAW-processing, and editing architecture on which this version is based.

This version has been modified substantially, especially in the following areas:

* local SQLite catalog architecture
* large-library performance
* catalog filtering, sorting, paging, albums, and smart groups
* import and catalog workflows
* separate RAW-engine updates
* separate Lensfun updates
* desktop-focused build
* local photo processing
* stability and performance improvements

Development and maintenance of these modifications have been made by ChatGPT. I dislike that i had to use ChatGPT but i did not have the knowledge or expetice to do this myself. The changes i did from the original projekt is vibecoded, i hate that. But i decided the direction and scope of the projekt. 

I would have prefered to use someone else not vibecoded projekt or purchase one instead.

See the upstream repository for the original project, its contributors, and its complete historical development record.

## For whom is this for?

This version of SlowRAW is for photographers who want a <strong>clean, fast, local workflow</strong> with both a real catalog and a straightforward RAW editor.

It is particularly focused on large photo libraries where folder-based browsing alone becomes cumbersome.

The application keeps the original files on normal local storage while using a database-backed catalog for browsing, filtering, ratings, tags, albums, metadata, and large-library navigation.

The editing workflow is intentionally focused rather than being expanded into a cloud or generative-AI platform.

## The Idea

This version grew from a simple goal: combine a straightforward RAW editor with a real local catalog that remains practical as a photo library grows.

The original RapidRAW project provided a strong base for the editor and UI. This version adds a stronger catalog-oriented workflow for large local collections.

The catalog was introduced because large libraries can become difficult to manage efficiently when the application relies mainly on filesystem browsing. A database-backed catalog makes filtering, sorting, metadata queries, albums, and large-library navigation much more practical.

The result is intended to stay simple.

## RAW Engine & Lens Profiles

### Built-in RAW Engine

RAW development uses [dnglab / rawler](https://github.com/dnglab/dnglab/tree/main/rawler) linked into SlowRAW. RAW files work immediately after installation; no separate engine, toolchain download, or temporary image file is required at runtime. Linear RAW Processing is available in Settings.

Standard RGB Bayer images use RCD demosaicing from [librtprocess](https://github.com/CarVac/librtprocess). Sharpening is applied manually in Details; Sharpness and Blur each range from 0 to 100 and reset the other control when changed.

To update the separately pinned RCD library before rebuilding:

```powershell
npm run update:rcd
```

The first clean source build requires Git, a C++ compiler and network access to fetch the library. Distributed builds include the compiled library.

Before compiling, update Rawler and kamadak-exif together from the project root:

```powershell
npm run update:metadata
```

This updates the source dependencies within the Cargo.toml requirements and records them in Cargo.lock. Compile SlowRAW normally afterward. See [docs/README.md](docs/README.md#updating-raw-and-exif-dependencies-together) for integration details and checks.

### Separately Updateable Lens Profiles

Lens correction uses [Lensfun](https://lensfun.github.io/).

Lens profiles can be updated separately from the main application so newer camera and lens profiles do not require a complete application update.

Supported corrections include:

* distortion
* transverse chromatic aberration (TCA)
* vignetting

## Supported Formats, Lenses & Languages
<details>

SlowRAW relies on [dnglab / rawler](https://github.com/dnglab/dnglab/tree/main/rawler) for RAW file decoding. See the **[dnglab Supported Cameras List](https://github.com/dnglab/dnglab/blob/main/SUPPORTED_CAMERAS.md)** for specific camera models.

</details>

<details>
<summary><strong>Lens Correction Support</strong></summary>

SlowRAW supports automatic lens profile detection, distortion, transverse chromatic aberration (TCA), and vignetting correction through **[Lensfun](https://lensfun.github.io/)**.

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

Supported desktop platforms:

* **Windows**
* **macOS**
* **Linux**

### Build from Source

You'll need [Rust](https://www.rust-lang.org/tools/install) and [Node.js](https://nodejs.org/).

```bash
# 1. Clone the repository
git clone <YOUR-REPOSITORY-URL>
cd <YOUR-REPOSITORY-DIRECTORY>

# 2. Install frontend dependencies
npm ci

# 3. Start the application
npm start
```

To build a release version:

```bash
npm run tauri build
```

The release binary is placed under:

```text
src-tauri/target/release/
```

On Windows, use the corresponding `.exe`.

### Validation

Before shipping changes:

```bash
npm run typecheck
npm run build
npm run lint
npm run i18n:check
```

For Rust changes:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Catalog changes should also run the relevant catalog regression scripts documented in the [maintainer code map](docs/README.md).

### Cleaning Build Output

```bash
npm run clean
```

This removes generated frontend and Rust build output.

To preview cleanup:

```bash
npm run clean -- --dry-run
```

To also remove `node_modules` and downloaded ONNX runtime libraries:

```bash
npm run clean:deep
```

Afterwards, run `npm ci` again before building.
The next native build downloads its ONNX runtime again, so it requires network access.
Lens profiles, other resources, reference folders and portable distributions are not
cleanup targets. Preview deep cleanup with `npm run clean:deep -- --dry-run`.

## Portable Windows Build

Create a portable Windows ZIP with:

```powershell
./scripts/build-portable.ps1 -OutputDirectory portable-artifacts
```

## Catalog

Browse, search and filter your local photos, and organize them in albums and smart groups. Original files remain at their existing locations. Removing an entry from the catalog does not delete its source file, and the internal index can be rebuilt.

## Non-Destructive Editing

Editing operations do not overwrite the original source image.

Adjustment information is stored in `.rrdata` sidecar files.

Clone and Heal are also non-destructive and use adjustment/mask data rather than modifying the original source image.

This allows the original image to remain independent of the editing state.

## System Requirements

SlowRAW is designed to remain lightweight while making extensive use of GPU acceleration.

### Operating System

* **Windows:** Windows 10 or newer
* **macOS:** macOS 13 (Ventura) or newer
* **Linux:** Ubuntu 22.04+ or a compatible modern distribution

## Maintainer Documentation

See [docs/README.md](docs/README.md) for the code structure, file responsibilities and development guidance.

### RAW Format Issues

If a camera RAW file is unsupported, first check whether it is supported by [dnglab / rawler](https://github.com/dnglab/dnglab/tree/main/rawler).

If support does not exist there, the appropriate upstream RAW-decoder issue is the normal starting point.

## Special Thanks

This project is based on and builds upon the following open-source projects:

* **[RapidRAW](https://github.com/CyberTimon/RapidRAW):** Original application and foundation.
* **[rawler](https://github.com/dnglab/dnglab/tree/main/rawler):** RAW decoding infrastructure.
* **[librtprocess](https://github.com/CarVac/librtprocess):** RCD Bayer demosaicing and border interpolation.
* **[lensfun](https://lensfun.github.io/):** Lens-correction library and profile database.
* **[nind-denoise](https://github.com/trougnouf/nind-denoise):** Local model-based noise reduction.
* **[NegPy](https://github.com/marcinz606/NegPy):** Inspiration for film-negative conversion.
* **[pixls.us](https://discuss.pixls.us/):** Open-source photography community and technical reference.
* **[darktable](https://github.com/darktable-org/darktable):** Reference implementations for parts of the image-processing workflow.

## License & Philosophy

This project is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.

See the [LICENSE](LICENSE) file for the complete license text.

This repository is based on RapidRAW by CyberTimon and contains substantial modifications.

### Update RAW and EXIF support before compiling

```powershell
npm run update:metadata
```

This updates Rawler and kamadak-exif in `src-tauri/Cargo.lock` within the declared
dependency requirements. Compile normally afterward. PNG export supports
8/16-bit color.
