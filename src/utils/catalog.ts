import { invoke } from './jobInvoke';
import { open } from '@tauri-apps/plugin-dialog';
import { AlbumItem, ImageFile, Invokes, SmartGroup } from '../components/ui/AppProperties';
import {
  CatalogFacets,
  CatalogFilterState,
  CatalogStatus,
  EMPTY_CATALOG_FILTER,
  useLibraryStore,
} from '../store/useLibraryStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useProcessStore } from '../store/useProcessStore';
import { useEditorStore } from '../store/useEditorStore';
import { getImageFlag, withPendingImageFlag } from './imageFlags';

export function normalizeCatalogFilter(filter: SmartGroup['filter']): CatalogFilterState {
  return {
    ...EMPTY_CATALOG_FILTER,
    ...filter,
    cameras: filter.cameras?.length ? filter.cameras : filter.camera ? [filter.camera] : [],
    lenses: filter.lenses?.length ? filter.lenses : filter.lens ? [filter.lens] : [],
    tags: filter.tags?.length ? filter.tags : filter.tag ? [filter.tag] : [],
    colors: filter.colors?.length ? filter.colors : filter.color ? [filter.color] : [],
    fileTypes: filter.fileTypes?.length ? filter.fileTypes : filter.fileType ? [filter.fileType] : [],
    ratings: filter.ratings?.length
      ? filter.ratings
      : filter.minimumRating == null
        ? []
        : filter.minimumRating === 0
          ? [0]
          : [1, 2, 3, 4, 5].filter((rating) => rating >= filter.minimumRating!),
    camera: '',
    lens: '',
    tag: '',
    color: '',
    fileType: '',
    minimumRating: null,
  };
}

export function updateSmartGroup(tree: AlbumItem[], id: string, name: string, filter: SmartGroup['filter']): boolean {
  return tree.some((item) => {
    if (item.type === 'smartGroup' && item.id === id) {
      item.name = name;
      item.filter = filter;
      return true;
    }
    return item.type === 'group' && updateSmartGroup(item.children, id, name, filter);
  });
}

export const CATALOG_PAGE_SIZE = 500;
let requestId = 0;

export async function loadCatalogPage() {
  const id = ++requestId;
  const state = useLibraryStore.getState();
  if (!state.isCatalogMode) return;
  state.setLibrary({ isViewLoading: true });
  try {
    const { images: rows, total } = await invoke<{ images: ImageFile[]; total: number }>(Invokes.CatalogQueryPage, {
      filter: {
        ...state.catalogFilter,
        date: state.catalogDateFilter,
        limit: CATALOG_PAGE_SIZE,
        sortKey: state.sortCriteria.key,
        sortDescending: state.sortCriteria.order === 'desc',
        offset: state.catalogPage * CATALOG_PAGE_SIZE,
      },
    });
    const latest = useLibraryStore.getState();
    if (
      id !== requestId ||
      !latest.isCatalogMode ||
      latest.catalogFilter !== state.catalogFilter ||
      latest.catalogPage !== state.catalogPage ||
      latest.catalogDateFilter !== state.catalogDateFilter ||
      latest.sortCriteria !== state.sortCriteria
    )
      return;
    const lastPage = Math.max(0, Math.ceil(total / CATALOG_PAGE_SIZE) - 1);
    if (state.catalogPage > lastPage) {
      state.setLibrary({ catalogPage: lastPage });
      return;
    }
    const files = rows.slice(0, CATALOG_PAGE_SIZE).map(withPendingImageFlag);
    const visible = new Set(files.map((file) => file.path));
    const edited = useEditorStore.getState().selectedImage?.path;
    if (edited) visible.add(edited);
    const imageRatings = Object.fromEntries(files.map((file) => [file.path, file.rating ?? 0]));
    const imageFlags = Object.fromEntries(files.map((file) => [file.path, getImageFlag(file.tags)]));
    if (edited && !(edited in imageFlags)) imageFlags[edited] = latest.imageFlags[edited] ?? null;
    // A rating change can remove the open image from the filtered page. Keep its
    // optimistic rating until the editor is closed instead of displaying 0 stars.
    if (edited && !(edited in imageRatings)) {
      imageRatings[edited] = latest.imageRatings[edited] ?? 0;
    }
    useProcessStore.getState().setProcess((process) => ({
      thumbnails: Object.fromEntries(Object.entries(process.thumbnails).filter(([path]) => visible.has(path))),
      mediumThumbnails: Object.fromEntries(
        Object.entries(process.mediumThumbnails).filter(([path]) => visible.has(path)),
      ),
    }));
    state.setLibrary({
      imageList: files,
      imageRatings,
      imageFlags,
      catalogHasMore: (state.catalogPage + 1) * CATALOG_PAGE_SIZE < total,
      catalogTotal: total,
      multiSelectedPaths: [],
      libraryActivePath: null,
      currentFolderPath: state.activeAlbumId ? state.currentFolderPath : 'Catalog',
    });
  } finally {
    if (id === requestId) state.setLibrary({ isViewLoading: false });
  }
}

export async function refreshCatalog() {
  const [catalogStatus, catalogFacets] = await Promise.all([
    invoke<CatalogStatus>(Invokes.CatalogStatus),
    invoke<CatalogFacets>(Invokes.CatalogFacets),
  ]);
  useLibraryStore
    .getState()
    .setLibrary((s) => ({ catalogStatus, catalogFacets, catalogRevision: s.catalogRevision + 1 }));
}

export function showAllCatalog() {
  ++requestId;
  useLibraryStore.getState().setLibrary({
    isCatalogMode: true,
    currentFolderPath: 'Catalog',
    activeAlbumId: null,
    editingSmartGroupId: null,
    catalogFilter: { ...EMPTY_CATALOG_FILTER },
    catalogDateFilter: '',
    catalogPage: 0,
    multiSelectedPaths: [],
    libraryActivePath: null,
  });
}

export async function addCatalogFolder() {
  const path = await open({ directory: true, multiple: false, title: 'Add folder to catalog' });
  if (typeof path !== 'string') return;
  useLibraryStore.getState().setLibrary({ isViewLoading: true });
  try {
    const status = await invoke<CatalogStatus>(Invokes.CatalogAddFolder, { path });
    const { appSettings, setAppSettings } = useSettingsStore.getState();
    // Backend already saved the folder membership; avoid a second stale settings write.
    if (appSettings) setAppSettings({ ...appSettings, catalogFolders: status.folders });
    showAllCatalog();
    await refreshCatalog();
  } finally {
    useLibraryStore.getState().setLibrary({ isViewLoading: false });
  }
}

export async function initializeCatalog() {
  let status = await invoke<CatalogStatus>(Invokes.CatalogStatus);
  if (!status.folders.length && useSettingsStore.getState().appSettings?.catalogFolders?.length) {
    status = await invoke<CatalogStatus>(Invokes.CatalogRebuild);
  }
  const albumTree = await invoke<AlbumItem[]>(Invokes.GetAlbums);
  useLibraryStore.getState().setLibrary({ catalogStatus: status, albumTree });
  await refreshCatalog();
}
