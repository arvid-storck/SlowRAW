import type { FormatEncoding } from '../components/ui/ExportImportProperties';

export function defaultFormatEncoding(format: string, quality = 90): FormatEncoding {
  return {
    quality,
    lossless: ['png', 'tiff'].includes(format),
    bitDepth: ['tiff', 'jxl'].includes(format) ? 16 : 8,
    effort: format === 'webp' ? 4 : format === 'png' ? 9 : 7,
  };
}

// Product policy belongs to the UI, independently of native codec validation.
export function allowsProPhoto(format: string, bits: number, encoding: FormatEncoding): boolean {
  if (['jpeg', 'webp', 'avif'].includes(format) || bits < 12) return false;
  const lossless = ['png', 'tiff'].includes(format) || encoding.lossless;
  const highEffort = format === 'tiff' || encoding.effort >= 7;
  return (lossless || encoding.quality === 100) && highEffort;
}
