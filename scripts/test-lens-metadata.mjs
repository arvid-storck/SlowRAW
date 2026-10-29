import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

function evaluate(file) {
  const exports = {};
  const source = readFileSync(new URL('../' + file, import.meta.url), 'utf8');
  vm.runInNewContext(ts.transpile(source, { module: ts.ModuleKind.CommonJS }), { exports });
  return exports;
}
const { cameraIso } = evaluate('src/utils/metadata.ts');
assert.equal(cameraIso({ ISO: '4000', PhotographicSensitivity: '3805.46', ISOSpeed: '64' }), '4000');
assert.equal(cameraIso({ PhotographicSensitivity: '2200' }), '2200');
assert.equal(cameraIso({ ISO: '65535', RecommendedExposureIndex: '102400' }), '102400');
assert.equal(cameraIso({ ISO: '0', ISOSpeedRatings: '800' }), '800');
assert.equal(cameraIso({ ISO: 'bad' }), undefined);

const { lensCapabilities } = evaluate('src/utils/lensProfile.ts');
const check = (params, expected) => {
  const actual = lensCapabilities(params);
  for (const key of ['distortion', 'tca', 'vignetting']) assert.equal(actual[key], expected.includes(key), key);
};
check(null, []);
check({ k1: 0.1, tca_vr: 1, tca_vb: 1 }, ['distortion']);
check({ embedded: { radial: { distortion: [1, 1.1], red: [1, 1.001], blue: [1, 1], gain: [1, 1.2] } } }, [
  'distortion',
  'tca',
  'vignetting',
]);
check({ embedded: { radial: { distortion: [1, 1], red: [1, 1], blue: [1, 1], gain: [1, 1] } } }, []);
check({ embedded: { warp: { coefficients: [[1, 0, 0, 0, 0, 0]] } } }, []);
check(
  {
    embedded: {
      warp: {
        coefficients: [
          [1, 0.1, 0, 0, 0, 0],
          [1, 0.2, 0, 0, 0, 0],
          [1, 0.3, 0, 0, 0, 0],
        ],
      },
    },
  },
  ['distortion', 'tca'],
);
check({ embedded: { vignette: { coefficients: [0.1, 0, 0, 0, 0] } } }, ['vignetting']);
console.log('Camera ISO and Lensfun/built-in correction capabilities verified.');
