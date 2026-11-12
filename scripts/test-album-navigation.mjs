import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { create } from 'zustand';

function load(path, mocks, logger = console) {
  const exports = {};
  const source = fs.readFileSync(new URL(path, import.meta.url), 'utf8');
  const output = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(output, {
    exports,
    console: logger,
    require: (name) => {
      assert.ok(name in mocks, name);
      return mocks[name];
    },
  });
  return exports;
}
const { useLibraryStore: store } = load('../src/store/useLibraryStore.ts', {
  zustand: { create },
  '../components/ui/AppProperties': { SortDirection: { Ascending: 'asc' }, RawStatus: { All: 'all' } },
  '../utils/adjustments': { INITIAL_ADJUSTMENTS: {} },
});
const album = { id: 'album', type: 'album', name: 'Album', images: [] };
const other = { id: 'other', type: 'album', name: 'Other', images: [] };
const group = { id: 'group', type: 'group', name: 'Group', children: [album] };
const smart = { id: 'smart', type: 'smartGroup', name: 'Smart', filter: {} };
function enter(item, tree) {
  store.getState().setLibrary({
    albumTree: tree,
    activeAlbumId: item.id,
    isCatalogMode: item.type === 'smartGroup',
    currentFolderPath: item.name,
    imageList: [{ path: 'old' }],
    multiSelectedPaths: ['old'],
    catalogOffset: 2,
  });
}
for (const [item, tree, remaining] of [
  [album, [album, other], [other]],
  [album, [group, other], [other]],
  [smart, [smart, other], [other]],
]) {
  enter(item, tree);
  store.getState().setLibrary({ albumTree: remaining });
  const state = store.getState();
  assert.equal(state.activeAlbumId, null);
  assert.equal(state.isCatalogMode, true);
  assert.equal(state.currentFolderPath, 'Catalog');
  assert.equal(state.catalogOffset, 0);
  assert.equal(state.multiSelectedPaths.length, 0);
  assert.equal(state.imageList.length, 0);
}
enter(album, [group, other]);
store.getState().setLibrary({ albumTree: [group] });
assert.equal(store.getState().activeAlbumId, album.id, 'Deleting another album preserves the current album');
store.getState().setLibrary({ albumTree: [other], activeAlbumId: other.id });
assert.equal(store.getState().activeAlbumId, other.id, 'Switching to a surviving album is preserved');

let finish;
const navigation = load('../src/hooks/useAppNavigation.ts', {
  i18next: { t: (key) => key },
  react: { useCallback: (callback) => callback },
  '@tauri-apps/api/core': {
    invoke: () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  },
  'react-toastify': { toast: { error() {} } },
  '../store/useLibraryStore': { useLibraryStore: store },
  '../store/useUIStore': { useUIStore: { getState: () => ({ setUI() {} }) } },
  '../store/useEditorStore': {},
  '../store/useProcessStore': {},
  '../components/ui/AppProperties': { Invokes: { GetAlbumImages: 'album' } },
  '../utils/catalog': {},
  '../utils/adjustments': {},
  '../utils/ImageLRUCache': {},
  './useEditorActions': {},
  './useImageLoader': {},
  '../utils/mediaTypes': {},
}).useAppNavigation({ refs: {}, clearThumbnailQueue() {} });
enter(album, [album]);
const loading = navigation.handleSelectAlbum(album.id, album.name, [], true);
store.getState().setLibrary({ albumTree: [] });
store.getState().setLibrary({ isViewLoading: true });
finish([{ path: 'stale' }]);
await loading;
assert.equal(store.getState().activeAlbumId, null);
assert.equal(store.getState().imageList.length, 0, 'Late album replies cannot overwrite the catalog');
assert.equal(store.getState().isViewLoading, true, 'Late replies leave the current catalog load alone');
console.log(
  'Passed album/group/smart-group deletion, unrelated deletion, surviving selection and stale album-load checks.',
);

const editor = {
  selectedImage: { path: 'current.nef' },
  hasRenderedFirstFrame: true,
  setEditor(changes) {
    Object.assign(editor, changes);
  },
};
const ui = {
  activeView: 'editor',
  setUI(changes) {
    Object.assign(ui, changes);
  },
};
const evicted = [];
const errors = [];
const { handleImageLoadFailure } = load(
  '../src/hooks/useImageLoader.ts',
  {
    i18next: { t: (_, { value1 }) => value1 },
    react: {},
    '@tauri-apps/api/core': {},
    'react-toastify': { toast: { error: (error) => errors.push(error) } },
    '../store/useEditorStore': { useEditorStore: { getState: () => editor } },
    '../store/useLibraryStore': { useLibraryStore: store },
    '../store/useSettingsStore': {},
    '../store/useUIStore': { useUIStore: { getState: () => ui } },
    '../components/ui/AppProperties': {},
    '../utils/adjustments': {},
    '../utils/ImageLRUCache': { globalImageCache: { delete: (path) => evicted.push(path) } },
  },
  { error() {} },
);
handleImageLoadFailure('previous.nef', 'Stale error');
assert.equal(editor.selectedImage.path, 'current.nef');
assert.equal(ui.activeView, 'editor');
handleImageLoadFailure('current.nef', 'Decode failed');
assert.equal(editor.selectedImage, null);
assert.equal(editor.hasRenderedFirstFrame, false);
assert.equal(store.getState().isViewLoading, false);
assert.equal(ui.activeView, 'library');
assert.deepEqual(evicted, ['current.nef']);
assert.deepEqual(errors, ['Decode failed']);
console.log('Passed image-load recovery and stale-error checks.');
