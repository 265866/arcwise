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

/// Decoded pixels kept in memory, in bytes: room for both sides of a map and the pictures of
/// the landings on and around the selected one, about 90 MB at most. The GPU holds a second
/// copy of those drawn.
const BUDGET: usize = 128 << 20;
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

/// A picture decoded off the UI thread, ready to go into [`Images`].
pub type Picture = (PathBuf, Look, anyhow::Result<Arc<RenderImage>>);

/// Decodes pictures on a few threads at once.
pub fn decode_all(wanted: Vec<(PathBuf, Look)>) -> Vec<Picture> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
    let chunk = wanted.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = wanted
            .chunks(chunk)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(path, look)| (path.clone(), *look, decode(path, *look)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("a decode thread panicked"))
            .collect()
    })
}

/// How an image is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Look {
    Plain,
    /// Black wherever the image is opaque: an icon to draw over a bright fill.
    Silhouette,
    /// Shrunk by a whole factor while it keeps at least this many rows, for a picture shown
    /// smaller than it is drawn.
    Fit(u32),
    /// An SVG drawn this many pixels across.
    Across(u32),
    /// A window `width` by `height` pixels onto an SVG drawn `across` pixels across, centered on
    /// the point `x`, `y` hundredths of a percent of its width and height from its top left.
    Window {
        across: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

#[derive(Default)]
pub struct Images {
    /// One map per look, so a lookup by `&Path` needs no allocation.
    entries: HashMap<Look, HashMap<PathBuf, Entry>>,
    bytes: usize,
    clock: u64,
    /// Whether an image asked for with [`Images::get`] since the last [`Images::begin`] is still
    /// loading.
    waiting: bool,
}

impl Images {
    fn entries(&mut self, look: Look) -> &mut HashMap<PathBuf, Entry> {
        self.entries.entry(look).or_default()
    }

    /// The image at `path`, starting to load it if it is not in memory.
    pub fn get(&mut self, path: &Path, cx: &mut Context<Self>) -> Image {
        self.get_as(path, Look::Plain, cx)
    }

    /// Takes in pictures decoded ahead, such as at launch.
    pub fn seed(&mut self, pictures: Vec<Picture>) {
        for (path, look, decoded) in pictures {
            self.clock += 1;
            let (image, bytes) = match decoded {
                Ok(image) => {
                    let bytes = image.as_bytes(0).map_or(0, <[u8]>::len);
                    (Image::Ready(image), bytes)
                }
                Err(e) => {
                    if path.exists() {
                        eprintln!("arcwise: {e:#}");
                    }
                    (Image::Missing, 0)
                }
            };
            self.bytes += bytes;
            let used = self.clock;
            self.entries(look)
                .insert(path, Entry { image, bytes, used });
        }
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
    pub fn fetch(&mut self, path: &Path, look: Look, cx: &mut Context<Self>) {
        self.load(path, look, cx);
    }

    /// The image at `path`, drawn as `look`, if it is in memory, without loading it.
    pub fn peek(&self, path: &Path, look: Look) -> Option<Arc<RenderImage>> {
        self.entries.get(&look)?.get(path)?.image.ready()
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
            let oldest = self
                .entries
                .iter()
                .flat_map(|(&look, entries)| {
                    entries
                        .iter()
                        .filter(|(_, e)| e.bytes > 0)
                        .map(move |(path, e)| (e.used, look, path))
                })
                .min_by_key(|(used, ..)| *used)
                .map(|(_, look, path)| (look, path.clone()));
            let Some((look, path)) = oldest else { break };
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
        for (_, entry) in self.entries.drain().flat_map(|(_, entries)| entries) {
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
        let svg =
            render_svg(&bytes, look).with_context(|| format!("drawing {}", path.display()))?;
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
            Look::Silhouette => px.0[..3].fill(0),
            _ if !bgra => px.0.swap(0, 2),
            _ => {}
        }
    }
    if let Look::Fit(rows) = look {
        let factor = (pixels.height() / rows.max(1)).max(1);
        pixels = shrink(pixels, factor);
    }
    Ok(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
}

/// Shrinks a picture by a whole factor, each pixel the average of a `factor`-wide square.
fn shrink(image: image::RgbaImage, factor: u32) -> image::RgbaImage {
    if factor <= 1 {
        return image;
    }
    let (width, height) = (image.width() / factor, image.height() / factor);
    let stride = image.width() as usize * 4;
    let src = image.as_raw();
    let area = factor * factor;
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut sum = [0u32; 4];
            for row in 0..factor as usize {
                let start = (y * factor as usize + row) * stride + x * factor as usize * 4;
                for px in src[start..start + factor as usize * 4].as_chunks::<4>().0 {
                    for (total, &channel) in sum.iter_mut().zip(px) {
                        *total += u32::from(channel);
                    }
                }
            }
            out.extend(sum.map(|total| ((total + area / 2) / area) as u8));
        }
    }
    image::RgbaImage::from_raw(width, height, out).expect("the buffer fits the size")
}

/// Has libwebp fill in its tables of decoding functions, once, before any decode. It fills them
/// on first use, without a lock on Windows, and pictures decoded at once on several threads at
/// launch could catch a table half filled and loop forever. Decoding a tiny picture of each kind
/// here, on one thread, fills them all.
fn set_up_libwebp() {
    static SET_UP: std::sync::Once = std::sync::Once::new();
    SET_UP.call_once(|| {
        // Lossy, lossy with alpha (whose alpha is lossless), and lossless.
        for (lossless, alpha) in [(false, 255u8), (false, 128), (true, 255)] {
            let rgba = [200u8, 100, 50, alpha].repeat(4);
            let mut encoded = std::ptr::null_mut();
            let mut bgra = [0u8; 16];
            // SAFETY: `rgba` holds 2x2 pixels at a stride of 8 bytes, and `bgra` room for as
            // many; libwebp reads at most the `len` bytes it encoded, then frees them.
            unsafe {
                let len = if lossless {
                    libwebp_sys::WebPEncodeLosslessRGBA(rgba.as_ptr(), 2, 2, 8, &mut encoded)
                } else {
                    libwebp_sys::WebPEncodeRGBA(rgba.as_ptr(), 2, 2, 8, 90., &mut encoded)
                };
                if len > 0 {
                    libwebp_sys::WebPDecodeBGRAInto(encoded, len, bgra.as_mut_ptr(), 16, 8);
                }
                libwebp_sys::WebPFree(encoded.cast());
            }
        }
    });
}

/// Decodes a WebP picture to BGRA with libwebp, three times faster than the `image` crate's
/// decoder for the library's stills.
fn decode_webp(bytes: &[u8]) -> anyhow::Result<image::RgbaImage> {
    set_up_libwebp();
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

fn render_svg(bytes: &[u8], look: Look) -> anyhow::Result<image::RgbaImage> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())?;
    let size = tree.size();
    let (across, window) = match look {
        Look::Across(across) => (across, None),
        Look::Window {
            across,
            x,
            y,
            width,
            height,
        } => (across, Some((x, y, width, height))),
        Look::Plain | Look::Silhouette | Look::Fit(_) => (SVG_SIZE, None),
    };
    let scale = across as f32 / size.width().max(size.height());
    let (w, h, dx, dy) = match window {
        None => (
            (size.width() * scale).round() as u32,
            (size.height() * scale).round() as u32,
            0.,
            0.,
        ),
        // The point moves to the window's center.
        Some((x, y, width, height)) => (
            width,
            height,
            width as f32 / 2. - size.width() * scale * x as f32 / 10_000.,
            height as f32 / 2. - size.height() * scale * y as f32 / 10_000.,
        ),
    };
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).context("SVG has no size")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy),
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
        let image = render_svg(svg, Look::Plain).unwrap();
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
    fn draws_an_svg_across_a_size_or_a_window_onto_it() {
        // Red left half, blue right half.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <rect x="0" y="0" width="50" height="100" fill="#ff0000"/>
            <rect x="50" y="0" width="50" height="100" fill="#0000ff"/>
        </svg>"##;
        assert_eq!(
            render_svg(svg, Look::Across(300)).unwrap().dimensions(),
            (300, 300)
        );
        // Drawn 1000 across, a 40x20 window centered at (49%, 50%): the halves meet at x = 30.
        let window = Look::Window {
            across: 1000,
            x: 4900,
            y: 5000,
            width: 40,
            height: 20,
        };
        let image = render_svg(svg, window).unwrap();
        assert_eq!(image.dimensions(), (40, 20));
        assert_eq!(image.get_pixel(25, 10).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(35, 10).0, [0, 0, 255, 255]);
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
    fn shrinks_by_averaging_whole_squares() {
        let mut image = image::RgbaImage::new(4, 2);
        for (x, _, px) in image.enumerate_pixels_mut() {
            px.0 = if x == 0 {
                [200, 100, 0, 255]
            } else {
                [0, 0, 0, 255]
            };
        }
        let half = shrink(image.clone(), 2);
        assert_eq!(half.dimensions(), (2, 1));
        assert_eq!(half.get_pixel(0, 0).0, [100, 50, 0, 255]);
        assert_eq!(half.get_pixel(1, 0).0, [0, 0, 0, 255]);
        assert_eq!(shrink(image, 1).dimensions(), (4, 2));
    }

    #[test]
    fn fits_a_picture_to_at_least_the_rows_asked_for() {
        let dir = std::env::temp_dir().join(format!("arcwise-fit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grey.png");
        image::RgbaImage::from_pixel(8, 6, image::Rgba([90, 90, 90, 255]))
            .save(&path)
            .unwrap();
        let size = |look| {
            let image = decode(&path, look).unwrap();
            let size = image.size(0);
            (size.width.0, size.height.0)
        };
        assert_eq!(size(Look::Fit(3)), (4, 3));
        assert_eq!(
            size(Look::Fit(2)),
            (2, 2),
            "a third, the most that keeps two rows"
        );
        assert_eq!(
            size(Look::Fit(4)),
            (8, 6),
            "never fewer rows than asked for"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(decode(Path::new("no/such/image.webp"), Look::Plain).is_err());
    }
}
