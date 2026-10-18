# SlowRAW code map

This is a maintainer's map of the current source tree. It covers hand-maintained
application files and their responsibilities. Read this alongside the
user-facing [root README](../README.md) and the separate [changelog](CHANGELOG.md).
This document describes the current implementation.

## Runtime diagnostics

The application uses the SlowRAW product/executable name. The internal
`io.github.arvid-storck.SlowRAW` identifier owns its application-data directory.
Startup copies legacy settings, albums, models and catalog data into the new
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

Export-setting presets and crop aspect-ratio choices are independent features.
The composition guide is Rule of Thirds; `O` toggles it on/off.

### Pick and Reject flags

`P` picks, `X` rejects, and `U` clears flags on
selected images. The bottom bar has toggles; thumbnails/list/filmstrip show badges
and dim rejects. Catalog flag filters combine with metadata filters and smart groups.
Flags are mutually exclusive internal `flag:pick`/`flag:reject` sidecar tags; SQLite
rebuilds the flag column from those tags. Flagging updates sidecar metadata and the catalog index.
`node scripts/test-image-flags.mjs` checks selection scope, queued writes, toggling,
clear, pending-page overlays and error rollback with in-memory stores/IPC only.

### Adjustment panels and curves

Adjustment panel customization is accessible in Settings → Customization only. Drag handles reorder sections and tool groups; eye buttons hide them.
Section/tool collapse state and layout are saved in `adjustmentLayout`. Hiding a
control changes its UI visibility; section enable/disable controls govern processing.
Startup migrates saved `adjustmentVisibility` preferences into
the current layout when `adjustmentLayout` is absent.
Shift/Alt slows curve dragging to one fifth; inactive channels show colored curves
and parametric reference markers.
The reset button restores the active curve channel. Point dragging is bounded by
neighboring Y values; parametric curves and native curve uploads use monotonically
non-decreasing points. Shared slider labels are keyboard-accessible reset controls.

### RAW development and neutral detail controls

Rawler decodes and normalizes sensor samples. Standard RGB Bayer images use the
statically linked upstream librtprocess RCD implementation through the
`src-tauri/rcd` Rust adapter. Other sensor layouts and Fuji rotated sensors use
Rawler's sensor-specific development. RCD runs before Rawler's camera color
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
  -> RCD demosaicing for supported Bayer sensors
  -> Rawler: camera white balance, color calibration and sensor crop
  -> linear RGB (32-bit float per channel), then EXIF orientation
  -> configured Highlight Recovery and optional color noise reduction
  -> in-memory base-image cache
  -> preview rendering: geometry, adjustments, tone mapping and sRGB encoding
  -> in-memory JPEG preview -> frontend image canvas
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
Other sensor layouts use Rawler's sensor-specific development; already-demosaiced
Linear RAW data follows the Linear RAW branch.

librtprocess is an external source dependency statically linked into SlowRAW.
The adapter passes pixel buffers within the same process. See
[Updating the RCD library](#updating-the-rcd-library) for its pinned source revision
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
developed base image. Tone mapping and the display sRGB transfer function turn
linear light values into display-ready values. This display conversion is
separate from RAW decoding and demosaicing.

The backend returns the rendered preview as JPEG bytes in memory. The frontend
creates a Blob URL and displays it through `ImageCanvas.tsx`; zoomed updates can
use a rendered region-of-interest patch. This JPEG is a display preview, while
the cached floating-point base remains available for subsequent renders and
export. The RAW-to-editor handoff uses memory buffers rather than an intermediate
image file on disk.

### Guided filtering and export profiles

Guided-filter coefficients are built from log luminance/dark-channel statistics at
bounded resolution, cached with the GPU input, and sampled across render tiles.
Clarity, structure, dehaze and tonal detail recovery use them in preview and export.
Gaussian blur serves sharpening/glow/halation where needed. JPEG/PNG
encoders embed the bundled CC0 sRGB profile independently of EXIF preservation.
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
mounts. PNG supports 8/16-bit RGB/RGBA output with an embedded sRGB profile.
PNG 16-bit uses the GPU's 16-bit render output.
The metadata panel shows camera/exposure/file details and a collapsed, independently
scrollable list of additional readable EXIF values. Binary EXIF fields are skipped
during extraction.
Read-only metadata values support normal text selection.

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

Right-click a smart group → Edit filters opens the filter panel and loads its
criteria into a draft. Saving updates that group's ID in the existing hierarchy.
Cancel editing leaves the stored group
unchanged. Legacy single-value filters are normalized by `normalizeCatalogFilter`;
`updateSmartGroup` updates nested smart groups in their existing positions.

Catalog folders → Rescan folders incrementally checks disk changes; internal
refreshes reload index/facet data. The export panel handles GUI export, and external
edit sessions coordinate round trips with other editors.

## Application overview

This is a customized SlowRAW build focused on local photo management and
non-destructive RAW editing.

- SQLite indexes the local photo library.
- Original photos remain on the filesystem; edits are stored separately.
- RAW development and image processing run locally.
- Model-based processing is used for noise reduction.

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
| Noise reduction | `src/components/modals/DenoiseModal.tsx` | `src-tauri/src/denoising.rs`, `src-tauri/src/ai_processing.rs` |
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
| `vite.config.mjs` | Frontend bundling and dev-server configuration. |
| `tsconfig.json` | TypeScript compiler settings. |
| `eslint.config.js` | Lint rules. |
| `.prettierrc` / `.prettierignore` | Formatter rules and exclusions. |
| `.gitignore` | Generated/local paths excluded from version control. |
| `i18next.config.ts` | Translation extraction/check configuration. |
| `src-tauri/Cargo.toml` / `Cargo.lock` | Rust dependencies / locked versions, including the linked rawler source revision. |
| `src-tauri/rust-toolchain.toml` | Rust toolchain selection. |
| `src-tauri/build.rs` | Build-time resource/dependency preparation. |
| `src-tauri/tauri.conf.json` | Main Tauri app/build configuration. |
| `src-tauri/tauri.linux.conf.json` / `tauri.macos.conf.json` | Platform-specific Tauri overrides. |
| `src-tauri/capabilities/` | Tauri permissions. |
| `src-tauri/icons/` | App icons. |
| `src-tauri/lensfun_db/` | Bundled lens profiles; separately updatable at runtime. |
| `src-tauri/resources/` | Bundled runtime resources, licenses, and native libraries. |
| `data/` | Linux desktop entry and AppStream metadata. |
| `packaging/` | Linux/Flatpak and dependency-source packaging metadata. |

### Scripts and existing docs

| File | Responsibility |
| --- | --- |
| `scripts/build-portable.ps1` | Builds and packages a portable Windows ZIP in a fresh output folder. |
| `scripts/clean.mjs` | Removes explicit generated build paths; `--dependencies` also removes node_modules and downloaded ONNX runtime files, with project-boundary and symlink checks. |
| `scripts/test-desktop-workflow.mjs` | Zoom focus/scale, export-session persistence, metadata and branding regression checks. |
| `scripts/test-catalog.mjs` | In-memory catalog paging/count/stale-reply, smart-filter editing and album-group creation checks. |
| `scripts/test-logging.mjs` | Production console suppression, debug console availability and diagnostic configuration checks. |
| `scripts/test-image-flags.mjs` | In-memory flag selection, write queue and rollback regression checks. |
| `scripts/test-white-balance.mjs` | Frontend white-balance defaults, bounds and Kelvin slider scaling checks. |
| `docs/README.md` | Source-file map and technical overview (this document). |
| `docs/catalog-focused-build.md` | Catalog filtering, paging, cache and validation notes. |

## Frontend files (`src/`)

### Entry, state, and navigation

| File | Responsibility |
| --- | --- |
| `main.tsx` | Starts React; disables browser console output in production builds. |
| `App.tsx` | Composes the application shell, views, global panels, events, and interactions. |
| `styles.css` | Global styles and theme-related CSS. |
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
| `components/panel/right/CropPanel.tsx` | Crop, straighten, and geometry controls. |
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
| `components/ui/DepthRangePicker.tsx` | Depth-range selection control. |
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
| `utils/themes.ts` | Theme definitions. |
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
| `ai_processing.rs` | Model-backed noise reduction and remaining compatibility types. |
| `app_settings.rs` | Persistent settings schema, defaults, loading/saving, workspace defaults. |
| `app_state.rs` | Shared native application state and caches. |
| `cache_utils.rs` | Image/render cache keys and cache containers. |
| `catalog.rs` | SQLite schema/index, catalog folders, metadata facets, filtering, albums/smart groups, paged queries. |
| `component_updates.rs` | Downloads Lensfun profile updates. |
| `culling.rs` | Image culling analysis/actions. |
| `denoising.rs` | Noise-reduction algorithms and save/batch flows. |
| `exif_processing.rs` | EXIF extraction and sidecar reading/writing. |
| `export_processing.rs` | Export formats and output encoding, including true 8/16-bit TIFF. |
| `file_management.rs` | Filesystem operations, import/move/copy, sidecars, and path handling. |
| `focus_stacking.rs` | Focus-stack processing. |
| `formats.rs` | Supported image, RAW, and video-format recognition. |
| `gpu_processing.rs` | GPU render pipeline and shader integration. |
| `shaders/guided.wgsl` | GPU log-statistics, box filtering and guided coefficient passes. |
| `../icc/sRGB-v2-magic.icc`, `../icc/README.md` | Embedded export profile and CC0 attribution. |
| `guided_perspective.rs` | Guided perspective correction. |
| `hdr_deghosting.rs` | HDR input alignment/deghosting. |
| `image_loader.rs` | Image decoding/loading, in-process RAW dispatch, Highlight Recovery, and composite preview support. |
| `highlight_recovery.rs` | Adjustable highlight recovery applied after RAW development. |
| `raw_processing.rs` | In-process rawler development, Linear RAW modes, and RAW orientation. |
| `../rcd/` | Rust/C++ adapter and build recipe for the pinned upstream librtprocess RCD library. |
| `white_balance.rs` | Camera white-balance metadata, Kelvin/tint conversion, cached references and Bradford adaptation. |
| `multi_exposure.rs` | Canon in-camera multi-exposure white-balance handling used by RAW development. |
| `image_processing.rs` | Core adjustment pipeline, geometry, color, masks, histogram/waveform data. |
| `launch_request.rs` | File/open and external editor round-trip requests. |
| `lens_blur.rs` | Lens/portrait blur processing. |
| `lens_correction.rs` | Lensfun lookup and optical corrections. |
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
| `tagging_utils/mod.rs` | Tagging helper module exports. |
| `tagging_utils/candidates.rs` | Candidate tag handling helpers. |
| `tagging_utils/hierarchy.rs` | Tag hierarchy helpers. |
| `window_customizer.rs` | Window/webview customization. |
| `shaders/blur.wgsl` | GPU blur shader. |
| `shaders/display.wgsl` | GPU display shader. |
| `shaders/flare.wgsl` | GPU flare shader. |
| `shaders/shader.wgsl` | Main GPU image-processing shader. |

## Updating the RAW decoder and rawler metadata reader

From the project root, updating rawler in the source code only requires:

```powershell
npm run update:rawler
```

`Cargo.toml` points to upstream rawler and `Cargo.lock` records the selected
revision. Keep the updated lockfile. Rebuild SlowRAW normally afterward.
If upstream changes its API, adapt `raw_processing.rs` (development/orientation)
and `exif_processing.rs` (metadata). `image_loader.rs` receives decoded pixels
directly in memory.
`multi_exposure.rs` and `highlight_recovery.rs` provide additional processing.
Linear RAW Processing and Highlight Recovery are persisted in `app_settings.rs`.

## Update kamadak-exif

```powershell
npm run update:kamadak-exif
```

## Updating the RCD library

```powershell
npm run update:rcd
```

The command updates `src-tauri/rcd/upstream-revision` to librtprocess's current
upstream HEAD. Keep this pinned revision with the source. The next native build
fetches that commit and statically rebuilds the RCD library. Run the workspace tests
after updating. An upstream API change is handled in `rcd/bridge.cpp`; Rawler remains
the RAW decoder. The bundled library license is in `resources/licenses/librtprocess.txt`.

## Push to github

```powershell
git init
git remote add origin https://github.com/arvid-storck/SlowRAW.git
git fetch origin
git reset --mixed origin/main
git branch -M main
git add -A
git commit -m "Text här"
git push -u origin main
```

## Development checks

```powershell
npm run typecheck
npm run lint
npm run i18n:check
npm run i18n:lint
node scripts/test-catalog.mjs
node scripts/test-desktop-workflow.mjs
node scripts/test-logging.mjs
node scripts/test-mask-compat.mjs
node scripts/test-image-flags.mjs
node scripts/test-white-balance.mjs
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Run `npm ci` first if dependencies were cleaned. Frontend build, TypeScript,
translation validation and native tests are separate checks.

The optimized debug profile disables incremental application codegen to avoid
stale anonymous LLVM symbols in cross-crate `image_hasher` code when linking the
Windows executable (`LNK2019` followed by `LNK1120`). Dependency artifacts remain
cached. Keep `CARGO_INCREMENTAL=0` when reproducing the linker issue.
Native library tests verify the library's test cases. Run
`cargo build --manifest-path src-tauri/Cargo.toml --bin SlowRAW` to
check debug linking, and `npm run tauri -- build --no-bundle` to verify the
production executable with its embedded frontend.

The native app uses Windows/macOS/Linux icons referenced by Tauri and the
Linux packaging icon.

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
- ORT uses the verified ONNX runtime supplied by `build.rs`; its
  enabled features provide standard-library, ndarray and tracing support.
  The runtime-load regression test checks native library initialization.
- Release uses speed optimization, LTO and symbol stripping, with unwinding
  retained for panics.
- Portable Windows packaging includes the EXE, Windows ONNX runtime and
  application resources/licenses. It reports EXE, folder and ZIP sizes.

`npm run clean` removes generated build caches. Export portable output before
cleaning, and use a fresh output directory for portable builds.
`npm run clean:deep` adds `node_modules` and the explicit runtime files
`src-tauri/resources/onnxruntime.dll`, `libonnxruntime.so` and
`libonnxruntime.dylib` to cleanup. `build.rs` downloads the required platform
runtime when needed. Both commands support `-- --dry-run` to preview cleanup targets.

Package sizes depend on the toolchain, dependency versions and bundled resources.
Use the portable script's size report for the build being distributed.
Interactive UI and real-camera regression checks are separate from automated tests.
