# SlowRAW code map

This is a maintainer's map of the current source tree. It covers hand-maintained
application files and their responsibilities. Read this alongside the
user-facing [root README](../README.md) and the separate [changelog](CHANGELOG.md).
This document describes the current implementation. Build and update commands
are also in [README](../README.md); workflow and documentation ownership belong in
[DEVELOPMENT](DEVELOPMENT.md); tracked bugs belong in [PLANNING](PLANNING.md).

## Export data flow

`useExportSettings.ts` owns session drafts and preset migration. Each format has
its own quality, lossless mode, sample depth and effort record. PNG/TIFF depths
retain their existing fields. The native ExportPreset uses serde flattening to
preserve additional frontend fields through settings saves and reloads.

`ExportPanel.tsx` assembles matching requests for estimates and real exports.
ProPhoto availability is enforced in the UI only: 12+ bits/channel, lossless or
maximum lossy quality, and high effort where the encoder offers it.

`export_processing.rs` chooses render precision before resizing, watermarking and
encoding. JPEG is 8-bit lossy; PNG is 8/16-bit lossless with fast/default/best
compression; TIFF is uncompressed 8/16-bit RGB. WebP uses explicit libwebp modes
and method 0–6. JPEG XL uses 8/16-bit RGBA, explicit modes, ICC and effort 1–10.
Lossy quality 100 retains a nonzero distance. AVIF uses image's 8-bit encoder
with explicit quality and inverse speed/effort 1–10. WebP/AVIF currently use sRGB.
ICC writing is independent of optional camera metadata.

`exif_processing.rs` retains the full EXIF path for JPEG/PNG/WebP.
`export_metadata.rs` independently writes a minimal XMP packet containing the
source's IPTC Digital Source Type and optional author/copyright. It reads
decoder-provided XMP (including compressed PNG text), cached ExifTool fields and
sidecar attribution overrides. Recognized source-type URIs in original bytes
also cover uncompressed C2PA assertions. Source signatures are not copied onto
modified image pixels.

XMP uses JPEG APP1, PNG iTXt, WebP XMP, TIFF tag 700, JPEG XL xml boxes, or an
AVIF MIME metadata item associated with the primary image by cdsc. TIFF
relocates its first IFD; AVIF relocates meta while retaining absolute image
offsets. Required-metadata write failures propagate to the export. This path
runs after optional EXIF handling regardless of the metadata switch.

## Metadata and lens profiles

`exiftool.rs` drives Phil Harvey's official ExifTool through a persistent read-only
`-stay_open` process. Requests use UTF-8 filenames, numbered response markers,
bounded output and a timeout. Windows bundles its Perl runtime; macOS/Linux
require system Perl to compile and run SlowRAW. Desktop installations can use
an explicit executable or PATH.

`src-tauri/.taurignore` excludes ExifTool staging directories and its generated
runtime from development watching, allowing build-time preparation to finish.

Standard EXIF fields take precedence over duplicate preview/XMP fields, and
Composite lens IDs fill missing lens names. `exif_processing.rs` owns sidecar
handling, cached orientation/exposure/date helpers and export writing.
ExifTool supplies all source EXIF reads, including GPS and attribution.
`little_exif` copies full EXIF from supported non-RAW containers and writes EXIF.
The shared metadata/profile
cache holds at most eight folders with 512 entries each, in an ExifTool-specific
cache namespace. Rawler continues to decode RAW sensor data.
SlowRAW targets Windows, macOS and Linux.
`image-hdr` still requires `kamadak-exif` transitively. SlowRAW supplies decoded
pixels, exposure and ISO through `HDRInput::with_image`, bypassing that library's
EXIF reader.

Automatic lens selection is shared by the editor, thumbnail generation and export.
The source metadata cache stores display fields and a typed embedded profile
together with embedded JPEG offsets and lengths, keyed by file size and
modification time. ExifTool reads these in one
request. Numeric correction fields precede `-all` so their unformatted values
win, and binary extraction is limited to explicitly requested DNG opcodes.
Profile construction uses the full response before display strings are truncated.
Automatic selection consumes this cached data without a separate profile read.
Thumbnail loading reads preview bytes from the source at cached offsets and
uses the corresponding cached orientation. Preview pixels stay out of the
metadata cache. Invalid or undersized previews fall back to RAW development.
Standard EXIF ISO takes precedence over internal MakerNote sensitivity and is
also saved under the legacy `PhotographicSensitivity` key. `utils/metadata.ts`
shares the same ISO choice between the metadata panel, toolbar, library labels
and frontend sorting; catalog SQL accepts the current and legacy ISO names.

`embedded_lens.rs` interprets DNG `OpcodeList2` FixVignetteRadial and `OpcodeList3`
WarpRectilinear/WarpFisheye blocks. `lens_metadata/` contains separate mathematical
models for Sony's 16-point tables, Fujifilm's 9/11-point tables, Olympus/OM System
radial/channel coefficients, Panasonic/Leica's distortion polynomial and Nikon's
version-0100 distortion and vignette coefficients. ExifTool performs EXIF/MakerNote decoding;
these modules convert its values into rendering parameters. Nikon uses ExifTool's
decoded coefficient fields, with no separate TIFF reader. The bundled
`resources/exiftool.config` adds Nikon's complete eight-coefficient vignette
series and its count to ExifTool's tag definitions. The persistent process loads
this configuration once; the existing metadata request includes the unformatted
values. The vignette model evaluates the polynomial with an implicit constant
of one and supports profiles with vignette data alone. Unknown versions or
incomplete coefficient arrays are skipped. Metadata and thumbnail cache revisions
invalidate earlier profiles that contained distortion only. Panasonic's model
currently supplies distortion only. Unknown layouts, including Canon proprietary
correction models, fall back to Lensfun when no supported built-in profile exists.

The geometry pass maps crop/orientation coordinates back to the active image
area, applies per-channel warps and vignette gain, and respects the three
correction strengths/switches. Supported embedded profiles take precedence over
Lensfun; unsupported required DNG opcodes lead to fallback. The UI reports the
selected profile's source. Model references and license notices live in
`src-tauri/resources/licenses/vendor-lens.txt`. Synthetic metadata fixtures check
ExifTool extraction and known model values; real-camera validation remains useful.

Neutral built-in tables (zero corrections/unit multipliers) are treated as
absent, including older cached profiles, so Auto can fall back to Lensfun.
Manual selection stores `lensProfileSource`: off, embedded, or lensfun. Embedded
is offered only when a meaningful profile exists. The shared resolver enforces
the source during reload, copy/paste, thumbnail rendering and export; choosing
off clears stale parameters. Crop controls use `utils/lensProfile.ts` to derive
their available corrections from either embedded or Lensfun parameters.

Lensfun automatic matching first accepts unambiguous normalized identities.
After fixed-camera matching it can use fuzzy name matching, requiring matching
optical numbers, a compatible maker, a high similarity score and a clear lead
over the next candidate. Ambiguous or optically different candidates are rejected.
Fixed-lens cameras use the camera's dedicated mount; Ricoh GR II shares the
ricohGR profile. Explicitly identified converter profiles take precedence over
the standard fixed lens. Failed matching clears automatic correction parameters.
The thumbnail renderer revision invalidates results from the previous matcher.

`scripts/test-lens-metadata.mjs` checks camera ISO and frontend capabilities.
The ignored Rust test `supplied_raw_files_use_camera_iso_and_shared_profile_selection`
reads the six developer-provided `testbilder` RAW files with official ExifTool,
checks camera ISO, Ricoh matching and manual source handling, and confirms that
profile selection uses cached metadata without further ExifTool requests.

Catalog scanning reads file metadata outside write transactions and commits up
to 256 rows with `BEGIN IMMEDIATE`, using prepared statements. Short sidecar
refresh batches use the same writer. A scan mutex orders imports/rescans, and
ancestor scan roots absorb selected subfolders. WAL permits concurrent readers;
ordinary metadata writes can run between batches. Rescans refresh existing rows
in place, and stale-row removal follows a completed directory walk. An interrupted
scan preserves committed batches and reports its error. Full catalog rebuilds
and 60,000-file/concurrent-writer behavior have focused regression coverage.

## Runtime diagnostics

Tailwind scans `src/` explicitly via `source('./')` in `src/styles.css`.
Vite excludes `.rrdata`, `.rrexif` and `.xmp` from development file watching.
Photo metadata is therefore independent of CSS source scanning: Tailwind's
Vite 8 integration would otherwise treat scanned sidecars as asset modules
and request a full page reload after autosave. `scripts/test-dev-sidecars.mjs`
reproduces this with an isolated Vite/Tailwind server and verifies that UI
classes are generated while sidecar saves cannot reload the editor.

The application uses the SlowRAW product/executable name. The internal
`io.github.arvid-storck.SlowRAW` identifier owns its application-data directory.
Startup copies legacy settings, albums and catalog data into the new
directory once, preserving existing destination files. SQLite catalogs are
snapshotted with their committed WAL data. Portable builds keep their own data
directory and `rapidraw.db` path. Desktop icons are generated from root `icon.png`.

`logging.rs` sends native debug diagnostics to stderr. `RUST_LOG` selects a single
log level. Release builds, including portable distributions, use
`release_max_level_off` to compile out native log calls. `main.tsx` assigns silent
console methods in the production frontend.

Run `node scripts/test-logging.mjs` to check production console suppression,
debug console availability and diagnostic configuration.

## Current editing implementation

Export-setting presets and crop aspect-ratio state are stored independently.
CompositionOverlays renders the Rule of Thirds guide. Workspace border radii are
centralized in styles.css, with rounded switch tracks/knobs and slider handles.
Native macOS content/webview layers also use zero radius.

### Pick and Reject flags

Flag actions share selected-image state across the library and editor. Catalog
flag filters combine with metadata filters and smart groups.
Flags are mutually exclusive internal `flag:pick`/`flag:reject` sidecar tags; SQLite
rebuilds the flag column from those tags. Flagging updates sidecar metadata and the catalog index.
`node scripts/test-image-flags.mjs` checks selection scope, queued writes, toggling,
clear, pending-page overlays and error rollback with in-memory stores/IPC only.

### Adjustment panels and curves

Adjustment section/tool order, visibility and collapse state are persisted by
the settings and adjustment-layout components.
Section/tool collapse state and layout are saved in `adjustmentLayout`. Hiding a
control changes its UI visibility; section enable/disable controls govern processing.
Startup migrates saved `adjustmentVisibility` preferences into
the current layout when `adjustmentLayout` is absent.
Curves keeps reset state per channel. Point dragging is bounded by
neighboring Y values; parametric curves and native curve uploads use monotonically
non-decreasing points. Shared slider labels are keyboard-accessible reset controls.

### RAW development and neutral detail controls

Rawler decodes and normalizes sensor samples. Standard RGB Bayer images use the
statically linked upstream librtprocess RCD implementation through the
`src-tauri/rcd` Rust adapter. X-Trans (6x6 CFA), other sensor layouts and Fuji rotated
sensors use Rawler's sensor-specific development, including its demosaicing.
RCD is Bayer-only and runs before Rawler's camera color
calibration and final crop; floating-point scaling preserves highlight headroom.
The adapter builds upstream RCD and border interpolation from a pinned Git commit,
cached under `src-tauri/target/rcd-source`. The first clean build needs Git and network
access. The library is linked into the executable for desktop distribution.

Sharpness and Blur are exclusive 0–100 controls in Details, including mask
adjustments. They share the native signed detail-filter parameter: positive values
sharpen, negative values blur, and zero returns the input color. Dedicated blur
passes for this filter are generated only when an active detail amount requires them.
The base image loader offers optional color noise reduction, defaulting to zero.
Clarity and Structure are separate adjustments, also defaulting to zero.

Run `cargo test --manifest-path src-tauri/Cargo.toml --workspace --lib` for native
and RCD checks, and `node scripts/test-neutral-processing.mjs` for curve/control checks.

### From RAW sensor data to an image on screen

The normal editor loading path is:

```text
RAW file bytes
  -> Rawler: decode sensor samples and camera metadata
  -> black-level subtraction and floating-point normalization
  -> demosaicing: RCD for supported 2x2 RGB Bayer; Rawler for X-Trans/other layouts
  -> Rawler: camera white balance, color calibration and sensor crop
  -> linear RGB (32-bit float per channel), then EXIF orientation
  -> configured Highlight Recovery and optional color noise reduction
  -> in-memory base-image cache
  -> preview rendering: geometry, adjustments, tone mapping and selected output encoding
  -> native GPU surface (Windows/macOS; Wayland/Vulkan on Linux)
```

#### 1. Reading and decoding the sensor data

`image_loader.rs` reads the source through `read_file_mapped`, falling back to a
normal file read when memory mapping fails. Loading runs on a blocking worker;
generation checks discard obsolete loads when the user switches images.

`raw_processing.rs::develop_internal` gives these bytes to Rawler's format-specific
decoder. The result contains sensor samples, the color-filter-array pattern,
black/white levels, camera color matrices, white-balance coefficients and crop
information. EXIF supplies display orientation. For an ordinary Bayer sensor,
each sensor position measures one color component, rather than a complete RGB
pixel. Decoding unpacks these measurements; demosaicing reconstructs the missing
components in the next stage.

#### 2. Normalization and demosaicing

Rawler's `apply_scaling` subtracts sensor black levels and scales samples into
floating-point values. SlowRAW temporarily raises the normalization white level
and compensates after development using the original black/white range. This
keeps highlight values available above nominal white during development.

`demosaic_bayer` selects RCD for single-component, ordinary 2x2 RGB Bayer mosaics
at least 32x32 pixels, excluding Fuji rotated-sensor layouts. The `src-tauri/rcd`
adapter calls upstream librtprocess to estimate the missing RGB components from
neighboring sensor samples. It handles the sensor's actual CFA pattern and scales
its scratch buffer to the library's numeric range before restoring the input units.
Rawler's own rescale/demosaic steps are then skipped because they have already run.
X-Trans uses Rawler's own 6x6-CFA demosaicing, not the RCD adapter. Other
unsupported layouts also use Rawler; already-demosaiced Linear RAW data follows
the Linear RAW branch.

librtprocess is an external source dependency statically linked into SlowRAW.
The adapter passes pixel buffers within the same process. See
[Updating the RCD library](../README.md#updating-the-rcd-library) for its pinned source revision
and update command.

#### 3. Camera color becomes working RGB

The normal calibrated path uses Rawler's camera white-balance coefficients and
camera color matrix to convert sensor-dependent colors to linear working RGB.
These are global color operations: demosaicing has supplied three channels, but
the matrix is still needed to interpret what the camera's red, green and blue
measurements mean. `multi_exposure.rs` handles the special Canon multi-exposure
white-balance case.

Rawler handles the sensor-specific rotation/crop where applicable and the default
image crop. SlowRAW restores the original sensor-range scale, bounds negative
values and limits highlight headroom. The normal editor requests a high limit
of 1000; fast loading uses a limit of 1. The result is an `ImageRgb32F` buffer,
with three `f32` channels per pixel, rotated/flipped according to EXIF orientation.

Rawler's sRGB gamma step is omitted here, so the normal base image remains linear.
The Linear RAW Processing setting controls optional inverse-gamma handling and
whether calibration runs for files already identified as Linear RAW.

#### 4. Base-image processing and caching

Back in `image_loader.rs`, `highlight_recovery.rs` applies the configured Highlight
Recovery setting. Optional RAW color noise reduction runs for non-fast loading
when its amount is above zero; its default is zero. The resulting base image and
EXIF are cached in memory, and `LoadedImage` retains a shared image reference plus
the camera's reference white balance. Here, "original image" in application state
means this developed base image, not the untouched sensor mosaic.

RCD reconstructs color detail; the base loader adds no separate sharpening pass.
Sharpness and Blur default to zero, which bypasses their detail filter. Clarity
and Structure are independent controls and also default to zero. Demosaicing and
later preview resampling still affect the appearance of fine detail, even when
additional sharpening is disabled.

#### 5. Linear RGB becomes a visible preview

`useImageProcessing.ts` requests a preview sized for the current viewport and zoom.
The native image/GPU pipeline applies the current geometry and edits to the
developed base image. Tone mapping and the fixed preview transfer function turn
linear light values into display-ready values. This display conversion is
separate from RAW decoding and demosaicing.

#### 6. Native GPU presentation

Editor previews use a native GPU surface on every desktop platform. On Linux,
the supported editor session is Wayland/Vulkan. gpu_processing.rs creates
WgpuDisplay; the shared high-precision adjustment pipeline writes RGBA16Float
tiles into floating-point working/display textures. The display shader samples
that texture directly. The transparent webview supplies controls and overlays.

lib.rs::process_preview_job requires an available native Vulkan surface. apply_adjustments returns completion/error, not image bytes.
The wgpu-frame-ready event enables the native canvas. Errors travel through the
preview worker as Result values and appear in the editor. update_wgpu_transform
redraws the existing texture for pan/zoom without rerunning RAW development.

wayland_display.rs creates an input-transparent, desynchronized child surface
below GTK’s webview using its existing Wayland connection in guest mode. GTK
retains the parent surface and event queue; the bridge owns its child and
dispatches only its events. GTK allocation and scale determine viewport size
and position, including native-titlebar offsets. If GTK replaces the parent
surface or presentation fails, the editor reports display loss and requests restart.

GPU readback remains available for exports, thumbnails and histogram/waveform
analysis. Crop and before/after helper images and denoise-dialog images retain
their existing encoding; they are separate UI assets, not an alternative editor
preview renderer. preview_encoding.rs owns lossless PNG16/JPEG XL encoding
for auxiliary images; export_processing.rs configures export encoders separately.
Library thumbnails use their own cache.

### Color spaces and ICC conversion

color_management.rs reuses moxcms (already an image dependency) with extended-range
floating-point conversion. Input ICC profiles are read through ImageDecoder;
RGB source images are converted to extended sRGB without discarding negative
components. RAW calibration likewise retains negative RGB components, which can
represent valid colors outside sRGB. Neutral HSL controls bypass their color
clamping. Earlier creative adjustments still have their own gamut constraints.

The output tone/curve stage uses one shared D50 ProPhoto-primary working basis,
independent of the output selection. This is an internal processing basis, not a
display option. The shader converts into the requested output primaries after
tone/curves, applies the matching transfer function, then clips to that output
gamut. Changing this working basis can change the appearance of older edits,
especially saturated colors; sidecar settings and source files are retained.

Built-in output profiles cover sRGB, Adobe RGB (1998), Display P3, SDR Rec. 2020
and ProPhoto RGB. The profile writer uses D50 PCS white and a proper Bradford
white adaptation in chad, rather than a cone-response matrix. JPEG/PNG/TIFF and
JXL embeds the actual output profile; wide outputs use EXIF ColorSpace
65535 rather than incorrectly claiming sRGB. Watermarks are converted to the
export space before blending. WebP/AVIF remain sRGB-only and reject other spaces.
JPEG XL uses explicit 8/16-bit lossy/lossless encoding with an ICC profile and selected effort.

Preview processing has a fixed Rec. 2020 output encoding, independent of the screen
and exports. The common display shader decodes that texture to linear Rec. 2020
before applying the screen transform. It converts the native canvas background
from sRGB separately. The webview still supplies the controls and overlays.

The display-only matrix/TRC approach is inspired by
[vkdt's display color management](https://github.com/hanatos/vkdt/blob/master/doc/howto/colour-display/readme.md).
This is a new Rust/WGSL implementation using the existing moxcms dependency;
vkdt's RAW pipeline, UI and source files are not bundled.

native_display.rs selects only advertised Vulkan surface pairs. Managed presentation
prefers float scRGB, then extended sRGB, 10-bit/float PQ or P3, with SDR sRGB as
the last available transport. Negative/extended scRGB values are preserved.
PQ uses a fixed 80-nit SDR reference white, not HDR highlight expansion.
SDR prefers an advertised 10-bit UNORM format over 8-bit; actual scan-out precision
also depends on the OS and driver.

windows_display.rs associates the main window with its current monitor, queries
active DisplayConfig paths and prefers GET_ADVANCED_COLOR_INFO_2 activeColorMode
(SDR/WCG/HDR), with the older enabled/enforced flags on older Windows.
With Advanced Color active, or an inconclusive query, the app uses compositor-managed
presentation without applying a monitor profile itself. On legacy unmanaged SDR,
GetICMProfileW supplies the monitor profile. The shader uses its RGB matrix and
inverse per-channel TRCs on an SDR UNORM surface; the sRGB Vulkan label does not
mean the app clips those device values to sRGB first. This relies on the legacy
desktop passing device values through without an additional color transform.
Missing/unreadable, LUT-based or invalid profiles report an explicit editor error.
Profile errors retain the display object for retry after correcting the OS profile;
only actual surface loss drops the display and requires restart.

Monitor profiles are cached by device name, profile path and modification time.
Display state is refreshed during rendering (at most once per second), and window
moves/resizes invalidate it. The matrix is a uniform; inverse TRCs use a cached
4096x1 RGBA16Float texture with denser sampling near black. CPU clear colors use
the same curves as the image shader. No image readback or compressed transfer is
introduced for presentation. Windows calibration ramps remain OS-owned.

Wayland and MoltenVK/ColorSync presentation delegate the actual monitor conversion
to the compositor using the selected surface description. If it exposes only sRGB,
the system-managed transport is sRGB; the app does not force unsupported wide modes.
No Linux/macOS physical wide-gamut validation has been performed here. Monitor ICC
conversion is not calibration or soft proofing. Input ICC conversion and
ExportSettings.color_space remain independent; export settings persist in the
session draft and presets.

Run the native color_management, preview_encoding, native_display, color_tests
and export tests for profile round-trips, lossless export/helper encoding,
extended input colors, native display shader transfer/matrix/alpha behavior,
surface selection, monitor TRC round-trips and EXIF/ICC consistency.

### Noise reduction

Details adjustments apply luminance/color noise reduction in the GPU processing
pipeline. The RAW loader optionally reduces chroma noise after demosaicing,
controlled by the preprocessing settings.

`denoising.rs` implements traditional BM3D for the single-image and batch dialog.
Block matching groups similar patches; two collaborative DCT-filtering passes
produce the result. RAW results receive the default CPU RAW rendering before
color conversion. Comparison previews use ICC-tagged Rec.2020 PNG16. Saves reuse
the export encoder: TIFF16 for RAW input, PNG16 for decoded input, with the
Rec.2020 profile embedded. Source images stay at their original locations.

### Guided filtering and export profiles

Guided-filter coefficients are built from log luminance/dark-channel statistics at
bounded resolution, cached with the GPU input, and sampled across render tiles.
Clarity, structure, dehaze and tonal detail recovery use them in preview and export.
Gaussian blur serves sharpening/glow/halation where needed. JPEG/PNG
encoders embed the selected output ICC independently of EXIF preservation.
Thumbnail disk cache keys include the renderer revision; mismatched renders are
regenerated on demand.

### Kelvin white balance and area sampling

White balance uses a `whiteBalance: { temperature, tint }` adjustment
object (or `null` for camera/default balance), including masks. Frontend adjustment
normalization retains this object as the white-balance input. `src/utils/whiteBalance.ts`
defines UI limits and logarithmic Kelvin slider scaling. `Color.tsx` displays Kelvin
and tint; `ImageCanvas.tsx` maps clicked/dragged regions through crop, orientation,
flips and perspective into original-image coordinates.
Temperature and tint use native `f32` storage. Frontend values are rounded to the
same precision with `Math.fround`; chromaticity and Bradford calculations use `f64`.
Sidecars represent these values as JSON numbers, whose text format has no bit-depth type.

`src-tauri/src/white_balance.rs` converts camera matrices/neutral coefficients into
as-shot Kelvin/tint and computes Bradford LMS adaptation. Its bounded metadata cache
is keyed by path, modification time and file length. `raw_processing.rs` reads the
camera white from RAW metadata. `LoadedImage` carries the reference
white through preview, thumbnails and export. The `sample_white_balance` command
checks the requested image path, averages linear original pixels in a quadrilateral
on a blocking worker, and caps work at 262144 samples. It rejects invalid/dark samples.
Frontend session/generation guards discard stale picks when switching images/tools.
Run `cargo test --lib white_balance` in `src-tauri` for numerical and area-sampling tests.
Run `node scripts/test-white-balance.mjs` for frontend defaults, limits and slider scaling.

### Brush interactions

`ImageCanvas.tsx` keeps active Clone/Heal stroke points in local refs. A Konva
line previews the unfinished stroke; release commits one sidecar mask update.
Changing the image or active mask cancels an unfinished stroke. Normal Brush/Flow
masks retain live previews. Image processing and thumbnail invalidation follow
the normal adjustment path.

Legacy `quick-selection` sidecar masks load as ordinary Brush masks, retaining
their painted strokes, opacity and combination mode. The frontend normalizes
the old type; native mask dispatch accepts it as a brush for direct sidecar loads.
The resulting bitmap uses the stored brush shape.
`node scripts/test-mask-compat.mjs` checks that frontend normalization preserves
strokes, IDs, visibility, opacity and blend mode while preserving the loaded object.
The native `legacy_brush_tests` test verifies the same bitmap as an ordinary brush.

### Export bit depth and session state

`useExportSettings.ts` stores export options in a session draft shared across panel
mounts. PNG supports 8/16-bit RGB/RGBA output with the selected embedded output profile.
PNG 16-bit uses the GPU's 16-bit render output.
EXIF extraction omits binary fields; readable values pass to the metadata panel.

### Workspace panel layout

`EditorView.tsx` uses the same left/right side-panel layout at every window width.
`Editor.tsx` preserves the image-space viewport center and physical magnification
when the workspace size changes. Crop previews request zoom-dependent resolution
and cache geometry/render output by resolution.

`LibraryView.tsx` and `EditorView.tsx` each own one `PanelVisibilityMenu` within
their central workspace.
`BottomBar.tsx` reserves space to the right of its action icons for this control,
which stays accessible when the bottom bar is hidden. Import is available in the
bottom bar and empty-library prompt.

The library's flex chain uses `min-h-0` and clips the grid to its allotted viewport.
`LibraryGrid.tsx` gives react-window a 100%-height flex viewport. Its ResizeObserver
tracks width for row layout. The bottom bar retains its height and uses a separate
stacking level to remain visible above the thumbnail grid.

### Catalog panels, paging and smart groups

`CatalogPanel.tsx` renders capture-date years/months/days and the recursive album
hierarchy. Groups use persisted `expandedAlbumGroups`; creating a group expands
it and its parent. The saved panel ID `folderTree` is retained only as a workspace
compatibility identifier for `Panel.Catalog`.

`useCatalog.ts`, mounted by App, initializes and loads the catalog independently
of panel visibility. Search is debounced and stale query replies are discarded.
`catalog_query_page` returns bounded rows and a filtered SQL count from one SQLite
read transaction. The shared filter builder ensures the page and count match;
the header shows page X of Y, match count and catalog total. Empty results show
page 1 of 1. When results shrink, an out-of-range page is clamped and reloaded.
Catalog mode owns the page-navigation controls.

Smart-group editing loads saved criteria into a draft. Saving updates the same
group ID in the hierarchy; cancellation discards the draft. Legacy single-value filters are normalized by `normalizeCatalogFilter`;
`updateSmartGroup` updates nested smart groups in their existing positions.

Catalog-folder rescans incrementally check disk changes; internal
refreshes reload index/facet data. The export panel handles GUI export, and external
edit sessions coordinate round trips with other editors.

## Application overview

- SQLite indexes the local photo library.
- Original photos remain on the filesystem; edits are stored separately.
- RAW development and image processing run locally.
- Traditional luminance/color filtering and BM3D provide noise reduction.

## How the parts fit together

```text
src/main.tsx -> src/App.tsx -> views/panels -> hooks and Zustand stores
                                      |
                                      v
                        Tauri invoke / event listeners
                                      |
                                      v
                  src-tauri/src/lib.rs -> Rust modules
                                      |
                       files, sidecars, SQLite, RAW engine
```

The React frontend owns presentation and temporary UI state. Rust owns disk
I/O, image decoding/rendering, export, and the SQLite catalog. Shared command
names and frontend data contracts live in `src/components/ui/AppProperties.tsx`;
Tauri command registration is near the end of `src-tauri/src/lib.rs`.

The catalog database is a rebuildable index. Photos stay at their paths, and
sidecar/XMP files store edits and metadata. The catalog view queries bounded pages.

Thumbnail requests and caches are bounded. Removing a catalog entry is separate
from deleting its source file. File operations synchronize the catalog index.
See [catalog-focused-build.md](catalog-focused-build.md) for query/loading details.

Catalog filters are combined in SQLite, while
`src/utils/catalog.ts` retains the active image's optimistic rating when a new
rating removes it from the filtered page. Opening a video catalog entry launches
the system player.
`catalog_images.exif_json` uses key-sorted serialization through `BTreeMap`.
Schema version 2 stores `is_raw` and `is_edited` as constrained 0/1 integers;
the schema upgrader validates these fields and canonicalizes existing EXIF JSON.
Thumbnail validity is determined by the thumbnail cache files and renderer keys.
RAW development has one path: `raw_processing.rs` uses the linked rawler crate
with the RCD adapter for standard RGB Bayer demosaicing, and hands pixels to
`image_loader.rs` in memory. `highlight_recovery.rs` applies the
user setting after development; Linear RAW Processing is controlled by the
persistent `linear_raw_mode` setting. Clone/Heal strokes live in sidecar mask adjustments and
are composited during image rendering; the source image remains intact.

## Where to edit common features

| Task | Start here | Then check |
| --- | --- | --- |
| Catalog search, filters, dates, albums, smart groups | `src/components/panel/library/CatalogFilterPanel.tsx`, `src/utils/catalog.ts` | `src/store/useLibraryStore.ts`, `src-tauri/src/catalog.rs` |
| Import, copy/move, duplicate policy | `src/components/modals/ImportSettingsModal.tsx`, `src/components/panel/library/ImportChoiceMenu.tsx` | `src/hooks/useFileOperations.ts`, `src-tauri/src/file_management.rs`, `src-tauri/src/catalog.rs` |
| Video import and system-player opening | `src/hooks/useFileOperations.ts`, `src/hooks/useAppNavigation.ts`, `src/utils/mediaTypes.ts` | `src-tauri/src/formats.rs`, `src-tauri/src/catalog.rs`, `src/components/panel/library/LibraryItems.tsx` |
| Thumbnails and large-library performance | `src/hooks/useThumbnails.ts`, `src/components/panel/library/LibraryGrid.tsx` | `src-tauri/src/image_loader.rs`, `src-tauri/src/cache_utils.rs`, `src-tauri/src/catalog.rs` |
| Editor adjustment or preview | `src/components/panel/Editor.tsx`, `src/components/adjustments/` | `src/hooks/useImageProcessing.ts`, `src-tauri/src/image_processing.rs`, `src-tauri/src/gpu_processing.rs` |
| Clone/Heal spot removal | `src/components/panel/right/MasksPanel.tsx`, `src/components/panel/editor/ImageCanvas.tsx` | `src/utils/maskUtils.ts`, `src-tauri/src/spot_removal.rs`, `src-tauri/src/image_loader.rs` |
| Highlight Recovery | `src/components/panel/SettingsPanel.tsx` | `src-tauri/src/app_settings.rs`, `src-tauri/src/image_loader.rs`, `src-tauri/src/highlight_recovery.rs` |
| Noise reduction | `src/components/modals/DenoiseModal.tsx` | `src-tauri/src/denoising.rs` |
| RAW dependency update | `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` | `src-tauri/src/raw_processing.rs`, `src-tauri/src/exif_processing.rs`; rebuild the app |
| Lens profile update | `src/components/panel/SettingsPanel.tsx` | `src-tauri/src/component_updates.rs`, `src-tauri/src/lens_correction.rs` |
| TIFF bit depth and export metadata | `src/components/panel/right/ExportPanel.tsx`, `src/hooks/useExportSettings.ts` | `src-tauri/src/export_processing.rs`, `src-tauri/src/exif_processing.rs` |
| Keyboard shortcuts and Info-panel restoration | `src/hooks/useKeyboardShortcuts.ts` | `src/store/useUIStore.ts`, `src/components/panel/PanelVisibilityMenu.tsx` |
| Zoom and panel visibility | `src/components/panel/SettingsPanel.tsx`, `src/components/panel/PanelVisibilityMenu.tsx` | `src/utils/zoom.ts`, `src/store/useUIStore.ts`, `src-tauri/src/app_settings.rs` |
| Portable data and ZIP | `scripts/build-portable.ps1` | `src-tauri/src/portable_paths.rs`, `src-tauri/tauri.conf.json` |
| UI text | `src/i18n/locales/en.json` | `src/i18n/index.ts`, `i18next.config.ts` |

## Root and build files

| File or directory | Responsibility |
| --- | --- |
| `package.json` / `package-lock.json` | Frontend dependencies and npm commands / locked versions. |
| `index.html` | Vite's HTML entry point. |
| `vite.config.mjs` | Frontend bundling and dev-server configuration; excludes metadata sidecars from watching. |
| `tsconfig.json` | TypeScript compiler settings. |
| `eslint.config.js` | Lint rules. |
| `.prettierrc` / `.prettierignore` | Formatter rules and exclusions. |
| `.gitignore` | Generated/local paths excluded from version control. |
| `i18next.config.ts` | Translation extraction/check configuration. |
| `src-tauri/Cargo.toml` / `Cargo.lock` | Rust dependencies / locked versions, including the linked rawler source revision. |
| `src-tauri/rust-toolchain.toml` | Rust toolchain selection. |
| `src-tauri/build.rs` | Prepares missing ExifTool resources for desktop builds, macOS MoltenVK and Tauri resources. |
| `src-tauri/tauri.conf.json` | Main Tauri app/build configuration. |
| `src-tauri/tauri.linux.conf.json` / `tauri.macos.conf.json` | Platform-specific Tauri overrides. |
| `src-tauri/capabilities/` | Tauri permissions. |
| `src-tauri/icons/` | App icons. |
| `src-tauri/lensfun_db/` | Bundled lens profiles; separately updatable at runtime. |
| `src-tauri/resources/` | Bundled runtime resources, licenses, and native libraries. |
| `src-tauri/resources/exiftool.config` | Additional Nikon vignette tag definitions loaded by the persistent ExifTool process. |
| `data/` | Linux desktop entry and AppStream metadata. |
| `packaging/` | Linux/Flatpak and dependency-source packaging metadata. |

### Scripts and existing docs

| File | Responsibility |
| --- | --- |
| `scripts/build-portable.ps1` | Builds and packages a portable Windows ZIP in a fresh output folder. |
| `scripts/update-exiftool.mjs` | Stages the pinned official Windows or Unix distribution; verifies SHA-256/version and reuses complete local copies. |
| `scripts/update-rcd.mjs` | Updates the pinned upstream librtprocess revision used by the RCD adapter. |
| `scripts/clean.mjs` | Removes explicit generated build paths; `--dependencies` also removes node_modules, with project-boundary and symlink checks. |
| `scripts/test-desktop-workflow.mjs` | Zoom focus/scale, export-session persistence, metadata and branding regression checks. |
| `scripts/test-catalog.mjs` | In-memory catalog paging/count/stale-reply, smart-filter editing and album-group creation checks. |
| `scripts/test-logging.mjs` | Production console suppression, debug console availability and diagnostic configuration checks. |
| `scripts/test-image-flags.mjs` | In-memory flag selection, write queue and rollback regression checks. |
| `scripts/test-white-balance.mjs` | Frontend white-balance defaults, bounds and Kelvin slider scaling checks. |
| `scripts/test-color-workflow.mjs` | Automatic monitor transforms, Vulkan display and independent export-profile checks. |
| `scripts/test-export-settings.mjs` | Format-specific options, preset/session persistence and ProPhoto policy checks. |
| `scripts/test-neutral-processing.mjs` | Neutral detail controls and monotonic curve checks. |
| `scripts/test-mask-compat.mjs` | Normalizes legacy mask types while preserving stored brush data. |
| `scripts/test-lens-metadata.mjs` | Camera ISO selection and available embedded/Lensfun corrections. |
| `scripts/test-dev-sidecars.mjs` | Isolated Vite/Tailwind regression for autosave-triggered page reloads. |
| `scripts/test-exiftool-staging.mjs` | Platform layouts, offline reuse, checksum rejection and staging cleanup. |
| `docs/CODEMAP.md` | Source-file map and technical overview (this document). |
| `docs/catalog-focused-build.md` | Catalog filtering, paging, cache and validation notes. |

## Frontend files (`src/`)

### Entry, state, and navigation

| File | Responsibility |
| --- | --- |
| `main.tsx` | Starts React; disables browser console output in production builds. |
| `App.tsx` | Composes the application shell, views, global panels, events, and interactions. |
| `styles.css` | Global styles, theme variables, square workspace corners and Tailwind source scope restricted to src/. |
| `window/TitleBar.tsx` | Custom window title bar. |
| `store/useEditorStore.ts` | Selected image, adjustments, editor state, undo/copy state. |
| `store/useLibraryStore.ts` | Library images, catalog mode, filters, pages, albums, selection; default sort is capture date ascending, with saved preferences restored at startup. |
| `store/useProcessStore.ts` | Long-running process/progress state. |
| `store/useSettingsStore.ts` | Loaded application settings and settings updates. |
| `store/useUIStore.ts` | Active view, panel layout/visibility, dialogs, UI state; discards unsupported panel identifiers from saved layouts. |
| `components/views/LibraryView.tsx` | Library/catalog layout, grid, bottom bar, filter panel. |
| `components/views/EditorView.tsx` | Editor layout, side panels, toolbar, filmstrip. |
| `components/managers/ImageLoaderManager.tsx` | Mounts the image-loading hook. |
| `components/managers/ImageProcessingManager.tsx` | Mounts the image-processing hook and passes render job refs. |
| `context/ContextMenuContext.tsx` | Context-menu provider and menu actions. |
| `context/TaggingSubMenu.tsx` | Tag actions inside image context menus. |

### Hooks

| File | Responsibility |
| --- | --- |
| `hooks/useAppContextMenus.ts` | App-level context-menu construction. |
| `hooks/useAppInitialization.ts` | Loads settings, workspace and startup state. |
| `hooks/useCatalog.ts` | Catalog initialization, debounced paged loading and sort-page reset independent of panel visibility. |
| `hooks/useAppNavigation.ts` | Switches between library/editor and opens catalog videos in the system player. |
| `hooks/useEditorActions.ts` | Editor commands such as zoom and adjustment actions. |
| `hooks/useExportSettings.ts` | Export option state and defaults. |
| `hooks/useExternalEditSession.ts` | Round-trip editing with another application. |
| `hooks/useFileOperations.ts` | Frontend file, folder, import, rename, and delete actions. |
| `hooks/useImageLoader.ts` | Requests image data and coordinates loading. |
| `hooks/useImageProcessing.ts` | Requests rendered previews/processing from Rust. |
| `hooks/useImageRenderSize.ts` | Computes render dimensions for the editor. |
| `hooks/useKeyboardShortcuts.ts` | Application keyboard bindings. |
| `hooks/useLibraryActions.ts` | Library selection, rating, tag, album, and catalog actions. |
| `hooks/useOsPlatform.ts` | OS detection for UI choices. |
| `hooks/usePopover.ts` | Open state, container reference and outside-click/Escape handling for popover menus. |
| `hooks/useProductivityActions.ts` | Higher-level batch/productivity actions. |
| `hooks/useSortedLibrary.ts` | Non-catalog sorting, grouping, and filtering. Catalog mode is paged in Rust. |
| `hooks/useTauriListeners.ts` | Subscribes to backend events and progress notifications. |
| `hooks/useThumbnails.ts` | Thumbnail request batching and cache coordination. |
| `hooks/useWaveformControls.ts` | Waveform display controls. |

### Library and editor panels

| File | Responsibility |
| --- | --- |
| `components/panel/MainLibrary.tsx` | Main library header and image grid host; sort, display, size and layout selects share theme-aware styling. |
| `components/panel/BottomBar.tsx` | Rating/copy/export controls, zoom, and optional filmstrip. |
| `components/panel/Filmstrip.tsx` | Scrollable editor thumbnail strip. |
| `components/panel/Editor.tsx` | Editor canvas, interactions, render requests, and toolbar integration. |
| `components/panel/SettingsPanel.tsx` | Settings categories and controls, including zoom/update actions. |
| `components/panel/PanelSwitcher.tsx` | Switches active editor side panel. |
| `components/panel/PanelVisibilityMenu.tsx` | Shows/hides top, bottom, side, and filmstrip panels. |
| `components/panel/SidePanelArea.tsx` | Dockable/resizable editor side panels. |
| `components/panel/library/CatalogFilterPanel.tsx` | Multi-select facets, smart-group draft/save controls and catalog-folder management. |
| `components/panel/library/ImportChoiceMenu.tsx` | Import file/folder choice. |
| `components/panel/library/LibraryGrid.tsx` | Image grid/list container and virtualization behavior. |
| `components/panel/library/LibraryItems.tsx` | Individual thumbnail/list item rendering. |
| `components/panel/editor/EditorToolbar.tsx` | Editor top toolbar actions and image information. |
| `components/panel/editor/ExifIcons.tsx` | Reusable EXIF symbols. |
| `components/panel/editor/ImageCanvas.tsx` | Image canvas display and interaction layer. |
| `components/panel/editor/overlays/CompositionOverlays.tsx` | Rule of Thirds SVG guide; accepts only off/thirds modes. |
| `components/panel/editor/Waveform.tsx` | Image waveform visualization. |
| `components/panel/right/ControlsPanel.tsx` | Main adjustment controls. |
| `components/panel/right/CropPanel.tsx` | Crop/geometry requests and automatic or explicit off/embedded/Lensfun source selection. |
| `components/panel/right/ExportPanel.tsx` | Export configuration, including 8/16-bit TIFF, and action. |
| `components/panel/library/CatalogPanel.tsx` | Capture-date tree and recursive albums, groups and smart groups. |
| `components/panel/right/Masks.tsx` | Mask editing UI pieces. |
| `components/panel/right/MasksPanel.tsx` | Mask list and controls. |
| `components/panel/right/MetadataPanel.tsx` | EXIF/file metadata display. |

### Adjustments and dialogs

| File | Responsibility |
| --- | --- |
| `components/adjustments/Basic.tsx` | Basic tone and exposure controls. |
| `components/adjustments/Color.tsx` | Color adjustment controls. |
| `components/adjustments/Curves.tsx` | Tone/color curve controls. |
| `components/adjustments/AdjustmentSubSection.tsx` | Persistent collapsible tool groups; UI-only visibility. |
| `components/panel/right/AdjustmentSectionsSubMenu.tsx` | Drag ordering, visibility and reset for sections/tools. |
| `components/adjustments/Details.tsx` | Sharpening and detail controls. |
| `components/adjustments/Effects.tsx` | Effects and finishing controls. |
| `components/modals/AppModals.tsx` | Mounts application dialogs from UI state. |
| `components/modals/CollageModal.tsx` | Collage creation. |
| `components/modals/ConfirmModal.tsx` | Shared confirmation dialog. |
| `components/modals/CopyPasteSettingsModal.tsx` | Chooses adjustment types to copy/paste. |
| `components/modals/CreateFolderModal.tsx` | Folder creation dialog. |
| `components/modals/CullingModal.tsx` | Culling workflow dialog. |
| `components/modals/DenoiseModal.tsx` | Noise-reduction dialog. |
| `components/modals/FocusStackModal.tsx` | Focus-stacking dialog. |
| `components/modals/HdrModal.tsx` | HDR merge dialog. |
| `components/modals/ImportSettingsModal.tsx` | Import source/destination, copy/move, and folder structure choices. |
| `components/modals/NegativeConversionModal.tsx` | Film negative conversion dialog. |
| `components/modals/PanoramaModal.tsx` | Panorama stitching dialog. |
| `components/modals/RenameFileModal.tsx` | File rename dialog. |
| `components/modals/RenameFolderModal.tsx` | Folder rename dialog. |

### Reusable UI and frontend utilities

| File | Responsibility |
| --- | --- |
| `components/ui/AppProperties.tsx` | Shared frontend types, enums, and Tauri command names. |
| `components/ui/Button.tsx` | Shared button. |
| `components/ui/CollapsibleSection.tsx` | Expandable settings/control section. |
| `components/ui/IndeterminateProgress.tsx` | Animated progress indicator shared by HDR, panorama and denoising dialogs. |
| `components/ui/ColorWheel.tsx` | Color-wheel control. |
| `components/ui/Dropdown.tsx` | Shared dropdown/select control. |
| `components/ui/ExportImportProperties.tsx` | Shared export/import data definitions. |
| `components/ui/ExportPresetsList.tsx` | Export preset list. |
| `components/ui/ExternalEditBar.tsx` | External-edit session banner/actions. |
| `components/ui/GlobalTooltip.tsx` | Shared tooltip display. |
| `components/ui/ImagePicker.tsx` | Image selection control. |
| `components/ui/Input.tsx` | Shared input. |
| `components/ui/Resizer.tsx` | Draggable panel divider. |
| `components/ui/Slider.tsx` | Shared slider. |
| `components/ui/FlagToggles.tsx` | Bottom-bar Pick/Reject toggles for editor/library selections. |
| `utils/imageFlags.ts` | Flag selection, optimistic state and serialized sidecar/index writes. |
| `components/ui/Switch.tsx` | Shared toggle switch. |
| `components/ui/Text.tsx` | Typography wrapper. |
| `types/typography.ts` | Text variants/color/weight types. |
| `utils/adjustments.ts` | Adjustment defaults, color labels, and conversion helpers. |
| `utils/catalog.ts` | Catalog IPC helpers, page loading, refresh, request handling, and preserving the edited image's visible rating when it leaves an active filter. |
| `utils/CollageVariants.tsx` | Collage layout definitions/icons. |
| `utils/cropUtils.ts` | Crop geometry helpers. |
| `utils/imageGrouping.ts` | Grouping related images and expanding group selections. |
| `utils/ImageLRUCache.ts` | Frontend image LRU cache. |
| `utils/keyboardUtils.ts` | Shortcut definitions, parsing, and normalization. |
| `utils/maskUtils.ts` | Mask conversion and helper functions. |
| `utils/mediaTypes.ts` | Recognizes video paths for frontend media dispatch. |
| `utils/metadata.ts` | Shared camera-reported ISO selection with legacy metadata-key fallbacks. |
| `utils/lensProfile.ts` | Derives distortion, chromatic-aberration and vignette availability from embedded or Lensfun parameters. |
| `utils/themes.ts` | Theme definitions. |
| `utils/colorSpaces.ts` | Shared export RGB color-space list. |
| `utils/exportEncoding.ts` | Per-format encoder defaults and UI-only ProPhoto eligibility. |
| `utils/zoom.ts` | Zoom constants and constraints. |
| `utils/whiteBalance.ts` | Kelvin/tint types and limits, default resolution and logarithmic temperature slider scaling. |

### Localization and miscellaneous frontend files

| File | Responsibility |
| --- | --- |
| `i18n/index.ts` | Initializes i18next and registers locale bundles. |
| `i18n/check-runtime.mjs` | Checks translation keys used at runtime. |
| `i18n/update_translations.py` | Updates zoom-click strings in existing locale files. |
| `i18n/locales/en.json` | English source strings. |
| `@types/i18next.d.ts` | TypeScript declarations for translation keys. |

## Rust backend files (`src-tauri/src/`)

| File | Responsibility |
| --- | --- |
| `main.rs` | Native executable entry point; calls `rapidraw_lib::run()`. |
| `lib.rs` | Tauri setup, command implementations/registration and application wiring. |
| `adjustment_utils.rs` | Shared adjustment-processing helpers. |
| `app_settings.rs` | Persistent settings schema, defaults, loading/saving, workspace defaults. |
| `app_state.rs` | Shared native application state and caches. |
| `cache_utils.rs` | Image/render cache keys and cache containers. |
| `catalog.rs` | SQLite schema/index, catalog folders, metadata facets, filtering, albums/smart groups, paged queries. |
| `component_updates.rs` | Downloads Lensfun profile updates. |
| `culling.rs` | Image culling analysis/actions. |
| `denoising.rs` | Traditional noise reduction and standalone save/batch flows. |
| `exif_processing.rs` | Shared stamped metadata/profile cache, sidecar handling, byte-level EXIF helpers and export EXIF writing. |
| `exiftool.rs` | Persistent official ExifTool process, bundled/configured executable lookup, JSON normalization and profile extraction. |
| `embedded_lens.rs` | DNG opcode interpretation, embedded profile geometry/gain and neutral-profile detection. |
| `lens_metadata/mod.rs` | Shared numeric-tag access, radial correction tables and camera-model dispatch. |
| `lens_metadata/sony.rs` | Sony correction tables. |
| `lens_metadata/fujifilm.rs` | Supported Fujifilm 9/11-point correction tables. |
| `lens_metadata/olympus.rs` | Olympus/OM System radial and channel correction coefficients. |
| `lens_metadata/panasonic.rs` | Panasonic/Leica distortion-polynomial inversion. |
| `lens_metadata/nikon.rs` | Nikon version-0100 distortion and vignette polynomial models. |
| `lens_metadata/tests.rs` | Numeric-model and official-ExifTool metadata fixtures. |
| `export_processing.rs` | Export/batch/size-estimate flow, render precision and format encoders. |
| `export_metadata.rs` | Selective source-type/attribution extraction and XMP container writing. |
| `file_management.rs` | Filesystem operations, import/move/copy, sidecars, and path handling. |
| `focus_stacking.rs` | Focus-stack processing. |
| `formats.rs` | Supported image, RAW, and video-format recognition. |
| `gpu_processing.rs` | Shared GPU render pipeline, native editor presentation and readback for exports/analysis. |
| `native_display.rs` | Vulkan transport selection, monitor matrix/TRC validation and display color uniforms. |
| `windows_display.rs` | Window-to-monitor lookup, Advanced Color state and legacy SDR ICC profile discovery. |
| `vulkan_instance.rs` | Vulkan-only instance creation; direct bundled MoltenVK loading on macOS. |
| `wayland_display.rs` | Linux GTK/Wayland child-surface ownership, input transparency, viewport sizing and event dispatch. |
| `shaders/guided.wgsl` | GPU log-statistics, box filtering and guided coefficient passes. |
| `../icc/sRGB-v2-magic.icc`, `../icc/README.md` | Embedded export profile and CC0 attribution. |
| `guided_perspective.rs` | Guided perspective correction. |
| `hdr_deghosting.rs` | HDR input alignment/deghosting. |
| `image_loader.rs` | Image decoding/loading, in-process RAW dispatch, Highlight Recovery, and composite preview support. |
| `highlight_recovery.rs` | Adjustable highlight recovery applied after RAW development. |
| `color_management.rs` | Built-in RGB profiles, extended-range ICC transforms and GPU output matrices. |
| `preview_encoding.rs` | Shared ICC-tagged lossless PNG16/JPEG XL encoding for exports and auxiliary images. |
| `raw_processing.rs` | Rawler decoding/development, Bayer RCD dispatch, Rawler X-Trans path, Linear RAW modes and orientation. |
| `../rcd/` | Rust/C++ adapter and build recipe for the pinned upstream librtprocess RCD library. |
| `white_balance.rs` | Camera white-balance metadata, Kelvin/tint conversion, cached references and Bradford adaptation. |
| `multi_exposure.rs` | Canon in-camera multi-exposure white-balance handling used by RAW development. |
| `image_processing.rs` | Core adjustment pipeline, geometry, color, masks, histogram/waveform data. |
| `launch_request.rs` | File/open and external editor round-trip requests. |
| `lens_blur.rs` | Lens/portrait blur processing. |
| `lens_correction.rs` | Shared profile selection, exact/fixed-camera/guarded fuzzy Lensfun matching and coefficient interpolation. |
| `logging.rs` | Sends debug diagnostics to stderr with the configured log level. |
| `mask_generation.rs` | Mask definitions/generation with legacy type aliases for sidecar compatibility. |
| `negative_conversion.rs` | Film-negative conversion. |
| `panorama_stitching.rs` | Panorama commands and stitching workflow. |
| `panorama_utils/mod.rs` | Panorama utility module exports. |
| `panorama_utils/processing.rs` | Panorama image-processing helpers. |
| `panorama_utils/stitching.rs` | Panorama matching/stitching helpers. |
| `portable_paths.rs` | Portable marker detection and app-data/config paths. |
| `spot_removal.rs` | Non-destructive source-based Clone and Heal compositing for mask strokes. |
| `tagging.rs` | Per-image tag add/remove and sidecar synchronization. |
| `window_customizer.rs` | Window/webview customization, including square macOS content layers. |
| `shaders/blur.wgsl` | GPU blur shader. |
| `shaders/display.wgsl` | GPU display shader. |
| `shaders/flare.wgsl` | GPU flare shader. |
| `shaders/shader.wgsl` | Main GPU image-processing shader. |

### Desktop graphics backends

wgpu defaults are disabled. Only the Vulkan backend is compiled on Windows, Linux
and macOS. vulkan_instance.rs creates the shared Vulkan instance; there is no
backend menu or DX12/native-Metal renderer. Old backend/display-space settings
are ignored when reading saved JSON.

On macOS, vulkan_instance.rs loads the bundled libMoltenVK.dylib directly through
ash, enables HAL-requested extensions plus portability enumeration, and transfers
instance ownership to wgpu-hal. MoltenVK translates the common Vulkan shaders and
commands to Metal. It is a platform runtime, not another application rendering path.
The normal initialization and compute-only export/analysis retry use this factory.

### Clean up tool

```powershell
npm run clean -- --dry-run
npm run clean
```

```powershell
npm run clean:deep -- --dry-run
npm run clean:deep
```

### Updating common libraries

Update one dependency or related group at a time, retain the lockfiles, rebuild
and run the affected checks. Cargo update stays within Cargo.toml constraints;
major-version upgrades also require reviewing the manifest and API changes.

```powershell
npm outdated
npm update
cargo update --manifest-path src-tauri/Cargo.toml -p image
cargo update --manifest-path src-tauri/Cargo.toml -p moxcms
cargo update --manifest-path src-tauri/Cargo.toml -p jxl-encoder -p jxl-oxide
cargo update --manifest-path src-tauri/Cargo.toml -p webp
```

Review frontend/native dependencies together when upgrading Tauri. Lensfun
profiles update separately through Settings and do not require a RAW rebuild.

### Updating the RAW decoder

```powershell
npm run update:rawler
```

This updates the upstream Rawler revision in src-tauri/Cargo.lock. Keep the
lockfile and rebuild the application. Rawler API changes may require adapting
raw_processing.rs and exif_processing.rs. RAW support is linked into the app.

### Updating the RCD library

```powershell
npm run update:rcd
```

This updates src-tauri/rcd/upstream-revision to upstream librtprocess HEAD.
Keep the pinned revision; the next native build fetches and statically rebuilds
it. Run workspace tests after updates. API changes belong in rcd/bridge.cpp.
Rawler remains responsible for RAW decoding and non-Bayer development.

### Compiling the program

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

## Push to github

```powershell
git init
git remote add origin https://github.com/arvid-storck/SlowRAW.git
git fetch origin
git reset --mixed origin/main
git branch -M main
git add -A
git commit -m "Read docs/CHANGELOG.md"
git push -u origin main
```

### Dependency and distribution size

- Application file/process operations run in Rust. `tauri-plugin-fs` arrives transitively through
  the dialog plugin, whose filesystem scope integration is optional.
- Frontend icons use Lucide. Process/file operations use native Rust commands;
  futures crates arrive as transitive dependencies.
- Tokio explicitly requests its multithreaded runtime, synchronization and
  timers. Networking features needed by HTTPS dependencies remain transitive.
- Imageproc enables Rayon for parallel image processing.
  Separate image hashing and supported codecs have their own dependencies.
- Sysinfo enables disk and system/memory queries. Reqwest enables JSON/HTTPS.
- Release uses speed optimization, LTO and symbol stripping, with unwinding
  retained for panics.
- Portable Windows packaging includes the EXE and application resources/licenses. It reports EXE, folder and ZIP sizes.

`npm run clean` removes generated build caches. Export portable output before
cleaning. Both cleanup modes also remove interrupted ExifTool staging directories.
Use a fresh output directory for portable builds.
`npm run clean:deep` also removes `node_modules`, downloaded ExifTool and generated MoltenVK notices. Both commands support `-- --dry-run` to preview cleanup targets.

Package sizes depend on the toolchain, dependency versions and bundled resources.
Use the portable script's size report for the build being distributed.
Interactive UI and real-camera regression checks are separate from automated tests.
