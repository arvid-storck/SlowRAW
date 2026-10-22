import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = (file) => readFileSync(new URL('../' + file, import.meta.url), 'utf8');
const evaluate = (file, globals = {}) => {
  const exports = {};
  vm.runInNewContext(ts.transpile(source(file), { module: ts.ModuleKind.CommonJS }), { exports, ...globals });
  return exports;
};
const { COLOR_SPACES } = evaluate('src/utils/colorSpaces.ts');
assert.equal(COLOR_SPACES.length, 5);
assert.ok(COLOR_SPACES.some((space) => space.value === 'prophoto-rgb'));
const settings = source('src/components/panel/SettingsPanel.tsx');
assert.doesNotMatch(settings, /DISPLAY_COLOR_SPACES|displayColorSpace|processingBackend/);
assert.ok(!settings.includes("value: 'native'"));
for (const file of [
  'src/hooks/useImageLoader.ts',
  'src/components/panel/Editor.tsx',
  'src/components/panel/editor/ImageCanvas.tsx',
]) {
  assert.doesNotMatch(source(file), /!appSettings\?\.displayColorSpace/);
}
assert.doesNotMatch(source('src-tauri/src/lib.rs'), /settings\.display_color_space\.is_none\(\)/);
assert.match(source('src-tauri/src/lib.rs'), /display\.prepare\(/);
assert.ok(!source('src/hooks/useImageProcessing.ts').includes('new Blob'));
assert.ok(!source('src-tauri/src/lib.rs').includes('apply_adjustments_fallback'));
assert.ok(!source('src-tauri/src/lib.rs').includes('preview_jxl_probe'));
assert.match(source('src/App.tsx'), /const isWgpuActive[\s\S]*?hasRenderedFirstFrame/);
assert.doesNotMatch(source('src/App.tsx'), /!appSettings\?\.displayColorSpace/);
assert.match(source('src-tauri/src/native_display.rs'), /MonitorProfile/);
assert.ok(source('src-tauri/src/windows_display.rs').includes('advanced_color(&device)'));
assert.match(source('src-tauri/src/shaders/display.wgsl'), /display_curves/);
assert.match(source('src-tauri/src/vulkan_instance.rs'), /Backends::VULKAN/);
console.log('Automatic monitor transform, Vulkan-only display and independent export profile checks passed.');
