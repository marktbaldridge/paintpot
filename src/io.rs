//! Loading and saving pictures.

use crate::canvas::Image;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// Largest width/height we accept (keeps us under common GPU texture limits).
pub const MAX_DIM: u32 = 8192;

pub const OPEN_EXTS: &[&str] = &["png", "bmp", "jpg", "jpeg", "gif", "tif", "tiff", "webp", "ico"];

pub fn load(path: &Path) -> Result<Image, String> {
    let img = image::open(path).map_err(|e| format!("Could not open {}:\n{e}", path.display()))?.to_rgba8();
    let (w, h) = img.dimensions();
    if w > MAX_DIM || h > MAX_DIM {
        return Err(format!("{}x{} is too big (the limit is {MAX_DIM}x{MAX_DIM}).", w, h));
    }
    Ok(Image::from_rgba(w as usize, h as usize, img.as_raw()))
}

pub fn save(img: &Image, path: &Path) -> Result<(), String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    let (w, h) = (img.w as u32, img.h as u32);
    let res = match ext.as_str() {
        "jpg" | "jpeg" => {
            let f = File::create(path).map_err(|e| e.to_string())?;
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(BufWriter::new(f), 92);
            enc.encode(&img.to_rgb(), w, h, image::ExtendedColorType::Rgb8)
        }
        "gif" | "tif" | "tiff" | "ico" | "webp" => image::RgbaImage::from_raw(w, h, img.to_rgba()).unwrap().save(path),
        _ => image::RgbImage::from_raw(w, h, img.to_rgb()).unwrap().save(path),
    };
    res.map_err(|e| format!("Could not save {}:\n{e}", path.display()))
}
