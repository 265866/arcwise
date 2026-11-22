//! Decodes the library's images off the UI thread and keeps the recently used ones.
//!
//! GPUI's own `img()` keeps every image it has loaded for the life of the app; browsing a few
//! hundred lineups would hold gigabytes of stills. This store forgets the least recently used
//! images past a budget and frees their GPU memory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use anyhow::Context as _;
use arcwise_remote::remote;
use gpui::{App, AsyncApp, Context, RenderImage, WeakEntity};
use image::{Frame, RgbaImage};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};

use crate::trace;

/// Decoded pixels kept in memory, in bytes, for a viewer [`BUDGET_ROWS`] rows high: room for
/// both sides of a map and the pictures of the landings on and around the selected one, about
/// 90 MB at most. Bigger viewers show bigger pictures, and get more; see [`Images::fit_rows`].
/// The GPU holds a second copy of those drawn.
const BUDGET: usize = 128 << 20;
const BUDGET_ROWS: u32 = 450;
/// Map SVGs are drawn this many pixels across.
const SVG_SIZE: u32 = 2048;

/// An image in the store.
#[derive(Clone, Debug)]
pub(crate) enum Image {
    Loading,
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "GPUI draws and frees a decoded picture through an Arc<RenderImage>"
        )
    )]
    Ready(Arc<RenderImage>),
    /// The file is missing or could not be decoded.
    Missing,
}

impl Image {
    /// The picture, once it is decoded.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "GPUI draws and frees a decoded picture through an Arc<RenderImage>"
        )
    )]
    #[must_use]
    pub(crate) fn ready(&self) -> Option<Arc<RenderImage>> {
        match self {
            Image::Ready(image) => Some(image.clone()),
            Image::Loading | Image::Missing => None,
        }
    }
}

#[derive(Debug)]
struct Entry {
    image: Image,
    bytes: usize,
    used: u64,
}

/// A picture decoded off the UI thread, ready to go into [`Images`].
#[derive(Debug)]
pub(crate) struct Picture {
    pub(crate) path: PathBuf,
    pub(crate) look: Look,
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "GPUI draws and frees a decoded picture through an Arc<RenderImage>"
        )
    )]
    pub(crate) decoded: anyhow::Result<Arc<RenderImage>>,
}

/// A picture to decode ahead with [`decode_all`]: its file, and how it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Wanted {
    pub(crate) path: PathBuf,
    pub(crate) look: Look,
}

/// Decodes pictures on a few threads at once, at most four.
///
/// # Panics
///
/// When a decode panics, which is a bug.
pub(crate) fn decode_all(wanted: &[Wanted]) -> Vec<Picture> {
    let threads = thread::available_parallelism().map_or(1, |n| n.get().min(4));
    let chunk = wanted.len().div_ceil(threads).max(1);
    thread::scope(|scope| {
        let workers: Vec<_> = wanted
            .chunks(chunk)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|Wanted { path, look }| Picture {
                            path: path.clone(),
                            look: *look,
                            decoded: decode(path, *look),
                        })
                        .collect::<Vec<Picture>>()
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
pub(crate) enum Look {
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

/// The images in memory, by how they are drawn and where their files are.
#[derive(Debug, Default)]
pub(crate) struct Images {
    /// One map per look, so a lookup by `&Path` needs no allocation.
    entries: HashMap<Look, HashMap<PathBuf, Entry>>,
    bytes: usize,
    clock: u64,
    /// Whether an image asked for with [`Images::get`] since the last [`Images::begin`] is still
    /// loading.
    waiting: bool,
    /// Bytes kept; [`BUDGET`] until [`Images::fit_rows`] says otherwise.
    budget: Option<usize>,
}

impl Images {
    fn entries(&mut self, look: Look) -> &mut HashMap<PathBuf, Entry> {
        self.entries.entry(look).or_default()
    }

    /// The image at `path`, starting to load it if it is not in memory.
    pub(crate) fn get(&mut self, path: &Path, cx: &mut Context<'_, Self>) -> Image {
        self.get_as(path, Look::Plain, cx)
    }

    /// The image at `path`, drawn as `look`, starting to load it if it is not in memory. One
    /// still loading counts as on screen, so [`Images::waiting`] says so.
    pub(crate) fn get_as(&mut self, path: &Path, look: Look, cx: &mut Context<'_, Self>) -> Image {
        let image = self.load(path, look, cx);
        self.waiting |= matches!(image, Image::Loading);
        image
    }

    /// Starts loading the image at `path` for later, without counting it as on screen.
    pub(crate) fn fetch(&mut self, path: &Path, look: Look, cx: &mut Context<'_, Self>) {
        self.load(path, look, cx);
    }

    /// The entry for `path` drawn as `look`, marked as just used, or a new one loading off the
    /// UI thread.
    fn load(&mut self, path: &Path, look: Look, cx: &mut Context<'_, Self>) -> Image {
        use gpui::AppContext as _;

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
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| this.finish(&path, look, decoded, cx));
            }
        })
        .detach();
        Image::Loading
    }

    /// Takes in pictures decoded ahead, such as at launch.
    pub(crate) fn seed(&mut self, pictures: Vec<Picture>) {
        for Picture {
            path,
            look,
            decoded,
        } in pictures
        {
            self.clock += 1;
            let (image, bytes) = match decoded {
                Ok(image) => {
                    let bytes = image.as_bytes(0).map_or(0, <[u8]>::len);
                    (Image::Ready(image), bytes)
                }
                Err(e) => {
                    report(&path, &e);
                    (Image::Missing, 0)
                }
            };
            self.bytes += bytes;
            let used = self.clock;
            if let Some(replaced) = self
                .entries(look)
                .insert(path, Entry { image, bytes, used })
            {
                self.bytes -= replaced.bytes;
            }
        }
    }

    /// Starts drawing a frame: forgets what the last one waited for.
    pub(crate) fn begin(&mut self) {
        self.waiting = false;
    }

    /// Whether the frame drawn since [`Images::begin`] shows an image still loading.
    #[must_use]
    pub(crate) fn waiting(&self) -> bool {
        self.waiting
    }

    /// The image at `path`, drawn as `look`, if it is in memory, without loading it.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "GPUI draws and frees a decoded picture through an Arc<RenderImage>"
        )
    )]
    pub(crate) fn peek(&mut self, path: &Path, look: Look) -> Option<Arc<RenderImage>> {
        self.clock += 1;
        let entry = self.entries.get_mut(&look)?.get_mut(path)?;
        entry.used = self.clock;
        entry.image.ready()
    }

    /// Sizes the budget for a viewer `rows` high, by the area of what it shows: 128 MB at 450
    /// rows, up to four times that.
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the budget is 128 MB scaled by 1 to 4, a whole number of bytes well inside a usize once truncated"
    )]
    pub(crate) fn fit_rows(&mut self, rows: u32) {
        let scale = (f64::from(rows) / f64::from(BUDGET_ROWS))
            .powi(2)
            .clamp(1., 4.);
        self.budget = Some((BUDGET as f64 * scale) as usize);
    }

    /// Puts a decode's result in the entry waiting for it, if the store still wants it.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "GPUI draws and frees a decoded picture through an Arc<RenderImage>"
        )
    )]
    fn finish(
        &mut self,
        path: &Path,
        look: Look,
        decoded: anyhow::Result<Arc<RenderImage>>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(entry) = self.entries(look).get_mut(path) else {
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
                entry.image = Image::Missing;
                report(path, &e);
            }
        }
        self.evict(cx);
        cx.notify();
    }

    /// Forgets the least recently used images until the rest fit the budget. Runs only when a
    /// decode finishes, outside any window's update, so `drop_image` reaches every window.
    fn evict(&mut self, cx: &mut App) {
        while self.bytes > self.budget.unwrap_or(BUDGET) {
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
    pub(crate) fn clear(&mut self, cx: &mut App) {
        for (_, entries) in self.entries.drain() {
            for (_, entry) in entries {
                if let Image::Ready(image) = entry.image {
                    cx.drop_image(image, None);
                }
            }
        }
        self.bytes = 0;
    }
}

/// Reports a picture that could not be decoded, unless its file is simply not there. It only
/// leaves its place empty in the window, which says nothing of why.
fn report(path: &Path, error: &anyhow::Error) {
    if path.exists() {
        remote::report(format_args!("{error:#}"));
    }
}

/// Reads the file at `path`, from a library's folder or from the web into it, and decodes it as
/// `look` asks, timed in the trace.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        shared_ownership,
        reason = "GPUI draws and frees a decoded picture through an Arc<RenderImage>"
    )
)]
fn decode(path: &Path, look: Look) -> anyhow::Result<Arc<RenderImage>> {
    let start = Instant::now();
    let image = remote::read(path)
        .with_context(|| format!("reading {}", path.display()))
        .and_then(|bytes| decode_bytes(path, &bytes, look));
    trace::task(|| format!("decode {}", path.display()), start);
    image
}

/// Decodes the bytes of the file at `path`, by its extension: an SVG, a WebP picture, or any
/// other format the `image` crate reads. `RenderImage` holds BGRA, whatever its buffer's type
/// says, which libwebp writes directly.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        shared_ownership,
        reason = "GPUI draws and frees a decoded picture through an Arc<RenderImage>"
    )
)]
fn decode_bytes(path: &Path, bytes: &[u8], look: Look) -> anyhow::Result<Arc<RenderImage>> {
    let is = |ext: &str| {
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(ext))
    };
    let (mut pixels, bgra) = if is("svg") {
        let svg = render_svg(bytes, look).with_context(|| format!("drawing {}", path.display()))?;
        (svg, false)
    } else if is("webp") {
        let webp = decode_webp(bytes).with_context(|| format!("decoding {}", path.display()))?;
        (webp, true)
    } else {
        let image = image::load_from_memory(bytes)
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
    Ok(Arc::new(RenderImage::new([Frame::new(pixels)])))
}

/// Shrinks a picture by a whole factor, each pixel the average of a `factor`-wide square.
fn shrink(image: RgbaImage, factor: u32) -> RgbaImage {
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
            let mut sum = [0_u32; 4];
            for row in 0..factor as usize {
                let start = (y * factor as usize + row) * stride + x * factor as usize * 4;
                for px in src[start..start + factor as usize * 4].as_chunks::<4>().0 {
                    for (total, &channel) in sum.iter_mut().zip(px) {
                        *total += u32::from(channel);
                    }
                }
            }
            for total in sum {
                out.push(u8::try_from((total + area / 2) / area).unwrap_or(u8::MAX));
            }
        }
    }
    RgbaImage::from_raw(width, height, out).expect("the buffer fits the size")
}

/// Decodes a WebP picture to BGRA with libwebp, three times faster than the `image` crate's
/// decoder for the library's stills.
fn decode_webp(bytes: &[u8]) -> anyhow::Result<RgbaImage> {
    let bgra = arcwise_ffi::decode_bgra(bytes)?;
    RgbaImage::from_raw(bgra.width, bgra.height, bgra.pixels)
        .context("WebP pixels do not fit its size")
}

/// Draws an SVG as `look` asks, in straight RGBA.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "sizes and offsets here are a few thousand pixels at most, exact in an f32, and the \
              rounded sizes of a drawing with a positive size are positive"
)]
fn render_svg(bytes: &[u8], look: Look) -> anyhow::Result<RgbaImage> {
    let tree = Tree::from_data(bytes, &Options::default())?;
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
        Some((x, y, width, height)) => (
            width,
            height,
            width as f32 / 2. - size.width() * scale * x as f32 / 10_000.,
            height as f32 / 2. - size.height() * scale * y as f32 / 10_000.,
        ),
    };
    let mut pixmap = Pixmap::new(w, h).context("SVG has no size")?;
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale).post_translate(dx, dy),
        &mut pixmap.as_mut(),
    );
    let mut pixels = pixmap.take();
    straighten(&mut pixels);
    RgbaImage::from_raw(w, h, pixels).context("SVG pixels do not fit its size")
}

/// Turns tiny-skia's premultiplied alpha into the straight alpha GPUI expects.
fn straighten(pixels: &mut [u8]) {
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = u8::try_from(u16::from(*c) * 255 / u16::from(a)).unwrap_or(u8::MAX);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::{env, fs, process};

    use image::{ImageFormat, Rgba};

    use super::*;

    fn png(image: &RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

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
        let red_left_blue_right =
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <rect x="0" y="0" width="50" height="100" fill="#ff0000"/>
            <rect x="50" y="0" width="50" height="100" fill="#0000ff"/>
        </svg>"##;
        let svg = red_left_blue_right;
        assert_eq!(
            render_svg(svg, Look::Across(300)).unwrap().dimensions(),
            (300, 300)
        );
        let window = Look::Window {
            across: 1000,
            x: 4900,
            y: 5000,
            width: 40,
            height: 20,
        };
        let image = render_svg(svg, window).unwrap();
        assert_eq!(image.dimensions(), (40, 20));
        assert_eq!(
            image.get_pixel(25, 10).0,
            [255, 0, 0, 255],
            "drawn 1000 across, a window centered at (49%, 50%) has the halves meet at x = 30"
        );
        assert_eq!(image.get_pixel(35, 10).0, [0, 0, 255, 255]);
    }

    #[test]
    fn decodes_to_bgra() {
        let path = Path::new("red.png");
        let bytes = png(&RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255])));
        let image = decode_bytes(path, &bytes, Look::Plain).unwrap();
        assert_eq!(
            &decode_bytes(path, &bytes, Look::Silhouette)
                .unwrap()
                .as_bytes(0)
                .unwrap()[..4],
            &[0, 0, 0, 255]
        );
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn decodes_webp_to_bgra() {
        let path = Path::new("red.webp");
        let rgba = [255_u8, 0, 0, 255].repeat(6);
        let encoded = arcwise_ffi::encode_lossless_rgba(&rgba, 3, 2).unwrap();
        let image = decode_bytes(path, &encoded, Look::Plain).unwrap();
        assert_eq!((image.size(0).width.0, image.size(0).height.0), (3, 2));
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        let silhouette = decode_bytes(path, &encoded, Look::Silhouette).unwrap();
        assert_eq!(&silhouette.as_bytes(0).unwrap()[..4], &[0, 0, 0, 255]);
        assert!(decode_bytes(path, b"RIFF....WEBPVP8 broken", Look::Plain).is_err());
    }

    #[test]
    fn shrinks_by_averaging_whole_squares() {
        let mut image = RgbaImage::new(4, 2);
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
        let path = Path::new("grey.png");
        let bytes = png(&RgbaImage::from_pixel(8, 6, Rgba([90, 90, 90, 255])));
        let size = |look| {
            let image = decode_bytes(path, &bytes, look).unwrap();
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
    }

    #[test]
    fn a_picture_seeded_twice_counts_once() {
        let picture = || {
            let image = RgbaImage::new(4, 4);
            let image = Arc::new(RenderImage::new([Frame::new(image)]));
            Picture {
                path: PathBuf::from("same.webp"),
                look: Look::Plain,
                decoded: Ok(image),
            }
        };
        let mut images = Images::default();
        images.seed(vec![picture(), picture()]);
        assert_eq!(images.bytes, 4 * 4 * 4);
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(decode(Path::new("no/such/image.webp"), Look::Plain).is_err());
    }

    #[test]
    fn decodes_files_on_disk_on_several_threads() {
        let dir = env::temp_dir().join(format!("arcwise-images-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        let red = dir.join("red.png");
        fs::write(
            &red,
            png(&RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]))),
        )
        .unwrap();
        let missing = dir.join("missing.png");
        let wanted = vec![
            Wanted {
                path: red.clone(),
                look: Look::Plain,
            },
            Wanted {
                path: missing.clone(),
                look: Look::Plain,
            },
            Wanted {
                path: red.clone(),
                look: Look::Silhouette,
            },
        ];
        let pictures = decode_all(&wanted);
        fs::remove_dir_all(&dir).unwrap();
        let found: Vec<(&Path, Look, bool)> = pictures
            .iter()
            .map(|p| (p.path.as_path(), p.look, p.decoded.is_ok()))
            .collect();
        assert_eq!(
            found,
            [
                (red.as_path(), Look::Plain, true),
                (missing.as_path(), Look::Plain, false),
                (red.as_path(), Look::Silhouette, true),
            ],
            "every picture asked for comes back in order, a missing file as an error"
        );
        let silhouette = pictures[2].decoded.as_ref().unwrap();
        assert_eq!(&silhouette.as_bytes(0).unwrap()[..4], &[0, 0, 0, 255]);
    }
}
