use crate::file_management::{parse_virtual_path, read_file_mapped};
use crate::formats::is_raw_file;
use crate::image_processing::{PRIMARIES_SRGB, WP_D65, primaries_to_xyz_matrix};
use glam::{DMat3, DVec3, Mat3};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

pub const MIN_TEMPERATURE: f64 = 2000.0;
pub const MAX_TEMPERATURE: f64 = 50000.0;
pub const MAX_TINT: f64 = 150.0;

const TINT_SCALE: f64 = -3000.0;
const ILLUMINANT_A_TEMPERATURE: f64 = 2856.0;
const ILLUMINANT_D65_TEMPERATURE: f64 = 6504.0;
const MIN_LMS_FRACTION_OF_LOCUS: f64 = 0.25;

const BRADFORD: DMat3 = DMat3::from_cols_array(&[
    0.8951, -0.7502, 0.0389, 0.2664, 1.7135, -0.0685, -0.1614, 0.0367, 1.0296,
]);

const ROBERTSON_ISOTHERMS: [[f64; 4]; 31] = [
    [0.0, 0.18006, 0.26352, -0.24341],
    [10.0, 0.18066, 0.26589, -0.25479],
    [20.0, 0.18133, 0.26846, -0.26876],
    [30.0, 0.18208, 0.27119, -0.28539],
    [40.0, 0.18293, 0.27407, -0.30470],
    [50.0, 0.18388, 0.27709, -0.32675],
    [60.0, 0.18494, 0.28021, -0.35156],
    [70.0, 0.18611, 0.28342, -0.37915],
    [80.0, 0.18740, 0.28668, -0.40955],
    [90.0, 0.18880, 0.28997, -0.44278],
    [100.0, 0.19032, 0.29326, -0.47888],
    [125.0, 0.19462, 0.30141, -0.58204],
    [150.0, 0.19962, 0.30921, -0.70471],
    [175.0, 0.20525, 0.31647, -0.84901],
    [200.0, 0.21142, 0.32312, -1.0182],
    [225.0, 0.21807, 0.32909, -1.2168],
    [250.0, 0.22511, 0.33439, -1.4512],
    [275.0, 0.23247, 0.33904, -1.7298],
    [300.0, 0.24010, 0.34308, -2.0637],
    [325.0, 0.24792, 0.34655, -2.4681],
    [350.0, 0.25591, 0.34951, -2.9641],
    [375.0, 0.26400, 0.35200, -3.5814],
    [400.0, 0.27218, 0.35407, -4.3633],
    [425.0, 0.28039, 0.35577, -5.3762],
    [450.0, 0.28863, 0.35714, -6.7262],
    [475.0, 0.29685, 0.35823, -8.5955],
    [500.0, 0.30505, 0.35907, -11.324],
    [525.0, 0.31320, 0.35968, -15.628],
    [550.0, 0.32129, 0.36011, -23.325],
    [575.0, 0.32931, 0.36038, -40.770],
    [600.0, 0.33724, 0.36051, -116.45],
];

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct WhiteBalance {
    pub temperature: f32,
    pub tint: f32,
}

fn isotherm_direction(slope: f64) -> (f64, f64) {
    let len = (1.0 + slope * slope).sqrt();
    (1.0 / len, slope / len)
}

impl WhiteBalance {
    pub fn reference() -> Self {
        Self::from_xy(WP_D65.x as f64, WP_D65.y as f64)
    }

    fn from_xyz(xyz: DVec3) -> Option<Self> {
        let sum = xyz.element_sum();
        if !sum.is_finite() || sum <= 0.0 || xyz.y <= 0.0 {
            return None;
        }
        Some(Self::from_xy(xyz.x / sum, xyz.y / sum))
    }

    pub fn from_xy(x: f64, y: f64) -> Self {
        let denom = 1.5 - x + 6.0 * y;
        let u = 2.0 * x / denom;
        let v = 3.0 * y / denom;

        let mut last_distance = 0.0;
        let mut last_direction = (0.0, 0.0);
        for i in 1..ROBERTSON_ISOTHERMS.len() {
            let [mired, iso_u, iso_v, slope] = ROBERTSON_ISOTHERMS[i];
            let direction = isotherm_direction(slope);
            let distance = (v - iso_v) * direction.0 - (u - iso_u) * direction.1;
            let is_last = i == ROBERTSON_ISOTHERMS.len() - 1;

            if distance <= 0.0 || is_last {
                let distance = (-distance).max(0.0);
                let f = if i == 1 {
                    0.0
                } else {
                    distance / (last_distance + distance)
                };
                let [prev_mired, prev_u, prev_v, _] = ROBERTSON_ISOTHERMS[i - 1];
                let locus_u = prev_u * f + iso_u * (1.0 - f);
                let locus_v = prev_v * f + iso_v * (1.0 - f);
                let du = direction.0 * (1.0 - f) + last_direction.0 * f;
                let dv = direction.1 * (1.0 - f) + last_direction.1 * f;
                let len = (du * du + dv * dv).sqrt();
                let offset = ((u - locus_u) * du + (v - locus_v) * dv) / len;

                return Self {
                    temperature: (1.0e6 / (prev_mired * f + mired * (1.0 - f))) as f32,
                    tint: (offset * TINT_SCALE) as f32,
                };
            }

            last_distance = distance;
            last_direction = direction;
        }
        unreachable!()
    }

    pub fn to_xy(self) -> (f64, f64) {
        let mired = 1.0e6 / f64::from(self.temperature);
        let i = ROBERTSON_ISOTHERMS
            .iter()
            .skip(1)
            .position(|row| mired < row[0])
            .map_or(ROBERTSON_ISOTHERMS.len() - 1, |p| p + 1);

        let [prev_mired, prev_u, prev_v, prev_slope] = ROBERTSON_ISOTHERMS[i - 1];
        let [next_mired, next_u, next_v, next_slope] = ROBERTSON_ISOTHERMS[i];
        let f = (next_mired - mired) / (next_mired - prev_mired);

        let prev_direction = isotherm_direction(prev_slope);
        let next_direction = isotherm_direction(next_slope);
        let du = prev_direction.0 * f + next_direction.0 * (1.0 - f);
        let dv = prev_direction.1 * f + next_direction.1 * (1.0 - f);
        let len = (du * du + dv * dv).sqrt();

        let u = prev_u * f + next_u * (1.0 - f) + du / len * f64::from(self.tint) / TINT_SCALE;
        let v = prev_v * f + next_v * (1.0 - f) + dv / len * f64::from(self.tint) / TINT_SCALE;
        let denom = u - 4.0 * v + 2.0;
        (1.5 * u / denom, v / denom)
    }

    pub fn clamped(self) -> Self {
        Self {
            temperature: if self.temperature.is_finite() {
                self.temperature
                    .clamp(MIN_TEMPERATURE as f32, MAX_TEMPERATURE as f32)
            } else {
                ILLUMINANT_D65_TEMPERATURE as f32
            },
            tint: if self.tint.is_finite() {
                self.tint.clamp(-MAX_TINT as f32, MAX_TINT as f32)
            } else {
                0.0
            },
        }
    }

    fn chromaticity_lms(self) -> DVec3 {
        let (x, y) = self.to_xy();
        BRADFORD * DVec3::new(x / y, 1.0, (1.0 - x - y) / y)
    }

    fn lms(self) -> DVec3 {
        let locus = Self { tint: 0.0, ..self }.chromaticity_lms();
        self.chromaticity_lms()
            .max(locus * MIN_LMS_FRACTION_OF_LOCUS)
    }

    pub fn from_camera_neutral(xyz_to_camera: &[f32], neutral: &[f32]) -> Option<Self> {
        let channels = xyz_to_camera.len() / 3;
        if channels < 3 || neutral.len() < channels {
            return None;
        }
        let (normal, rhs) = xyz_to_camera.as_chunks::<3>().0.iter().zip(neutral).fold(
            (DMat3::ZERO, DVec3::ZERO),
            |(normal, rhs), (row, &n)| {
                let r = DVec3::from_array(row.map(f64::from));
                (
                    normal + DMat3::from_cols(r * r.x, r * r.y, r * r.z),
                    rhs + r * n as f64,
                )
            },
        );
        if normal.determinant().abs() < 1e-12 {
            return None;
        }
        Self::from_xyz(normal.inverse() * rhs)
    }

    pub fn from_dual_illuminant_camera_neutral(
        xyz_to_camera_a: &[f32],
        xyz_to_camera_d65: &[f32],
        neutral: &[f32],
    ) -> Option<Self> {
        if xyz_to_camera_a.len() != xyz_to_camera_d65.len() {
            return Self::from_camera_neutral(xyz_to_camera_d65, neutral);
        }

        let mut estimate = Self::from_camera_neutral(xyz_to_camera_d65, neutral)?;
        for _ in 0..20 {
            let weight = ((1.0 / f64::from(estimate.temperature)
                - 1.0 / ILLUMINANT_D65_TEMPERATURE)
                / (1.0 / ILLUMINANT_A_TEMPERATURE - 1.0 / ILLUMINANT_D65_TEMPERATURE))
                .clamp(0.0, 1.0) as f32;
            let xyz_to_camera: Vec<f32> = xyz_to_camera_a
                .iter()
                .zip(xyz_to_camera_d65)
                .map(|(a, d65)| weight * a + (1.0 - weight) * d65)
                .collect();
            let next = Self::from_camera_neutral(&xyz_to_camera, neutral)?;
            let converged =
                (f64::from(next.temperature) - f64::from(estimate.temperature)).abs() < 0.1;
            estimate = next;
            if converged {
                break;
            }
        }
        Some(estimate)
    }
}

pub fn adaptation_log_gains(current: WhiteBalance, target: WhiteBalance) -> [f32; 3] {
    (current.lms() / target.lms())
        .to_array()
        .map(|gain| gain.ln() as f32)
}

pub fn rgb_to_lms() -> Mat3 {
    (BRADFORD * primaries_to_xyz_matrix(&PRIMARIES_SRGB, WP_D65).as_dmat3()).as_mat3()
}

pub fn from_adjustments(adjustments: &Value, as_shot: WhiteBalance) -> WhiteBalance {
    adjustments
        .get("whiteBalance")
        .and_then(|v| serde_json::from_value::<WhiteBalance>(v.clone()).ok())
        .unwrap_or(as_shot)
        .clamped()
}

pub fn pick_white_balance(sample: [f64; 3], current: WhiteBalance) -> Option<WhiteBalance> {
    let rgb_to_lms = rgb_to_lms().as_dmat3();
    let sample_lms = rgb_to_lms * DVec3::from_array(sample);
    if sample_lms.min_element() <= 0.0 {
        return None;
    }
    let white_lms = rgb_to_lms * DVec3::ONE;
    let illuminant_xyz = BRADFORD.inverse() * (current.lms() * sample_lms / white_lms);
    WhiteBalance::from_xyz(illuminant_xyz).map(WhiteBalance::clamped)
}

type CacheKey = (String, Option<std::time::SystemTime>, u64);
fn as_shot_cache() -> &'static Mutex<HashMap<CacheKey, WhiteBalance>> {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, WhiteBalance>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn as_shot_white_balance(path: &str) -> WhiteBalance {
    let (source_path, _) = parse_virtual_path(path);
    let source_path = source_path.to_string_lossy().to_string();
    if !is_raw_file(&source_path) {
        return WhiteBalance::reference();
    }
    let metadata = std::fs::metadata(&source_path).ok();
    let key = (
        source_path.clone(),
        metadata.as_ref().and_then(|m| m.modified().ok()),
        metadata.as_ref().map_or(0, |m| m.len()),
    );
    if let Some(cached) = as_shot_cache().lock().unwrap().get(&key) {
        return *cached;
    }

    let white_balance = read_file_mapped(Path::new(&source_path))
        .ok()
        .and_then(|mmap| {
            std::panic::catch_unwind(|| crate::raw_processing::read_as_shot_white_balance(&mmap))
                .ok()
                .flatten()
        })
        .unwrap_or_else(WhiteBalance::reference)
        .clamped();

    let mut cache = as_shot_cache().lock().unwrap();
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(key, white_balance);
    white_balance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_stored_as_f32_and_coordinates_calculated_as_f64() {
        assert_eq!(std::mem::size_of::<WhiteBalance>(), 8);
        let value = WhiteBalance {
            temperature: 5432.123456_f64 as f32,
            tint: -12.3456789_f64 as f32,
        };
        let encoded = serde_json::to_string(&value).unwrap();
        let decoded: WhiteBalance = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, value);
        let xy: (f64, f64) = decoded.to_xy();
        assert!(xy.0.is_finite() && xy.1.is_finite());
    }

    #[test]
    fn equal_white_balances_have_identity_gains() {
        let white = WhiteBalance::reference();
        assert_eq!(adaptation_log_gains(white, white), [0.0; 3]);
    }

    #[test]
    fn kelvin_and_tint_round_trip() {
        for temperature in [2000.0, 2856.0, 5000.0, 6504.0, 10000.0, 25000.0, 50000.0] {
            for tint in [-50.0, 0.0, 50.0] {
                let original = WhiteBalance { temperature, tint };
                let (x, y) = original.to_xy();
                let roundtrip = WhiteBalance::from_xy(x, y);
                assert!(
                    (roundtrip.temperature / temperature - 1.0).abs() < 0.015,
                    "{original:?} -> {roundtrip:?}"
                );
                assert!((roundtrip.tint - tint).abs() < 0.5);
            }
        }
    }

    #[test]
    fn sampled_color_becomes_neutral() {
        let current = WhiteBalance::reference();
        let rgb = DVec3::new(0.7, 0.5, 0.3);
        let target = pick_white_balance(rgb.to_array(), current).unwrap();
        let gains =
            DVec3::from_array(adaptation_log_gains(current, target).map(|v| (v as f64).exp()));
        let matrix = rgb_to_lms().as_dmat3();
        let balanced = matrix.inverse() * ((matrix * rgb) * gains);
        assert!((balanced.x - balanced.y).abs() < 0.005, "{balanced:?}");
        assert!((balanced.z - balanced.y).abs() < 0.005, "{balanced:?}");
    }

    #[test]
    fn invalid_samples_and_singular_camera_matrix_are_rejected() {
        assert!(pick_white_balance([0.0; 3], WhiteBalance::reference()).is_none());
        assert!(pick_white_balance([f64::NAN; 3], WhiteBalance::reference()).is_none());
        assert!(WhiteBalance::from_camera_neutral(&[0.0; 9], &[1.0; 3]).is_none());
    }

    #[test]
    fn only_kelvin_object_controls_white_balance() {
        let reference = WhiteBalance::reference();
        assert_eq!(
            from_adjustments(
                &serde_json::json!({"temperature": 99, "tint": -99}),
                reference
            ),
            reference
        );
        let target = from_adjustments(
            &serde_json::json!({"whiteBalance":{"temperature":4500,"tint":12}}),
            reference,
        );
        assert_eq!(
            target,
            WhiteBalance {
                temperature: 4500.0,
                tint: 12.0
            }
        );
    }
}
