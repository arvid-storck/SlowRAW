import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import ts from 'typescript';

function compile(
  path,
  require = () => {
    throw new Error('Unexpected import');
  },
) {
  const source = readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
  const code = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
  const exports = {};
  runInNewContext(code, { exports, require });
  return exports;
}

const { defaultFormatEncoding, allowsProPhoto } = compile('src/utils/exportEncoding.ts');
for (const format of ['jpeg', 'png', 'tiff', 'webp', 'jxl', 'avif']) {
  const encoding = defaultFormatEncoding(format);
  assert.equal(encoding.lossless, ['png', 'tiff'].includes(format));
  assert.equal(encoding.quality, 90);
  assert.equal(allowsProPhoto(format, 8, { ...encoding, lossless: true, effort: 10 }), false);
}
for (const quality of [1, 90, 99, 100]) {
  for (const effort of [1, 6, 7, 10]) {
    for (const lossless of [false, true]) {
      const encoding = { ...defaultFormatEncoding('jxl'), quality, effort, lossless };
      assert.equal(allowsProPhoto('jxl', 16, encoding), effort >= 7 && (lossless || quality === 100));
    }
  }
}
assert.equal(allowsProPhoto('tiff', 16, defaultFormatEncoding('tiff')), true);
assert.equal(allowsProPhoto('png', 16, { ...defaultFormatEncoding('png'), effort: 6 }), false);
assert.equal(allowsProPhoto('png', 16, defaultFormatEncoding('png')), true);

const slots = [];
let cursor = 0;
const react = {
  useState(initial) {
    const index = cursor++;
    if (!(index in slots)) slots[index] = typeof initial === 'function' ? initial() : initial;
    return [
      slots[index],
      (next) => {
        slots[index] = typeof next === 'function' ? next(slots[index]) : next;
      },
    ];
  },
  useMemo: (fn) => fn(),
  useCallback: (fn) => fn,
};
const { useExportSettings } = compile('src/hooks/useExportSettings.ts', (name) => {
  if (name === 'react') return react;
  if (name.endsWith('exportEncoding')) return { defaultFormatEncoding };
  if (name.endsWith('colorSpaces')) return { COLOR_SPACES: [{ value: 'srgb' }, { value: 'prophoto-rgb' }] };
  if (name.endsWith('ExportImportProperties'))
    return {
      WatermarkAnchor: { BottomRight: 'bottomRight' },
      FILE_FORMATS: ['jpeg', 'png', 'tiff', 'webp', 'jxl', 'avif'].map((id) => ({ id })),
    };
  throw new Error(name);
});
const render = () => {
  cursor = 0;
  return useExportSettings();
};
let settings = render();
settings.setFileFormat('webp');
settings = render();
settings.setEncoding({ quality: 73, lossless: true, effort: 6 });
settings.setKeepAuthor(true);
settings.setFileFormat('jxl');
settings = render();
assert.equal(settings.encoding.lossless, false);
assert.equal(settings.encoding.quality, 90);
settings.setEncoding({ quality: 100, lossless: false, bitDepth: 16, effort: 9 });
settings.setFileFormat('webp');
settings = render();
assert.equal(settings.encoding.quality, 73);
assert.equal(settings.encoding.lossless, true);
const preset = JSON.parse(JSON.stringify(settings.currentSettingsObject));
settings.handleApplyPreset({ ...preset, fileFormat: 'jxl' });
settings = render();
assert.equal(settings.encoding.lossless, false);
assert.equal(settings.encoding.quality, 100);
assert.equal(settings.encoding.effort, 9);
assert.equal(settings.keepAuthor, true);
settings.handleApplyPreset({ ...preset, formatEncodings: undefined, fileFormat: 'jxl', jpegQuality: 100 });
settings = render();
assert.equal(settings.encoding.lossless, true); // migrate old implicit lossless preset
assert.equal(settings.encoding.bitDepth, 16);
slots.length = 0; // new panel mount restores the session draft
settings = render();
assert.equal(settings.fileFormat, 'jxl');
assert.equal(settings.encoding.lossless, true);
console.log('Export settings: format isolation, presets, session restoration and ProPhoto policy passed.');
