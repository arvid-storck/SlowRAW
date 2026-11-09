import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = fs.readFileSync(new URL('../src/hooks/useKeyboardShortcuts.ts', import.meta.url), 'utf8');
const file = ts.createSourceFile('shortcuts.ts', source, ts.ScriptTarget.Latest, true);
let initializer;
function find(node) {
  if (ts.isVariableDeclaration(node) && node.name.getText(file) === 'builtinShortcuts') initializer = node.initializer;
  ts.forEachChild(node, find);
}
find(file);
assert.ok(initializer);
const sortedListRef = { current: Array.from({ length: 17 }, (_, i) => ({ path: String(i) })) };
let selected;
const state = {
  library: {
    libraryActivePath: '7',
    libraryColumnCount: 4,
    setLibrary(update) {
      Object.assign(this, update);
    },
  },
};
const context = vm.createContext({
  sortedListRef,
  handleImageSelect: (path) => {
    selected = path;
  },
});
vm.runInContext(
  ts.transpileModule(`var shortcuts = ${initializer.getText(file)};`, {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText,
  context,
);
const arrow = context.shortcuts.at(-1);
function press(code, path, columns) {
  state.library.libraryActivePath = path;
  state.library.libraryColumnCount = columns;
  arrow.execute({ code, preventDefault() {} }, state);
  return selected;
}
assert.equal(press('ArrowDown', '7', 4), '11');
assert.equal(press('ArrowUp', '7', 4), '3');
assert.equal(press('ArrowDown', '7', 6), '13');
assert.equal(press('ArrowUp', '7', 1), '6');
assert.equal(press('ArrowDown', '15', 4), '16');
assert.equal(press('ArrowUp', '1', 4), '0');
assert.equal(press('ArrowLeft', '0', 4), '16');
assert.equal(press('ArrowRight', '16', 4), '0');
console.log('Passed row-size navigation, resize, list mode, partial-row boundaries and horizontal wrapping checks.');
