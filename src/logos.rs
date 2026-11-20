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
/// OS IDs displayed in Simple mode's Quick Create distro tiles.
pub const QUICK_CREATE_DISTROS: &[&str] =
    &["debian", "ubuntu", "fedora", "arch", "alpine", "opensuse"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoRequest {
    pub slug: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub enum DecodedLogo {
    Raster {
        buffer: SharedPixelBuffer<Rgba8Pixel>,
        accent: Option<[u8; 3]>,
    },
    Svg(Vec<u8>),
}

const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<DecodedLogo>();
};

impl DecodedLogo {
    #[must_use]
    pub fn into_slint_logo(self) -> Option<(Image, Option<[u8; 3]>)> {
        match self {
            Self::Raster { buffer, accent } => {
                Some((Image::from_rgba8_premultiplied(buffer), accent))
            }
            Self::Svg(bytes) => {
                let img = Image::load_from_svg_data(&bytes).ok()?;
                let sz = img.size();
                let accent =
                    if sz.width > 0 && sz.height > 0 && sz.width <= 1024 && sz.height <= 1024 {
                        img.to_rgba8()
                            .and_then(|buf| extract_accent_rgb(buf.as_bytes()))
                    } else {
                        None
                    };
                Some((img, accent))
            }
        }
    }
}

#[must_use]
pub fn os_logo_key_and_url(distro_id: &str) -> Option<(String, &'static str)> {
    let url = match distro_id {
        "alma" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/almalinux.webp",
        "alpine" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/alpine-linux.webp",
        "arch" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/arch-linux.webp",
        "cachyos" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/cachyos.webp",
        "centos" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/centos.webp",
        "debian" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/debian.webp",
        "devuan" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/devuan.webp",
        "fedora" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/fedora.webp",
        "freebsd" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/freebsd.webp",
        "gentoo" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/gentoo.webp",
        "kali" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/kali-linux.webp",
        "nixos" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/nixos.webp",
        "openeuler" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/openeuler.webp",
        "opensuse" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/opensuse.webp",
        "openwrt" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/openwrt.webp",
        "oracle" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/oracle.webp",
        "rocky" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/rocky-linux.webp",
        "ubuntu" => "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/ubuntu.webp",
        _ => return None,
    };
    Some((format!("os-{distro_id}"), url))
}

#[derive(Clone, Copy, Default)]
struct HueBucket {
    weight: u64,
    r_sum: u64,
    g_sum: u64,
    b_sum: u64,
}

#[must_use]
pub fn extract_accent_rgb(rgba: &[u8]) -> Option<[u8; 3]> {
    let mut buckets = [HueBucket::default(); 12];
    let mut total_chromatic_pixels = 0usize;

    for px in rgba.as_chunks::<4>().0 {
        let [r, g, b, a] = *px;
        if a < 128 {
            continue;
        }
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        if delta < 30 || max < 40 || min > 230 {
            continue;
        }

        let rf = f32::from(r);
        let gf = f32::from(g);
        let bf = f32::from(b);
        let df = f32::from(delta);
        let raw_hue = if max == r {
            60.0 * (((gf - bf) / df) % 6.0)
        } else if max == g {
            60.0 * (((bf - rf) / df) + 2.0)
        } else {
            60.0 * (((rf - gf) / df) + 4.0)
        };
        let hue = raw_hue.rem_euclid(360.0);
        let bucket_idx = ((hue / 30.0) as usize) % 12;

        let weight = u64::from(delta) * u64::from(a);
        total_chromatic_pixels += 1;
        let bucket = &mut buckets[bucket_idx];
        bucket.weight += weight;
        bucket.r_sum += u64::from(r) * weight;
        bucket.g_sum += u64::from(g) * weight;
        bucket.b_sum += u64::from(b) * weight;
    }

    if total_chromatic_pixels < 6 {
        return None;
    }

    let best = buckets.iter().max_by_key(|bucket| bucket.weight)?;
    if best.weight == 0 {
        return None;
    }

    let mut out_r = ((best.r_sum + best.weight / 2) / best.weight).min(255) as f32;
    let mut out_g = ((best.g_sum + best.weight / 2) / best.weight).min(255) as f32;
    let mut out_b = ((best.b_sum + best.weight / 2) / best.weight).min(255) as f32;

    let peak = out_r.max(out_g).max(out_b);
    if peak > 0.0 && peak < 104.0 {
        let scale = 128.0 / peak;
        out_r = (out_r * scale).clamp(0.0, 255.0);
        out_g = (out_g * scale).clamp(0.0, 255.0);
        out_b = (out_b * scale).clamp(0.0, 255.0);
    }

    Some([
        out_r.round() as u8,
        out_g.round() as u8,
        out_b.round() as u8,
    ])
}

#[must_use]
pub fn app_cache_dir() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("incus-cubby"));
    }
    let home = env::var_os("HOME").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(home).join(".cache").join("incus-cubby"))
}

#[must_use]
pub fn cache_dir() -> Option<PathBuf> {
    app_cache_dir().map(|dir| dir.join("logos"))
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

    let accent = extract_accent_rgb(rgba.as_raw());
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

    Some(DecodedLogo::Raster {
        buffer: SharedPixelBuffer::clone_from_slice(&pixels, w, h),
        accent,
    })
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
        let DecodedLogo::Raster {
            buffer: buf,
            accent,
        } = decoded
        else {
            panic!("expected raster buffer");
        };
        assert_eq!(buf.width(), 64);
        assert_eq!(buf.height(), 32);
        assert_eq!(accent, Some([200, 100, 50]));
        let first = buf.as_slice()[0];
        assert_eq!(first.a, 128);
        assert!(first.r <= first.a);
        assert!(first.g <= first.a);
        assert!(first.b <= first.a);
    }

    #[test]
    fn extract_accent_rgb_finds_vibrant_color_and_skips_monochrome() {
        let mut orange_white = Vec::new();
        for _ in 0..8 {
            orange_white.extend_from_slice(&[249, 115, 22, 255]);
        }
        for _ in 0..20 {
            orange_white.extend_from_slice(&[255, 255, 255, 255]);
        }
        assert_eq!(extract_accent_rgb(&orange_white), Some([249, 115, 22]));

        let mut dark_navy = Vec::new();
        for _ in 0..8 {
            dark_navy.extend_from_slice(&[10, 30, 60, 255]);
        }
        let lifted = extract_accent_rgb(&dark_navy).expect("dark navy should lift");
        assert!(
            lifted[2] >= 120,
            "expected lifted blue >= 120, got {lifted:?}"
        );
        assert!(lifted[2] > lifted[1] && lifted[1] > lifted[0]);

        let mut mono = Vec::new();
        for _ in 0..10 {
            mono.extend_from_slice(&[24, 24, 24, 255]);
            mono.extend_from_slice(&[128, 128, 128, 255]);
            mono.extend_from_slice(&[250, 250, 250, 255]);
        }
        assert_eq!(extract_accent_rgb(&mono), None);

        let mut multi = Vec::new();
        for _ in 0..12 {
            multi.extend_from_slice(&[36, 150, 237, 255]);
        }
        for _ in 0..6 {
            multi.extend_from_slice(&[239, 68, 68, 255]);
        }
        assert_eq!(extract_accent_rgb(&multi), Some([36, 150, 237]));
    }

    #[test]
    fn svg_into_slint_logo_extracts_accent_color() {
        let colored_svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><rect width="24" height="24" fill="#2496ED"/></svg>"##;
        let (img, accent) = DecodedLogo::Svg(colored_svg.to_vec())
            .into_slint_logo()
            .expect("colored svg should load");
        assert_eq!(img.size().width, 24);
        assert_eq!(accent, Some([36, 150, 237]));

        let mono_svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><rect width="24" height="24" fill="#181818"/></svg>"##;
        let (_, mono_accent) = DecodedLogo::Svg(mono_svg.to_vec())
            .into_slint_logo()
            .expect("mono svg should load");
        assert_eq!(mono_accent, None);
    }

    #[test]
    fn os_logo_key_and_url_maps_known_distros_and_namespaces_keys() {
        assert_eq!(
            os_logo_key_and_url("debian"),
            Some((
                "os-debian".to_string(),
                "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/debian.webp"
            ))
        );
        assert_eq!(
            os_logo_key_and_url("arch"),
            Some((
                "os-arch".to_string(),
                "https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/arch-linux.webp"
            ))
        );
        assert_eq!(os_logo_key_and_url("generic"), None);
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
