import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { randomUUID } from 'node:crypto';
import ts from 'typescript';

const source = readFileSync(new URL('../src/utils/spotRemoval.ts', import.meta.url), 'utf8');
const code = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
const pending = [];
const state = { selectedImage: { path: 'photo.raw' } };
const exports = {};
runInNewContext(code, {
  exports,
  crypto: { randomUUID },
  require: (name) => {
    if (name === '@tauri-apps/api/core')
      return {
        invoke: (command, args) => new Promise((resolve, reject) => pending.push({ command, args, resolve, reject })),
      };
    if (name.endsWith('/Masks')) return { Mask: { Heal: 'heal' } };
    if (name.endsWith('/useEditorStore')) return { useEditorStore: { getState: () => state } };
    throw new Error(name);
  },
});
const template = { id: 'empty', type: 'heal', parameters: { lines: [] } };
let adjustments = { rotation: 15, orientationSteps: 1, masks: [{ id: 'container', subMasks: [template] }] };
const commit = (fn) => {
  adjustments = fn(adjustments);
};
const stroke = { points: [{ x: 12, y: 8 }], brushSize: 10, tool: 'brush' };
const first = exports.createSpot('container', template, stroke, 'photo.raw', adjustments, commit);
const second = exports.createSpot('container', template, stroke, 'photo.raw', adjustments, commit);
assert.equal(pending[0].command, 'find_spot_source');
assert.equal(pending[0].args.parameters.coordinateFrame.rotation, 15);
pending[1].resolve([30, 20]);
const b = await second;
pending[0].resolve([40, 20]);
const a = await first;
assert.notEqual(a.id, b.id);
assert.equal(adjustments.masks[0].subMasks.length, 2, 'Overlapping source searches must keep both strokes');
assert.ok(adjustments.masks[0].subMasks.every((m) => m.parameters.lines.length === 1));
assert.equal(template.parameters.lines.length, 0, 'Template stays immutable');

const stale = exports.createSpot('container', template, stroke, 'photo.raw', adjustments, commit);
state.selectedImage = { path: 'other.raw' };
pending[2].resolve([30, 30]);
assert.equal(await stale, null);
assert.equal(adjustments.masks[0].subMasks.length, 2, 'Late results must not edit the next photo');

state.selectedImage = { path: 'photo.raw' };
const deleted = exports.createSpot('container', template, stroke, 'photo.raw', adjustments, commit);
adjustments = { ...adjustments, masks: [] };
pending[3].resolve([30, 30]);
assert.equal(await deleted, null);
assert.equal(adjustments.masks.length, 0, 'Late results must not recreate a removed container');

adjustments = { masks: [{ id: 'container', subMasks: [] }] };
const manual = { ...template, parameters: { manualSource: true, sourceX: 7, sourceY: 9, lines: [] } };
const result = await exports.createSpot('container', manual, stroke, 'photo.raw', adjustments, commit);
assert.ok(result.sourceFound);
assert.equal(pending.length, 4, 'Manual sources bypass automatic search');
console.log('Independent spots, concurrent searches, manual sources and stale-result checks passed.');
