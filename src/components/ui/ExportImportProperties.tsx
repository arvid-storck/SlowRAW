import { Progress } from './AppProperties';

export enum FileFormats {
  Jpeg = 'jpeg',
  Png = 'png',
  Tiff = 'tiff',
  Webp = 'webp',
  Jxl = 'jxl',
  Avif = 'avif',
}

export const FILE_FORMATS: Array<FileFormat> = [
  { id: FileFormats.Jpeg, name: 'JPEG', extensions: ['jpg', 'jpeg'] },
  { id: FileFormats.Png, name: 'PNG', extensions: ['png'] },
  { id: FileFormats.Tiff, name: 'TIFF', extensions: ['tiff'] },
  { id: FileFormats.Webp, name: 'WebP', extensions: ['webp'] },
  { id: FileFormats.Jxl, name: 'JPEG XL', extensions: ['jxl'] },
  { id: FileFormats.Avif, name: 'AVIF', extensions: ['avif'] },
];

export const FILENAME_VARIABLES: Array<string> = [
  '{original_filename}',
  '{sequence}',
  '{YYYY}',
  '{MM}',
  '{DD}',
  '{hh}',
  '{mm}',
];

export type TiffBitDepth = 8 | 16;
export type PngBitDepth = 8 | 16;

export interface ExportSettings {
  encoding?: FormatEncoding;
  keepAuthor?: boolean;
  filenameTemplate: string | null;
  jpegQuality: number;
  tiffBitDepth: TiffBitDepth;
  pngBitDepth: PngBitDepth;
  colorSpace: import('../../utils/colorSpaces').ColorSpace;
  keepMetadata: boolean;
  preserveTimestamps: boolean;
  resize: {
    mode: string;
    value: number;
    dontEnlarge: boolean;
  } | null;
  stripGps: boolean;
  watermark: WatermarkSettings | null;
  exportMasks?: boolean;
  preserveFolders?: boolean;
  destinationType?: string;
  subfolder?: string;
}

export enum WatermarkAnchor {
  TopLeft = 'topLeft',
  TopCenter = 'topCenter',
  TopRight = 'topRight',
  CenterLeft = 'centerLeft',
  Center = 'center',
  CenterRight = 'centerRight',
  BottomLeft = 'bottomLeft',
  BottomCenter = 'bottomCenter',
  BottomRight = 'bottomRight',
}

interface WatermarkSettings {
  path: string;
  anchor: WatermarkAnchor;
  scale: number;
  spacing: number;
  opacity: number;
}

export interface ExportState {
  errorMessage: string;
  progress: Progress;
  status: Status;
}

export interface FileFormat {
  extensions: Array<string>;
  id: string;
  name: string;
}

export interface ImportState {
  errorMessage: string;
  path?: string;
  progress?: Progress;
  status: Status;
}

export enum Status {
  Cancelled = 'cancelled',
  Cancelling = 'cancelling',
  Exporting = 'exporting',
  Error = 'error',
  Idle = 'idle',
  Importing = 'importing',
  Success = 'success',
}

export interface ExportPreset {
  formatEncodings?: Record<string, FormatEncoding>;
  keepAuthor?: boolean;
  id: string;
  name: string;
  fileFormat: string;
  jpegQuality: number;
  tiffBitDepth?: TiffBitDepth;
  pngBitDepth?: PngBitDepth;
  colorSpace?: import('../../utils/colorSpaces').ColorSpace;
  enableResize: boolean;
  resizeMode: string;
  resizeValue: number;
  dontEnlarge: boolean;
  keepMetadata: boolean;
  preserveTimestamps: boolean;
  stripGps: boolean;
  exportMasks?: boolean;
  preserveFolders?: boolean;
  filenameTemplate: string;
  enableWatermark: boolean;
  watermarkPath: string | null;
  watermarkAnchor: string;
  watermarkScale: number;
  watermarkSpacing: number;
  watermarkOpacity: number;
  lastExportPath?: string;
  destinationType?: string;
  subfolder?: string;
}

export interface FormatEncoding {
  quality: number;
  lossless: boolean;
  bitDepth: 8 | 16;
  effort: number;
}
