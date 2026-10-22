import { ExportPreset } from './ExportImportProperties';
import { CopyPasteSettings } from '../../utils/adjustments';
import { ToolType } from '../panel/right/Masks';

export const GLOBAL_KEYS = [
  ' ',
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'f',
  'p',
  'x',
  'u',
  'b',
  'a',
  's',
  'd',
  'r',
  'm',
  'k',
  'i',
  'e',
  '0',
  '1',
  '2',
  '3',
  '4',
  '5',
  'Enter',
];
export const OPTION_SEPARATOR = 'separator';

export enum Invokes {
  AddTagForPaths = 'add_tag_for_paths',
  ApplyAdjustments = 'apply_adjustments',
  ApplyAdjustmentsToPaths = 'apply_adjustments_to_paths',
  ApplyAutoAdjustmentsToPaths = 'apply_auto_adjustments_to_paths',
  ApplyDenoising = 'apply_denoising',
  CalculateAutoAdjustments = 'calculate_auto_adjustments',
  CancelExport = 'cancel_export',
  ClearAllSidecars = 'clear_all_sidecars',
  ClearThumbnailCache = 'clear_thumbnail_cache',
  CatalogStatus = 'catalog_status',
  CatalogAddFolder = 'catalog_add_folder',
  CatalogRemoveFolder = 'catalog_remove_folder',
  CatalogRemoveImages = 'catalog_remove_images',
  CatalogRefreshPaths = 'catalog_refresh_paths',
  CatalogRebuild = 'catalog_rebuild',
  CatalogQueryPage = 'catalog_query_page',
  CatalogFacets = 'catalog_facets',
  UpdateLensfun = 'update_lensfun',
  CopyFiles = 'copy_files',
  CreateVirtualCopy = 'create_virtual_copy',
  CullImages = 'cull_images',
  DuplicateFile = 'duplicate_file',
  EstimateExportSizes = 'estimate_export_sizes',
  ExportImages = 'export_images',
  GeneratePreviewForPath = 'generate_preview_for_path',
  GenerateMaskOverlay = 'generate_mask_overlay',
  GenerateUncroppedPreview = 'generate_uncropped_preview',
  GetSupportedFileTypes = 'get_supported_file_types',
  ImportFiles = 'import_files',
  ListImagesInDir = 'list_images_in_dir',
  ListImagesRecursive = 'list_images_recursive',
  LoadImage = 'load_image',
  SampleWhiteBalance = 'sample_white_balance',
  LoadMetadata = 'load_metadata',
  LoadSettings = 'load_settings',
  MoveFiles = 'move_files',
  ReadExifForPaths = 'read_exif_for_paths',
  RemoveTagForPaths = 'remove_tag_for_paths',
  RenameFiles = 'rename_files',
  ResetAdjustmentsForPaths = 'reset_adjustments_for_paths',
  SaveMetadataAndUpdateThumbnail = 'save_metadata_and_update_thumbnail',
  SaveCollage = 'save_collage',
  SaveDenoisedImage = 'save_denoised_image',
  SavePanorama = 'save_panorama',
  SaveHdr = 'save_hdr',
  SaveSettings = 'save_settings',
  SetColorLabelForPaths = 'set_color_label_for_paths',
  SetRatingForPaths = 'set_rating_for_paths',
  SetFlagForPaths = 'set_flag_for_paths',
  ShowInFinder = 'show_in_finder',
  StartBackgroundIndexing = 'start_background_indexing',
  StitchPanorama = 'stitch_panorama',
  StitchFocusStack = 'stitch_focus_stack',
  SaveFocusStack = 'save_focus_stack',
  MergeHdr = 'merge_hdr',
  UpdateWgpuTransform = 'update_wgpu_transform',
  UpdateExifFields = 'update_exif_fields',
  GetAlbums = 'get_albums',
  SaveAlbums = 'save_albums',
  AddToAlbum = 'add_to_album',
  GetAlbumImages = 'get_album_images',
}

export enum ExifOverlay {
  Off = 'off',
  Hover = 'hover',
  Always = 'always',
}

export enum Panel {
  Adjustments = 'adjustments',
  Crop = 'crop',
  Export = 'export',
  Masks = 'masks',
  Metadata = 'metadata',
  Catalog = 'folderTree',
}

export type PanelRegion = 'leftTop' | 'leftBottom' | 'rightTop' | 'rightBottom';

export enum RawStatus {
  All = 'all',
  NonRawOnly = 'nonRawOnly',
  RawOnly = 'rawOnly',
}

export enum SortDirection {
  Ascending = 'asc',
  Descending = 'desc',
}

export enum Theme {
  Arctic = 'arctic',
  Blue = 'blue',
  Dark = 'dark',
  Grey = 'grey',
  Light = 'light',
  MutedGreen = 'muted-green',
  Sepia = 'sepia',
  Snow = 'snow',
}

export enum ThumbnailAspectRatio {
  Cover = 'cover',
  Contain = 'contain',
  Justified = 'justified',
}

export interface WorkspaceState {
  leftPanelWidth: number;
  rightPanelWidth: number;
  leftTopHeight: number;
  rightTopHeight: number;
  panelLayout: Record<PanelRegion, Panel[]>;
  activePanels: Record<PanelRegion, Panel | null>;
  panelSwitcherPlacement: Record<PanelRegion, 'left' | 'right' | 'top' | 'bottom'>;
}

export type GroupPreference = 'jpeg' | 'raw';
export type GroupingMode = 'off' | GroupPreference;

export interface AdjustmentLayout {
  openSections?: Record<string, boolean>;
  collapsedTools?: string[];
  hiddenSections?: string[];
  hiddenTools?: string[];
  sectionOrder?: string[];
  toolOrder?: Record<string, string[]>;
}

export interface AppSettings {
  adjustmentLayout?: AdjustmentLayout;
  enableToolFocusMode?: boolean;
  decorations?: any;
  editorPreviewResolution?: number;
  smallThumbnailResolution?: number;
  mediumThumbnailResolution?: number;
  enableZoomHifi?: boolean;
  useFullDpiRendering?: boolean;
  highResZoomMultiplier?: number;
  enableLivePreviews?: boolean;
  livePreviewQuality?: string;
  filterCriteria?: FilterCriteria;
  lastFolderState?: any;
  lastRootPath: string | null;
  rootFolders?: string[];
  catalogFolders?: string[];
  libraryViewMode?: LibraryViewMode;
  sortCriteria?: SortCriteria;
  theme: Theme;
  thumbnailSize?: ThumbnailSize;
  thumbnailAspectRatio?: ThumbnailAspectRatio;
  uiVisibility?: UiVisibility;
  adjustmentVisibility?: { [key: string]: boolean };
  rawHighlightCompression?: number;
  rawHighlightRecoveryEnabled?: boolean;
  exportPresets?: ExportPreset[];
  myLenses?: any;
  displayEditIcon?: boolean;
  editorNeutralGreyBg?: boolean;
  linearRawMode?: string;
  enableXmpSync?: boolean;
  createXmpIfMissing?: boolean;
  isWaveformVisible?: boolean;
  waveformHeight?: number;
  activeWaveformChannel?: string;
  canvasInputMode?: 'mouse' | 'trackpad';
  zoomSpeedMultiplier?: number;
  maxZoomPercent?: number;
  zoomPhotoToPixelClick?: boolean;
  keybinds?: { [action: string]: string[] };
  tonemapperOverrideEnabled?: boolean;
  defaultRawTonemapper?: string;
  defaultNonRawTonemapper?: string;
  copyPasteSettings?: CopyPasteSettings;
  enableFocusMode?: boolean;
  openTreeSections?: string[];
  folderIcons?: Record<string, string>;
  exifOverlay?: ExifOverlay;
  language?: string;
  fontFamily?: string;
  taggingShortcuts?: string[];
  libraryDisplayMode?: LibraryDisplayMode;
  grouping?: GroupingMode;
  requireMatchingExif?: boolean;
  groupEditedFiles?: boolean;
  groupPreferredType?: GroupPreference; // legacy
  alwaysDecodeRawThumbnails?: boolean;
  workspace?: WorkspaceState;
}

export interface BrushSettings {
  feather: number;
  size: number;
  tool: ToolType;
}

export enum LibraryViewMode {
  Flat = 'flat',
  Recursive = 'recursive',
}

export const EditedStatus = {
  All: 'all',
  EditedOnly: 'editedOnly',
  UneditedOnly: 'uneditedOnly',
} as const;

export type EditedStatus = (typeof EditedStatus)[keyof typeof EditedStatus];

export interface FilterCriteria {
  colors: Array<string>;
  rating: number;
  rawStatus: RawStatus;
  editedStatus?: EditedStatus;
}

export interface ImageFile {
  is_edited: boolean;
  modified: number;
  path: string;
  rating: number;
  tags: Array<string> | null;
  exif: { [key: string]: string } | null;
  is_virtual_copy: boolean;
  is_cloud_placeholder: boolean;
  is_raw: boolean;
  group_id: string | null;
}

export interface Option {
  color?: string;
  disabled?: boolean;
  icon?: any;
  isDestructive?: boolean;
  label?: string;
  onClick?(): void;
  onRightClick?(): void;
  submenu?: any;
  type?: string;
}

export enum Orientation {
  Horizontal = 'horizontal',
  Vertical = 'vertical',
}

export interface Progress {
  completed?: number;
  current?: number;
  total: number;
}

export interface SelectedImage {
  asShotWhiteBalance?: import('../../utils/whiteBalance').WhiteBalance;
  exif: any;
  group_id?: string | null;
  height: number;
  isRaw: boolean;
  isReady: boolean;
  metadata?: any;
  original_base64?: string;
  path: string;
  thumbnailUrl: string;
  width: number;
}

export interface SortCriteria {
  key: string;
  label?: string;
  order: string;
}

export interface SupportedTypes {
  nonRaw: Array<string>;
  raw: Array<string>;
  video?: Array<string>;
}

export enum LibraryDisplayMode {
  Grid = 'grid',
  Cull = 'cull',
  List = 'list',
}

export enum ThumbnailSize {
  Large = 'large',
  Medium = 'medium',
  Small = 'small',
}

export interface TransformState {
  positionX: number;
  positionY: number;
  scale: number;
}

export interface UiVisibility {
  topPanel: boolean;
  bottomPanel: boolean;
  filmstrip: boolean;
  leftPanel: boolean;
  rightPanel: boolean;
}

export interface WaveformData {
  blue: string;
  green: string;
  height: number;
  luma: string;
  red: string;
  rgb: string;
  parade: string;
  vectorscope: string;
  width: number;
}

export interface CullingSettings {
  similarityThreshold: number;
  blurThreshold: number;
  groupSimilar: boolean;
  filterBlurry: boolean;
}

interface ImageAnalysisResult {
  path: string;
  qualityScore: number;
  sharpnessMetric: number;
  centerFocusMetric: number;
  exposureMetric: number;
  width: number;
  height: number;
}

interface CullGroup {
  representative: ImageAnalysisResult;
  duplicates: ImageAnalysisResult[];
}

export interface CullingSuggestions {
  similarGroups: CullGroup[];
  blurryImages: ImageAnalysisResult[];
  failedPaths: string[];
}

export type AlbumItem = Album | AlbumGroup | SmartGroup;

export interface Album {
  type: 'album';
  id: string;
  name: string;
  icon?: string;
  images: string[];
}

export interface AlbumGroup {
  type: 'group';
  id: string;
  name: string;
  icon?: string;
  children: AlbumItem[];
}

export interface SmartGroup {
  type: 'smartGroup';
  id: string;
  name: string;
  icon?: string;
  filter: {
    cameras?: string[];
    lenses?: string[];
    tags?: string[];
    colors?: string[];
    fileTypes?: string[];
    ratings?: number[];
    text?: string;
    camera?: string;
    lens?: string;
    date?: string;
    minimumRating?: number;
    tag?: string;
    color?: string;
    flag?: string;
    fileType?: string;
  };
}
