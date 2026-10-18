import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import ts from 'typescript';

const read = (path) => readFileSync(new URL('../' + path, import.meta.url), 'utf8');
const load = (source, modules = {}) => {
  const exports = {};
  runInNewContext(
    ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX } })
      .outputText,
    {
      exports,
      require: (name) => {
        assert.ok(name in modules, name);
        return modules[name];
      },
    },
  );
  return exports;
};
const adjustments = load(read('src/utils/adjustments.ts'), {
  uuid: { v4: () => 'id' },
  '../components/panel/right/Masks': { Mask: { Brush: 'brush' }, SubMaskMode: { Additive: 'additive' } },
});
const { monotonicCurve, normalizeDetailSettings } = adjustments;
const points = [
  { x: 255, y: 200 },
  { x: 0, y: 10 },
  { x: 90, y: 240 },
  { x: 150, y: 20 },
];
const normalized = monotonicCurve(points);
assert.equal(points[0].x, 255, 'normalizing must not modify input');
assert.equal(normalized.at(-1).y, 200, 'retain the white endpoint');
for (let i = 1; i < normalized.length; i++) {
  assert.ok(normalized[i].x > normalized[i - 1].x);
  assert.ok(normalized[i].y >= normalized[i - 1].y);
}
assert.equal(normalizeDetailSettings({ sharpness: 90, blur: 20 }).sharpness, 0);
assert.equal(normalizeDetailSettings({ sharpness: 140 }).sharpness, 100);
assert.equal(normalizeDetailSettings({ sharpness: -30 }).blur, 0, 'no legacy negative-sharpness conversion');

const curves = load(
  read('src/components/adjustments/Curves.tsx') + '\nexport { buildParametricPoints, getCurvePath };',
  {
    react: {},
    'react/jsx-runtime': {},
    'framer-motion': {},
    'lucide-react': {},
    'react-i18next': {},
    '../../utils/adjustments': adjustments,
    '../ui/AppProperties': {},
    '../../context/ContextMenuContext': {},
    '../ui/Text': {},
    '../ui/Slider': {},
    '../../types/typography': {},
  },
);
for (const highlights of [-100, 0, 100]) {
  for (const shadows of [-100, 0, 100]) {
    const result = curves.buildParametricPoints({
      ...adjustments.DEFAULT_PARAMETRIC_CURVE_SETTINGS,
      highlights,
      shadows,
      lights: -highlights,
      darks: -shadows,
    });
    for (let i = 1; i < result.length; i++) assert.ok(result[i].y >= result[i - 1].y);
    assert.ok(!curves.getCurvePath(result).includes('NaN'));
  }
}
const slider = read('src/components/ui/Slider.tsx');
assert.doesNotMatch(slider, /onDoubleClick|DOUBLE_CLICK_THRESHOLD/);
assert.match(slider, /onClick=.*handleReset/);
assert.doesNotMatch(read('src/components/panel/right/MetadataPanel.tsx'), /clipboard|handleCopy/);
assert.doesNotMatch(read('src-tauri/src/image_loader.rs'), /sharpening_amount|raw_preprocessing_sharpening/);
console.log('Passed monotonic curves, neutral detail controls, reset and read-only EXIF checks.');
