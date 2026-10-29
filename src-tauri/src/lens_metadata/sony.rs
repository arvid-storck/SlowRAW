// SPDX-License-Identifier: GPL-3.0-or-later
// Models adapted from darktable (Copyright 2019-2026 darktable developers)
// and ART; original source references and notices are in resources/licenses/vendor-lens.txt.
use super::{RadialProfile, Tags};

pub(super) fn decode(tags: &Tags) -> Option<RadialProfile> {
    if tags.number("DistortionCorrParamsPresent") == Some(0.0) {
        return None;
    }
    let d = tags.numbers("DistortionCorrParams")?;
    let prefixed = d.len() == 17;
    let count = if prefixed {
        d[0]
    } else {
        tags.number("DistortionCorrParamsNumber")
            .unwrap_or(d.len() as f64)
    };
    if count.fract() != 0.0 || !(2.0..=16.0).contains(&count) {
        return None;
    }
    let n = count as usize;
    let unpack = |name: &str, ca: bool| -> Option<Vec<f64>> {
        let data = tags.numbers(name)?;
        let expected = if ca { 2 * n } else { n };
        let offset = if data.len() == if ca { 33 } else { 17 } {
            if data[0] != expected as f64 {
                return None;
            }
            1
        } else {
            0
        };
        data.get(offset..offset + expected).map(<[f64]>::to_vec)
    };
    let distortion = unpack("DistortionCorrParams", false)?
        .iter()
        .map(|v| 1.0 + v / 16384.0)
        .collect();
    let ca = unpack("ChromaticAberrationCorrParams", true);
    let gain = unpack("VignettingCorrParams", false)
        .map(|v| {
            v.iter()
                .map(|v| 2_f64.powf(2_f64.powf(v / 8192.0 - 1.0) - 0.5))
                .collect()
        })
        .unwrap_or_default();
    let knots = (0..n)
        .map(|i| (i as f64 + 0.5) / (n - 1) as f64)
        .collect::<Vec<_>>();
    Some(RadialProfile {
        gain_knots: knots.clone(),
        knots,
        distortion,
        red: ca
            .as_ref()
            .map(|v| v[..n].iter().map(|v| 1.0 + v / 2097152.0).collect())
            .unwrap_or_default(),
        blue: ca
            .as_ref()
            .map(|v| v[n..].iter().map(|v| 1.0 + v / 2097152.0).collect())
            .unwrap_or_default(),
        gain,
    })
}
