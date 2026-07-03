use exif::{Exif, In, Reader, Tag};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Serialize)]
struct PhotoEntry {
    filename: String,
    path: String,
    lat: f64,
    lng: f64,
    timestamp: String,
    media_type: String,
}

fn parse_ffprobe_location(loc: &str) -> Option<(f64, f64)> {
    // Formats: "+40.4839+25.4786/" or "+40.4839+25.4786+0.000/"
    let s = loc.trim_end_matches('/');
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    // Find the second +/- that starts the longitude
    let first_sign_pos = bytes.iter().position(|&b| b == b'+' || b == b'-')?;
    let rest = &s[first_sign_pos + 1..];
    let second_sign_pos = rest.bytes().position(|b| b == b'+' || b == b'-')?;
    let lat_str = &s[first_sign_pos..=first_sign_pos + second_sign_pos];
    let lng_part = &rest[second_sign_pos + 1..];
    // lng_part may contain altitude separated by another +/- or end here
    let lng_str = if let Some(alt_pos) = lng_part.bytes().position(|b| b == b'+' || b == b'-') {
        &lng_part[..alt_pos]
    } else {
        lng_part
    };
    let lat: f64 = lat_str.parse().ok()?;
    let lng: f64 = lng_str.parse().ok()?;
    Some((lat, lng))
}

fn dms_to_decimal(deg: f64, min: f64, sec: f64, ref_: &str) -> f64 {
    let mut result = deg + min / 60.0 + sec / 3600.0;
    if ref_ == "S" || ref_ == "W" {
        result = -result;
    }
    result
}

fn extract_rational_array(val: &exif::Value) -> Option<[f64; 3]> {
    match val {
        exif::Value::Rational(rats) if rats.len() >= 3 => {
            Some([rats[0].to_f64(), rats[1].to_f64(), rats[2].to_f64()])
        }
        _ => None,
    }
}

fn process_jpeg(path: &Path, rel_path: &str) -> Option<PhotoEntry> {
    let file = fs::File::open(path).ok()?;
    let reader = Reader::new();
    let exif: Exif = reader
        .read_from_container(&mut std::io::BufReader::new(file))
        .ok()?;

    let lat_val = exif.get_field(Tag::GPSLatitude, In::PRIMARY)?;
    let lat_ref = exif.get_field(Tag::GPSLatitudeRef, In::PRIMARY)?;
    let lng_val = exif.get_field(Tag::GPSLongitude, In::PRIMARY)?;
    let lng_ref = exif.get_field(Tag::GPSLongitudeRef, In::PRIMARY)?;

    let lat_dms = extract_rational_array(&lat_val.value)?;
    let lng_dms = extract_rational_array(&lng_val.value)?;

    let lat_ref_str = match &lat_ref.value {
        exif::Value::Ascii(v) => String::from_utf8_lossy(&v[0]).trim().to_string(),
        _ => return None,
    };
    let lng_ref_str = match &lng_ref.value {
        exif::Value::Ascii(v) => String::from_utf8_lossy(&v[0]).trim().to_string(),
        _ => return None,
    };

    let lat = dms_to_decimal(lat_dms[0], lat_dms[1], lat_dms[2], &lat_ref_str);
    let lng = dms_to_decimal(lng_dms[0], lng_dms[1], lng_dms[2], &lng_ref_str);

    let timestamp = exif
        .get_field(Tag::DateTimeOriginal, In::PRIMARY)
        .or_else(|| exif.get_field(Tag::DateTime, In::PRIMARY))
        .map(|f| match &f.value {
            exif::Value::Ascii(v) => String::from_utf8_lossy(&v[0]).trim().to_string(),
            _ => "unknown".to_string(),
        })
        .unwrap_or_else(|| "unknown".to_string());

    let filename = path.file_name()?.to_str()?.to_string();

    Some(PhotoEntry {
        filename,
        path: format!("photos/{}", rel_path),
        lat,
        lng,
        timestamp,
        media_type: "image/jpeg".into(),
    })
}

fn process_via_ffprobe(path: &Path, rel_path: &str) -> Option<PhotoEntry> {
    let filename = path.file_name()?.to_str()?.to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let media_type = match ext.as_str() {
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        _ => "video/mp4",
    };

    let output = Command::new("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_format"])
        .arg(path.as_os_str())
        .output()
        .ok()?;

    let stdout = String::from_utf8(output.stdout).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&stdout).ok()?;
    let tags = parsed.get("format")?.get("tags")?;

    // Parse location like  "+40.4839+25.4786/"
    let loc = tags.get("location")?.as_str()?;
    let (lat, lng) = parse_ffprobe_location(loc)?;

    // Parse creation_time like "2026-06-18T16:00:37.000000Z"
    let ts = tags
        .get("creation_time")
        .or_else(|| tags.get("creation_time-eng"))
        .or_else(|| tags.get("date"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let timestamp = if !ts.is_empty() {
        // Convert ISO 8601 to EXIF-like format
        ts.replace('T', " ")
            .trim_end_matches('Z')
            .trim_end_matches(|c: char| c.is_ascii_digit() || c == '.')
            .to_string()
    } else {
        "unknown".to_string()
    };

    Some(PhotoEntry {
        filename,
        path: format!("photos/{}", rel_path),
        lat,
        lng,
        timestamp,
        media_type: media_type.into(),
    })
}

fn is_supported(ext: &str) -> bool {
    matches!(
        ext,
        "jpg"
            | "jpeg"
            | "mp4"
            | "mov"
            | "avi"
            | "webm"
            | "heic"
            | "heif"
            | "webp"
            | "png"
            | "gif"
            | "tiff"
            | "tif"
    )
}

fn main() {
    // Photos are served directly from `Photos-3-001` (see the `/photos` route in
    // src/main.rs), so this tool only extracts metadata — it never copies media.
    let photos_dir = Path::new("Photos-3-001");

    let mut entries = Vec::new();

    for entry in fs::read_dir(photos_dir).expect("read Photos-3-001") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if !is_supported(&ext) {
            continue;
        }

        let filename = path
            .file_name()
            .expect("filename")
            .to_str()
            .expect("utf8")
            .to_string();

        let photo = match ext.as_str() {
            "jpg" | "jpeg" => process_jpeg(&path, &filename),
            _ => process_via_ffprobe(&path, &filename),
        };

        if let Some(photo) = photo {
            entries.push(photo);
            eprintln!("  OK  {}", filename);
        } else {
            eprintln!(" SKIP {}  (no GPS or metadata)", filename);
        }
    }

    let json = serde_json::to_string_pretty(&entries).expect("serialize");
    fs::write("assets/photo_data.json", &json).expect("write photo_data.json");

    eprintln!(
        "\nWrote {} entries to assets/photo_data.json",
        entries.len()
    );
}
