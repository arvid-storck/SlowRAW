import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8');
const load = (file, modules = {}) => {
  const exports = {};
  vm.runInNewContext(ts.transpile(read(file), { module: ts.ModuleKind.CommonJS }), {
    exports,
    require: (name) => {
      assert.ok(name in modules, name);
      return modules[name];
    },
  });
  return exports;
};
const { resizeZoomViewport } = load('src/utils/zoom.ts');
const first = { containerWidth: 1000, containerHeight: 700, width: 1000, height: 500, offsetX: 0, offsetY: 100 };
const second = { containerWidth: 600, containerHeight: 500, width: 600, height: 300, offsetX: 0, offsetY: 100 };
const original = { scale: 2, positionX: -200, positionY: -300 };
const resized = resizeZoomViewport(first, second, original, 8);
assert.ok(Math.abs(resized.scale * second.width - original.scale * first.width) < 1e-9);
const restored = resizeZoomViewport(
  second,
  first,
  { scale: resized.scale, positionX: resized.x, positionY: resized.y },
  8,
);
assert.ok(Math.abs(restored.x - original.positionX) < 1e-9);
assert.ok(Math.abs(restored.y - original.positionY) < 1e-9);
assert.equal(restored.scale, original.scale);

let states = [],
  cursor = 0;
const react = {
  useState(initial) {
    const index = cursor++;
    states[index] = typeof initial === 'function' ? initial() : initial;
    return [
      states[index],
      (next) => {
        states[index] = typeof next === 'function' ? next(states[index]) : next;
      },
    ];
  },
  useCallback: (fn) => fn,
  useMemo: (fn) => fn(),
};
const { useExportSettings } = load('src/hooks/useExportSettings.ts', {
  react,
  '../utils/colorSpaces': load('src/utils/colorSpaces.ts'),
  '../utils/exportEncoding': load('src/utils/exportEncoding.ts'),
  '../components/ui/ExportImportProperties': {
    WatermarkAnchor: { BottomRight: 'bottom-right' },
    FILE_FORMATS: [{ id: 'jpeg' }, { id: 'png' }, { id: 'tiff' }],
  },
});
const draft = useExportSettings();
draft.setFileFormat('png');
draft.setPngBitDepth(16);
draft.setColorSpace('prophoto-rgb');
draft.setJpegQuality(72);
draft.setResizeValue(1234);
draft.setFilenameTemplate('session-test');
draft.setStripGps(false);
states = [];
cursor = 0;
const reopened = useExportSettings();
assert.equal(reopened.fileFormat, 'png');
assert.equal(reopened.pngBitDepth, 16);
assert.equal(reopened.colorSpace, 'prophoto-rgb');
assert.equal(reopened.jpegQuality, 72);
assert.equal(reopened.resizeValue, 1234);
assert.equal(reopened.filenameTemplate, 'session-test');
assert.equal(reopened.stripGps, false);
reopened.handleApplyPreset({ ...reopened.currentSettingsObject, fileFormat: 'cube', pngBitDepth: 4 });
states = [];
cursor = 0;
const migrated = useExportSettings();
assert.equal(migrated.fileFormat, 'jpeg');
assert.equal(migrated.pngBitDepth, 8);
assert.doesNotMatch(read('src/App.tsx'), /COMPACT_EDITOR_MAX_WIDTH|useCompactPanels|compactEditorPanel/);
assert.doesNotMatch(read('src/components/panel/right/MetadataPanel.tsx'), /openstreetmap|<iframe/);
assert.match(read('src/components/panel/right/MetadataPanel.tsx'), /<details key=\{selectedImage.path\}/);
assert.match(read('src/hooks/useImageProcessing.ts'), /targetResolution:/);
assert.match(read('src-tauri/src/lib.rs'), /target_dim.hash\(&mut geometry_hasher\)/);
assert.equal(JSON.parse(read('src-tauri/tauri.conf.json')).productName, 'SlowRAW');
const native = read('src-tauri/src/gpu_processing.rs');
const bridge = read('src-tauri/src/wayland_display.rs');
const jobs = read('src-tauri/src/lib.rs');
assert.match(bridge, /from_foreign_display/);
assert.match(bridge, /subsurface.set_desync/);
assert.match(bridge, /subsurface.place_below/);
assert.match(bridge, /set_input_region/);
assert.match(bridge, /scale_factor/);
assert.match(native, /instance\s*\.create_surface\(target\)/);
assert.match(native, /retrying compute-only/);
assert.doesNotMatch(native, /panic!\("Failed to acquire surface texture"\)/);
assert.match(jobs, /NATIVE_DISPLAY_UNAVAILABLE/);
assert.doesNotMatch(jobs, /apply_adjustments_fallback/);
assert.match(jobs, /responder.send\(result\)/);
assert.match(read('src/components/panel/Editor.tsx'), /role="alert"/);
assert.doesNotMatch(read('src/components/panel/SettingsPanel.tsx'), /useWgpuRenderer|linuxGpuOptimization/);
assert.match(read('src/hooks/useImageProcessing.ts'), /wgpu-display-unavailable/);
assert.doesNotMatch(read('src/components/panel/SettingsPanel.tsx'), /disabled=\{osPlatform === 'linux'\}/);
const manifest = read('src-tauri/Cargo.toml');
assert.doesNotMatch(manifest, /features[^\n]*"(gles|dx12|metal)"/);
assert.match(manifest, /features[^\n]*"vulkan"/);
assert.doesNotMatch(
  read('src/components/panel/SettingsPanel.tsx'),
  /filteredBackendOptions|processingBackend|displayColorSpace/,
);
assert.match(read('src-tauri/src/vulkan_instance.rs'), /Backends::VULKAN/);
assert.match(read('src-tauri/src/vulkan_instance.rs'), /libMoltenVK.dylib/);
assert.match(read('src-tauri/tauri.macos.conf.json'), /libMoltenVK.dylib/);
assert.match(read('src-tauri/src/native_display.rs'), /S::ExtendedSrgb/);
assert.match(read('src-tauri/src/native_display.rs'), /S::Bt2100Pq/);
const css = read('src/styles.css');
assert.match(css, /\.switch-round\s*\{\s*border-radius: 9999px !important/);
assert.match(css, /::-webkit-slider-thumb\s*\{[^}]*border-radius: 50% !important/);
assert.match(css, /::-moz-range-thumb\s*\{[^}]*border-radius: 50% !important/);
assert.match(css, /\.macos-window-shell\s*\{\s*border-radius: 0;/);
assert.match(css, /::-webkit-scrollbar-thumb\s*\{[^}]*border-radius: 0;/);
assert.equal((read('src/components/ui/Switch.tsx').match(/switch-round/g) || []).length, 2);
console.log('Passed desktop workflow, native-only display error wiring and rounded-control checks.');
