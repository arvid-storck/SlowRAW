// SPDX-License-Identifier: GPL-3.0-or-later
// Models adapted from darktable (Copyright 2019-2026 darktable developers)
// and ART; original source references and notices are in resources/licenses/vendor-lens.txt.
use super::{RadialProfile, Tags};

pub(super) fn decode(tags: &Tags) -> Option<RadialProfile> {
    // ExifTool exposes these as unknown numeric TIFF tags with -u.
    let d = tags
        .numbers("Olympus_ImageProcessing_0x150a")
        .or_else(|| tags.numbers("DistortionCorrectionParams"));
    let c = tags
        .numbers("Olympus_ImageProcessing_0x150c")
        .or_else(|| tags.numbers("ChromaticAberrationCorrectionParams"));
    if d.as_ref().is_some_and(|v| v.len() != 4) || c.as_ref().is_some_and(|v| v.len() != 6) {
        return None;
    }
    if d.is_none() && c.is_none() {
        return None;
    }
    let mut table = RadialProfile::default();
    for i in 0..=128 {
        let r = i as f64 / 128.0;
        table.knots.push(r);
        let m = if let Some(d) = &d {
            let x = (r * d[3]).powi(2);
            d[3] * (1.0 + x * (d[0] + x * (d[1] + x * d[2])))
        } else {
            1.0
        };
        table.distortion.push(m);
        if let Some(c) = &c {
            let x = (r * m).powi(2);
            table.red.push(1.0 + c[0] + x * (c[1] + x * c[2]));
            table.blue.push(1.0 + c[3] + x * (c[4] + x * c[5]));
        }
    }
    Some(table)
}
