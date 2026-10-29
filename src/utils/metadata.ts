/** Camera-reported ISO, with compatibility for metadata saved by older readers. */
export function cameraIso(exif?: Record<string, unknown> | null): string | undefined {
  for (const key of [
    'ISO',
    'PhotographicSensitivity',
    'ISOSpeedRatings',
    'RecommendedExposureIndex',
    'StandardOutputSensitivity',
    'ISOSpeed',
  ]) {
    const value = Number(exif?.[key]);
    if (Number.isFinite(value) && value > 0 && value !== 65535) return String(value);
  }
}
