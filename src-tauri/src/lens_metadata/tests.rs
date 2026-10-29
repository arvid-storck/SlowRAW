use super::*;
use serde_json::json;

fn profile(make: &str, fields: Value) -> EmbeddedLensProfile {
    let mut tags = fields.as_object().unwrap().clone();
    tags.insert("IFD0:Make".into(), json!(make));
    tags.insert("ExifIFD:ExifImageWidth".into(), json!(101));
    tags.insert("ExifIFD:ExifImageHeight".into(), json!(101));
    tags.insert("IFD0:Orientation".into(), json!(1));
    from_metadata(&json!([tags])).expect("Supported correction model")
}

#[test]
fn neutral_camera_tables_are_not_builtin_corrections() {
    let mut sony = vec![16.0];
    sony.extend([0.0; 16]);
    let mut fuji = vec![0.0];
    fuji.extend((0..9).map(|i| i as f64 / 8.0));
    fuji.extend([0.0; 9]);
    for fields in [
        json!({"Make":"SONY", "DistortionCorrParams":sony}),
        json!({"Make":"FUJIFILM", "GeometricDistortionParams":fuji}),
        json!({"Make":"OLYMPUS", "Olympus_ImageProcessing_0x150a":[0,0,0,1], "Olympus_ImageProcessing_0x150c":[0,0,0,0,0,0]}),
        json!({"Make":"Panasonic", "DistortionScale":1,"DistortionParam08":0,"DistortionParam04":0,"DistortionParam11":0}),
        json!({"Make":"NIKON", "DistortionCorrectionVersion":"0100","RadialDistortionCoefficient1":0,"RadialDistortionCoefficient2":0,"RadialDistortionCoefficient3":0,
            "VignetteCorrectionVersion":"0100","NikonVignetteCoefficientCount":8,"NikonVignetteCoefficients":[0,0,0,0,0,0,0,0]}),
    ] {
        let mut fields = fields.as_object().unwrap().clone();
        fields.insert("ImageWidth".into(), json!(101));
        fields.insert("ImageHeight".into(), json!(101));
        assert!(crate::embedded_lens::from_metadata(&json!([fields])).is_none());
    }
}

#[test]
fn sony_preserves_precision_and_independent_strengths() {
    let mut d = vec![16.0];
    d.extend([1638.4123456789; 16]);
    let mut ca = vec![32.0];
    ca.extend([2097.152; 16]);
    ca.extend([-2097.152; 16]);
    let mut v = vec![16.0];
    v.extend([8192.0; 16]);
    let p = profile(
        "SONY",
        json!({
            "Sony:DistortionCorrParams": d,
            "Sony:ChromaticAberrationCorrParams": ca,
            "Sony:VignettingCorrParams": v
        }),
    );
    let table = p.radial.as_ref().unwrap();
    let expected = 1.0 + 1638.4123456789 / 16384.0;
    assert!((table.factor(0.5, 1, 1.0, 1.0) - expected).abs() < 1e-14);
    assert!((table.factor(0.5, 0, 1.0, 1.0) / expected - 1.001).abs() < 1e-12);
    assert!((table.factor(0.5, 2, 0.0, 1.0) - 0.999).abs() < 1e-12);
    assert!((table.gain(0.5, 1.0) - 2_f64.sqrt()).abs() < 1e-12);
    assert_eq!(p.sample([0.9, 0.4], 0, 0.0, 0.0, 0.0), ([0.9, 0.4], 1.0));
    assert!(p.sample([0.9, 0.4], 0, 1.0, 0.0, 0.0).0[0] > 0.9);
}

#[test]
fn fuji_converts_source_radius_to_output_radius() {
    let mut d = vec![0.0];
    d.extend((0..9).map(|i| i as f64 / 8.0));
    d.extend([10.0; 9]);
    let p = profile("FUJIFILM", json!({"Fujifilm:GeometricDistortionParams":d}));
    let table = p.radial.unwrap();
    assert!((table.knots[128] - 1.0 / 1.1).abs() < 1e-12);
    assert!((table.factor(0.5, 1, 1.0, 0.0) - 1.1).abs() < 1e-12);
    assert!(
        from_metadata(&json!([{"Make":"FUJIFILM","GeometricDistortionParams":[1,2]}])).is_none()
    );
}

#[test]
fn olympus_uses_scaled_radius_and_channel_coefficients() {
    let p = profile(
        "OM Digital Solutions",
        json!({
            "Olympus:Olympus_ImageProcessing_0x150a":[0.1,0.0,0.0,0.9],
            "Olympus:Olympus_ImageProcessing_0x150c":[0.001,0.0,0.0,-0.002,0.0,0.0]
        }),
    );
    let table = p.radial.unwrap();
    let expected = 0.9 * (1.0 + 0.1 * (0.5_f64 * 0.9).powi(2));
    assert!((table.factor(0.5, 1, 1.0, 0.0) - expected).abs() < 1e-12);
    assert!((table.factor(0.5, 2, 1.0, 1.0) - expected * 0.998).abs() < 1e-12);
}

#[test]
fn panasonic_inverts_the_forward_polynomial() {
    let p = profile(
        "Panasonic",
        json!({"DistortionScale":1.0,"DistortionParam08":-0.1,"DistortionParam04":0.02,"DistortionParam11":0.0}),
    );
    let table = p.radial.unwrap();
    for index in [1, 32, 64, 96] {
        let target = index as f64 / 128.0;
        let source = target * table.distortion[index];
        assert!(
            (source * (1.0 - 0.1 * source.powi(2) + 0.02 * source.powi(4)) - target).abs() < 1e-12
        );
    }
    assert!(from_metadata(&json!([{"Make":"Panasonic","DistortionScale":1,"DistortionParam08":-1,"DistortionParam04":0,"DistortionParam11":0}])).is_none());
}

/// Little-endian TIFF fixture builder, with offsets relative to its own header.
#[test]
fn official_reader_extracts_olympus_unknown_numeric_correction_tags() {
    let coefficients = [0.1_f64, 0.0, 0.0, 0.9];
    // Olympus IFD offsets are relative to the MakerNote header.
    let ifd = tiff(vec![(
        0x150a,
        12,
        coefficients
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect(),
    )]);
    let mut image_processing = ifd[8..].to_vec();
    // The value starts after the nested IFD and must include the MakerNote's base.
    image_processing[10..14].copy_from_slice(&48_u32.to_le_bytes());
    let mut maker = b"OLYMPUS\0II\x03\0".to_vec();
    maker.extend(1_u16.to_le_bytes());
    maker.extend(0x2040_u16.to_le_bytes());
    maker.extend(4_u16.to_le_bytes());
    maker.extend(1_u32.to_le_bytes());
    maker.extend(30_u32.to_le_bytes());
    maker.extend(0_u32.to_le_bytes());
    maker.extend(image_processing);
    let exif_ifd = tiff(vec![(0x927c, 7, maker)]);
    // Relocate the MakerNote from the isolated EXIF IFD into the root TIFF.
    let mut exif = exif_ifd[8..].to_vec();
    let make = b"OLYMPUS IMAGING CORP.\0".to_vec();
    let exif_offset = 8 + 2 + 4 * 12 + 4 + make.len();
    exif[10..14].copy_from_slice(&((exif_offset + 18) as u32).to_le_bytes());
    let mut file = tiff(vec![
        (256, 4, 101_u32.to_le_bytes().to_vec()),
        (257, 4, 101_u32.to_le_bytes().to_vec()),
        (271, 2, make),
        (0x8769, 4, (exif_offset as u32).to_le_bytes().to_vec()),
    ]);
    file.extend(exif);
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("olympus.orf");
    std::fs::write(&path, file).unwrap();
    let metadata = crate::exif_processing::read_source_metadata(&path).unwrap();
    let p = metadata
        .lens_profile
        .unwrap_or_else(|| panic!("Missing Olympus profile: {:?}", metadata.fields));
    assert_eq!(p.source, "Olympus / OM System");
    assert!((p.radial.unwrap().factor(0.5, 1, 1.0, 0.0) - 0.918225).abs() < 1e-12);
}

/// Little-endian TIFF fixture builder, with offsets relative to its own header.
pub(super) fn tiff(entries: Vec<(u16, u16, Vec<u8>)>) -> Vec<u8> {
    let mut data = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
    data.extend((entries.len() as u16).to_le_bytes());
    let mut extra = Vec::new();
    let start = 8 + 2 + 12 * entries.len() + 4;
    for (tag, kind, value) in entries {
        let unit = match kind {
            3 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => 1,
        };
        data.extend(tag.to_le_bytes());
        data.extend(kind.to_le_bytes());
        data.extend((value.len() as u32 / unit).to_le_bytes());
        if value.len() <= 4 {
            let mut inline = [0; 4];
            inline[..value.len()].copy_from_slice(&value);
            data.extend(inline);
        } else {
            data.extend(((start + extra.len()) as u32).to_le_bytes());
            extra.extend(value);
        }
    }
    data.extend(0_u32.to_le_bytes());
    data.extend(extra);
    data
}

#[test]
fn official_reader_extracts_nikon_profile_in_the_shared_metadata_read() {
    let mut block = vec![0_u8; 20];
    block[..4].copy_from_slice(b"0100");
    block[16..20].copy_from_slice(&4_u32.to_le_bytes());
    for numerator in [1_i32, 2, 3, 0, 0] {
        block.extend(numerator.to_le_bytes());
        block.extend(100_i32.to_le_bytes());
    }
    let mut vignette = vec![0_u8; 20];
    vignette[..4].copy_from_slice(b"0100");
    vignette[16..20].copy_from_slice(&8_u32.to_le_bytes());
    for numerator in [
        -123456789_i32,
        0,
        300000000,
        0,
        200000000,
        0,
        100000000,
        0,
        0,
    ] {
        vignette.extend(numerator.to_le_bytes());
        vignette.extend(1_000_000_000_i32.to_le_bytes());
    }
    let mut blob = b"Nikon\0\x02\x10\0\0".to_vec();
    blob.extend(tiff(vec![(5, 7, block), (6, 7, vignette)]));
    let file = tiff(vec![
        (256, 4, 101_u32.to_le_bytes().to_vec()),
        (257, 4, 101_u32.to_le_bytes().to_vec()),
        (271, 2, b"NIKON CORPORATION\0".to_vec()),
        (0xc7d5, 7, blob),
    ]);
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("nikon.nef");
    std::fs::write(&path, file).unwrap();
    let metadata = crate::exif_processing::read_source_metadata(&path).unwrap();
    let p = metadata
        .lens_profile
        .expect("ExifTool must decode Nikon correction coefficients");
    assert_eq!(p.source, "Nikon");
    assert!(!metadata.fields.contains_key("NikonNEFInfo"));
    assert_eq!(metadata.fields["NikonVignetteCoefficientCount"], "8");
    assert!(metadata.fields["NikonVignetteCoefficients"].contains("-0.123456789"));
    let requests = crate::exiftool::request_count();
    let (_, corner_gain) = p.sample([1.0, 1.0], 1, 0.0, 0.0, 1.0);
    assert!((corner_gain - 1.476543211).abs() < 1e-12);
    assert_eq!(p.sample([0.5, 0.5], 1, 0.0, 0.0, 1.0).1, 1.0);
    assert_eq!(p.sample([1.0, 1.0], 1, 0.0, 0.0, 0.0).1, 1.0);
    assert!((p.sample([1.0, 1.0], 1, 0.0, 0.0, 0.5).1 - (1.0 + corner_gain) / 2.0).abs() < 1e-12);
    let cached = crate::exif_processing::read_source_metadata(&path).unwrap();
    let selected = crate::lens_correction::select_auto_profile(
        None,
        &cached.fields,
        cached.lens_profile.as_ref(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(selected.params).unwrap()["embedded"],
        serde_json::to_value(&p).unwrap()
    );
    assert_eq!(crate::exiftool::request_count(), requests);
    assert_eq!(
        serde_json::to_value(cached.lens_profile).unwrap(),
        serde_json::to_value(&p).unwrap()
    );
    let table = p.radial.unwrap();
    let r = 0.5_f64;
    assert!(
        (table.gain(r, 1.0)
            - (1.0 - 0.123456789 * r.powi(8)
                + 0.3 * r.powi(6)
                + 0.2 * r.powi(4)
                + 0.1 * r.powi(2)))
        .abs()
            < 1e-12
    );
    assert!(
        (table.factor(r, 1, 1.0, 0.0)
            - (1.0 + 0.01 * r.powi(4) + 0.02 * r.powi(3) + 0.03 * r.powi(2)))
        .abs()
            < 1e-12
    );
}

#[test]
fn nikon_vignette_only_profiles_require_complete_known_layouts() {
    let mut fields = json!({"Make":"NIKON", "ImageWidth":101, "ImageHeight":101,
        "VignetteCorrectionVersion":"0100", "NikonVignetteCoefficientCount":8,
        "NikonVignetteCoefficients":[0.2,0.0,0.0,0.0,0.0,0.0,0.0,0.0]});
    let p = from_metadata(&json!([fields.clone()])).unwrap();
    assert!(!p.has_warp());
    assert!(p.has_vignette());
    assert!((p.sample([1.0, 1.0], 1, 0.0, 0.0, 1.0).1 - 1.2).abs() < 1e-12);
    fields["VignetteCorrectionVersion"] = json!("0200");
    assert!(from_metadata(&json!([fields.clone()])).is_none());
    fields["VignetteCorrectionVersion"] = json!("0100");
    fields["NikonVignetteCoefficientCount"] = json!(7);
    assert!(from_metadata(&json!([fields.clone()])).is_none());
    fields["NikonVignetteCoefficientCount"] = json!(8);
    fields["NikonVignetteCoefficients"] = json!([0.2, 0.0]);
    assert!(from_metadata(&json!([fields])).is_none());
}
