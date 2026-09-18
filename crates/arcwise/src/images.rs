//! Decodes the library's images off the UI thread and keeps the recently used ones.
//!
//! GPUI's own `img()` keeps every image it has loaded for the life of the app; browsing a few
//! hundred lineups would hold gigabytes of stills. This store forgets the least recently used
//! images past a budget and hands them back to the window to free their GPU memory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use gpui::{AsyncApp, Context, RenderImage, WeakEntity, Window, prelude::*};

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
    entries: HashMap<(PathBuf, Look), Entry>,
    bytes: usize,
    clock: u64,
    /// Evicted images, freed from the window's atlas on its next frame.
    retired: Vec<Arc<RenderImage>>,
}

impl Images {
    /// The image at `path`, starting to load it if it is not in memory.
    pub fn get(&mut self, path: &Path, cx: &mut Context<Self>) -> Image {
        self.get_as(path, Look::Plain, cx)
    }

    pub fn get_as(&mut self, path: &Path, look: Look, cx: &mut Context<Self>) -> Image {
        self.clock += 1;
        let key = (path.to_owned(), look);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.used = self.clock;
            return entry.image.clone();
        }
        self.entries.insert(
            key.clone(),
            Entry {
                image: Image::Loading,
                bytes: 0,
                used: self.clock,
            },
        );
        let task = cx.background_spawn({
            let path = path.to_owned();
            async move { decode(&path, look) }
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let decoded = task.await;
            this.update(cx, |this, cx| this.finish(key, decoded, cx))
                .ok();
        })
        .detach();
        Image::Loading
    }

    /// Starts loading `path` without waiting for it.
    pub fn prefetch(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.get(path, cx);
    }

    fn finish(
        &mut self,
        key: (PathBuf, Look),
        decoded: anyhow::Result<Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        match decoded {
            Ok(image) => {
                entry.bytes = image.as_bytes(0).map_or(0, <[u8]>::len);
                entry.image = Image::Ready(image);
                self.bytes += entry.bytes;
            }
            Err(e) => {
                if key.0.exists() {
                    eprintln!("arcwise: {e:#}");
                }
                entry.image = Image::Missing;
            }
        }
        self.evict();
        cx.notify();
    }

    fn evict(&mut self) {
        while self.bytes > BUDGET {
            let Some(oldest) = self
                .entries
                .iter()
                .filter(|(_, e)| e.bytes > 0)
                .min_by_key(|(_, e)| e.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes -= entry.bytes;
                if let Image::Ready(image) = entry.image {
                    self.retired.push(image);
                }
            }
        }
    }

    /// Frees evicted images from the window. Call while rendering.
    pub fn release(&mut self, window: &mut Window) {
        for image in self.retired.drain(..) {
            window.drop_image(image).ok();
        }
    }
}

fn decode(path: &Path, look: Look) -> anyhow::Result<Arc<RenderImage>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut rgba = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    {
        render_svg(&bytes).with_context(|| format!("drawing {}", path.display()))?
    } else {
        image::load_from_memory(&bytes)
            .with_context(|| format!("decoding {}", path.display()))?
            .into_rgba8()
    };
    for px in rgba.pixels_mut() {
        match look {
            // RenderImage holds BGRA, whatever the buffer's type says.
            Look::Plain => px.0.swap(0, 2),
            Look::Silhouette => px.0[..3].fill(0),
        }
    }
    Ok(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
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
    fn a_missing_file_is_an_error() {
        assert!(decode(Path::new("no/such/image.webp"), Look::Plain).is_err());
    }
}
