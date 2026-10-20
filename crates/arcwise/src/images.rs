//! Decodes the library's images off the UI thread and keeps the recently used ones.
//!
//! GPUI's own `img()` keeps every image it has loaded for the life of the app; browsing a few
//! hundred lineups would hold gigabytes of stills. This store forgets the least recently used
//! images past a budget and frees their GPU memory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use gpui::{App, AsyncApp, Context, RenderImage, WeakEntity, prelude::*};

/// Decoded pixels kept in memory, in bytes.
const BUDGET: usize = 512 << 20;
/// Map SVGs are drawn this many pixels across.
const SVG_SIZE: u32 = 2048;

#[derive(Clone)]
pub enum Image {
    Loading,
    Ready(Arc<RenderImage>),
    /// The file is missing or could not be decoded.
    Missing,
}

impl Image {
    pub fn ready(&self) -> Option<Arc<RenderImage>> {
        match self {
            Image::Ready(image) => Some(image.clone()),
            _ => None,
        }
    }
}

struct Entry {
    image: Image,
    bytes: usize,
    used: u64,
}

/// How an image is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Look {
    Plain,
    /// Black wherever the image is opaque: an icon to draw over a bright fill.
    Silhouette,
}

#[derive(Default)]
pub struct Images {
    /// One map per look, so a lookup by `&Path` needs no allocation.
    plain: HashMap<PathBuf, Entry>,
    silhouette: HashMap<PathBuf, Entry>,
    bytes: usize,
    clock: u64,
    /// Whether an image asked for with [`Images::get`] since the last [`Images::begin`] is still
    /// loading.
    waiting: bool,
}

impl Images {
    fn entries(&mut self, look: Look) -> &mut HashMap<PathBuf, Entry> {
        match look {
            Look::Plain => &mut self.plain,
            Look::Silhouette => &mut self.silhouette,
        }
    }

    /// The image at `path`, starting to load it if it is not in memory.
    pub fn get(&mut self, path: &Path, cx: &mut Context<Self>) -> Image {
        self.get_as(path, Look::Plain, cx)
    }

    /// Starts drawing a frame: forgets what the last one waited for.
    pub fn begin(&mut self) {
        self.waiting = false;
    }

    /// Whether the frame drawn since [`Images::begin`] shows an image still loading.
    pub fn waiting(&self) -> bool {
        self.waiting
    }

    /// Starts loading the image at `path` for later, without counting it as on screen.
    pub fn fetch(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.load(path, Look::Plain, cx);
    }

    pub fn get_as(&mut self, path: &Path, look: Look, cx: &mut Context<Self>) -> Image {
        let image = self.load(path, look, cx);
        self.waiting |= matches!(image, Image::Loading);
        image
    }

    fn load(&mut self, path: &Path, look: Look, cx: &mut Context<Self>) -> Image {
        self.clock += 1;
        let used = self.clock;
        if let Some(entry) = self.entries(look).get_mut(path) {
            entry.used = used;
            return entry.image.clone();
        }
        self.entries(look).insert(
            path.to_owned(),
            Entry {
                image: Image::Loading,
                bytes: 0,
                used,
            },
        );
        let path = path.to_owned();
        let task = cx.background_spawn({
            let path = path.clone();
            async move { decode(&path, look) }
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let decoded = task.await;
            this.update(cx, |this, cx| this.finish(path, look, decoded, cx))
                .ok();
        })
        .detach();
        Image::Loading
    }

    fn finish(
        &mut self,
        path: PathBuf,
        look: Look,
        decoded: anyhow::Result<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.entries(look).get_mut(&path) else {
            return;
        };
        match decoded {
            Ok(image) => {
                let bytes = image.as_bytes(0).map_or(0, <[u8]>::len);
                entry.bytes = bytes;
                entry.image = Image::Ready(image);
                self.bytes += bytes;
            }
            Err(e) => {
                if path.exists() {
                    eprintln!("arcwise: {e:#}");
                }
                entry.image = Image::Missing;
            }
        }
        self.evict(cx);
        cx.notify();
    }

    /// Forgets the least recently used images until the rest fit the budget. Runs only when a
    /// decode finishes, outside any window's update, so `drop_image` reaches every window.
    fn evict(&mut self, cx: &mut App) {
        while self.bytes > BUDGET {
            let oldest = [Look::Plain, Look::Silhouette]
                .into_iter()
                .flat_map(|look| {
                    let entries = match look {
                        Look::Plain => &self.plain,
                        Look::Silhouette => &self.silhouette,
                    };
                    entries
                        .iter()
                        .filter(|(_, e)| e.bytes > 0)
                        .map(move |(path, e)| (e.used, look, path.clone()))
                })
                .min_by_key(|(used, ..)| *used);
            let Some((_, look, path)) = oldest else { break };
            if let Some(entry) = self.entries(look).remove(&path) {
                self.bytes -= entry.bytes;
                if let Image::Ready(image) = entry.image {
                    cx.drop_image(image, None);
                }
            }
        }
    }

    /// Frees every image, before the store is dropped. Call outside any window's update.
    pub fn clear(&mut self, cx: &mut App) {
        for (_, entry) in self.plain.drain().chain(self.silhouette.drain()) {
            if let Image::Ready(image) = entry.image {
                cx.drop_image(image, None);
            }
        }
        self.bytes = 0;
    }
}

fn decode(path: &Path, look: Look) -> anyhow::Result<Arc<RenderImage>> {
    let start = std::time::Instant::now();
    let image = decode_now(path, look);
    crate::trace::task(|| format!("decode {}", path.display()), start);
    image
}

fn decode_now(path: &Path, look: Look) -> anyhow::Result<Arc<RenderImage>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let is = |ext: &str| {
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(ext))
    };
    // RenderImage holds BGRA, whatever the buffer's type says; libwebp writes it directly.
    let (mut pixels, bgra) = if is("svg") {
        let svg = render_svg(&bytes).with_context(|| format!("drawing {}", path.display()))?;
        (svg, false)
    } else if is("webp") {
        let webp = decode_webp(&bytes).with_context(|| format!("decoding {}", path.display()))?;
        (webp, true)
    } else {
        let image = image::load_from_memory(&bytes)
            .with_context(|| format!("decoding {}", path.display()))?
            .into_rgba8();
        (image, false)
    };
    for px in pixels.pixels_mut() {
        match look {
            Look::Plain if !bgra => px.0.swap(0, 2),
            Look::Plain => {}
            Look::Silhouette => px.0[..3].fill(0),
        }
    }
    Ok(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
}

/// Decodes a WebP picture to BGRA with libwebp, three times faster than the `image` crate's
/// decoder for the library's stills.
fn decode_webp(bytes: &[u8]) -> anyhow::Result<image::RgbaImage> {
    let (mut width, mut height) = (0, 0);
    // SAFETY: libwebp reads at most `bytes.len()` bytes and writes only the two sizes.
    let known =
        unsafe { libwebp_sys::WebPGetInfo(bytes.as_ptr(), bytes.len(), &mut width, &mut height) };
    anyhow::ensure!(known != 0, "not a WebP picture");
    let (width, height) = (u32::try_from(width)?, u32::try_from(height)?);
    let stride = width as usize * 4;
    let mut bgra = vec![0u8; stride * height as usize];
    // SAFETY: `bgra` holds `stride` bytes for each of `height` rows, which bounds what libwebp
    // writes; it reads at most `bytes.len()` bytes.
    let decoded = unsafe {
        libwebp_sys::WebPDecodeBGRAInto(
            bytes.as_ptr(),
            bytes.len(),
            bgra.as_mut_ptr(),
            bgra.len(),
            i32::try_from(stride)?,
        )
    };
    anyhow::ensure!(!decoded.is_null(), "the WebP data is damaged");
    image::RgbaImage::from_raw(width, height, bgra).context("WebP pixels do not fit its size")
}

fn render_svg(bytes: &[u8]) -> anyhow::Result<image::RgbaImage> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())?;
    let size = tree.size();
    let scale = SVG_SIZE as f32 / size.width().max(size.height());
    let (w, h) = (
        (size.width() * scale).round() as u32,
        (size.height() * scale).round() as u32,
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).context("SVG has no size")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied alpha; GPUI expects straight alpha.
    let mut pixels = pixmap.take();
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = (u16::from(*c) * 255 / u16::from(a)) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(w, h, pixels).context("SVG pixels do not fit its size")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_an_svg_at_the_map_size() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <rect x="0" y="0" width="50" height="100" fill="#ff0000"/>
            <rect x="50" y="0" width="50" height="100" fill="#0000ff" fill-opacity="0.5"/>
        </svg>"##;
        let image = render_svg(svg).unwrap();
        assert_eq!(image.dimensions(), (SVG_SIZE, SVG_SIZE));
        assert_eq!(image.get_pixel(10, 10).0, [255, 0, 0, 255]);
        let half = image.get_pixel(SVG_SIZE - 10, 10).0;
        assert_eq!(half[3], 128);
        assert!(
            half[2] >= 254,
            "straight alpha, not premultiplied: {half:?}"
        );
    }

    #[test]
    fn decodes_to_bgra() {
        let dir = std::env::temp_dir().join(format!("arcwise-images-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("red.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();
        let image = decode(&path, Look::Plain).unwrap();
        assert_eq!(
            &decode(&path, Look::Silhouette)
                .unwrap()
                .as_bytes(0)
                .unwrap()[..4],
            &[0, 0, 0, 255]
        );
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn decodes_webp_to_bgra() {
        let dir = std::env::temp_dir().join(format!("arcwise-webp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("red.webp");
        let rgba = [255u8, 0, 0, 255].repeat(6);
        let mut out = std::ptr::null_mut();
        // SAFETY: `rgba` holds 3x2 pixels at a stride of 12 bytes; the encoded bytes are copied
        // out before libwebp's buffer is freed.
        let encoded = unsafe {
            let len = libwebp_sys::WebPEncodeLosslessRGBA(rgba.as_ptr(), 3, 2, 12, &mut out);
            let encoded = std::slice::from_raw_parts(out, len).to_vec();
            libwebp_sys::WebPFree(out.cast());
            encoded
        };
        std::fs::write(&path, encoded).unwrap();
        let image = decode(&path, Look::Plain).unwrap();
        assert_eq!((image.size(0).width.0, image.size(0).height.0), (3, 2));
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        let silhouette = decode(&path, Look::Silhouette).unwrap();
        assert_eq!(&silhouette.as_bytes(0).unwrap()[..4], &[0, 0, 0, 255]);
        std::fs::write(&path, b"RIFF....WEBPVP8 broken").unwrap();
        assert!(decode(&path, Look::Plain).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(decode(Path::new("no/such/image.webp"), Look::Plain).is_err());
    }
}
