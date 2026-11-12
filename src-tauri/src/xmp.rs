use crate::image_processing::ImageMetadata;
use roxmltree::{Document, Node};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const DM: &str = "http://ns.adobe.com/xmp/1.0/DynamicMedia/";
const EXIF: &str = "http://ns.adobe.com/exif/1.0/";
const TIFF: &str = "http://ns.adobe.com/tiff/1.0/";
const EXIF_EXT: &str = "http://cipa.jp/exif/1.0/";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const EMPTY_PACKET: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
</rdf:RDF>
</x:xmpmeta>"#;

type Field = (&'static str, &'static str);
type Properties = BTreeMap<Field, Vec<String>>;
const RATING: Field = (XMP, "Rating");
const LABEL: Field = (XMP, "Label");
const KEYWORDS: Field = (DC, "subject");
const FLAG: Field = (DM, "pick");
const SHARED_FIELDS: &[(&str, Field)] = &[
    ("Artist", (DC, "creator")),
    ("Copyright", (DC, "rights")),
    ("ImageDescription", (DC, "title")),
    ("UserComment", (EXIF, "UserComment")),
    ("Make", (TIFF, "Make")),
    ("Model", (TIFF, "Model")),
    ("Orientation", (TIFF, "Orientation")),
    ("Software", (TIFF, "Software")),
    ("DateTimeOriginal", (EXIF, "DateTimeOriginal")),
    ("ExposureTime", (EXIF, "ExposureTime")),
    ("FNumber", (EXIF, "FNumber")),
    ("FocalLength", (EXIF, "FocalLength")),
    ("FocalLengthIn35mmFilm", (EXIF, "FocalLengthIn35mmFilm")),
    ("ISO", (EXIF, "ISOSpeedRatings")),
    ("LensModel", (EXIF_EXT, "LensModel")),
];

fn append_xmp(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".xmp");
    path.with_file_name(name)
}

fn path_candidates(source: &Path, edits: &Path) -> Vec<PathBuf> {
    if edits.with_extension("") != source {
        return vec![edits.with_extension("xmp"), edits.with_extension("XMP")];
    }
    let full_name = append_xmp(source);
    let stem_name = source.with_extension("xmp");
    let full_name_upper = full_name.with_extension("XMP");
    if crate::formats::is_raw_file(source) {
        vec![
            full_name,
            full_name_upper,
            stem_name,
            source.with_extension("XMP"),
        ]
    } else {
        let mut candidates = vec![full_name, full_name_upper];
        let stem_path = [stem_name, source.with_extension("XMP")]
            .into_iter()
            .find(|path| path.exists());
        if let Some(stem_path) = stem_path
            && !crate::formats::RAW_EXTENSIONS.iter().any(|(extension, _)| {
                source.with_extension(extension).is_file()
                    || source
                        .with_extension(extension.to_ascii_uppercase())
                        .is_file()
            })
        {
            candidates.push(stem_path);
        }
        candidates
    }
}

pub fn existing_path(source: &Path, edits: &Path) -> Option<PathBuf> {
    path_candidates(source, edits)
        .into_iter()
        .find(|path| path.exists())
}

pub fn primary_path(source: &Path) -> Option<PathBuf> {
    existing_path(
        source,
        &crate::exif_processing::get_primary_sidecar_path(source),
    )
}

fn metadata_path(source: &Path, edits: &Path) -> PathBuf {
    existing_path(source, edits).unwrap_or_else(|| {
        if edits.with_extension("") != source {
            edits.with_extension("xmp")
        } else if crate::formats::is_raw_file(source) {
            source.with_extension("xmp")
        } else {
            append_xmp(source)
        }
    })
}

pub fn copy_sidecar(
    source: &Path,
    edits: &Path,
    target: &Path,
    target_edits: &Path,
) -> Result<(), String> {
    if let Some(path) = existing_path(source, edits) {
        let target = metadata_path(target, target_edits);
        let mut input =
            fs::File::open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(target.parent().unwrap_or_else(|| Path::new(".")))
                .map_err(|error| error.to_string())?;
        std::io::copy(&mut input, temporary.as_file_mut()).map_err(|error| error.to_string())?;
        temporary
            .persist_noclobber(&target)
            .map_err(|error| format!("{}: {error}", target.display()))?;
    }
    Ok(())
}

fn description_nodes<'a, 'input>(rdf: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    rdf.children()
        .filter(|node| node.has_tag_name((RDF, "Description")))
}

fn rdf_node<'a, 'input>(doc: &'a Document<'input>) -> Result<Node<'a, 'input>, String> {
    doc.descendants()
        .find(|node| node.has_tag_name((RDF, "RDF")))
        .ok_or_else(|| "XMP packet has no RDF element".into())
}

fn get_properties(rdf: Node<'_, '_>) -> Properties {
    [RATING, LABEL, KEYWORDS, FLAG]
        .into_iter()
        .chain(SHARED_FIELDS.iter().map(|(_, field)| *field))
        .filter_map(|field| {
            description_nodes(rdf)
                .find_map(|node| {
                    if let Some(value) = node.attribute(field) {
                        return Some(vec![value.to_owned()]);
                    }
                    let value = node.children().find(|child| child.has_tag_name(field))?;
                    let entries: Vec<_> = value
                        .descendants()
                        .filter(|child| child.has_tag_name((RDF, "li")))
                        .collect();
                    if let Some(default) = entries
                        .iter()
                        .find(|entry| entry.attribute((XML, "lang")) == Some("x-default"))
                    {
                        return Some(vec![default.text().unwrap_or("").to_owned()]);
                    }
                    if entries.is_empty() && field != KEYWORDS {
                        return Some(vec![value.text().unwrap_or("").to_owned()]);
                    }
                    Some(
                        entries
                            .into_iter()
                            .map(|entry| entry.text().unwrap_or("").to_owned())
                            .collect(),
                    )
                })
                .map(|values| (field, values))
        })
        .collect()
}

fn first(properties: &Properties, field: Field) -> Option<&str> {
    properties.get(&field)?.first().map(String::as_str)
}

fn from_metadata(meta: &ImageMetadata) -> Properties {
    let tags = meta.tags.as_deref().unwrap_or_default();
    let label = tags
        .iter()
        .find_map(|tag| tag.strip_prefix("color:"))
        .unwrap_or_default();
    let mut label_chars = label.chars();
    let label = label_chars
        .next()
        .map(|first| first.to_uppercase().to_string() + label_chars.as_str())
        .unwrap_or_default();
    let mut properties = Properties::from([
        (RATING, vec![meta.rating.to_string()]),
        (LABEL, vec![label]),
        (
            FLAG,
            vec![
                if tags.iter().any(|tag| tag == "flag:reject") {
                    "-1"
                } else if tags.iter().any(|tag| tag == "flag:pick") {
                    "1"
                } else {
                    "0"
                }
                .to_owned(),
            ],
        ),
        (
            KEYWORDS,
            tags.iter()
                .filter(|tag| !tag.starts_with("color:") && !tag.starts_with("flag:"))
                .map(|tag| tag.strip_prefix("user:").unwrap_or(tag).to_owned())
                .collect(),
        ),
    ]);
    if let Some(exif) = &meta.exif {
        for &(key, field) in SHARED_FIELDS {
            if let Some(value) = exif.get(key) {
                properties.insert(field, vec![value.clone()]);
            }
        }
    }
    properties
}

fn apply_properties(meta: &mut ImageMetadata, properties: &Properties) {
    let rating = first(properties, RATING).and_then(|value| value.parse::<f32>().ok());
    if let Some(rating) = rating {
        meta.rating = rating.clamp(0.0, 5.0).round() as u8;
    }
    let tags = meta.tags.get_or_insert_default();
    if let Some(keywords) = properties.get(&KEYWORDS) {
        tags.retain(|tag| tag.starts_with("color:") || tag.starts_with("flag:"));
        tags.extend(
            keywords
                .iter()
                .filter(|keyword| {
                    !keyword.is_empty()
                        && !keyword.starts_with("flag:")
                        && !keyword.starts_with("color:")
                })
                .map(|keyword| {
                    format!("user:{}", keyword.strip_prefix("user:").unwrap_or(keyword))
                }),
        );
    }
    if let Some(label) = first(properties, LABEL) {
        tags.retain(|tag| !tag.starts_with("color:"));
        if !label.is_empty() {
            tags.push(format!("color:{}", label.to_lowercase()));
        }
    }
    let legacy_flag = properties.get(&KEYWORDS).and_then(|keywords| {
        keywords.iter().find_map(|tag| match tag.as_str() {
            "flag:pick" => Some("1"),
            "flag:reject" => Some("-1"),
            _ => None,
        })
    });
    if let Some(flag) = first(properties, FLAG)
        .or(legacy_flag)
        .or_else(|| (rating == Some(-1.0)).then_some("-1"))
    {
        tags.retain(|tag| !tag.starts_with("flag:"));
        match flag {
            "1" => tags.push("flag:pick".into()),
            "-1" => tags.push("flag:reject".into()),
            _ => {}
        }
    }
    tags.sort_unstable();
    tags.dedup();
    if tags.is_empty() {
        meta.tags = None;
    }
    for &(key, field) in SHARED_FIELDS {
        if let Some(value) = first(properties, field) {
            meta.exif
                .get_or_insert_default()
                .insert(key.into(), value.to_owned());
        }
    }
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

pub fn load_metadata(source: &Path, edits: &Path) -> ImageMetadata {
    let mut meta = edits_only(crate::exif_processing::load_sidecar(edits));
    if let Some(path) = existing_path(source, edits) {
        let mut read = || -> Result<(), String> {
            if let Some(text) = read_optional(&path)? {
                let doc = Document::parse(&text).map_err(|error| error.to_string())?;
                apply_properties(&mut meta, &get_properties(rdf_node(&doc)?));
            }
            Ok(())
        };
        if let Err(error) = read() {
            log::warn!("Could not read {}: {error}", path.display());
        }
    }
    meta
}

fn edits_only(mut metadata: ImageMetadata) -> ImageMetadata {
    metadata.rating = 0;
    metadata.tags = None;
    if let Some(exif) = &mut metadata.exif {
        for &(key, _) in SHARED_FIELDS {
            exif.remove(key);
        }
    }
    metadata
}

fn patch_packet(text: &str, rdf: Node<'_, '_>, changes: &Properties) -> Result<String, String> {
    let mut body = String::new();
    for (&(namespace, local), values) in changes {
        let property = |value: &str| quick_xml::escape::escape(value).into_owned();
        let list = match (namespace, local) {
            KEYWORDS => Some("Bag"),
            (DC, "creator") | (EXIF, "ISOSpeedRatings") => Some("Seq"),
            (DC, "title" | "rights") | (EXIF, "UserComment") => Some("Alt"),
            _ => None,
        };
        body.push_str(&format!("<m:{local} xmlns:m=\"{namespace}\">"));
        if let Some(list) = list {
            body.push_str(&format!("<rdf:{list}>"));
            for value in values {
                let lang = if list == "Alt" {
                    " xml:lang=\"x-default\""
                } else {
                    ""
                };
                body.push_str(&format!("<rdf:li{lang}>{}</rdf:li>", property(value)));
            }
            body.push_str(&format!("</rdf:{list}>"));
        } else if let Some(value) = values.first() {
            body.push_str(&property(value));
        }
        body.push_str(&format!("</m:{local}>\n"));
    }
    let about = description_nodes(rdf)
        .find_map(|node| node.attribute((RDF, "about")))
        .unwrap_or("");
    let about = quick_xml::escape::escape(about);
    let description = format!(
        "\n<rdf:Description xmlns:rdf=\"{RDF}\" rdf:about=\"{about}\">\n{body}</rdf:Description>\n"
    );
    let range = rdf.range();
    let mut removals = Vec::new();
    for node in description_nodes(rdf) {
        for &field in changes.keys() {
            removals.extend(
                node.attributes()
                    .filter(|attr| (attr.namespace(), attr.name()) == (Some(field.0), field.1))
                    .map(|attr| attr.range()),
            );
            removals.extend(
                node.children()
                    .filter(|child| child.has_tag_name(field))
                    .map(|child| child.range()),
            );
        }
    }
    let mut output = text.to_owned();
    let rdf_text = &text[range.clone()];
    if rdf_text.ends_with("/>") {
        let qname = rdf_text[1..]
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("rdf:RDF");
        output.replace_range(
            range.end - 2..range.end,
            &format!(">{description}</{qname}>"),
        );
    } else {
        let closing = rdf_text
            .rfind("</")
            .ok_or("XMP packet has no RDF closing tag")?;
        output.insert_str(range.start + closing, &description);
    }
    removals.sort_unstable_by_key(|range| std::cmp::Reverse(range.start));
    for range in removals {
        output.replace_range(range, "");
    }
    Ok(output)
}

pub fn update_metadata(
    source: &Path,
    edits: &Path,
    update: impl FnOnce(&mut ImageMetadata),
) -> Result<(), String> {
    update_sidecars(source, edits, None, update)
}

pub fn save_edits(source: &Path, edits: &Path, adjustments: Value) -> Result<(), String> {
    update_sidecars(source, edits, Some(adjustments), |_| {})
}

fn update_sidecars(
    source: &Path,
    edits: &Path,
    adjustments: Option<Value>,
    update: impl FnOnce(&mut ImageMetadata),
) -> Result<(), String> {
    static LOCKS: OnceLock<[Mutex<()>; 64]> = OnceLock::new();
    let path = metadata_path(source, edits);
    let hash = blake3::hash(path.as_os_str().as_encoded_bytes());
    let locks = LOCKS.get_or_init(|| std::array::from_fn(|_| Mutex::new(())));
    let _guard = locks[usize::from(hash.as_bytes()[0]) % locks.len()]
        .lock()
        .map_err(|error| error.to_string())?;
    let existing_edits = read_optional(edits)?;
    let mut edit_document: Value = existing_edits
        .as_deref()
        .map(|text| serde_json::from_str(text.trim_start_matches('\u{feff}')))
        .transpose()
        .map_err(|error| format!("{}: {error}", edits.display()))?
        .unwrap_or_else(|| serde_json::json!({"version": 1}));
    let mut metadata = edits_only(
        serde_json::from_value(edit_document.clone()).map_err(|error| error.to_string())?,
    );
    if adjustments.is_none() {
        let packet = read_optional(&path)?.unwrap_or_else(|| EMPTY_PACKET.into());
        let doc =
            Document::parse(&packet).map_err(|error| format!("{}: {error}", path.display()))?;
        let rdf = rdf_node(&doc)?;
        let existing = get_properties(rdf);
        apply_properties(&mut metadata, &existing);
        let before = from_metadata(&metadata);
        update(&mut metadata);
        let after = from_metadata(&metadata);
        let mut changes: Properties = after
            .iter()
            .filter_map(|(&field, values)| {
                (before.get(&field) != Some(values)).then(|| (field, values.clone()))
            })
            .collect();
        if first(&existing, RATING) == Some("-1") && !existing.contains_key(&FLAG) {
            if changes.contains_key(&FLAG) {
                changes
                    .entry(RATING)
                    .or_insert_with(|| after[&RATING].clone());
            } else if changes.contains_key(&RATING) {
                changes.insert(FLAG, after[&FLAG].clone());
            }
        }
        if !changes.is_empty() {
            let packet = patch_packet(&packet, rdf, &changes)?;
            crate::portable_paths::write_atomic(&path, packet.as_bytes())?;
        }
    }
    let has_adjustments = adjustments.is_some();
    let object = edit_document
        .as_object_mut()
        .ok_or("Edit sidecar must contain a JSON object")?;
    let migrated = object.remove("rating").is_some() | object.remove("tags").is_some();
    let mut private_fields = metadata.exif.unwrap_or_default();
    for &(key, _) in SHARED_FIELDS {
        private_fields.remove(key);
    }
    let new_private_fields =
        (!private_fields.is_empty()).then(|| serde_json::json!(private_fields));
    let private_changed = object.get("exif") != new_private_fields.as_ref();
    if let Some(fields) = new_private_fields {
        object.insert("exif".into(), fields);
    } else {
        object.remove("exif");
    }
    if let Some(adjustments) = adjustments {
        object.insert("adjustments".into(), adjustments);
    }
    let removed_rating = object
        .get_mut("adjustments")
        .and_then(Value::as_object_mut)
        .is_some_and(|adjustments| adjustments.remove("rating").is_some());
    if has_adjustments || migrated || removed_rating || private_changed {
        let json = serde_json::to_vec_pretty(&edit_document).map_err(|error| error.to_string())?;
        crate::portable_paths::write_atomic(edits, &json)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_metadata_and_edit_sidecars_round_trip_independently() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("photo.NEF");
        let edits = crate::exif_processing::get_primary_sidecar_path(&source);
        let xmp = source.with_extension("xmp");
        let initial = format!(
            r#"<r:RDF xmlns:r="{RDF}" xmlns:x="{XMP}" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><r:Description x:Rating="2" crs:Exposure2012="0.50"/></r:RDF>"#
        );
        fs::write(&xmp, initial).unwrap();
        fs::write(&edits, r#"{"version":1,"rating":5,"tags":["flag:reject"],"adjustments":{"rating":5,"exposure":0.1}}"#).unwrap();
        assert_eq!(load_metadata(&source, &edits).rating, 2);

        update_metadata(&source, &edits, |metadata| {
            metadata.rating = 4;
            metadata.tags = Some(vec![
                "flag:pick".into(),
                "color:red".into(),
                "user:A & B".into(),
            ]);
            metadata
                .exif
                .get_or_insert_default()
                .insert("Artist".into(), "A < B".into());
        })
        .unwrap();
        let before_edit = fs::read_to_string(&xmp).unwrap();
        assert!(before_edit.contains(r#"crs:Exposure2012="0.50""#));
        assert!(before_edit.contains("A &amp; B"));
        save_edits(&source, &edits, serde_json::json!({"exposure": 0.8})).unwrap();
        assert_eq!(fs::read_to_string(&xmp).unwrap(), before_edit);
        let metadata = load_metadata(&source, &edits);
        assert_eq!(metadata.rating, 4);
        assert_eq!(
            metadata.tags.unwrap(),
            ["color:red", "flag:pick", "user:A & B"]
        );
        assert_eq!(metadata.exif.unwrap()["Artist"], "A < B");
        let json: Value = serde_json::from_slice(&fs::read(&edits).unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"version": 1, "adjustments": {"exposure": 0.8}})
        );

        update_metadata(&source, &edits, |metadata| {
            metadata.rating = 0;
            metadata.tags = None;
            metadata
                .exif
                .as_mut()
                .unwrap()
                .insert("Artist".into(), String::new());
        })
        .unwrap();
        let metadata = load_metadata(&source, &edits);
        assert_eq!(metadata.rating, 0);
        assert!(metadata.tags.is_none());
        assert_eq!(metadata.exif.unwrap()["Artist"], "");
    }

    #[test]
    fn metadata_only_writes_do_not_create_edit_files_or_share_ratings_with_jpeg_pairs() {
        let folder = tempfile::tempdir().unwrap();
        let raw = folder.path().join("photo.NEF");
        let jpeg = folder.path().join("photo.JPG");
        fs::write(&raw, "").unwrap();
        let raw_edits = crate::exif_processing::get_primary_sidecar_path(&raw);
        let jpeg_edits = crate::exif_processing::get_primary_sidecar_path(&jpeg);
        update_metadata(&raw, &raw_edits, |metadata| metadata.rating = 1).unwrap();
        update_metadata(&jpeg, &jpeg_edits, |metadata| metadata.rating = 3).unwrap();
        assert!(!raw_edits.exists() && !jpeg_edits.exists());
        assert_eq!(load_metadata(&raw, &raw_edits).rating, 1);
        assert_eq!(load_metadata(&jpeg, &jpeg_edits).rating, 3);
    }
}
