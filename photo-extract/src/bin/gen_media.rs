//! Web-sized JPEGs of the original photos, published to the `photos` GitHub
//! release by `scripts/publish_media.sh` and served by the app at
//! `/photos/<filename>`.
//!
//! Usage: `gen-media --out <dir> <photo>...`
//!
//! Each output keeps the original's filename. Originals (~12 MP, ~5 MB) are
//! scaled to about 4 MP (~1 MB) — enough for the app's zoom — with the EXIF
//! orientation baked into the pixels, since the re-encoded file carries no
//! EXIF (which also drops the GPS/camera metadata from the published copies).
//! An output newer than its source is skipped, so re-runs are cheap.

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageReader};

/// Target pixel count; area-based so panoramas keep a usable height.
const MAX_PIXELS: f64 = 4_000_000.0;
const JPEG_QUALITY: u8 = 85;

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn up_to_date(src: &Path, dst: &Path) -> bool {
    matches!((mtime(src), mtime(dst)), (Some(s), Some(d)) if d >= s)
}

fn make_display(src: &Path, dst: &Path) -> Result<(), String> {
    let mut decoder = ImageReader::open(src)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);

    let (w, h) = img.dimensions();
    let scale = (MAX_PIXELS / (f64::from(w) * f64::from(h))).sqrt();
    if scale < 1.0 {
        let nw = (f64::from(w) * scale).round() as u32;
        let nh = (f64::from(h) * scale).round() as u32;
        img = img.resize(nw, nh, FilterType::Lanczos3);
    }

    // Write under a temp name so an interrupted run never leaves a truncated
    // file that looks up to date.
    let tmp = dst.with_extension("part");
    let out = fs::File::create(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
    JpegEncoder::new_with_quality(BufWriter::new(out), JPEG_QUALITY)
        .encode_image(&img.to_rgb8())
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, dst).map_err(|e| e.to_string())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(flag), Some(out_dir)) = (args.next(), args.next()) else {
        eprintln!("usage: gen-media --out <dir> <photo>...");
        std::process::exit(2);
    };
    if flag != "--out" {
        eprintln!("usage: gen-media --out <dir> <photo>...");
        std::process::exit(2);
    }
    let out_dir = PathBuf::from(out_dir);
    fs::create_dir_all(&out_dir).expect("create output dir");
    let sources: Vec<PathBuf> = args.map(PathBuf::from).collect();

    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let Some(src) = sources.get(next.fetch_add(1, Ordering::Relaxed)) else {
                    break;
                };
                let name = src.file_name().expect("source has a filename");
                let dst = out_dir.join(name);
                if up_to_date(src, &dst) {
                    continue;
                }
                match make_display(src, &dst) {
                    Ok(()) => eprintln!("  OK  {}", dst.display()),
                    Err(e) => {
                        eprintln!(" FAIL {} ({e})", src.display());
                        failures.lock().unwrap().push(src.clone());
                    }
                }
            });
        }
    });

    let failures = failures.into_inner().unwrap();
    if !failures.is_empty() {
        eprintln!("{} of {} photos failed", failures.len(), sources.len());
        std::process::exit(1);
    }
}
