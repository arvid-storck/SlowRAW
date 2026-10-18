export interface WhiteBalance {
  temperature: number;
  tint: number;
}

export const MIN_TEMPERATURE = 2000;
export const MAX_TEMPERATURE = 50000;
export const MAX_TINT = 150;
export const kelvinSliderScale = { fromPosition: Math.exp, toPosition: Math.log };
export const REFERENCE_WHITE_BALANCE: WhiteBalance = { temperature: 6504, tint: 10 };

export function resolveWhiteBalance(
  asShot: WhiteBalance,
  adjustments: { whiteBalance?: WhiteBalance | null },
): WhiteBalance {
  return adjustments.whiteBalance ? withKelvinWhiteBalance({}, adjustments.whiteBalance).whiteBalance : asShot;
}

export function withKelvinWhiteBalance<T>(
  adjustments: T,
  whiteBalance: WhiteBalance,
): T & { whiteBalance: WhiteBalance } {
  return {
    ...adjustments,
    whiteBalance: {
      temperature: Number.isFinite(whiteBalance.temperature)
        ? Math.fround(Math.max(MIN_TEMPERATURE, Math.min(MAX_TEMPERATURE, whiteBalance.temperature)))
        : REFERENCE_WHITE_BALANCE.temperature,
      tint: Number.isFinite(whiteBalance.tint)
        ? Math.fround(Math.max(-MAX_TINT, Math.min(MAX_TINT, whiteBalance.tint)))
        : REFERENCE_WHITE_BALANCE.tint,
    },
  };
}
