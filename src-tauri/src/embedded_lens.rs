//! DNG 1.3 lens opcodes, read by official ExifTool; coordinates follow Adobe DNG chapter 7.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddedLensProfile {
    #[serde(default = "dng_source")]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radial: Option<crate::lens_metadata::RadialProfile>,
    pub warp: Option<Warp>,
    pub vignette: Option<Vignette>,
    pub frame: [f64; 2],
    pub crop: [f64; 4],
    pub orientation: u32,
}

fn dng_source() -> String {
    "DNG".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Warp {
    #[serde(default)]
    pub fisheye: bool,
    pub coefficients: Vec<[f64; 6]>,
    pub center: [f64; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vignette {
    pub coefficients: [f64; 5],
    pub center: [f64; 2],
}

fn numbers(value: &Value) -> Option<Vec<f64>> {
    let values = if let Some(array) = value.as_array() {
        array
            .iter()
            .map(Value::as_f64)
            .collect::<Option<Vec<_>>>()?
    } else if let Some(number) = value.as_f64() {
        vec![number]
    } else {
        value
            .as_str()?
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|v| !v.is_empty())
            .map(str::parse)
            .collect::<Result<Vec<_>, _>>()
            .ok()?
    };
    values.iter().all(|n| n.is_finite()).then_some(values)
}

pub fn from_metadata(metadata: &Value) -> Option<EmbeddedLensProfile> {
    from_dng_metadata(metadata)
        .or_else(|| crate::lens_metadata::from_metadata(metadata))
        .filter(EmbeddedLensProfile::has_correction)
}

fn from_dng_metadata(metadata: &Value) -> Option<EmbeddedLensProfile> {
    let tags = metadata.as_array()?.first()?.as_object()?;
    let tag = |name: &str| {
        tags.iter()
            .find(|(key, _)| key.rsplit(':').next() == Some(name))
            .map(|(_, value)| value)
    };
    tag("DNGVersion")?;
    let dimension = |name: &str| {
        tags.get(&format!("SubIFD:{name}"))
            .or_else(|| tags.get(&format!("IFD0:{name}")))
            .and_then(numbers)?
            .first()
            .copied()
    };
    let size = [dimension("ImageWidth")?, dimension("ImageHeight")?];
    let active = tag("ActiveArea")
        .and_then(numbers)
        .unwrap_or_else(|| vec![0.0, 0.0, size[1], size[0]]);
    if active.len() != 4 {
        return None;
    }
    let frame = [active[3] - active[1], active[2] - active[0]];
    let origin = tag("DefaultCropOrigin")
        .and_then(numbers)
        .unwrap_or_else(|| vec![0.0, 0.0]);
    let crop_size = tag("DefaultCropSize")
        .and_then(numbers)
        .unwrap_or_else(|| frame.to_vec());
    if origin.len() != 2
        || crop_size.len() != 2
        || frame.iter().any(|v| *v < 2.0)
        || crop_size.iter().any(|v| *v < 2.0)
    {
        return None;
    }
    if let Some(scale) = tag("DefaultScale").and_then(numbers) {
        if scale.len() != 2 || (scale[0] - scale[1]).abs() > 1e-6 {
            return None;
        }
    }
    let mut profile = EmbeddedLensProfile {
        source: dng_source(),
        radial: None,
        warp: None,
        vignette: None,
        frame,
        crop: [origin[0], origin[1], crop_size[0], crop_size[1]],
        orientation: tag("Orientation").and_then(Value::as_u64).unwrap_or(1) as u32,
    };
    for (name, stage) in [("OpcodeList2", 2), ("OpcodeList3", 3)] {
        if let Some(value) = tag(name) {
            let bytes = STANDARD
                .decode(value.as_str()?.strip_prefix("base64:")?)
                .ok()?;
            profile.parse_list(&bytes, stage)?;
        }
    }
    (profile.warp.is_some() || profile.vignette.is_some()).then_some(profile)
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let bytes = self.0.get(..count)?;
        self.0 = &self.0[count..];
        Some(bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn doubles<const N: usize>(&mut self) -> Option<[f64; N]> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = f64::from_be_bytes(self.take(8)?.try_into().ok()?);
            if !value.is_finite() {
                return None;
            }
        }
        Some(values)
    }
}

impl EmbeddedLensProfile {
    pub fn auto_crop_scale(&self, distortion: f64, tca: f64) -> f64 {
        if !self.has_warp() || (distortion == 0.0 && tca == 0.0) {
            return 1.0;
        }
        let mut scale = 1.0;
        for _ in 0..12 {
            let mut overflow = 1.0_f64;
            for step in 0..=32 {
                let edge = step as f64 / 32.0;
                for [x, y] in [[edge, 0.0], [edge, 1.0], [0.0, edge], [1.0, edge]] {
                    for plane in 0..3 {
                        let (uv, _) = self.sample(
                            [0.5 + (x - 0.5) / scale, 0.5 + (y - 0.5) / scale],
                            plane,
                            distortion,
                            tca,
                            0.0,
                        );
                        overflow = overflow
                            .max((2.0 * uv[0] - 1.0).abs())
                            .max((2.0 * uv[1] - 1.0).abs());
                    }
                }
            }
            if overflow <= 1.00001 {
                break;
            }
            scale *= overflow * 1.001;
        }
        scale
    }

    fn parse_list(&mut self, bytes: &[u8], stage: u32) -> Option<()> {
        let mut list = Reader(bytes);
        let count = list.u32()?;
        if count > 1024 {
            return None;
        }
        for _ in 0..count {
            let id = list.u32()?;
            let version = list.u32()?;
            let flags = list.u32()?;
            let size = list.u32()? as usize;
            let mut data = Reader(list.take(size)?);
            // WarpRectilinear2 has ordering rules for a following legacy warp.
            // Fall back rather than interpreting that list as the older model.
            if id == 14 {
                return None;
            }
            if version > 0x01030000 || !matches!((stage, id), (2, 3) | (3, 1) | (3, 2)) {
                if flags & 1 != 0 {
                    continue;
                } else {
                    return None;
                }
            }
            match id {
                1 | 2 => {
                    if self.warp.is_some() {
                        return None;
                    }
                    let planes = data.u32()?;
                    if !matches!(planes, 1 | 3) {
                        return None;
                    }
                    let coefficients = (0..planes)
                        .map(|_| {
                            if id == 1 {
                                data.doubles()
                            } else {
                                data.doubles::<4>()
                                    .map(|k| [k[0], k[1], k[2], k[3], 0.0, 0.0])
                            }
                        })
                        .collect::<Option<Vec<[f64; 6]>>>()?;
                    let center = data.doubles()?;
                    if center.iter().any(|c| !(0.0..=1.0).contains(c)) {
                        return None;
                    }
                    self.warp = Some(Warp {
                        fisheye: id == 2,
                        coefficients,
                        center,
                    });
                }
                3 => {
                    if self.vignette.is_some() {
                        return None;
                    }
                    let coefficients = data.doubles()?;
                    let center = data.doubles()?;
                    if center.iter().any(|c| !(0.0..=1.0).contains(c)) {
                        return None;
                    }
                    self.vignette = Some(Vignette {
                        coefficients,
                        center,
                    });
                }
                _ => unreachable!(),
            }
            if !data.0.is_empty() {
                return None;
            }
        }
        list.0.is_empty().then_some(())
    }

    fn orient(&self, [x, y]: [f64; 2], inverse: bool) -> [f64; 2] {
        let orientation = if inverse {
            match self.orientation {
                6 => 8,
                8 => 6,
                other => other,
            }
        } else {
            self.orientation
        };
        match orientation {
            2 => [1.0 - x, y],
            3 => [1.0 - x, 1.0 - y],
            4 => [x, 1.0 - y],
            5 => [y, x],
            6 => [1.0 - y, x],
            7 => [1.0 - y, 1.0 - x],
            8 => [y, 1.0 - x],
            _ => [x, y],
        }
    }

    fn radius(&self, point: [f64; 2], center: [f64; 2]) -> ([f64; 2], [f64; 2], f64) {
        let center = [
            center[0] * (self.frame[0] - 1.0),
            center[1] * (self.frame[1] - 1.0),
        ];
        let radius = center[0]
            .max(self.frame[0] - 1.0 - center[0])
            .hypot(center[1].max(self.frame[1] - 1.0 - center[1]));
        (
            [
                (point[0] - center[0]) / radius,
                (point[1] - center[1]) / radius,
            ],
            center,
            radius,
        )
    }

    fn warped(&self, point: [f64; 2], plane: usize) -> [f64; 2] {
        let Some(warp) = &self.warp else {
            return point;
        };
        let k = warp.coefficients[if warp.coefficients.len() == 1 {
            0
        } else {
            plane
        }];
        let ([x, y], center, radius) = self.radius(point, warp.center);
        let r2 = x * x + y * y;
        let radial = if warp.fisheye && r2 > 1e-16 {
            let angle = r2.sqrt().atan();
            let a2 = angle * angle;
            angle / r2.sqrt() * (k[0] + a2 * (k[1] + a2 * (k[2] + a2 * k[3])))
        } else {
            k[0] + r2 * (k[1] + r2 * (k[2] + r2 * k[3]))
        };
        [
            center[0] + radius * (x * radial + 2.0 * k[4] * x * y + k[5] * (r2 + 2.0 * x * x)),
            center[1] + radius * (y * radial + 2.0 * k[5] * x * y + k[4] * (r2 + 2.0 * y * y)),
        ]
    }

    pub fn sample(
        &self,
        uv: [f64; 2],
        plane: usize,
        distortion: f64,
        tca: f64,
        vignette: f64,
    ) -> ([f64; 2], f32) {
        let uv = self.orient(uv, true);
        let point = [
            self.crop[0] + uv[0] * (self.crop[2] - 1.0),
            self.crop[1] + uv[1] * (self.crop[3] - 1.0),
        ];
        if let Some(table) = &self.radial {
            let ([x, y], center, radius) = self.radius(point, [0.5, 0.5]);
            let r = x.hypot(y);
            let factor = table.factor(r, plane, distortion, tca);
            let mapped = [
                center[0] + radius * x * factor,
                center[1] + radius * y * factor,
            ];
            let gain = table.gain(r * factor, vignette);
            return (
                self.orient(
                    [
                        (mapped[0] - self.crop[0]) / (self.crop[2] - 1.0),
                        (mapped[1] - self.crop[1]) / (self.crop[3] - 1.0),
                    ],
                    false,
                ),
                gain as f32,
            );
        }
        let green = self.warped(point, 1);
        let channel = self.warped(point, plane);
        let mapped = [
            point[0] + distortion * (green[0] - point[0]) + tca * (channel[0] - green[0]),
            point[1] + distortion * (green[1] - point[1]) + tca * (channel[1] - green[1]),
        ];
        let gain = self
            .vignette
            .as_ref()
            .map(|profile| {
                let ([x, y], _, _) = self.radius(mapped, profile.center);
                let r2 = x * x + y * y;
                let k = profile.coefficients;
                (1.0 + vignette * r2 * (k[0] + r2 * (k[1] + r2 * (k[2] + r2 * (k[3] + r2 * k[4])))))
                    .max(0.0) as f32
            })
            .unwrap_or(1.0);
        (
            self.orient(
                [
                    (mapped[0] - self.crop[0]) / (self.crop[2] - 1.0),
                    (mapped[1] - self.crop[1]) / (self.crop[3] - 1.0),
                ],
                false,
            ),
            gain,
        )
    }

    pub fn has_warp(&self) -> bool {
        self.warp.is_some()
            || self.radial.as_ref().is_some_and(|p| {
                !p.distortion.is_empty() || !p.red.is_empty() || !p.blue.is_empty()
            })
    }
    pub fn has_vignette(&self) -> bool {
        self.vignette.is_some() || self.radial.as_ref().is_some_and(|p| !p.gain.is_empty())
    }

    /// Empty/neutral camera tables do not constitute a usable built-in profile.
    pub fn has_correction(&self) -> bool {
        let differs =
            |values: &[f64], neutral: f64| values.iter().any(|v| (v - neutral).abs() > 1e-12);
        self.radial.as_ref().is_some_and(|p| {
            differs(&p.distortion, 1.0)
                || differs(&p.red, 1.0)
                || differs(&p.blue, 1.0)
                || differs(&p.gain, 1.0)
        }) || self
            .vignette
            .as_ref()
            .is_some_and(|v| differs(&v.coefficients, 0.0))
            || self.warp.as_ref().is_some_and(|w| {
                w.coefficients.iter().any(|k| {
                    differs(k, 0.0)
                        && (w.fisheye || differs(&k[1..], 0.0) || (k[0] - 1.0).abs() > 1e-12)
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn opcode(coefficients: [f64; 6]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for number in [1_u32, 1, 0x01030000, 0, 68, 1] {
            bytes.extend(number.to_be_bytes());
        }
        for number in coefficients.into_iter().chain([0.5, 0.5]) {
            bytes.extend(number.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn official_reader_extracts_dng_opcodes_and_geometry_uses_them() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("profile.dng");
        let payload = opcode([1.0, 0.1, 0.0, 0.0, 0.0, 0.0]);
        // Minimal TIFF/DNG metadata fixture. Pixel decoding is not needed to read opcodes.
        let mut tiff = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        tiff.extend(4_u16.to_le_bytes());
        for (tag, kind, count, value) in [
            (256_u16, 4_u16, 1_u32, 101_u32),
            (257, 4, 1, 101),
            (50706, 1, 4, 0x00000301),
            (51022, 7, payload.len() as u32, 62),
        ] {
            tiff.extend(tag.to_le_bytes());
            tiff.extend(kind.to_le_bytes());
            tiff.extend(count.to_le_bytes());
            tiff.extend(value.to_le_bytes());
        }
        tiff.extend(0_u32.to_le_bytes());
        tiff.extend(payload);
        std::fs::write(&path, tiff).unwrap();
        let metadata = crate::exif_processing::read_source_metadata(&path).unwrap();
        assert_eq!(metadata.fields["ImageWidth"], "101");
        let cached = crate::exif_processing::read_source_metadata(&path).unwrap();
        assert_eq!(
            serde_json::to_value(&cached).unwrap(),
            serde_json::to_value(&metadata).unwrap()
        );
        let profile = metadata.lens_profile.expect("Missing DNG profile");
        let (mapped, gain) = profile.sample([1.0, 0.5], 1, 1.0, 0.0, 0.0);
        assert!((mapped[0] - 1.025).abs() < 1e-8);
        assert_eq!(gain, 1.0);
        let (neutral, _) = profile.sample([1.0, 0.5], 1, 0.0, 0.0, 0.0);
        assert_eq!(neutral, [1.0, 0.5]);
        assert!(profile.auto_crop_scale(1.0, 1.0) > 1.0);
        let mut adjustments = serde_json::json!({"lensDistortionParams":{"embedded":profile}});
        assert!(!crate::image_processing::is_geometry_identity(
            &crate::image_processing::get_geometry_params_from_json(&adjustments)
        ));
        adjustments["lensDistortionEnabled"] = false.into();
        adjustments["lensTcaEnabled"] = false.into();
        adjustments["lensVignetteEnabled"] = false.into();
        assert!(crate::image_processing::is_geometry_identity(
            &crate::image_processing::get_geometry_params_from_json(&adjustments)
        ));
    }

    #[test]
    fn fisheye_opcode_maps_radius_and_preserves_disabled_identity() {
        let mut profile = EmbeddedLensProfile {
            source: dng_source(),
            radial: None,
            warp: None,
            vignette: None,
            frame: [101.0, 101.0],
            crop: [0.0, 0.0, 101.0, 101.0],
            orientation: 1,
        };
        let mut bytes = Vec::new();
        for value in [1_u32, 2, 0x01030000, 0, 52, 1] {
            bytes.extend(value.to_be_bytes());
        }
        for value in [1.0_f64, 0.0, 0.0, 0.0, 0.5, 0.5] {
            bytes.extend(value.to_be_bytes());
        }
        profile.parse_list(&bytes, 3).unwrap();
        let r = 0.5_f64.sqrt();
        let (mapped, _) = profile.sample([1.0, 0.5], 1, 1.0, 0.0, 0.0);
        assert!((mapped[0] - (0.5 + 0.5 * r.atan() / r)).abs() < 1e-12);
        assert_eq!(
            profile.sample([1.0, 0.5], 1, 0.0, 0.0, 0.0),
            ([1.0, 0.5], 1.0)
        );
        profile.orientation = 6;
        assert_eq!(
            profile.sample([0.25, 0.75], 1, 0.0, 0.0, 0.0),
            ([0.25, 0.75], 1.0)
        );
    }

    #[test]
    fn malformed_or_unknown_required_opcodes_are_rejected() {
        let mut profile = EmbeddedLensProfile {
            source: dng_source(),
            radial: None,
            warp: None,
            vignette: None,
            frame: [101.0, 101.0],
            crop: [0.0, 0.0, 101.0, 101.0],
            orientation: 1,
        };
        let mut valid = opcode([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(profile.parse_list(&valid[..valid.len() - 1], 3).is_none());
        valid[4..8].copy_from_slice(&99_u32.to_be_bytes());
        assert!(profile.parse_list(&valid, 3).is_none());
        valid[12..16].copy_from_slice(&1_u32.to_be_bytes());
        assert!(profile.parse_list(&valid, 3).is_some());
    }
}
