// SPDX-License-Identifier: GPL-3.0-or-later
// Correction polynomial: ART, Copyright (c) 2020 Alberto Griggio.
// See resources/licenses/vendor-lens.txt.
use super::{RadialProfile, Tags};

/// ExifTool decodes both blocks. Version 0100 stores distortion's r^4, r^3,
/// r^2 coefficients (r^1 is zero), and vignette's r^8 through r coefficients.
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
        == Some("0100")
        && tags.number("NikonVignetteCoefficientCount") == Some(8.0))
    .then(|| tags.numbers("NikonVignetteCoefficients"))
    .flatten()
    .filter(|values| values.len() == 8);
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
        if let Some(coefficients) = &vignette {
            table.gain_knots.push(r);
            table
                .gain
                .push(1.0 + coefficients.iter().fold(0.0, |p, c| p * r + c) * r);
        }
    }
    Some(table)
}
