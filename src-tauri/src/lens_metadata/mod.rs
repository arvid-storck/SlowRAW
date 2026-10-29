// SPDX-License-Identifier: GPL-3.0-or-later
// Models adapted from darktable (Copyright 2019-2026 darktable developers)
// and ART; original source references and notices are in resources/licenses/vendor-lens.txt.
//! Interpret correction data from the shared ExifTool metadata result.
mod fujifilm;
mod nikon;
mod olympus;
mod panasonic;
mod sony;
#[cfg(test)]
mod tests;

use crate::embedded_lens::EmbeddedLensProfile;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RadialProfile {
    pub knots: Vec<f64>,
    pub distortion: Vec<f64>,
    pub red: Vec<f64>,
    pub blue: Vec<f64>,
    pub gain_knots: Vec<f64>,
    pub gain: Vec<f64>,
}

fn interpolate(knots: &[f64], values: &[f64], radius: f64) -> f64 {
    if values.is_empty() {
        return 1.0;
    }
    let right = knots.partition_point(|k| *k < radius);
    if right == 0 {
        return values[0];
    }
    if right >= knots.len() {
        return *values.last().unwrap();
    }
    let t = (radius - knots[right - 1]) / (knots[right] - knots[right - 1]);
    values[right - 1] + t * (values[right] - values[right - 1])
}

impl RadialProfile {
    pub fn factor(&self, radius: f64, plane: usize, distortion: f64, tca: f64) -> f64 {
        let base = 1.0 + distortion * (interpolate(&self.knots, &self.distortion, radius) - 1.0);
        let ca = match plane {
            0 => &self.red,
            2 => &self.blue,
            _ => return base,
        };
        base * (1.0 + tca * (interpolate(&self.knots, ca, radius) - 1.0))
    }
    pub fn gain(&self, radius: f64, amount: f64) -> f64 {
        1.0 + amount * (interpolate(&self.gain_knots, &self.gain, radius) - 1.0)
    }
    fn valid(&self) -> bool {
        let curve = |knots: &[f64], values: &[f64]| {
            values.is_empty()
                || (knots.len() >= 2
                    && knots.len() == values.len()
                    && knots.iter().all(|v| v.is_finite() && *v >= 0.0)
                    && knots.windows(2).all(|w| w[1] > w[0])
                    && values
                        .iter()
                        .all(|v| v.is_finite() && *v > 0.0 && *v < 64.0))
        };
        curve(&self.knots, &self.distortion)
            && curve(&self.knots, &self.red)
            && curve(&self.knots, &self.blue)
            && curve(&self.gain_knots, &self.gain)
            && (!self.distortion.is_empty()
                || !self.red.is_empty()
                || !self.blue.is_empty()
                || !self.gain.is_empty())
            && (self.distortion.is_empty()
                || self
                    .knots
                    .iter()
                    .zip(&self.distortion)
                    .map(|(r, m)| r * m)
                    .collect::<Vec<_>>()
                    .windows(2)
                    .all(|w| w[1] > w[0]))
    }
}

struct Tags<'a>(&'a Map<String, Value>);
impl Tags<'_> {
    fn value(&self, name: &str) -> Option<&Value> {
        // Raw image fields take precedence over preview/makernote duplicates.
        ["SubIFD", "ExifIFD", "IFD0"]
            .iter()
            .find_map(|group| self.0.get(&format!("{group}:{name}")))
            .or_else(|| {
                self.0
                    .iter()
                    .find(|(key, _)| key.rsplit(':').next() == Some(name))
                    .map(|(_, v)| v)
            })
    }
    fn numbers(&self, name: &str) -> Option<Vec<f64>> {
        let value = self.value(name)?;
        let values = if let Some(a) = value.as_array() {
            a.iter().map(Value::as_f64).collect::<Option<Vec<_>>>()?
        } else if let Some(v) = value.as_f64() {
            vec![v]
        } else {
            value
                .as_str()?
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .map(str::parse)
                .collect::<Result<Vec<_>, _>>()
                .ok()?
        };
        values.iter().all(|v| v.is_finite()).then_some(values)
    }
    fn number(&self, name: &str) -> Option<f64> {
        self.numbers(name)?.first().copied()
    }
}

pub fn from_metadata(metadata: &Value) -> Option<EmbeddedLensProfile> {
    let tags = Tags(metadata.as_array()?.first()?.as_object()?);
    if tags.value("DNGVersion").is_some() {
        return None;
    }
    let maker = tags.value("Make")?.as_str()?.to_ascii_lowercase();
    let (source, table) = if maker.starts_with("sony") {
        ("Sony", sony::decode(&tags)?)
    } else if maker.starts_with("fuji") {
        ("Fujifilm", fujifilm::decode(&tags)?)
    } else if maker.starts_with("nikon") {
        ("Nikon", nikon::decode(&tags)?)
    } else if maker.starts_with("olympus")
        || maker.starts_with("om digital")
        || maker.starts_with("om system")
    {
        ("Olympus / OM System", olympus::decode(&tags)?)
    } else if maker.starts_with("panasonic") || maker.starts_with("leica") {
        ("Panasonic / Leica", panasonic::decode(&tags)?)
    } else {
        return None;
    };
    if !table.valid() {
        return None;
    }
    let width = tags
        .number("ExifImageWidth")
        .or_else(|| tags.number("ImageWidth"))?;
    let height = tags
        .number("ExifImageHeight")
        .or_else(|| tags.number("ImageHeight"))?;
    if width < 2.0 || height < 2.0 {
        return None;
    }
    Some(EmbeddedLensProfile {
        source: source.into(),
        radial: Some(table),
        warp: None,
        vignette: None,
        frame: [width, height],
        crop: [0.0, 0.0, width, height],
        orientation: tags.number("Orientation").unwrap_or(1.0) as u32,
    })
}
