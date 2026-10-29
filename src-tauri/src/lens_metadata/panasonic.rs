// SPDX-License-Identifier: GPL-3.0-or-later
// Models adapted from darktable (Copyright 2019-2026 darktable developers)
// and ART; original source references and notices are in resources/licenses/vendor-lens.txt.
use super::{RadialProfile, Tags};

pub(super) fn decode(tags: &Tags) -> Option<RadialProfile> {
    let scale = tags.number("DistortionScale")?;
    let a = scale * tags.number("DistortionParam08")?;
    let b = scale * tags.number("DistortionParam04")?;
    let c = scale * tags.number("DistortionParam11")?;
    let forward = |r: f64| r * (1.0 + r * r * (a + r * r * (b + r * r * c)));
    // Bracketed inversion avoids a fixed iteration count silently producing a wrong warp.
    if (0..=256).any(|i| {
        let x = i as f64 / 256.0;
        let x2 = x * x;
        1.0 + x2 * (3.0 * a + x2 * (5.0 * b + x2 * 7.0 * c)) <= 0.0
    }) {
        return None;
    }
    let mut table = RadialProfile::default();
    for i in 0..=128 {
        let target = i as f64 / 128.0;
        let mut lo = 0.0;
        let mut hi = 1.0;
        if forward(hi) < target {
            table.knots.push(target);
            table.distortion.push(1.0 / forward(hi));
            continue;
        }
        for _ in 0..48 {
            let mid = (lo + hi) * 0.5;
            if forward(mid) < target {
                lo = mid
            } else {
                hi = mid
            }
        }
        table.knots.push(target);
        table.distortion.push(if i == 0 {
            1.0
        } else {
            (lo + hi) * 0.5 / target
        });
    }
    Some(table)
}
