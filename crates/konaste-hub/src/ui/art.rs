//! Game and service artwork.
//!
//! Service logos are bundled. Stage backgrounds live in the repository's
//! `assets/backgrounds` and are downloaded into the data folder; the bundled
//! SVGs are original fallbacks until one arrives. Game icons come from the
//! user's own installation.
use crate::catalog::GameId;
use gpui_kit::RenderImage;
use image::{
    ImageFormat, ImageReader, Limits, RgbaImage,
    imageops::{self, FilterType},
};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

/// Fallback SVG raster size.
pub const FALLBACK: (u32, u32) = (1600, 900);
/// Icons are shown at 44-48 px; twice that stays sharp on high-DPI screens.
/// The GPU sampler has no mipmaps, so larger sources would alias.
pub const ICON_PX: u32 = 96;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const REFRESH: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const BACKGROUNDS: &str = "https://raw.githubusercontent.com/Momoko-Ayase/Konaste-Hub/main/assets/backgrounds/";

/// Original e-amusement pass card face: a wide strip, anchored right, that
/// fits a card sized by its content. Drawn at twice its display size.
pub const PASS_CARD: &[u8] = include_bytes!("../../assets/pass-card.svg");
pub const PASS_CARD_PX: (i32, i32) = (700, 200);

pub fn fallback_svg(id: GameId) -> &'static [u8] {
    match id {
        GameId::BomberGirl => include_bytes!("../../assets/art/bomber-girl.svg"),
        GameId::Sdvx => include_bytes!("../../assets/art/sdvx.svg"),
        GameId::Ddr => include_bytes!("../../assets/art/ddr.svg"),
        GameId::Popn => include_bytes!("../../assets/art/popn.svg"),
        GameId::Gitadora => include_bytes!("../../assets/art/gitadora.svg"),
        GameId::Nostalgia => include_bytes!("../../assets/art/nostalgia.svg"),
        GameId::Infinitas => include_bytes!("../../assets/art/infinitas.svg"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    Paseli,
    EAmusement,
}

impl Service {
    pub const ALL: [Service; 2] = [Service::Paseli, Service::EAmusement];

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Paseli => include_bytes!("../../assets/services/paseli.png"),
            Self::EAmusement => include_bytes!("../../assets/services/e-amusement.png"),
        }
    }

    /// Stored height: twice the largest display height.
    fn height(self) -> u32 {
        match self {
            Self::Paseli => 48,
            Self::EAmusement => 32,
        }
    }
}

fn file_name(id: GameId) -> &'static str {
    match id {
        GameId::BomberGirl => "bomber-girl",
        GameId::Sdvx => "sdvx",
        GameId::Ddr => "ddr",
        GameId::Popn => "popn",
        GameId::Gitadora => "gitadora",
        GameId::Nostalgia => "nostalgia",
        GameId::Infinitas => "infinitas",
    }
}

pub fn background_file(directory: &Path, id: GameId) -> PathBuf {
    directory.join("backgrounds").join(format!("{}.jpg", file_name(id)))
}

pub fn background_url(id: GameId) -> String {
    format!("{BACKGROUNDS}{}.jpg", file_name(id))
}

/// The cached stage background, if one was downloaded before.
pub fn cached_background(directory: &Path, id: GameId) -> Option<RgbaImage> {
    let mut file = File::open(background_file(directory, id)).ok()?.take((MAX_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    decode_background(&bytes)
}

/// Stage backgrounds carry their own title logo. The renderer covers the stage
/// directly with the decoded frame, preserving the supplied composition.
pub fn decode_background(bytes: &[u8]) -> Option<RgbaImage> {
    if !plausible(bytes) {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Jpeg);
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    reader.limits(limits);
    let mut image = reader.decode().ok()?.into_rgba8();
    if image.width() < 64 || image.height() < 64 {
        return None;
    }
    flatten(&mut image, [13, 14, 22]);
    Some(image)
}

/// Download `url` into `cache` when the cached copy is missing or older than a
/// week. Returns the bytes only when they changed, so the caller redraws only
/// then. A failed download keeps the cached copy.
pub async fn refresh(client: &reqwest::Client, cache: &Path, url: &str) -> Option<Vec<u8>> {
    let fresh = std::fs::metadata(cache)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .is_some_and(|age| age < REFRESH);
    if fresh {
        return None;
    }
    let bytes = download(client, url).await?;
    let changed = std::fs::read(cache).ok().is_none_or(|old| old != bytes);
    store(cache, &bytes);
    changed.then_some(bytes)
}

/// Reads at most `MAX_BYTES`, whatever the server claims.
async fn download(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let mut response = client.get(url).timeout(Duration::from_secs(60)).send().await.ok()?;
    if !response.status().is_success() || response.content_length().is_some_and(|length| length > MAX_BYTES as u64) {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    plausible(&bytes).then_some(bytes)
}

fn store(path: &Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let temporary = path.with_extension("tmp");
    if std::fs::write(&temporary, bytes).is_err() || crate::platform::replace_file(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
}

/// Forget a cached file that did not decode, so the next start downloads it again.
pub fn discard(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn plausible(bytes: &[u8]) -> bool {
    bytes.len() <= MAX_BYTES && bytes.starts_with(&[0xFF, 0xD8])
}

/// Decode a bundled service logo, scaled to its display height.
pub fn service_logo(service: Service) -> Option<RgbaImage> {
    let logo = image::load_from_memory_with_format(service.bytes(), ImageFormat::Png).ok()?.into_rgba8();
    if logo.width() == 0 || logo.height() == 0 {
        return None;
    }
    let height = service.height().min(logo.height());
    let width = ((logo.width() as u64 * height as u64) / logo.height() as u64).max(1) as u32;
    Some(imageops::resize(&logo, width, height, FilterType::Lanczos3))
}

/// gpui draws BGRA with straight alpha.
pub fn render_image(mut rgba: RgbaImage) -> Arc<RenderImage> {
    for px in rgba.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    Arc::new(RenderImage::new([image::Frame::new(rgba)]))
}

fn flatten(art: &mut RgbaImage, backing: [u8; 3]) {
    for px in art.pixels_mut() {
        if px[3] == 255 {
            continue;
        }
        let a = px[3] as f32 / 255.;
        for c in 0..3 {
            px[c] = (px[c] as f32 * a + backing[c] as f32 * (1. - a)).round() as u8;
        }
        px[3] = 255;
    }
}

/// The official icon of an installation: the launcher's, else the game module's.
pub fn install_icon(root: &Path, id: GameId) -> Option<RgbaImage> {
    let mut candidates = vec![root.join("launcher").join("modules").join("launcher.exe")];
    if let Some(module) = id.title().map(|title| title.module_path) {
        candidates.push(root.join(module));
    }
    candidates.into_iter().find_map(|exe| {
        let ico = exe_icon(&exe)?;
        let icon = image::load_from_memory_with_format(&ico, image::ImageFormat::Ico).ok()?.into_rgba8();
        (icon.width() >= 16).then(|| imageops::resize(&icon, ICON_PX, ICON_PX, FilterType::Lanczos3))
    })
}

/// Pick the largest, deepest image in a `GRPICONDIR` and return its resource id
/// and directory fields.
fn pick_icon(group: &[u8]) -> Option<([u8; 12], u16)> {
    let count = u16::from_le_bytes([*group.get(4)?, *group.get(5)?]) as usize;
    (0..count)
        .filter_map(|i| {
            let entry = group.get(6 + i * 14..20 + i * 14)?;
            let width = if entry[0] == 0 { 256 } else { entry[0] as u32 };
            let bits = u16::from_le_bytes([entry[6], entry[7]]);
            let id = u16::from_le_bytes([entry[12], entry[13]]);
            let mut fields = [0u8; 12];
            fields.copy_from_slice(&entry[..12]);
            Some(((width, bits), fields, id))
        })
        .max_by_key(|(rank, _, _)| *rank)
        .map(|(_, fields, id)| (fields, id))
}

/// A one-image `.ico` file around a raw `RT_ICON` resource.
fn single_ico(fields: [u8; 12], data: &[u8]) -> Vec<u8> {
    let mut ico = Vec::with_capacity(22 + data.len());
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    // width, height, colors, reserved, planes, bit count
    ico.extend_from_slice(&fields[..8]);
    ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(data);
    ico
}

#[cfg(windows)]
fn exe_icon(exe: &Path) -> Option<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{
            EnumResourceNamesW, FindResourceW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW, LoadResource,
            LockResource, SizeofResource,
        },
    };
    const RT_ICON: *const u16 = 3 as _;
    const RT_GROUP_ICON: *const u16 = 14 as _;

    unsafe fn resource(module: HMODULE, name: *const u16, kind: *const u16) -> Option<Vec<u8>> {
        unsafe {
            let info = FindResourceW(module, name, kind);
            if info.is_null() {
                return None;
            }
            let size = SizeofResource(module, info) as usize;
            let handle = LoadResource(module, info);
            let data = LockResource(handle) as *const u8;
            (!data.is_null() && size > 0 && size <= MAX_BYTES).then(|| std::slice::from_raw_parts(data, size).to_vec())
        }
    }

    unsafe extern "system" fn first_group(module: HMODULE, _: *const u16, name: *const u16, out: isize) -> windows_sys::core::BOOL {
        unsafe {
            let out = &mut *(out as *mut Option<Vec<u8>>);
            let found = resource(module, name, RT_GROUP_ICON).and_then(|group| {
                let (fields, id) = pick_icon(&group)?;
                let data = resource(module, id as usize as *const u16, RT_ICON)?;
                Some(single_ico(fields, &data))
            });
            let done = found.is_some();
            *out = found;
            // Stop at the first group that yields an image.
            (!done) as windows_sys::core::BOOL
        }
    }

    let text = exe.as_os_str().to_string_lossy();
    let plain = text.strip_prefix(r"\\?\").filter(|rest| rest.len() < 260).unwrap_or(&text).to_owned();
    if !Path::new(&plain).is_file() {
        return None;
    }
    let wide: Vec<u16> = std::ffi::OsStr::new(&plain).encode_wide().chain(Some(0)).collect();
    // Mapped as data: nothing in the file runs.
    let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE) };
    if module.is_null() {
        return None;
    }
    let mut found: Option<Vec<u8>> = None;
    unsafe {
        EnumResourceNamesW(module, RT_GROUP_ICON, Some(first_group), &mut found as *mut _ as isize);
        FreeLibrary(module);
    }
    found
}

#[cfg(not(windows))]
fn exe_icon(_: &Path) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn cached_background_decodes_and_skips_invalid_files() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("backgrounds")).unwrap();
        std::fs::write(background_file(directory.path(), GameId::Ddr), b"not an image").unwrap();
        let art = RgbaImage::from_pixel(128, 80, Rgba([10, 200, 30, 255]));
        image::DynamicImage::ImageRgba8(art).into_rgb8().save(background_file(directory.path(), GameId::Sdvx)).unwrap();
        let loaded = cached_background(directory.path(), GameId::Sdvx).unwrap();
        assert_eq!(loaded.dimensions(), (128, 80));
        assert!(cached_background(directory.path(), GameId::Ddr).is_none());
        assert!(cached_background(directory.path(), GameId::Popn).is_none());
    }

    #[test]
    fn background_urls_point_at_the_repository() {
        assert_eq!(background_url(GameId::Popn), format!("{BACKGROUNDS}popn.jpg"));
        assert!(BACKGROUNDS.starts_with("https://raw.githubusercontent.com/"));
    }

    #[test]
    fn every_title_has_a_background_in_the_repository() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/backgrounds");
        for game in &crate::catalog::CATALOG {
            let bytes = std::fs::read(root.join(format!("{}.jpg", file_name(game.id)))).unwrap();
            assert!(decode_background(&bytes).is_some(), "{:?}", game.id);
        }
    }

    #[test]
    fn bundled_service_logos_decode() {
        for service in Service::ALL {
            let logo = service_logo(service).unwrap();
            assert_eq!(logo.height(), service.height());
        }
    }

    #[test]
    fn transparent_art_is_flattened_onto_its_backing() {
        let mut art = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        flatten(&mut art, [255, 240, 230]);
        assert_eq!(art.get_pixel(0, 0), &Rgba([255, 240, 230, 255]));
    }

    #[test]
    fn largest_deepest_icon_wins_and_wraps_as_a_valid_ico() {
        let mut group = vec![0, 0, 1, 0, 3, 0];
        for (width, bits, id) in [(32u8, 32u16, 1u16), (0, 24, 2), (0, 32, 3)] {
            group.extend_from_slice(&[width, width, 0, 0, 1, 0]);
            group.extend_from_slice(&bits.to_le_bytes());
            group.extend_from_slice(&1234u32.to_le_bytes());
            group.extend_from_slice(&id.to_le_bytes());
        }
        let (fields, id) = pick_icon(&group).unwrap();
        assert_eq!(id, 3);
        let png = {
            let mut bytes = Vec::new();
            RgbaImage::from_pixel(256, 256, Rgba([1, 2, 3, 255]))
                .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
                .unwrap();
            bytes
        };
        let ico = single_ico(fields, &png);
        let decoded = image::load_from_memory_with_format(&ico, image::ImageFormat::Ico).unwrap();
        assert_eq!(decoded.width(), 256);
        assert!(pick_icon(&[0, 0, 1, 0]).is_none());
    }

    #[test]
    fn only_bounded_jpeg_is_cached() {
        assert!(!plausible(&[0x89, b'P', b'N', b'G', 0]));
        assert!(plausible(&[0xFF, 0xD8, 0xFF]));
        assert!(!plausible(b"<html>"));
        assert!(!plausible(&vec![0xFF; MAX_BYTES + 1]));
    }

    #[test]
    fn every_title_has_a_fallback() {
        for game in &crate::catalog::CATALOG {
            assert!(!fallback_svg(game.id).is_empty(), "{:?}", game.id);
        }
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "needs KONASTE_GITADORA_ROOT: an official GITADORA installation"]
    fn reads_the_official_launcher_icon() {
        let icon = install_icon(Path::new(&std::env::var("KONASTE_GITADORA_ROOT").unwrap()), GameId::Gitadora).unwrap();
        assert_eq!(icon.dimensions(), (ICON_PX, ICON_PX));
    }
}
