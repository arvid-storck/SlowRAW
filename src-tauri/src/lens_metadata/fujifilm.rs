// SPDX-License-Identifier: GPL-3.0-or-later
// Models adapted from darktable (Copyright 2019-2026 darktable developers)
// and ART; original source references and notices are in resources/licenses/vendor-lens.txt.
use super::{RadialProfile, Tags, interpolate};

pub(super) fn decode(tags: &Tags) -> Option<RadialProfile> {
    let d = tags.numbers("GeometricDistortionParams")?;
    let c = tags.numbers("ChromaticAberrationParams");
    let v = tags.numbers("VignettingParams");
    let n = match d.len() {
        19 => 9,
        23 => 11,
        _ => return None,
    };
    let mut knots = d[1..n + 1].to_vec();
    let mut dist = d[n + 1..]
        .iter()
        .map(|v| 1.0 + v / 100.0)
        .collect::<Vec<_>>();
    let mut red = Vec::new();
    let mut blue = Vec::new();
    if let Some(c) = c {
        if n == 9 && c.len() == 29 && c[1..10] == knots {
            red = c[10..19].iter().map(|v| 1.0 + v).collect();
            blue = c[19..28].iter().map(|v| 1.0 + v).collect();
        } else if n == 11 && c.len() == 31 && c[1..11] == knots[1..] {
            red = std::iter::once(1.0)
                .chain(c[11..21].iter().map(|v| 1.0 + v))
                .collect();
            blue = std::iter::once(1.0)
                .chain(c[21..31].iter().map(|v| 1.0 + v))
                .collect();
        } else {
            return None;
        }
    }
    let mut gain = if let Some(v) = v {
        if v.len() != d.len() || v[1..n + 1] != knots {
            return None;
        }
        v[n + 1..].iter().map(|v| 100.0 / v).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if matches!(tags.number("CropMode"), Some(2.0) | Some(4.0)) {
        for k in &mut knots {
            *k *= 1.25;
        }
    }
    if knots.first().is_some_and(|v| *v > 0.0) {
        knots.insert(0, 0.0);
        dist.insert(0, 1.0);
        if !red.is_empty() {
            red.insert(0, 1.0);
            blue.insert(0, 1.0);
        }
        if !gain.is_empty() {
            gain.insert(0, 1.0);
        }
    }
    if knots.windows(2).any(|w| w[1] <= w[0]) || dist.iter().any(|v| *v <= 0.0) {
        return None;
    }
    // Fuji tables are indexed by source radius. Convert the warp/TCA to output-radius knots.
    let mut table = RadialProfile {
        gain_knots: knots.clone(),
        gain,
        ..Default::default()
    };
    for i in 0..=128 {
        let source = i as f64 / 128.0;
        let multiplier = interpolate(&knots, &dist, source);
        table.knots.push(source / multiplier);
        table.distortion.push(multiplier);
        if !red.is_empty() {
            table.red.push(interpolate(&knots, &red, source));
            table.blue.push(interpolate(&knots, &blue, source));
        }
    }
    Some(table)
}
