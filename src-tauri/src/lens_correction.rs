use crate::AppState;
use crate::embedded_lens::EmbeddedLensProfile;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use tauri::{Manager, State};
use walkdir::WalkDir;

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Distortion {
    #[serde(rename = "@model")]
    pub model: String,
    #[serde(rename = "@focal")]
    pub focal: f32,
    #[serde(rename = "@real-focal")]
    pub real_focal: Option<f32>,
    #[serde(rename = "@k1")]
    pub k1: Option<f32>,
    #[serde(rename = "@k2")]
    pub k2: Option<f32>,
    #[serde(rename = "@k3")]
    pub k3: Option<f32>,
    #[serde(rename = "@a")]
    pub a: Option<f32>,
    #[serde(rename = "@b")]
    pub b: Option<f32>,
    #[serde(rename = "@c")]
    pub c: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Tca {
    #[serde(rename = "@model")]
    pub model: String,
    #[serde(rename = "@focal")]
    pub focal: f32,
    #[serde(rename = "@vr")]
    pub vr: Option<f32>,
    #[serde(rename = "@vb")]
    pub vb: Option<f32>,
    #[serde(rename = "@cr")]
    pub cr: Option<f32>,
    #[serde(rename = "@cb")]
    pub cb: Option<f32>,
    #[serde(rename = "@br")]
    pub br: Option<f32>,
    #[serde(rename = "@bb")]
    pub bb: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Vignetting {
    #[serde(rename = "@model")]
    pub model: String,
    #[serde(rename = "@focal")]
    pub focal: f32,
    #[serde(rename = "@aperture")]
    pub aperture: f32,
    #[serde(rename = "@distance")]
    pub distance: Option<f32>,
    #[serde(rename = "@k1")]
    pub k1: Option<f32>,
    #[serde(rename = "@k2")]
    pub k2: Option<f32>,
    #[serde(rename = "@k3")]
    pub k3: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum CalibrationElement {
    Distortion(Distortion),
    Tca(Tca),
    Vignetting(Vignetting),
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Calibration {
    #[serde(rename = "$value", default)]
    pub elements: Vec<CalibrationElement>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Focal {
    #[serde(rename = "@value")]
    pub value: Option<f32>,
    #[serde(rename = "@min")]
    pub min: Option<f32>,
    #[serde(rename = "@max")]
    pub max: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Aperture {
    #[serde(rename = "@min")]
    pub min: Option<f32>,
    #[serde(rename = "@max")]
    pub max: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Lens {
    #[serde(default)]
    pub maker: Vec<MultiName>,
    #[serde(default)]
    pub model: Vec<MultiName>,
    #[serde(default)]
    pub mount: Vec<String>,
    pub cropfactor: Option<f32>,
    pub calibration: Option<Calibration>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub focal: Option<Focal>,
    pub aspect_ratio: Option<String>,
    pub center: Option<String>,
    pub compat: Option<String>,
    pub notes: Option<String>,
    pub aperture: Option<Aperture>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Camera {
    pub maker: Vec<MultiName>,
    pub model: Vec<MultiName>,
    pub mount: String,
    pub cropfactor: f32,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct LensDatabase {
    #[serde(rename = "camera", default)]
    pub cameras: Vec<Camera>,
    #[serde(rename = "lens", default)]
    pub lenses: Vec<Lens>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct MultiName {
    #[serde(rename = "@lang")]
    lang: Option<String>,
    #[serde(rename = "$value")]
    value: String,
}

#[derive(Serialize)]
pub struct LensDistortionParams {
    k1: f64,
    k2: f64,
    k3: f64,
    model: u32,
    tca_vr: f64,
    tca_vb: f64,
    vig_k1: f64,
    vig_k2: f64,
    vig_k3: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    embedded: Option<EmbeddedLensProfile>,
}

fn strip_maker_prefix(name: &str, maker: &str) -> String {
    if name.to_lowercase().starts_with(&maker.to_lowercase())
        && let Some(rest) = name.get(maker.len()..)
    {
        let trimmed = rest.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    name.to_string()
}

impl Lens {
    pub fn get_full_model_name(&self) -> String {
        self.model
            .iter()
            .find(|m| m.lang.as_deref() == Some("en"))
            .or_else(|| self.model.first())
            .map(|m| m.value.clone())
            .unwrap_or_else(|| "Unknown Model".to_string())
    }

    pub fn get_canonical_model_name(&self) -> String {
        self.model
            .iter()
            .find(|m| m.lang.is_none())
            .or_else(|| self.model.first())
            .map(|m| m.value.clone())
            .unwrap_or_else(|| "Unknown Model".to_string())
    }

    pub fn get_name(&self) -> String {
        let raw_name = self.get_full_model_name();
        let maker = self.get_maker();

        if raw_name.to_lowercase().starts_with(&maker.to_lowercase())
            && let Some(rest) = raw_name.get(maker.len()..)
        {
            let stripped = rest.trim();
            if !stripped.is_empty() {
                return stripped.to_string();
            }
        }

        raw_name
    }

    pub fn get_maker(&self) -> String {
        self.maker
            .iter()
            .find(|m| m.lang.as_deref() == Some("en"))
            .or_else(|| self.maker.first())
            .map(|m| m.value.clone())
            .unwrap_or_else(|| "Misc".to_string())
    }

    pub fn get_display_name(&self, all_maker_lenses: &[&Lens]) -> String {
        let my_short = self.get_name();
        let short_count = all_maker_lenses
            .iter()
            .filter(|l| l.get_name() == my_short)
            .count();

        if short_count <= 1 {
            return my_short;
        }

        let maker = self.get_maker();
        let my_canonical_short = strip_maker_prefix(&self.get_canonical_model_name(), &maker);

        let canonical_short_count = all_maker_lenses
            .iter()
            .filter(|l| {
                strip_maker_prefix(&l.get_canonical_model_name(), &l.get_maker())
                    == my_canonical_short
            })
            .count();

        if canonical_short_count <= 1 {
            return my_canonical_short;
        }

        let my_canonical = self.get_canonical_model_name();
        let canonical_count = all_maker_lenses
            .iter()
            .filter(|l| l.get_canonical_model_name() == my_canonical)
            .count();

        if canonical_count <= 1 {
            return my_canonical;
        }

        if let Some(cf) = self.cropfactor {
            format!("{} (crop {:.1}x)", my_canonical_short, cf)
        } else {
            my_canonical_short
        }
    }

    pub fn get_distortion_params(
        &self,
        focal_length: f32,
        aperture: Option<f32>,
        distance: Option<f32>,
    ) -> Option<LensDistortionParams> {
        let cal = self.calibration.as_ref()?;

        let mut distortions: Vec<&Distortion> = cal
            .elements
            .iter()
            .filter_map(|e| {
                if let CalibrationElement::Distortion(d) = e {
                    Some(d)
                } else {
                    None
                }
            })
            .collect();

        let mut tcas: Vec<&Tca> = cal
            .elements
            .iter()
            .filter_map(|e| {
                if let CalibrationElement::Tca(t) = e {
                    Some(t)
                } else {
                    None
                }
            })
            .collect();

        let mut vignettings: Vec<&Vignetting> = cal
            .elements
            .iter()
            .filter_map(|e| {
                if let CalibrationElement::Vignetting(v) = e {
                    Some(v)
                } else {
                    None
                }
            })
            .collect();

        let (k1, k2, k3, model) = if distortions.is_empty() {
            (0.0, 0.0, 0.0, 0)
        } else {
            distortions.sort_by(|a, b| a.focal.partial_cmp(&b.focal).unwrap_or(Ordering::Equal));

            if let Some(exact) = distortions
                .iter()
                .find(|d| (d.focal - focal_length).abs() < 1e-5)
            {
                extract_dist_params(exact)
            } else if focal_length < distortions[0].focal {
                extract_dist_params(distortions[0])
            } else if focal_length > distortions.last().unwrap().focal {
                extract_dist_params(distortions.last().unwrap())
            } else {
                let mut res = (0.0, 0.0, 0.0, 0);
                for pair in distortions.windows(2) {
                    let (d1, d2) = (&pair[0], &pair[1]);

                    if focal_length >= d1.focal && focal_length <= d2.focal {
                        let p1 = extract_dist_params(d1);
                        let p2 = extract_dist_params(d2);

                        let range = d2.focal - d1.focal;
                        if range.abs() < 1e-5 || p1.3 != p2.3 {
                            res = p1;
                        } else {
                            let t = (focal_length - d1.focal) / range;
                            res = (
                                p1.0 + t as f64 * (p2.0 - p1.0),
                                p1.1 + t as f64 * (p2.1 - p1.1),
                                p1.2 + t as f64 * (p2.2 - p1.2),
                                p1.3,
                            );
                        }
                        break;
                    }
                }
                res
            }
        };

        let (tca_vr, tca_vb) = if tcas.is_empty() {
            (1.0, 1.0)
        } else {
            tcas.sort_by(|a, b| a.focal.partial_cmp(&b.focal).unwrap_or(Ordering::Equal));

            if let Some(exact) = tcas.iter().find(|d| (d.focal - focal_length).abs() < 1e-5) {
                extract_tca_params(exact)
            } else if focal_length < tcas[0].focal {
                extract_tca_params(tcas[0])
            } else if focal_length > tcas.last().unwrap().focal {
                extract_tca_params(tcas.last().unwrap())
            } else {
                let mut res = (1.0, 1.0);
                for pair in tcas.windows(2) {
                    let (d1, d2) = (&pair[0], &pair[1]);
                    if focal_length >= d1.focal && focal_length <= d2.focal {
                        let p1 = extract_tca_params(d1);
                        let p2 = extract_tca_params(d2);

                        let range = d2.focal - d1.focal;
                        if range.abs() < 1e-5 {
                            res = p1;
                        } else {
                            let t = (focal_length - d1.focal) / range;
                            res = (
                                p1.0 + t as f64 * (p2.0 - p1.0),
                                p1.1 + t as f64 * (p2.1 - p1.1),
                            );
                        }
                        break;
                    }
                }
                res
            }
        };

        let (vig_k1, vig_k2, vig_k3) = if vignettings.is_empty() {
            (0.0, 0.0, 0.0)
        } else {
            let target_aperture = aperture.unwrap_or(3.5);
            let target_distance = distance.unwrap_or(1000.0);

            vignettings.sort_by(|a, b| a.focal.partial_cmp(&b.focal).unwrap_or(Ordering::Equal));

            let find_best_vig = |items: &[&Vignetting]| -> (f64, f64, f64) {
                let best_aperture_item = items.iter().min_by(|a, b| {
                    (a.aperture - target_aperture)
                        .abs()
                        .partial_cmp(&(b.aperture - target_aperture).abs())
                        .unwrap_or(Ordering::Equal)
                });
                if let Some(best_ap) = best_aperture_item {
                    let candidates: Vec<&&Vignetting> = items
                        .iter()
                        .filter(|x| (x.aperture - best_ap.aperture).abs() < 0.01)
                        .collect();
                    let best_dist = candidates.into_iter().min_by(|a, b| {
                        let da = a.distance.unwrap_or(1000.0);
                        let db = b.distance.unwrap_or(1000.0);
                        (da - target_distance)
                            .abs()
                            .partial_cmp(&(db - target_distance).abs())
                            .unwrap_or(Ordering::Equal)
                    });
                    extract_vig_params(best_dist.unwrap_or(best_ap))
                } else {
                    (0.0, 0.0, 0.0)
                }
            };

            if focal_length <= vignettings[0].focal + 0.01 {
                let group: Vec<&Vignetting> = vignettings
                    .iter()
                    .filter(|x| (x.focal - vignettings[0].focal).abs() < 0.01)
                    .copied()
                    .collect();
                find_best_vig(&group)
            } else if focal_length >= vignettings.last().unwrap().focal - 0.01 {
                let last_focal = vignettings.last().unwrap().focal;
                let group: Vec<&Vignetting> = vignettings
                    .iter()
                    .filter(|x| (x.focal - last_focal).abs() < 0.01)
                    .copied()
                    .collect();
                find_best_vig(&group)
            } else {
                let mut res = (0.0, 0.0, 0.0);
                let unique_focals: Vec<f32> = {
                    let mut f: Vec<f32> = vignettings.iter().map(|v| v.focal).collect();
                    f.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
                    f.dedup_by(|a, b| (*a - *b).abs() < 0.01);
                    f
                };
                for pair in unique_focals.windows(2) {
                    let (f1, f2) = (pair[0], pair[1]);
                    if focal_length >= f1 && focal_length <= f2 {
                        let group1: Vec<&Vignetting> = vignettings
                            .iter()
                            .filter(|x| (x.focal - f1).abs() < 0.01)
                            .copied()
                            .collect();
                        let group2: Vec<&Vignetting> = vignettings
                            .iter()
                            .filter(|x| (x.focal - f2).abs() < 0.01)
                            .copied()
                            .collect();

                        let p1 = find_best_vig(&group1);
                        let p2 = find_best_vig(&group2);

                        let range = f2 - f1;
                        if range.abs() > 0.01 {
                            let t = (focal_length - f1) / range;
                            res = (
                                p1.0 + t as f64 * (p2.0 - p1.0),
                                p1.1 + t as f64 * (p2.1 - p1.1),
                                p1.2 + t as f64 * (p2.2 - p1.2),
                            );
                        } else {
                            res = p1;
                        }
                        break;
                    }
                }
                res
            }
        };

        Some(LensDistortionParams {
            k1,
            k2,
            k3,
            model,
            tca_vr,
            tca_vb,
            vig_k1,
            vig_k2,
            vig_k3,
            embedded: None,
        })
    }
}

fn extract_dist_params(dist: &Distortion) -> (f64, f64, f64, u32) {
    match dist.model.as_str() {
        "poly3" | "poly5" => (
            dist.k1.unwrap_or(0.0) as f64,
            dist.k2.unwrap_or(0.0) as f64,
            dist.k3.unwrap_or(0.0) as f64,
            0,
        ),
        "ptlens" => {
            let a = dist.a.unwrap_or(0.0) as f64;
            let b = dist.b.unwrap_or(0.0) as f64;
            let c = dist.c.unwrap_or(0.0) as f64;
            (a, b, c, 1)
        }
        _ => (0.0, 0.0, 0.0, 0),
    }
}

fn extract_tca_params(tca: &Tca) -> (f64, f64) {
    (tca.vr.unwrap_or(1.0) as f64, tca.vb.unwrap_or(1.0) as f64)
}

fn extract_vig_params(vig: &Vignetting) -> (f64, f64, f64) {
    (
        vig.k1.unwrap_or(0.0) as f64,
        vig.k2.unwrap_or(0.0) as f64,
        vig.k3.unwrap_or(0.0) as f64,
    )
}

fn lenses_for_maker<'a>(db: &'a LensDatabase, maker: &str) -> Vec<&'a Lens> {
    db.lenses
        .iter()
        .filter(|l| l.get_maker() == maker)
        .collect()
}

pub fn load_lensfun_db(app_handle: &tauri::AppHandle) -> LensDatabase {
    let mut combined_db = LensDatabase {
        cameras: Vec::new(),
        lenses: Vec::new(),
    };

    {
        let updated_path = std::env::var("RAPIDRAW_COMPONENT_DIR")
            .ok()
            .map(std::path::PathBuf::from)
            .map(|p| p.join("lensfun/current"));
        let bundled_path = app_handle
            .path()
            .resolve("lensfun_db", tauri::path::BaseDirectory::Resource)
            .expect("failed to resolve lensfun_db directory");
        let resource_path = updated_path
            .filter(|p| p.join("lensfun-database.dtd").exists())
            .unwrap_or(bundled_path);

        if !resource_path.exists() {
            log::error!("Lensfun DB directory not found at: {:?}", resource_path);
            return combined_db;
        }

        for entry in WalkDir::new(resource_path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "xml"))
        {
            let path = entry.path();
            log::info!("Processing file: {:?}", path);
            match fs::read_to_string(path) {
                Ok(xml_content) => match quick_xml::de::from_str::<LensDatabase>(&xml_content) {
                    Ok(mut db) => {
                        combined_db.cameras.append(&mut db.cameras);
                        combined_db.lenses.append(&mut db.lenses);
                    }
                    Err(e) => {
                        log::error!("Failed to parse Lensfun XML file {:?}: {}", path, e);
                    }
                },
                Err(e) => log::error!("Failed to read Lensfun XML file {:?}: {}", path, e),
            }
        }
    }

    log::info!(
        "Loaded {} lenses and {} cameras from Lensfun database.",
        combined_db.lenses.len(),
        combined_db.cameras.len()
    );
    combined_db
}

#[tauri::command]
pub fn get_lensfun_makers(state: State<AppState>) -> Result<Vec<String>, String> {
    let db_guard = state
        .lens_db
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    if let Some(db) = &*db_guard {
        let mut makers: Vec<String> = db.lenses.iter().map(|lens| lens.get_maker()).collect();
        makers.sort_unstable();
        makers.dedup();
        Ok(makers)
    } else {
        Err("Lens database not loaded".to_string())
    }
}

#[tauri::command]
pub fn get_lensfun_lenses_for_maker(
    maker: String,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    let db_guard = state
        .lens_db
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    if let Some(db) = &*db_guard {
        let maker_lenses = lenses_for_maker(db, &maker);

        let mut models: Vec<String> = maker_lenses
            .iter()
            .map(|lens| lens.get_display_name(&maker_lenses))
            .collect();
        models.sort_unstable();
        models.dedup();
        Ok(models)
    } else {
        Err("Lens database not loaded".to_string())
    }
}

fn identifier(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn maker_id(text: &str) -> String {
    identifier(text.split_whitespace().next().unwrap_or(""))
}

fn find_exact_lens_match(db: &LensDatabase, maker: &str, model: &str) -> Option<(String, String)> {
    let wanted = identifier(model.trim_matches('"'));
    if wanted.is_empty() || matches!(wanted.as_str(), "unknown" | "0" | "65535" | "fixedlens") {
        return None;
    }
    let mut matches = db.lenses.iter().filter(|lens| {
        let lens_maker = lens.get_maker();
        let same_maker = maker.trim().is_empty() || maker_id(maker) == maker_id(&lens_maker);
        let explicit_maker = wanted.starts_with(&identifier(&lens_maker));
        (same_maker || explicit_maker)
            && lens.model.iter().any(|name| {
                identifier(&name.value) == wanted
                    || identifier(&strip_maker_prefix(&name.value, &lens_maker)) == wanted
            })
    });
    let lens = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let maker = lens.get_maker();
    Some((
        maker.clone(),
        lens.get_display_name(&lenses_for_maker(db, &maker)),
    ))
}

pub fn find_best_lens_match(
    db: &LensDatabase,
    maker: &str,
    model: &str,
) -> Option<(String, String)> {
    if let Some(exact) = find_exact_lens_match(db, maker, model) {
        return Some(exact);
    }
    // Text similarity alone confused unrelated lenses. Require the same optical
    // numbers, a strong name match and a clear lead over the next candidate.
    static NUMBERS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let numbers = NUMBERS.get_or_init(|| regex::Regex::new(r"\d+(?:\.\d+)?").unwrap());
    let signature = |text: &str| {
        numbers
            .find_iter(text)
            .map(|m| m.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    let wanted_numbers = signature(model);
    if wanted_numbers.is_empty() {
        return None;
    }
    let wanted = identifier(model);
    let mut candidates = db
        .lenses
        .iter()
        .filter_map(|lens| {
            let lens_maker = lens.get_maker();
            let same_maker = maker.trim().is_empty() || maker_id(maker) == maker_id(&lens_maker);
            if !same_maker && !wanted.starts_with(&identifier(&lens_maker)) {
                return None;
            }
            let score = lens
                .model
                .iter()
                .filter(|name| signature(&name.value) == wanted_numbers)
                .map(|name| {
                    strsim::jaro_winkler(&wanted, &identifier(&name.value)).max(
                        strsim::jaro_winkler(
                            &wanted,
                            &identifier(&strip_maker_prefix(&name.value, &lens_maker)),
                        ),
                    )
                })
                .fold(0.0_f64, f64::max);
            (score >= 0.90).then_some((score, lens))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (score, lens) = candidates.first()?;
    if candidates.get(1).is_some_and(|next| score - next.0 < 0.025) {
        return None;
    }
    let maker = lens.get_maker();
    Some((
        maker.clone(),
        lens.get_display_name(&lenses_for_maker(db, &maker)),
    ))
}

fn fixed_camera_lens(db: &LensDatabase, maker: &str, model: &str) -> Option<(String, String)> {
    let mut camera_model = identifier(&strip_maker_prefix(
        model,
        maker.split_whitespace().next().unwrap_or(""),
    ));
    // GR and GR II share the 18.3mm F2.8 lens represented by Lensfun's ricohGR mount.
    if maker_id(maker) == "ricoh" && camera_model == "grii" {
        camera_model = "gr".into();
    }
    let camera = db.cameras.iter().find(|camera| {
        camera
            .maker
            .iter()
            .any(|name| maker_id(&name.value) == maker_id(maker))
            && camera.model.iter().any(|name| {
                identifier(&strip_maker_prefix(
                    &name.value,
                    maker.split_whitespace().next().unwrap_or(""),
                )) == camera_model
            })
    })?;
    let mut candidates = db.lenses.iter().filter(|lens| {
        lens.mount.contains(&camera.mount)
            && lens
                .get_full_model_name()
                .eq_ignore_ascii_case("fixed lens")
    });
    let lens = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    let maker = lens.get_maker();
    Some((
        maker.clone(),
        lens.get_display_name(&lenses_for_maker(db, &maker)),
    ))
}

#[tauri::command]
pub async fn autodetect_lens(
    path: String,
    profile_source: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<AutoLensProfile>, String> {
    let db = state
        .lens_db
        .lock()
        .map_err(|e| format!("Lock poisoned: {e}"))?
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (source, _) = crate::file_management::parse_virtual_path(&path);
        let metadata = crate::exif_processing::read_source_metadata(&source)?;
        Ok(select_auto_profile(
            if profile_source.as_deref() == Some("embedded") {
                None
            } else {
                db.as_deref()
            },
            &metadata.fields,
            if profile_source.as_deref() == Some("lensfun") {
                None
            } else {
                metadata.lens_profile.as_ref()
            },
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct AutoLensProfile {
    pub maker: String,
    pub model: String,
    pub params: LensDistortionParams,
}

pub fn select_auto_profile(
    db: Option<&LensDatabase>,
    exif: &HashMap<String, String>,
    embedded: Option<&EmbeddedLensProfile>,
) -> Option<AutoLensProfile> {
    if let Some(embedded) = embedded.filter(|profile| profile.has_correction()) {
        return Some(AutoLensProfile {
            maker: exif
                .get("LensMake")
                .or_else(|| exif.get("Make"))
                .cloned()
                .unwrap_or_default(),
            model: exif
                .get("LensModel")
                .cloned()
                .unwrap_or_else(|| format!("{} profile", embedded.source)),
            params: LensDistortionParams {
                k1: 0.0,
                k2: 0.0,
                k3: 0.0,
                model: 0,
                tca_vr: 1.0,
                tca_vb: 1.0,
                vig_k1: 0.0,
                vig_k2: 0.0,
                vig_k3: 0.0,
                embedded: Some(embedded.clone()),
            },
        });
    }
    let db = db?;
    let camera_make = exif.get("Make").map(String::as_str).unwrap_or("");
    let maker = exif
        .get("LensMake")
        .map(String::as_str)
        .unwrap_or(camera_make);
    let model = exif.get("LensModel").map(String::as_str).unwrap_or("");
    let (maker, model) = find_exact_lens_match(db, maker, model)
        .or_else(|| {
            fixed_camera_lens(
                db,
                camera_make,
                exif.get("Model").map(String::as_str).unwrap_or(""),
            )
        })
        .or_else(|| find_best_lens_match(db, maker, model))?;
    let focal = metadata_number(exif, "FocalLength")?;
    let params = resolve_lens_params(
        db,
        &maker,
        &model,
        focal,
        metadata_number(exif, "FNumber"),
        metadata_number(exif, "SubjectDistance"),
    )?;
    Some(AutoLensProfile {
        maker,
        model,
        params,
    })
}

pub fn metadata_number(exif: &HashMap<String, String>, key: &str) -> Option<f32> {
    let text = exif.get(key)?.trim().trim_start_matches("f/");
    let value = text.split_whitespace().next()?.parse::<f32>().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

#[tauri::command]
pub fn get_lens_distortion_params(
    maker: String,
    model: String,
    focal_length: f32,
    aperture: Option<f32>,
    distance: Option<f32>,
    state: State<AppState>,
) -> Result<Option<LensDistortionParams>, String> {
    let db_guard = state
        .lens_db
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    if let Some(db) = &*db_guard {
        let maker_lenses = lenses_for_maker(db, &maker);

        if let Some(lens) = maker_lenses
            .iter()
            .find(|l| l.get_display_name(&maker_lenses) == model)
        {
            return Ok(lens.get_distortion_params(focal_length, aperture, distance));
        }
    }
    Ok(None)
}

pub fn resolve_lens_params(
    db: &LensDatabase,
    maker: &str,
    model: &str,
    focal_length: f32,
    aperture: Option<f32>,
    distance: Option<f32>,
) -> Option<LensDistortionParams> {
    let maker_lenses = lenses_for_maker(db, maker);
    if let Some(lens) = maker_lenses
        .iter()
        .find(|l| l.get_display_name(&maker_lenses) == model)
    {
        lens.get_distortion_params(focal_length, aperture, distance)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Uses the developer-provided RAW files in testbilder"]
    fn supplied_raw_files_use_camera_iso_and_shared_profile_selection() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut db = LensDatabase {
            cameras: Vec::new(),
            lenses: Vec::new(),
        };
        for entry in WalkDir::new(root.join("lensfun_db"))
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.path().extension().is_some_and(|ext| ext == "xml") {
                if let Some(part) = fs::read_to_string(entry.path())
                    .ok()
                    .and_then(|xml| quick_xml::de::from_str::<LensDatabase>(&xml).ok())
                {
                    db.cameras.extend(part.cameras);
                    db.lenses.extend(part.lenses);
                }
            }
        }
        assert!(db.lenses.len() > 100);
        for (name, iso) in [
            ("Canon - EOS R6 Mark III - RAW (3_2).CR3", "100"),
            ("DSC_0234.NEF", "4000"),
            ("Olympus - PEN-F - 16bit (4_3).orf", "200"),
            ("Panasonic - DC-S1M2 - 3_2.RW2", "100"),
            ("R0019057.DNG", "2200"),
            ("Sony-a1-raw-00001.arw", "800"),
        ] {
            let path = root.parent().unwrap().join("testbilder").join(name);
            let metadata = crate::exif_processing::read_source_metadata(&path).unwrap();
            assert_eq!(metadata.fields["ISO"], iso, "{name}");
            assert_eq!(metadata.fields["PhotographicSensitivity"], iso, "{name}");
            let requests = crate::exiftool::request_count();
            let profile =
                select_auto_profile(Some(&db), &metadata.fields, metadata.lens_profile.as_ref());
            eprintln!(
                "{name}: ISO {iso}, {}",
                profile
                    .as_ref()
                    .map(|p| format!(
                        "{} / {}",
                        if p.params.embedded.is_some() {
                            "built-in"
                        } else {
                            "Lensfun"
                        },
                        p.model
                    ))
                    .unwrap_or("no matching profile".into())
            );
            if name == "R0019057.DNG" {
                let profile = profile.as_ref().unwrap();
                assert_eq!(profile.maker, "Ricoh");
                assert!(profile.params.embedded.is_none());
                assert!(profile.model.contains("GR & compatibles"));
            }
            for source in ["off", "embedded"] {
                let mut adjustments = serde_json::json!({"lensCorrectionMode":"manual","lensProfileSource":source,"lensDistortionParams":{"k1":99}});
                crate::file_management::resolve_lens_params_in_adjustments(
                    &mut adjustments,
                    &Some(metadata.fields.clone()),
                    Some(&db),
                    &path,
                );
                if source == "off" {
                    assert!(adjustments.get("lensDistortionParams").is_none());
                } else {
                    assert_eq!(
                        adjustments["lensDistortionParams"]["embedded"].is_object(),
                        metadata.lens_profile.is_some()
                    );
                }
            }
            assert_eq!(
                crate::exiftool::request_count(),
                requests,
                "Selection must reuse saved metadata for {name}"
            );
        }
    }
    #[test]
    fn fuzzy_matches_small_name_differences_but_rejects_wrong_optics_and_ambiguity() {
        let mut db: LensDatabase =
            quick_xml::de::from_str(include_str!("../lensfun_db/compact-ricoh.xml")).unwrap();
        let mut lens = db.lenses[0].clone();
        lens.maker = vec![MultiName {
            lang: None,
            value: "Sony".into(),
        }];
        lens.model = vec![MultiName {
            lang: None,
            value: "Sony FE 20mm f/1.8 G".into(),
        }];
        db.lenses = vec![lens.clone()];
        assert!(find_best_lens_match(&db, "Sony", "FE 20mm F1.8 G lens").is_some());
        assert!(find_best_lens_match(&db, "Sony", "FE 24mm F1.8 G lens").is_none());
        assert!(find_best_lens_match(&db, "Sony", "FE 20mm F2.8 G lens").is_none());
        assert!(find_best_lens_match(&db, "Ricoh", "FE 20mm F1.8 G lens").is_none());
        db.lenses.push(lens);
        assert!(find_best_lens_match(&db, "Sony", "FE 20mm F1.8 G lens").is_none());
    }

    #[test]
    fn neutral_embedded_profile_falls_back_to_lensfun() {
        let db: LensDatabase =
            quick_xml::de::from_str(include_str!("../lensfun_db/compact-ricoh.xml")).unwrap();
        let neutral = EmbeddedLensProfile {
            source: "DNG".into(),
            radial: Some(crate::lens_metadata::RadialProfile {
                knots: vec![0.0, 1.0],
                distortion: vec![1.0, 1.0],
                gain_knots: vec![0.0, 1.0],
                gain: vec![1.0, 1.0],
                ..Default::default()
            }),
            warp: None,
            vignette: None,
            frame: [101.0, 101.0],
            crop: [0.0, 0.0, 101.0, 101.0],
            orientation: 1,
        };
        let fields = HashMap::from([
            ("Make".into(), "Ricoh".into()),
            ("Model".into(), "GR II".into()),
            ("LensModel".into(), "GR LENS".into()),
            ("FocalLength".into(), "18.3".into()),
            ("FNumber".into(), "2.8".into()),
        ]);
        let profile = select_auto_profile(Some(&db), &fields, Some(&neutral)).unwrap();
        assert!(profile.params.embedded.is_none());
        assert!(profile.model.contains("GR & compatibles"));
    }
    #[test]
    fn ricoh_fixed_lens_uses_its_camera_mount_and_never_a_samyang_fuzzy_match() {
        let mut db: LensDatabase =
            quick_xml::de::from_str(include_str!("../lensfun_db/compact-ricoh.xml")).unwrap();
        let mut samyang = db.lenses[0].clone();
        samyang.maker = vec![MultiName {
            lang: None,
            value: "Samyang".into(),
        }];
        samyang.model = vec![MultiName {
            lang: None,
            value: "500mm f/6.3".into(),
        }];
        db.lenses.push(samyang);
        assert!(find_best_lens_match(&db, "Ricoh", "GR II").is_none());
        assert!(find_best_lens_match(&db, "Ricoh", "").is_none());
        assert!(find_best_lens_match(&db, "Ricoh", "Unknown").is_none());
        let (maker, model) =
            fixed_camera_lens(&db, "RICOH IMAGING COMPANY, LTD.", "GR II").unwrap();
        assert_eq!(maker, "Ricoh");
        assert!(model.contains("GR & compatibles"), "{model}");
        assert!(resolve_lens_params(&db, &maker, &model, 18.3, Some(2.8), None).is_some());
        assert!(fixed_camera_lens(&db, "Ricoh", "GR IIIx mystery").is_none());
        assert!(find_best_lens_match(&db, "Ricoh", "500mm f/6.3").is_none());
    }
}
