//! Metadata retained independently of the optional camera/EXIF metadata export.
//! Only source-type assertions and explicitly requested attribution are copied.
use image::ImageDecoder;
use std::{io::Cursor, path::Path};

const IPTC: &str = "http://cv.iptc.org/newscodes/digitalsourcetype/";

fn escape(value: &str) -> String {
    quick_xml::escape::escape(value).into_owned()
}

fn source_type(bytes: &[u8]) -> Option<String> {
    // These URIs occur in XMP and in uncompressed C2PA assertions. Keep the
    // assertion, not the original signature (which would no longer verify).
    const TERMS: &[&str] = &[
        "compositeWithTrainedAlgorithmicMedia",
        "trainedAlgorithmicMedia",
        "compositeSynthetic",
        "algorithmicMedia",
        "algorithmicallyEnhanced",
        "digitalCapture",
        "negativeFilm",
        "positiveFilm",
        "print",
        "digitalArt",
        "virtualRecording",
        "dataDrivenMedia",
        "minorHumanEdits",
        "compositeCapture",
    ];
    static PATTERN: std::sync::OnceLock<regex::bytes::Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::bytes::Regex::new(&format!(
            r"http://cv\.iptc\.org/newscodes/digitalsourcetype/({})\b",
            TERMS.join("|")
        ))
        .unwrap()
    });
    pattern
        .captures_iter(bytes)
        .filter_map(|found| {
            let term = std::str::from_utf8(found.get(1)?.as_bytes()).ok()?;
            Some((
                TERMS.iter().position(|known| *known == term)?,
                term.to_string(),
            ))
        })
        .min_by_key(|(priority, _)| *priority)
        .map(|(_, term)| format!("{IPTC}{term}"))
}

fn attribution_from_xmp(packet: &[u8]) -> (Option<String>, Option<String>) {
    use quick_xml::{events::Event, name::ResolveResult, reader::NsReader};
    let mut reader = NsReader::from_reader(packet);
    let mut creator_depth = None;
    let mut rights_depth = None;
    let mut depth = 0usize;
    let mut creator = String::new();
    let mut rights = String::new();
    loop {
        match reader.read_resolved_event() {
            Ok((ns, Event::Start(tag))) => {
                depth += 1;
                if tag.local_name().as_ref() == b"li" {
                    if creator_depth.is_some() && !creator.trim().is_empty() {
                        creator.push_str("; ");
                    }
                    if rights_depth.is_some() && !rights.trim().is_empty() {
                        rights.push_str("; ");
                    }
                }
                if matches!(ns, ResolveResult::Bound(ns) if ns.as_ref() == b"http://purl.org/dc/elements/1.1/")
                {
                    match tag.local_name().as_ref() {
                        b"creator" => creator_depth = Some(depth),
                        b"rights" => rights_depth = Some(depth),
                        _ => {}
                    }
                }
            }
            Ok((_, Event::Text(text))) => {
                if let Ok(value) = text.xml_content(quick_xml::XmlVersion::Implicit1_0) {
                    let value = quick_xml::escape::unescape(&value)
                        .map(|v| v.into_owned())
                        .unwrap_or_else(|_| value.into_owned());
                    if creator_depth.is_some() && !value.trim().is_empty() {
                        creator.push_str(&value);
                    }
                    if rights_depth.is_some() && !value.trim().is_empty() {
                        rights.push_str(&value);
                    }
                }
            }
            Ok((_, Event::GeneralRef(reference))) => {
                let character = reference.resolve_char_ref().ok().flatten().or_else(|| {
                    match reference.decode().ok().as_deref() {
                        Some("amp") => Some('&'),
                        Some("lt") => Some('<'),
                        Some("gt") => Some('>'),
                        Some("quot") => Some('"'),
                        Some("apos") => Some('\''),
                        _ => None,
                    }
                });
                if let Some(character) = character {
                    if creator_depth.is_some() {
                        creator.push(character);
                    }
                    if rights_depth.is_some() {
                        rights.push(character);
                    }
                }
            }
            Ok((_, Event::CData(text))) => {
                if let Ok(value) = text.decode() {
                    if creator_depth.is_some() {
                        creator.push_str(&value);
                    }
                    if rights_depth.is_some() {
                        rights.push_str(&value);
                    }
                }
            }
            Ok((_, Event::End(_))) => {
                if creator_depth == Some(depth) {
                    creator_depth = None;
                }
                if rights_depth == Some(depth) {
                    rights_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            Ok((_, Event::Eof)) | Err(_) => break,
            _ => {}
        }
    }
    (
        (!creator.trim().is_empty()).then(|| creator.trim().to_string()),
        (!rights.trim().is_empty()).then(|| rights.trim().to_string()),
    )
}

fn required_xmp(source: &str, keep_author: bool) -> Result<Option<Vec<u8>>, String> {
    let path = Path::new(source);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = crate::file_management::read_file_mapped(path).map_err(|e| e.to_string())?;
    let packet = image::ImageReader::new(Cursor::new(bytes.as_ref()))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_decoder().ok())
        .and_then(|mut decoder| decoder.xmp_metadata().ok().flatten())
        .or_else(|| {
            // ImageReader applies a pixel-sized TIFF allocation budget that is
            // too small for generic XMP tag values in small images. Metadata-only
            // reading through the TIFF decoder retains its own bounded limits.
            if bytes.starts_with(b"II") || bytes.starts_with(b"MM") {
                image::codecs::tiff::TiffDecoder::new(Cursor::new(bytes.as_ref()))
                    .ok()
                    .and_then(|mut decoder| decoder.xmp_metadata().ok().flatten())
            } else if bytes.starts_with(b"\xff\x0a") || bytes.starts_with(b"\0\0\0\x0cJXL ") {
                jxl_oxide::JxlImage::read_with_defaults(Cursor::new(bytes.as_ref()))
                    .ok()
                    .map(|image| image.aux_boxes().first_xml().unwrap_or(&[]).to_vec())
                    .filter(|packet| !packet.is_empty())
            } else {
                None
            }
        })
        .or_else(|| {
            // Uncompressed XML metadata items, including AVIF MIME items.
            let pattern =
                regex::bytes::Regex::new(r"<((?:[A-Za-z_][A-Za-z0-9_.-]*:)?xmpmeta)\b[^>]*>")
                    .ok()?;
            let found = pattern.captures(&bytes)?;
            let start = found.get(0)?.start();
            let mut ending = b"</".to_vec();
            ending.extend_from_slice(found.get(1)?.as_bytes());
            ending.push(b'>');
            let length = bytes[start..]
                .windows(ending.len())
                .position(|part| part == ending)?
                + ending.len();
            Some(bytes[start..start + length].to_vec())
        });
    let mut kind = source_type(&bytes);
    if let Some(packet) = &packet {
        if let Some(packet_kind) = source_type(packet) {
            // A weaker XMP assertion must not replace AI provenance found in
            // another source container, or vice versa for compressed XMP.
            kind =
                source_type(format!("{} {packet_kind}", kind.as_deref().unwrap_or("")).as_bytes());
        }
    }
    let (mut author, mut copyright) = if keep_author {
        packet
            .as_deref()
            .map(attribution_from_xmp)
            .unwrap_or_default()
    } else {
        (None, None)
    };
    if keep_author {
        if author.is_none() || copyright.is_none() {
            let metadata = crate::exif_processing::read_source_metadata(path)?;
            author = author.or_else(|| metadata.fields.get("Artist").cloned());
            copyright = copyright.or_else(|| metadata.fields.get("Copyright").cloned());
        }
        // Explicit metadata edits take precedence, including clearing a field.
        if let Some(map) = crate::exif_processing::read_rrexif_sidecar(path) {
            if let Some(value) = map.get("Artist") {
                author = Some(value.trim_matches('"').to_string());
            }
            if let Some(value) = map.get("Copyright") {
                copyright = Some(value.trim_matches('"').to_string());
            }
        }
    }
    let mut fields = String::new();
    if let Some(kind) = kind {
        fields.push_str(&format!(
            "<Iptc4xmpExt:DigitalSourceType>{}</Iptc4xmpExt:DigitalSourceType>",
            escape(&kind)
        ));
    }
    if let Some(author) = author.filter(|s| !s.is_empty()) {
        fields.push_str(&format!(
            "<dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>",
            escape(&author)
        ));
    }
    if let Some(rights) = copyright.filter(|s| !s.is_empty()) {
        fields.push_str(&format!(
            "<dc:rights><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:rights>",
            escape(&rights)
        ));
    }
    if fields.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:Iptc4xmpExt=\"http://iptc.org/std/Iptc4xmpExt/2008-02-29/\">{fields}</rdf:Description></rdf:RDF></x:xmpmeta>").into_bytes()))
}

pub fn preserve_required_metadata(
    data: &mut Vec<u8>,
    source: &str,
    format: &str,
    keep_author: bool,
    width: u32,
    height: u32,
) -> Result<(), String> {
    if let Some(xmp) = required_xmp(source, keep_author)? {
        embed_xmp(data, format, &xmp, width, height)?;
    }
    Ok(())
}

fn bmff_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(payload.len() + 8);
    data.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
    data.extend_from_slice(kind);
    data.extend_from_slice(payload);
    data
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn embed_xmp(
    data: &mut Vec<u8>,
    format: &str,
    xmp: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
    match format {
        "jpg" | "jpeg" => {
            let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
            payload.extend_from_slice(xmp);
            let size = u16::try_from(payload.len() + 2)
                .map_err(|_| "Attribution XMP is too large for JPEG")?;
            let mut segment = vec![0xff, 0xe1];
            segment.extend_from_slice(&size.to_be_bytes());
            segment.extend_from_slice(&payload);
            data.splice(2..2, segment);
        }
        "png" => {
            let mut payload = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
            payload.extend_from_slice(xmp);
            let mut chunk = (payload.len() as u32).to_be_bytes().to_vec();
            chunk.extend_from_slice(b"iTXt");
            chunk.extend_from_slice(&payload);
            chunk.extend_from_slice(&crc32(&chunk[4..]).to_be_bytes());
            // Immediately after IHDR, before IDAT.
            data.splice(33..33, chunk);
        }
        "webp" => {
            if data.get(12..16) == Some(b"VP8X") {
                data[20] |= 0x04;
            } else {
                let alpha = data.get(12..16) == Some(b"VP8L")
                    && data.get(24).is_some_and(|b| b & 0x10 != 0);
                let mut extended = b"VP8X\x0a\0\0\0".to_vec();
                extended.extend_from_slice(&[0x04 | if alpha { 0x10 } else { 0 }, 0, 0, 0]);
                extended.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
                extended.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
                data.splice(12..12, extended);
            }
            data.extend_from_slice(b"XMP ");
            data.extend_from_slice(&(xmp.len() as u32).to_le_bytes());
            data.extend_from_slice(xmp);
            if xmp.len() % 2 != 0 {
                data.push(0);
            }
            let size = (data.len() - 8) as u32;
            data[4..8].copy_from_slice(&size.to_le_bytes());
        }
        "jxl" => {
            if !data.starts_with(b"\0\0\0\x0cJXL \x0d\x0a\x87\x0a") {
                let codestream = std::mem::take(data);
                data.extend_from_slice(b"\0\0\0\x0cJXL \x0d\x0a\x87\x0a");
                data.extend(bmff_box(b"ftyp", b"jxl \0\0\0\0jxl "));
                data.extend(bmff_box(b"jxlc", &codestream));
            }
            data.extend(bmff_box(b"xml ", xmp));
        }
        "tif" | "tiff" => embed_tiff(data, xmp)?,
        "avif" => embed_avif(data, xmp)?,
        _ => return Err(format!("Cannot preserve required metadata in {format}")),
    }
    Ok(())
}

fn embed_tiff(data: &mut Vec<u8>, xmp: &[u8]) -> Result<(), String> {
    // Relocate the first IFD; existing pixel/profile offsets remain unchanged.
    let little = data.starts_with(b"II");
    let u16_at = |bytes: &[u8]| {
        if little {
            u16::from_le_bytes(bytes.try_into().unwrap())
        } else {
            u16::from_be_bytes(bytes.try_into().unwrap())
        }
    };
    let u32_at = |bytes: &[u8]| {
        if little {
            u32::from_le_bytes(bytes.try_into().unwrap())
        } else {
            u32::from_be_bytes(bytes.try_into().unwrap())
        }
    };
    let put16 = |n: u16| {
        if little {
            n.to_le_bytes()
        } else {
            n.to_be_bytes()
        }
    };
    let put32 = |n: u32| {
        if little {
            n.to_le_bytes()
        } else {
            n.to_be_bytes()
        }
    };
    let start = u32_at(data.get(4..8).ok_or("Invalid TIFF header")?) as usize;
    let count = u16_at(data.get(start..start + 2).ok_or("Invalid TIFF IFD")?) as usize;
    let entries = data
        .get(start + 2..start + 2 + 12 * count)
        .ok_or("Invalid TIFF entries")?;
    let mut entries = entries.chunks_exact(12).map(Vec::from).collect::<Vec<_>>();
    entries.retain(|entry| u16_at(&entry[..2]) != 700);
    let next = data
        .get(start + 2 + 12 * count..start + 6 + 12 * count)
        .ok_or("Invalid TIFF chain")?
        .to_vec();
    let xmp_offset = data.len() as u32;
    let mut entry = Vec::new();
    entry.extend_from_slice(&put16(700));
    entry.extend_from_slice(&put16(1));
    entry.extend_from_slice(&put32(xmp.len() as u32));
    entry.extend_from_slice(&put32(xmp_offset));
    entries.push(entry);
    entries.sort_by_key(|entry| u16_at(&entry[..2]));
    data.extend_from_slice(xmp);
    if data.len() % 2 != 0 {
        data.push(0);
    }
    let ifd_offset = data.len() as u32;
    data.extend_from_slice(&put16(entries.len() as u16));
    for entry in entries {
        data.extend(entry);
    }
    data.extend(next);
    data[4..8].copy_from_slice(&put32(ifd_offset));
    Ok(())
}

fn boxes(data: &[u8]) -> Result<Vec<(usize, &[u8], &[u8])>, String> {
    let mut result = Vec::new();
    let mut position = 0;
    while position < data.len() {
        let header = data
            .get(position..position + 8)
            .ok_or("Truncated BMFF box")?;
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        if size < 8 {
            return Err("Unsupported BMFF box size".into());
        }
        let payload = data
            .get(position + 8..position + size)
            .ok_or("Invalid BMFF box length")?;
        result.push((position, &header[4..8], payload));
        position += size;
    }
    Ok(result)
}

fn embed_avif(data: &mut Vec<u8>, xmp: &[u8]) -> Result<(), String> {
    // avif-serialize writes v0 iloc with absolute 32-bit offsets. Append a new
    // meta box so all original image offsets still reference the same bytes.
    const ID: u16 = 0xfffe;
    let (meta_start, _, meta) = boxes(data)?
        .into_iter()
        .find(|(_, kind, _)| *kind == b"meta")
        .ok_or("Missing AVIF meta")?;
    let mut children = Vec::new();
    let xmp_offset = u32::try_from(data.len() + 8).map_err(|_| "AVIF exceeds 4 GB")?;
    let mut has_iinf = false;
    let mut has_iloc = false;
    let mut has_iref = false;
    for (_, kind, payload) in boxes(meta.get(4..).ok_or("Invalid AVIF meta")?)? {
        let mut payload = payload.to_vec();
        match kind {
            b"iinf" => {
                if payload.len() < 6 || payload[0] != 0 {
                    return Err("Unsupported AVIF iinf".into());
                }
                let count = u16::from_be_bytes(payload[4..6].try_into().unwrap());
                payload[4..6].copy_from_slice(&(count + 1).to_be_bytes());
                let mut item = vec![2, 0, 0, 0];
                item.extend_from_slice(&ID.to_be_bytes());
                item.extend_from_slice(&[0, 0]);
                item.extend_from_slice(b"mime\0application/rdf+xml\0\0");
                payload.extend(bmff_box(b"infe", &item));
                has_iinf = true;
            }
            b"iloc" => {
                if payload.len() < 8 || payload[..6] != [0, 0, 0, 0, 0x44, 0] {
                    return Err("Unsupported AVIF iloc".into());
                }
                let count = u16::from_be_bytes(payload[6..8].try_into().unwrap());
                payload[6..8].copy_from_slice(&(count + 1).to_be_bytes());
                payload.extend_from_slice(&ID.to_be_bytes());
                payload.extend_from_slice(&[0, 0, 0, 1]);
                payload.extend_from_slice(&xmp_offset.to_be_bytes());
                payload.extend_from_slice(&(xmp.len() as u32).to_be_bytes());
                has_iloc = true;
            }
            b"iref" => {
                if payload.first() != Some(&0) {
                    return Err("Unsupported AVIF iref".into());
                }
                let mut reference = ID.to_be_bytes().to_vec();
                reference.extend_from_slice(&[0, 1, 0, 1]); // one reference to primary item 1
                payload.extend(bmff_box(b"cdsc", &reference));
                has_iref = true;
            }
            _ => {}
        }
        children.extend(bmff_box(kind.try_into().unwrap(), &payload));
    }
    if !has_iinf || !has_iloc {
        return Err("Missing AVIF metadata item tables".into());
    }
    if !has_iref {
        let mut reference = ID.to_be_bytes().to_vec();
        reference.extend_from_slice(&[0, 1, 0, 1]);
        let mut payload = vec![0, 0, 0, 0];
        payload.extend(bmff_box(b"cdsc", &reference));
        children.extend(bmff_box(b"iref", &payload));
    }
    let mut new_meta = vec![0, 0, 0, 0];
    new_meta.extend(children);
    data[meta_start + 4..meta_start + 8].copy_from_slice(b"free");
    data.extend(bmff_box(b"mdat", xmp));
    data.extend(bmff_box(b"meta", &new_meta));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        color_management::ColorSpace,
        export_processing::{TiffBitDepth, encode_image_to_bytes},
    };
    use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba};

    #[test]
    fn required_metadata_survives_all_export_containers() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("source.png");
        let source =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(32, 32, Rgba([43, 129, 211, 190])));
        let mut bytes = encode_image_to_bytes(
            &source,
            "png",
            90,
            TiffBitDepth::Sixteen,
            16,
            ColorSpace::Srgb,
        )
        .unwrap();
        let source_xmp = format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:iptc=\"http://iptc.org/std/Iptc4xmpExt/2008-02-29/\" iptc:DigitalSourceType=\"{IPTC}trainedAlgorithmicMedia\"><dc:creator><rdf:Seq><rdf:li>A &amp; B</rdf:li></rdf:Seq></dc:creator><dc:rights><rdf:Alt><rdf:li>Copyright A</rdf:li></rdf:Alt></dc:rights><secret>PRIVATE GPS AND CAMERA</secret></rdf:Description></rdf:RDF></x:xmpmeta>"
        );
        embed_xmp(&mut bytes, "png", source_xmp.as_bytes(), 32, 32).unwrap();
        std::fs::write(&source_path, bytes).unwrap();
        for keep_author in [false, true] {
            let xmp = required_xmp(source_path.to_str().unwrap(), keep_author)
                .unwrap()
                .unwrap();
            let text = String::from_utf8(xmp.clone()).unwrap();
            assert!(text.contains("trainedAlgorithmicMedia"));
            assert_eq!(text.contains("A &amp; B"), keep_author);
            assert_eq!(text.contains("Copyright A"), keep_author);
            assert!(!text.contains("PRIVATE"));
            for format in ["jpeg", "png", "tiff", "webp", "jxl", "avif"] {
                let mut data = encode_image_to_bytes(
                    &source,
                    format,
                    100,
                    TiffBitDepth::Sixteen,
                    16,
                    ColorSpace::Srgb,
                )
                .unwrap();
                embed_xmp(&mut data, format, &xmp, 32, 32).unwrap();
                let output = dir.path().join(format!("roundtrip-{keep_author}.{format}"));
                std::fs::write(&output, &data).unwrap();
                assert_eq!(
                    required_xmp(output.to_str().unwrap(), true)
                        .unwrap()
                        .unwrap(),
                    xmp,
                    "source {format}"
                );
                if ["jpeg", "png", "tiff", "webp"].contains(&format) {
                    let mut decoder: Box<dyn ImageDecoder> = if format == "tiff" {
                        Box::new(image::codecs::tiff::TiffDecoder::new(Cursor::new(&data)).unwrap())
                    } else {
                        Box::new(
                            image::ImageReader::new(Cursor::new(&data))
                                .with_guessed_format()
                                .unwrap()
                                .into_decoder()
                                .unwrap(),
                        )
                    };
                    assert_eq!(
                        decoder
                            .xmp_metadata()
                            .unwrap_or_else(|e| panic!("{format}: {e}"))
                            .unwrap(),
                        xmp,
                        "{format}"
                    );
                    let decoded = DynamicImage::from_decoder(decoder).unwrap();
                    assert_eq!(decoded.dimensions(), source.dimensions());
                    if format == "png" {
                        assert_eq!(decoded.to_rgba8(), source.to_rgba8(), "{format}");
                    }
                    if format == "tiff" {
                        assert_eq!(decoded.to_rgb8(), source.to_rgb8(), "{format}");
                    }
                } else if format == "jxl" {
                    let decoder =
                        jxl_oxide::integration::JxlDecoder::new(Cursor::new(&data)).unwrap();
                    let decoded = DynamicImage::from_decoder(decoder).unwrap();
                    assert_eq!(decoded.to_rgba8(), source.to_rgba8());
                    assert!(data.windows(xmp.len()).any(|part| part == xmp));
                } else {
                    let top = boxes(&data).unwrap();
                    assert_eq!(
                        top.iter().filter(|(_, kind, _)| *kind == b"meta").count(),
                        1
                    );
                    let (_, _, meta) = top.iter().find(|(_, kind, _)| *kind == b"meta").unwrap();
                    let children = boxes(&meta[4..]).unwrap();
                    let (_, _, iloc) = children
                        .iter()
                        .find(|(_, kind, _)| *kind == b"iloc")
                        .unwrap();
                    let offset = u32::from_be_bytes(
                        iloc[iloc.len() - 8..iloc.len() - 4].try_into().unwrap(),
                    ) as usize;
                    assert_eq!(&data[offset..offset + xmp.len()], xmp);
                    assert!(children.iter().any(|(_, kind, payload)| {
                        *kind == b"iinf"
                            && payload
                                .windows(19)
                                .any(|part| part == b"application/rdf+xml")
                    }));
                }
            }
        }
    }
}
