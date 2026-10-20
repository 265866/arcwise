//! Lines drawn into a picture on the CPU. GPUI draws paths through a window-sized multisampled
//! texture, about 27 MB of GPU memory once any path is drawn; a picture of the lines costs only
//! its own size, and is drawn again only when the lines change.

use std::cell::RefCell;
use std::sync::Arc;

use gpui::{App, Bounds, Hsla, Pixels, Point, RenderImage, Size, Window, point, px};
use resvg::tiny_skia;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// From the top left of the bounds the lines are painted in.
    pub points: Vec<Point<Pixels>>,
    pub width: Pixels,
    pub color: Hsla,
    /// Dash and gap lengths, for a dashed line.
    pub dash: Option<(Pixels, Pixels)>,
}

struct Picture {
    lines: Vec<Line>,
    /// The size of the bounds the lines were drawn for, and the display's scale then.
    size: Size<Pixels>,
    scale: f32,
    /// The part of those bounds the picture covers: just around the lines.
    area: Bounds<Pixels>,
    image: Arc<RenderImage>,
}

/// The picture last drawn of some lines, kept until they change.
#[derive(Default)]
pub struct Drawn {
    last: RefCell<Option<Picture>>,
}

impl Drawn {
    /// Paints `lines` over `bounds`, drawing them into a new picture if they or the size changed.
    pub fn paint(&self, bounds: Bounds<Pixels>, lines: Vec<Line>, window: &mut Window) {
        let scale = window.scale_factor();
        let mut last = self.last.borrow_mut();
        if !last
            .as_ref()
            .is_some_and(|p| p.size == bounds.size && p.scale == scale && p.lines == lines)
        {
            if let Some(old) = last.take() {
                window.drop_image(old.image).ok();
            }
            let Some(area) = area(&lines, bounds.size, scale) else {
                return;
            };
            let Some(image) = draw(&lines, area.from, area.size, scale) else {
                return;
            };
            *last = Some(Picture {
                lines,
                size: bounds.size,
                scale,
                area: area.bounds,
                image,
            });
        }
        if let Some(picture) = last.as_ref() {
            let at = Bounds::new(bounds.origin + picture.area.origin, picture.area.size);
            window
                .paint_image(
                    bounds,
                    at,
                    Default::default(),
                    picture.image.clone(),
                    0,
                    false,
                )
                .ok();
        }
    }

    /// Frees the picture, before the owner is dropped. Call outside any window's update.
    pub fn release(&self, cx: &mut App) {
        if let Some(picture) = self.last.take() {
            cx.drop_image(picture.image, None);
        }
    }
}

/// Where a picture of lines goes, on whole device pixels.
struct Area {
    /// From the top left of the bounds the lines are painted in.
    bounds: Bounds<Pixels>,
    /// The device pixel it starts at, and its size in device pixels.
    from: (u32, u32),
    size: (u32, u32),
}

/// The part of bounds `size` big that `lines` touch.
fn area(lines: &[Line], size: Size<Pixels>, scale: f32) -> Option<Area> {
    let pad = lines.iter().map(|l| f32::from(l.width)).fold(0., f32::max) / 2. + 2.;
    let points = lines.iter().flat_map(|l| &l.points);
    let (mut left, mut top, mut right, mut bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        left = left.min(f32::from(p.x));
        top = top.min(f32::from(p.y));
        right = right.max(f32::from(p.x));
        bottom = bottom.max(f32::from(p.y));
    }
    let device = |x: f32, limit: Pixels| (x * scale).clamp(0., f32::from(limit) * scale);
    let (x0, y0) = (
        device(left - pad, size.width).floor(),
        device(top - pad, size.height).floor(),
    );
    let (x1, y1) = (
        device(right + pad, size.width).ceil(),
        device(bottom + pad, size.height).ceil(),
    );
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Area {
        bounds: Bounds::new(
            point(px(x0 / scale), px(y0 / scale)),
            gpui::size(px((x1 - x0) / scale), px((y1 - y0) / scale)),
        ),
        from: (x0 as u32, y0 as u32),
        size: ((x1 - x0) as u32, (y1 - y0) as u32),
    })
}

/// Draws `lines` into a picture `size` device pixels big whose top left is device pixel `from`.
fn draw(
    lines: &[Line],
    (from_x, from_y): (u32, u32),
    (width, height): (u32, u32),
    scale: f32,
) -> Option<Arc<RenderImage>> {
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    for line in lines {
        let mut points = line.points.iter().map(|p| {
            (
                f32::from(p.x) * scale - from_x as f32,
                f32::from(p.y) * scale - from_y as f32,
            )
        });
        let mut path = tiny_skia::PathBuilder::new();
        let (x, y) = points.next()?;
        path.move_to(x, y);
        for (x, y) in points {
            path.line_to(x, y);
        }
        let Some(path) = path.finish() else { continue };
        let color = gpui::Rgba::from(line.color);
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(
            channel(color.r),
            channel(color.g),
            channel(color.b),
            channel(color.a),
        );
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke {
            width: f32::from(line.width) * scale,
            dash: line.dash.and_then(|(dash, gap)| {
                tiny_skia::StrokeDash::new(
                    vec![f32::from(dash) * scale, f32::from(gap) * scale],
                    0.,
                )
            }),
            ..tiny_skia::Stroke::default()
        };
        pixmap.stroke_path(
            &path,
            &paint,
            &stroke,
            tiny_skia::Transform::identity(),
            None,
        );
    }
    // tiny-skia keeps premultiplied RGBA; GPUI takes straight BGRA.
    let mut pixels = pixmap.take();
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a == 0 {
            continue;
        }
        if a != 255 {
            for c in &mut px[..3] {
                *c = (u16::from(*c) * 255 / u16::from(a)) as u8;
            }
        }
        px.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(width, height, pixels)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(image)])))
}

fn channel(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_a_line_in_straight_bgra() {
        let line = Line {
            points: vec![point(px(0.), px(2.)), point(px(8.), px(2.))],
            width: px(2.),
            color: gpui::rgba(0xff000080).into(),
            dash: None,
        };
        let image = draw(&[line], (0, 0), (8, 4), 1.).unwrap();
        let bytes = image.as_bytes(0).unwrap();
        let at = |x: usize, y: usize| &bytes[(y * 8 + x) * 4..][..4];
        assert_eq!(at(0, 0), &[0, 0, 0, 0], "clear around the line");
        let red = at(4, 2);
        assert!(
            red[2] >= 254 && red[0] == 0,
            "red, not premultiplied: {red:?}"
        );
        assert!((126..=130).contains(&red[3]), "half see-through: {red:?}");
    }

    #[test]
    fn draws_only_around_the_lines() {
        let line = Line {
            points: vec![point(px(10.), px(20.)), point(px(30.), px(25.))],
            width: px(2.),
            color: gpui::white(),
            dash: None,
        };
        let size = gpui::size(px(100.), px(100.));
        // Padded by half the width and two pixels, on whole device pixels at a scale of 1.5.
        let at = area(std::slice::from_ref(&line), size, 1.5).unwrap();
        assert_eq!(at.from, (10, 25));
        assert_eq!(at.size, (40, 17));
        assert_eq!(at.bounds.origin, point(px(10. / 1.5), px(25. / 1.5)));
        // Clipped to the bounds.
        let edge = Line {
            points: vec![point(px(0.), px(0.)), point(px(200.), px(0.))],
            ..line
        };
        let at = area(&[edge], size, 1.).unwrap();
        assert_eq!((at.from, at.size), ((0, 0), (100, 3)));
        assert!(area(&[], size, 1.).is_none());
    }

    #[test]
    fn a_dashed_line_has_gaps() {
        let line = Line {
            points: vec![point(px(0.), px(1.)), point(px(20.), px(1.))],
            width: px(2.),
            color: gpui::white(),
            dash: Some((px(4.), px(6.))),
        };
        let image = draw(&[line], (0, 0), (20, 2), 1.).unwrap();
        let bytes = image.as_bytes(0).unwrap();
        let alpha = |x: usize| bytes[(20 + x) * 4 + 3];
        assert!(alpha(1) > 200, "in a dash");
        assert_eq!(alpha(7), 0, "in a gap");
    }
}
