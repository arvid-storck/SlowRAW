export const COLOR_SPACES = [
  { value: 'srgb', label: 'sRGB' },
  { value: 'adobe-rgb', label: 'Adobe RGB (1998)' },
  { value: 'display-p3', label: 'Display P3' },
  { value: 'rec2020', label: 'Rec. 2020' },
  { value: 'prophoto-rgb', label: 'ProPhoto RGB' },
] as const;
export type ColorSpace = (typeof COLOR_SPACES)[number]['value'];
