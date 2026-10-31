// SPDX-License-Identifier: GPL-3.0-or-later
// Correction polynomial: ART, Copyright (c) 2020 Alberto Griggio.
// See resources/licenses/vendor-lens.txt.
use super::{RadialProfile, Tags};

/// ExifTool's Nikon vignette coefficients are ordered r^6, r^4, r^2.
pub(super) fn decode(tags: &Tags) -> Option<RadialProfile> {
    let distortion = (tags
        .value("DistortionCorrectionVersion")
        .and_then(|v| v.as_str())
        == Some("0100"))
    .then(|| {
        Some([
            tags.number("RadialDistortionCoefficient1")?,
            tags.number("RadialDistortionCoefficient2")?,
            tags.number("RadialDistortionCoefficient3")?,
        ])
    })
    .flatten();
    let vignette = (tags
        .value("VignetteCorrectionVersion")
        .and_then(|v| v.as_str())
        == Some("0100"))
    .then(|| {
        Some([
            tags.number("VignetteCoefficient1")?,
            tags.number("VignetteCoefficient2")?,
            tags.number("VignetteCoefficient3")?,
        ])
    })
    .flatten();
    if distortion.is_none() && vignette.is_none() {
        return None;
    }
    let mut table = RadialProfile::default();
    for i in 0..=128 {
        let r = i as f64 / 128.0;
        table.knots.push(r);
        if let Some([a, b, c]) = distortion {
            table.distortion.push(1.0 + r * r * (c + r * (b + r * a)));
        }
        if let Some([a, b, c]) = vignette {
            table.gain_knots.push(r);
            let r2 = r * r;
            table.gain.push(1.0 + r2 * (c + r2 * (b + r2 * a)));
        }
    }
    Some(table)
}
