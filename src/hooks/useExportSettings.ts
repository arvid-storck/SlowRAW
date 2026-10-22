import { COLOR_SPACES, type ColorSpace } from '../utils/colorSpaces';
import { useState, useMemo, useCallback, type SetStateAction } from 'react';
import {
  ExportPreset,
  TiffBitDepth,
  PngBitDepth,
  WatermarkAnchor,
  FILE_FORMATS,
} from '../components/ui/ExportImportProperties';

// Session draft survives panel remounts and viewport/layout changes.
const exportDraft: Record<string, unknown> = {};
function useDraftState<T>(key: string, initial: T) {
  const [value, setValue] = useState<T>(() => (key in exportDraft ? (exportDraft[key] as T) : initial));
  const update = useCallback(
    (next: SetStateAction<T>) => {
      setValue((previous) => {
        const resolved = typeof next === 'function' ? (next as (value: T) => T)(previous) : next;
        exportDraft[key] = resolved;
        return resolved;
      });
    },
    [key],
  );
  return [value, update] as const;
}

export function useExportSettings() {
  const [fileFormat, setFileFormat] = useDraftState('fileFormat', 'jpeg');
  const [jpegQuality, setJpegQuality] = useDraftState('jpegQuality', 90);
  const [tiffBitDepth, setTiffBitDepth] = useDraftState<TiffBitDepth>('tiffBitDepth', 16);
  const [pngBitDepth, setPngBitDepth] = useDraftState<PngBitDepth>('pngBitDepth', 8);
  const [colorSpace, setColorSpace] = useDraftState<ColorSpace>('colorSpace', 'srgb');
  const [enableResize, setEnableResize] = useDraftState('enableResize', false);
  const [resizeMode, setResizeMode] = useDraftState('resizeMode', 'longEdge');
  const [resizeValue, setResizeValue] = useDraftState('resizeValue', 2048);
  const [dontEnlarge, setDontEnlarge] = useDraftState('dontEnlarge', true);
  const [keepMetadata, setKeepMetadata] = useDraftState('keepMetadata', true);
  const [preserveTimestamps, setPreserveTimestamps] = useDraftState('preserveTimestamps', false);
  const [stripGps, setStripGps] = useDraftState('stripGps', true);
  const [exportMasks, setExportMasks] = useDraftState('exportMasks', false);
  const [preserveFolders, setPreserveFolders] = useDraftState('preserveFolders', false);
  const [filenameTemplate, setFilenameTemplate] = useDraftState('filenameTemplate', '{original_filename}_edited');
  const [enableWatermark, setEnableWatermark] = useDraftState('enableWatermark', false);
  const [watermarkPath, setWatermarkPath] = useDraftState<string | null>('watermarkPath', null);
  const [watermarkAnchor, setWatermarkAnchor] = useDraftState<WatermarkAnchor>(
    'watermarkAnchor',
    WatermarkAnchor.BottomRight,
  );
  const [watermarkScale, setWatermarkScale] = useDraftState('watermarkScale', 10);
  const [watermarkSpacing, setWatermarkSpacing] = useDraftState('watermarkSpacing', 5);
  const [watermarkOpacity, setWatermarkOpacity] = useDraftState('watermarkOpacity', 75);
  const [destinationType, setDestinationType] = useDraftState<string>('destinationType', 'customFolder');
  const [subfolder, setSubfolder] = useDraftState<string>('subfolder', '');

  const handleApplyPreset = useCallback((preset: ExportPreset) => {
    setFileFormat(FILE_FORMATS.some((format) => format.id === preset.fileFormat) ? preset.fileFormat : 'jpeg');
    setJpegQuality(preset.jpegQuality);
    setTiffBitDepth(preset.tiffBitDepth ?? 16);
    setPngBitDepth(preset.pngBitDepth === 16 ? 16 : 8);
    setColorSpace(
      !['webp', 'avif'].includes(preset.fileFormat) && COLOR_SPACES.some((space) => space.value === preset.colorSpace)
        ? preset.colorSpace!
        : 'srgb',
    );
    setEnableResize(preset.enableResize);
    setResizeMode(preset.resizeMode);
    setResizeValue(preset.resizeValue);
    setDontEnlarge(preset.dontEnlarge);
    setKeepMetadata(preset.keepMetadata);
    setPreserveTimestamps(preset.preserveTimestamps ?? false);
    setStripGps(preset.stripGps);
    setExportMasks(preset.exportMasks ?? false);
    setPreserveFolders(preset.preserveFolders ?? false);
    setFilenameTemplate(preset.filenameTemplate);
    setEnableWatermark(preset.enableWatermark);
    setWatermarkPath(preset.watermarkPath);
    setWatermarkAnchor(preset.watermarkAnchor as WatermarkAnchor);
    setWatermarkScale(preset.watermarkScale);
    setWatermarkSpacing(preset.watermarkSpacing);
    setWatermarkOpacity(preset.watermarkOpacity);
    setDestinationType(preset.destinationType || 'customFolder');
    setSubfolder(preset.subfolder || '');
  }, []);

  const currentSettingsObject = useMemo(
    () => ({
      fileFormat,
      jpegQuality,
      tiffBitDepth,
      pngBitDepth,
      colorSpace,
      enableResize,
      resizeMode,
      resizeValue,
      dontEnlarge,
      keepMetadata,
      preserveTimestamps,
      stripGps,
      exportMasks,
      preserveFolders,
      filenameTemplate,
      enableWatermark,
      watermarkPath,
      watermarkAnchor,
      watermarkScale,
      watermarkSpacing,
      watermarkOpacity,
      destinationType,
      subfolder,
    }),
    [
      fileFormat,
      jpegQuality,
      tiffBitDepth,
      pngBitDepth,
      colorSpace,
      enableResize,
      resizeMode,
      resizeValue,
      dontEnlarge,
      keepMetadata,
      preserveTimestamps,
      stripGps,
      exportMasks,
      preserveFolders,
      filenameTemplate,
      enableWatermark,
      watermarkPath,
      watermarkAnchor,
      watermarkScale,
      watermarkSpacing,
      watermarkOpacity,
      destinationType,
      subfolder,
    ],
  );

  return {
    fileFormat,
    setFileFormat,
    jpegQuality,
    setJpegQuality,
    tiffBitDepth,
    pngBitDepth,
    colorSpace,
    setTiffBitDepth,
    setPngBitDepth,
    setColorSpace,
    enableResize,
    setEnableResize,
    resizeMode,
    setResizeMode,
    resizeValue,
    setResizeValue,
    dontEnlarge,
    setDontEnlarge,
    keepMetadata,
    setKeepMetadata,
    preserveTimestamps,
    setPreserveTimestamps,
    stripGps,
    setStripGps,
    exportMasks,
    setExportMasks,
    preserveFolders,
    setPreserveFolders,
    filenameTemplate,
    setFilenameTemplate,
    enableWatermark,
    setEnableWatermark,
    watermarkPath,
    setWatermarkPath,
    watermarkAnchor,
    setWatermarkAnchor,
    watermarkScale,
    setWatermarkScale,
    watermarkSpacing,
    setWatermarkSpacing,
    watermarkOpacity,
    setWatermarkOpacity,
    destinationType,
    setDestinationType,
    subfolder,
    setSubfolder,
    handleApplyPreset,
    currentSettingsObject,
  };
}
