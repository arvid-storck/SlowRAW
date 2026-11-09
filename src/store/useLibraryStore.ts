import { create } from 'zustand';
import {
  FilterCriteria,
  ImageFile,
  RawStatus,
  SortCriteria,
  SortDirection,
  AlbumItem,
} from '../components/ui/AppProperties';
import { Adjustments, INITIAL_ADJUSTMENTS } from '../utils/adjustments';
import { ColumnWidths } from '../components/panel/MainLibrary';

interface SearchCriteria {
  tags: string[];
  text: string;
  mode: 'AND' | 'OR';
}

export interface CatalogStatus {
  databasePath: string;
  folders: string[];
  imageCount: number;
}

export interface CatalogFacets {
  cameras: string[];
  lenses: string[];
  dates: string[];
  tags: string[];
  colors: string[];
  fileTypes: string[];
  dateDays: Array<{ date: string; count: number }>;
}

export interface CatalogFilterState {
  flag?: string;
  cameras: string[];
  lenses: string[];
  tags: string[];
  colors: string[];
  fileTypes: string[];
  ratings: number[];
  text: string;
  camera: string;
  lens: string;
  date: string;
  minimumRating: number | null;
  tag: string;
  color: string;
  fileType: string;
}

export const EMPTY_CATALOG_FILTER: CatalogFilterState = {
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
  minimumRating: null,
  tag: '',
  color: '',
  fileType: '',
};

interface LibraryState {
  libraryColumnCount: number;
  // Paths & Trees
  rootPaths: string[];
  currentFolderPath: string | null;
  isCatalogMode: boolean;
  catalogStatus: CatalogStatus | null;
  catalogFacets: CatalogFacets | null;
  catalogDateFilter: string;
  catalogFilter: CatalogFilterState;
  catalogPage: number;
  catalogHasMore: boolean;
  catalogTotal: number;
  editingSmartGroupId: string | null;
  catalogRevision: number;

  // Albums
  albumTree: AlbumItem[];
  activeAlbumId: string | null;
  expandedAlbumGroups: Set<string>;

  // Images & Selection
  imageList: Array<ImageFile>;
  imageRatings: Record<string, number>;
  imageFlags: Record<string, 'pick' | 'reject' | null>;
  multiSelectedPaths: Array<string>;
  selectionAnchorPath: string | null;
  libraryActivePath: string | null;
  libraryActiveAdjustments: Adjustments;

  // Sorting & Filtering
  sortCriteria: SortCriteria;
  filterCriteria: FilterCriteria;
  searchCriteria: SearchCriteria;

  // UI State specific to the Library View
  isViewLoading: boolean;
  libraryScrollTop: number;
  listColumnWidths: ColumnWidths;

  // Actions
  setLibrary: (updater: Partial<LibraryState> | ((state: LibraryState) => Partial<LibraryState>)) => void;
  clearSelection: () => void;
  setFilterCriteria: (criteria: Partial<FilterCriteria> | ((prev: FilterCriteria) => FilterCriteria)) => void;
  setSearchCriteria: (criteria: Partial<SearchCriteria> | ((prev: SearchCriteria) => SearchCriteria)) => void;
  setSortCriteria: (criteria: Partial<SortCriteria> | ((prev: SortCriteria) => SortCriteria)) => void;
}

export const useLibraryStore = create<LibraryState>((set) => ({
  libraryColumnCount: 1,
  rootPaths: [],
  currentFolderPath: 'Catalog',
  isCatalogMode: true,
  catalogStatus: null,
  catalogFacets: null,
  catalogDateFilter: '',
  catalogFilter: EMPTY_CATALOG_FILTER,
  catalogPage: 0,
  catalogHasMore: false,
  catalogTotal: 0,
  editingSmartGroupId: null,
  catalogRevision: 0,

  albumTree: [],
  activeAlbumId: null,
  expandedAlbumGroups: new Set<string>(),

  imageList: [],
  imageRatings: {},
  imageFlags: {},
  multiSelectedPaths: [],
  selectionAnchorPath: null,
  libraryActivePath: null,
  libraryActiveAdjustments: INITIAL_ADJUSTMENTS,

  sortCriteria: { key: 'date_taken', order: SortDirection.Ascending },
  filterCriteria: { colors: [], rating: 0, rawStatus: RawStatus.All },
  searchCriteria: { tags: [], text: '', mode: 'OR' },

  isViewLoading: false,
  libraryScrollTop: 0,
  listColumnWidths: {
    thumbnail: 4,
    name: 20,
    date: 15,
    rating: 8,
    color: 8,
    shutter: 10,
    aperture: 10,
    iso: 10,
    focal: 15,
  },

  setLibrary: (updater) =>
    set((state) => {
      const changes = typeof updater === 'function' ? updater(state) : updater;
      if (!changes.albumTree && !changes.activeAlbumId) return changes;
      const next = { ...state, ...changes };
      const contains = (items: AlbumItem[], id: string): boolean =>
        items.some((item) => item.id === id || (item.type === 'group' && contains(item.children, id)));
      if (next.activeAlbumId && !contains(next.albumTree, next.activeAlbumId)) {
        return {
          ...changes,
          activeAlbumId: null,
          editingSmartGroupId: null,
          isCatalogMode: true,
          currentFolderPath: 'Catalog',
          catalogFilter: { ...EMPTY_CATALOG_FILTER },
          catalogDateFilter: '',
          catalogPage: 0,
          imageList: [],
          multiSelectedPaths: [],
          libraryActivePath: null,
          selectionAnchorPath: null,
          libraryScrollTop: 0,
          isViewLoading: false,
        };
      }
      if (next.editingSmartGroupId && !contains(next.albumTree, next.editingSmartGroupId)) {
        return { ...changes, editingSmartGroupId: null };
      }
      return changes;
    }),

  clearSelection: () => set({ multiSelectedPaths: [], libraryActivePath: null }),

  setFilterCriteria: (criteria) =>
    set((state) => ({
      filterCriteria:
        typeof criteria === 'function' ? criteria(state.filterCriteria) : { ...state.filterCriteria, ...criteria },
    })),

  setSearchCriteria: (criteria) =>
    set((state) => ({
      searchCriteria:
        typeof criteria === 'function' ? criteria(state.searchCriteria) : { ...state.searchCriteria, ...criteria },
    })),

  setSortCriteria: (criteria) =>
    set((state) => ({
      sortCriteria:
        typeof criteria === 'function' ? criteria(state.sortCriteria) : { ...state.sortCriteria, ...criteria },
    })),
}));
