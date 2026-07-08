//! Re-reads EXIF/ffprobe metadata from Photos-3-001/ into assets/photo_data.json.
//! Photos are served directly from the source dir, so this only extracts
//! metadata — it never copies media.

use std::fs;
use std::path::Path;

use photo_extract::{extract_entry, PHOTOS_SRC_DIR};

fn main() {
    let photos_dir = Path::new(PHOTOS_SRC_DIR);

    let mut entries = Vec::new();

    for entry in fs::read_dir(photos_dir).expect("read Photos-3-001") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("")
            .to_string();

        match extract_entry(&path) {
            Some(photo) => {
                entries.push(photo);
                eprintln!("  OK  {}", filename);
            }
            None => eprintln!(" SKIP {}  (no GPS or metadata)", filename),
        }
    }

    let json = serde_json::to_string_pretty(&entries).expect("serialize");
    fs::write("assets/photo_data.json", &json).expect("write photo_data.json");

    eprintln!(
        "\nWrote {} entries to assets/photo_data.json",
        entries.len()
    );
}
