import type { Adjustments } from './adjustments';

export function lensCapabilities(params: Adjustments['lensDistortionParams']) {
  const differs = (values: number[] | undefined, neutral: number) =>
    values?.some((value) => Math.abs(value - neutral) > 1e-12) ?? false;
  const embedded = params?.embedded;
  const warp = embedded?.warp;
  return {
    distortion:
      differs([params?.k1 ?? 0, params?.k2 ?? 0, params?.k3 ?? 0], 0) ||
      differs(embedded?.radial?.distortion, 1) ||
      (warp?.coefficients.some((k) => warp.fisheye || Math.abs(k[0] - 1) > 1e-12 || differs(k.slice(1), 0)) ?? false),
    tca:
      differs([params?.tca_vr ?? 1, params?.tca_vb ?? 1], 1) ||
      differs(embedded?.radial?.red, 1) ||
      differs(embedded?.radial?.blue, 1) ||
      (warp?.coefficients.some((k) => k.some((value, i) => Math.abs(value - warp.coefficients[0][i]) > 1e-12)) ??
        false),
    vignetting:
      differs([params?.vig_k1 ?? 0, params?.vig_k2 ?? 0, params?.vig_k3 ?? 0], 0) ||
      differs(embedded?.vignette?.coefficients, 0) ||
      differs(embedded?.radial?.gain, 1),
  };
}
