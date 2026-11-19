use std::env;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use image::{ImageReader, Limits};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use tokio::sync::{Semaphore, mpsc};

use crate::incus::community::validate_slug;

/// Maximum downloaded or cached logo payload size (512 KiB).
const MAX_LOGO_BYTES: usize = 512 * 1024;
/// Strict decoder dimension cap to reject malformed or decompression-bomb images.
const MAX_DECODE_DIM: u32 = 2048;
/// Strict decoder heap cap (32 MiB) for raster image decoding.
const MAX_DECODE_ALLOC_BYTES: u64 = 32 * 1024 * 1024;
/// Target square bound for cached raster icons (crisp up to 3x `HiDPI` for 18px slots).
const TARGET_ICON_PX: u32 = 64;
/// Timeout for fetching an individual logo asset over HTTPS.
const LOGO_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// Maximum concurrent logo downloads to avoid bursting CDN connections.
const MAX_CONCURRENT_FETCHES: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoRequest {
    pub slug: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub enum DecodedLogo {
    Raster(SharedPixelBuffer<Rgba8Pixel>),
    Svg(Vec<u8>),
}

const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<DecodedLogo>();
};

impl DecodedLogo {
    #[must_use]
    pub fn into_slint_image(self) -> Option<Image> {
        match self {
            Self::Raster(buf) => Some(Image::from_rgba8_premultiplied(buf)),
            Self::Svg(bytes) => Image::load_from_svg_data(&bytes).ok(),
        }
    }
}

#[must_use]
pub fn cache_dir() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("incus-cubby").join("logos"));
    }
    let home = env::var_os("HOME").filter(|v| !v.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(".cache")
            .join("incus-cubby")
            .join("logos"),
    )
}

fn cache_path(dir: &Path, slug: &str) -> Option<PathBuf> {
    let valid = validate_slug(slug).ok()?;
    Some(dir.join(format!("{valid}.bin")))
}

async fn write_cache_file(path: &Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else {
        return;
    };
    if tokio::fs::create_dir_all(parent).await.is_err() {
        return;
    }
    let tmp_name = path.file_name().and_then(|n| n.to_str()).map_or_else(
        || format!("logo.{}.tmp", std::process::id()),
        |n| format!("{n}.{}.tmp", std::process::id()),
    );
    let tmp_path = parent.join(tmp_name);
    if tokio::fs::write(&tmp_path, bytes).await.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return;
    }
    if tokio::fs::rename(&tmp_path, path).await.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let after_bom = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let trimmed = after_bom.trim_ascii_start();
    if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case(b"<svg") {
        return true;
    }
    if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case(b"<?xml") {
        let header = &trimmed[..trimmed.len().min(1024)];
        return contains_ascii_case_insensitive(header, b"<svg");
    }
    false
}

#[must_use]
pub fn decode_logo_bytes(bytes: &[u8]) -> Option<DecodedLogo> {
    if bytes.is_empty() || bytes.len() > MAX_LOGO_BYTES {
        return None;
    }
    if looks_like_svg(bytes) {
        return Some(DecodedLogo::Svg(bytes.to_vec()));
    }

    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_DIM);
    limits.max_image_height = Some(MAX_DECODE_DIM);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    reader.limits(limits);

    let decoded = reader.decode().ok()?;
    let resized = if decoded.width() > TARGET_ICON_PX || decoded.height() > TARGET_ICON_PX {
        decoded.thumbnail(TARGET_ICON_PX, TARGET_ICON_PX)
    } else {
        decoded
    };

    let rgba = resized.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return None;
    }

    let mut pixels = rgba.into_raw();
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else if a < 255 {
            px[0] = ((u16::from(px[0]) * a + 127) / 255) as u8;
            px[1] = ((u16::from(px[1]) * a + 127) / 255) as u8;
            px[2] = ((u16::from(px[2]) * a + 127) / 255) as u8;
        }
    }

    Some(DecodedLogo::Raster(SharedPixelBuffer::clone_from_slice(
        &pixels, w, h,
    )))
}

async fn download_logo_bytes(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    if !url.starts_with("https://") {
        return None;
    }
    let mut resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    if resp
        .content_length()
        .is_some_and(|len| len > MAX_LOGO_BYTES as u64)
    {
        return None;
    }

    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        if buf.len().saturating_add(chunk.len()) > MAX_LOGO_BYTES {
            return None;
        }
        buf.extend_from_slice(&chunk);
    }
    if buf.is_empty() { None } else { Some(buf) }
}

pub fn spawn_logo_worker(
    mut rx: mpsc::UnboundedReceiver<LogoRequest>,
    on_loaded: impl Fn(String, DecodedLogo) + Send + Sync + 'static,
) {
    tokio::spawn(async move {
        let Ok(client) = reqwest::Client::builder()
            .user_agent("IncusCubby")
            .timeout(LOGO_HTTP_TIMEOUT)
            .build()
        else {
            return;
        };
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_FETCHES));
        let on_loaded = Arc::new(on_loaded);
        let dir = cache_dir();

        while let Some(req) = rx.recv().await {
            if validate_slug(&req.slug).is_err() {
                continue;
            }
            let client = client.clone();
            let semaphore = Arc::clone(&semaphore);
            let on_loaded = Arc::clone(&on_loaded);
            let dir = dir.clone();

            tokio::spawn(async move {
                let cached_path = dir.as_deref().and_then(|d| cache_path(d, &req.slug));
                if let Some(ref path) = cached_path
                    && let Ok(bytes) = tokio::fs::read(path).await
                {
                    if let Ok(Some(decoded)) =
                        tokio::task::spawn_blocking(move || decode_logo_bytes(&bytes)).await
                    {
                        on_loaded(req.slug, decoded);
                        return;
                    }
                    let _ = tokio::fs::remove_file(path).await;
                }

                if !req.url.starts_with("https://") {
                    return;
                }

                let Ok(permit) = semaphore.acquire_owned().await else {
                    return;
                };
                let downloaded = download_logo_bytes(&client, &req.url).await;
                drop(permit);

                let Some(raw_bytes) = downloaded else {
                    return;
                };
                let decode_bytes = raw_bytes.clone();
                let Ok(Some(decoded)) =
                    tokio::task::spawn_blocking(move || decode_logo_bytes(&decode_bytes)).await
                else {
                    return;
                };

                if let Some(ref path) = cached_path {
                    write_cache_file(path, &raw_bytes).await;
                }
                on_loaded(req.slug, decoded);
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};

    #[test]
    fn decode_logo_bytes_accepts_svg_and_rejects_html_and_oversized() {
        let svg = b"  <svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"></svg>";
        assert!(matches!(decode_logo_bytes(svg), Some(DecodedLogo::Svg(_))));

        let xml_svg = b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<SVG viewBox=\"0 0 10 10\"/>";
        assert!(matches!(
            decode_logo_bytes(xml_svg),
            Some(DecodedLogo::Svg(_))
        ));

        let html = b"<!DOCTYPE html><html><body>Not Found</body></html>";
        assert!(decode_logo_bytes(html).is_none());

        let xml_not_svg = b"<?xml version=\"1.0\"?><html><body>Nope</body></html>";
        assert!(decode_logo_bytes(xml_not_svg).is_none());

        assert!(decode_logo_bytes(b"").is_none());
        let oversized = vec![b'a'; MAX_LOGO_BYTES + 1];
        assert!(decode_logo_bytes(&oversized).is_none());
    }

    #[test]
    fn decode_logo_bytes_decodes_and_downscales_raster_png() {
        let img = RgbaImage::from_pixel(128, 64, Rgba([200, 100, 50, 128]));
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded)
            .write_image(img.as_raw(), 128, 64, image::ExtendedColorType::Rgba8)
            .expect("encode test png");

        let decoded = decode_logo_bytes(&encoded).expect("decode test png");
        let DecodedLogo::Raster(buf) = decoded else {
            panic!("expected raster buffer");
        };
        assert_eq!(buf.width(), 64);
        assert_eq!(buf.height(), 32);
        let first = buf.as_slice()[0];
        assert_eq!(first.a, 128);
        assert!(first.r <= first.a);
        assert!(first.g <= first.a);
        assert!(first.b <= first.a);
    }

    #[test]
    fn cache_path_rejects_invalid_slugs() {
        let base = Path::new("/tmp/cubby-logos");
        assert_eq!(
            cache_path(base, "docker").as_deref(),
            Some(Path::new("/tmp/cubby-logos/docker.bin"))
        );
        assert!(cache_path(base, "").is_none());
        assert!(cache_path(base, "../etc").is_none());
        assert!(cache_path(base, "a/b").is_none());
    }
}
