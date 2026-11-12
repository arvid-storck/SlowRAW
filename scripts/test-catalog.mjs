import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import { randomUUID } from 'node:crypto';
import ts from 'typescript';

// Exercise real application functions with in-memory IPC/stores only.
const state = {
  isCatalogMode: true,
  catalogFilter: {},
  catalogDateFilter: '',
  catalogOffset: 0,
  catalogTotal: 0,
  catalogStatus: {},
  catalogRevision: 0,
  multiSelectedPaths: [],
  libraryActivePath: null,
  selectionAnchorPath: null,
  imageList: [],
  sortCriteria: { key: 'date_taken', order: 'asc' },
  albumTree: [],
  expandedAlbumGroups: new Set(),
  imageRatings: {},
  imageFlags: {},
  setLibrary(update) {
    Object.assign(state, typeof update === 'function' ? update(state) : update);
  },
};
const ui = { albumActionTarget: null };
let savedTree = [],
  responseTotal = 1001;
let pending,
  failSave = false;
const errors = [];
const invocations = [];
const empty = {
  cameras: [],
  lenses: [],
  tags: [],
  colors: [],
  fileTypes: [],
  ratings: [],
  text: '',
  camera: '',
  lens: '',
  date: '',
  tag: '',
  color: '',
  fileType: '',
  minimumRating: null,
};
const invokes = new Proxy({}, { get: (_, key) => key });
const invoke = async (command, args) => {
  invocations.push({ command, args });
  if (command === 'SaveAlbums') {
    if (failSave) throw new Error('Test failure');
    savedTree = structuredClone(args.tree);
  }
  if (command === 'GetAlbums') return structuredClone(savedTree);
  if (command === 'CatalogQueryPage') {
    const { offset, limit } = args.filter;
    return (
      pending ?? {
        total: responseTotal,
        images: Array.from({ length: Math.max(0, Math.min(limit, responseTotal - offset)) }, (_, index) => ({
          path: offset + index === 0 ? 'a' : String(offset + index),
        })),
      }
    );
  }
};
const process = {
  thumbnails: { a: 'a', b: 'b' },
  mediumThumbnails: {},
  setProcess(update) {
    Object.assign(this, typeof update === 'function' ? update(this) : update);
  },
};
const common = {
  i18next: { t: (key) => key },
  react: { useCallback: (callback) => callback },
  '@tauri-apps/api/core': { invoke },
  './jobInvoke': { invoke },
  '../utils/jobInvoke': { invoke },
  '@tauri-apps/plugin-dialog': { open: async () => null },
  'react-toastify': { toast: { error: (error) => errors.push(error) } },
  '../components/ui/AppProperties': { Invokes: invokes },
  '../store/useLibraryStore': { EMPTY_CATALOG_FILTER: empty, useLibraryStore: { getState: () => ({ ...state }) } },
  '../store/useEditorStore': { useEditorStore: { getState: () => ({ selectedImage: { path: 'hidden' } }) } },
  '../store/useProcessStore': { useProcessStore: { getState: () => process } },
  '../store/useUIStore': { useUIStore: { getState: () => ui } },
  '../store/useSettingsStore': { useSettingsStore: { getState: () => ({}) } },
  './imageFlags': { getImageFlag: () => null, withPendingImageFlag: (file) => file },
  '../utils/ImageLRUCache': {},
  './useSortedLibrary': {},
  '../utils/imageGrouping': {},
};
function load(relative, extra = {}) {
  const source = fs.readFileSync(new URL(relative, import.meta.url), 'utf8');
  const output = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
    },
  }).outputText;
  const exports = {};
  vm.runInNewContext(output, {
    exports,
    require: (id) => {
      const mocks = { ...common, ...extra };
      assert.ok(id in mocks, id);
      return mocks[id];
    },
    structuredClone,
    crypto: { randomUUID },
    console,
  });
  return exports;
}
const catalog = load('../src/utils/catalog.ts');
const normalize = (value) => JSON.parse(JSON.stringify(value));
assert.deepEqual(normalize(catalog.normalizeCatalogFilter({ camera: 'Nikon', minimumRating: 4 })).ratings, [4, 5]);
assert.deepEqual(normalize(catalog.normalizeCatalogFilter({ minimumRating: 0 })).ratings, [0]);
assert.deepEqual(normalize(catalog.normalizeCatalogFilter({ cameras: ['A', 'B'] })).cameras, ['A', 'B']);
const tree = [
  {
    type: 'group',
    id: 'parent',
    children: [{ type: 'smartGroup', id: 'smart', name: 'Old', filter: { camera: 'A' } }],
  },
];
assert.equal(catalog.updateSmartGroup(tree, 'smart', 'New', { cameras: ['B', 'C'] }), true);
assert.equal(tree[0].children.length, 1, 'Editing must not duplicate the group');
assert.deepEqual(tree[0].children[0].filter.cameras, ['B', 'C']);
assert.equal(catalog.updateSmartGroup(tree, 'missing', 'New', {}), false);

state.imageRatings.hidden = 3;
await catalog.loadCatalogWindow();
assert.equal(state.catalogTotal, 1001);
assert.equal(state.imageList.length, 498);
assert.equal(state.imageRatings.hidden, 3, 'Retain rating of the filtered-out edited image');
assert.equal(process.thumbnails.b, undefined, 'Prune thumbnails outside the window');

await catalog.loadCatalogWindow(340);
assert.equal(state.catalogOffset, 166);
assert.equal(state.imageList[0].path, '166', 'Evict the first batch after scrolling');
assert.equal(state.imageList.length, 498);
await catalog.loadCatalogWindow(1000);
assert.equal(invocations.at(-1).args.filter.offset, 664);
assert.equal(invocations.at(-1).args.filter.limit, 498);
assert.equal(state.imageList.at(-1).path, '1000');

await catalog.loadCatalogWindow(100);
assert.equal(state.imageList[0].path, 'a', 'Reload evicted rows when scrolling back');
let resolve;
pending = new Promise((done) => {
  resolve = done;
});
const loading = catalog.loadCatalogWindow(340);
const requests = invocations.length;
await catalog.loadCatalogWindow(345);
assert.equal(invocations.length, requests, 'Deduplicate in-flight requests for the same window');
await catalog.loadCatalogWindow(100);
resolve({ images: [{ path: 'stale' }], total: 1001 });
await loading;
assert.equal(state.imageList[0].path, 'a', 'Discard a reply after scrolling back to the resident window');

pending = new Promise((done) => {
  resolve = done;
});
const stale = catalog.loadCatalogWindow(340);
state.catalogFilter = { text: 'new filter' };
resolve({ images: [{ path: 'stale' }], total: 1 });
await stale;
assert.equal(state.imageList[0].path, 'a', 'Discard replies from an old filter');
pending = null;

await catalog.loadCatalogWindow(1000);
responseTotal = 2;
await catalog.loadCatalogWindow(1000, true);
assert.equal(state.catalogOffset, 0, 'Clamp the window when results shrink');
assert.equal(state.imageList.length, 2);
catalog.resetCatalogWindow();
assert.equal(state.libraryScrollTop, 0);
responseTotal = 0;
await catalog.loadCatalogWindow();
assert.equal(state.catalogTotal, 0);
assert.equal(state.imageList.length, 0);

responseTotal = 60_000;
await catalog.loadCatalogWindow(0, true);
await catalog.loadCatalogWindow(30_000);
assert.equal(state.catalogTotal, 60_000);
assert.equal(state.imageList.length, 498);
assert.equal(state.imageList[30_000 - state.catalogOffset].path, '30000');

const actions = load('../src/hooks/useLibraryActions.ts', { '../utils/catalog': catalog }).useLibraryActions();
await actions.handleCreateAlbumItem('Root group', 'group');
const group = state.albumTree[0];
assert.equal(group.type, 'group');
assert.ok(state.expandedAlbumGroups.has(group.id));
ui.albumActionTarget = group.id;
await actions.handleCreateAlbumItem('Child album', 'album');
assert.equal(state.albumTree[0].children[0].name, 'Child album');
ui.albumActionTarget = state.albumTree[0].children[0].id;
await actions.handleCreateAlbumItem('Sibling group', 'group');
assert.equal(state.albumTree[0].children[1].type, 'group');
assert.ok(state.expandedAlbumGroups.has(state.albumTree[0].children[1].id));
const before = state.albumTree;
failSave = true;
await actions.handleCreateAlbumItem('Not saved', 'group');
assert.equal(state.albumTree, before, 'Failed saves leave visible album state unchanged');
assert.equal(errors.length, 1);
console.log('Passed catalog scrolling/count/stale-response, smart-filter editing and album-group creation checks.');
