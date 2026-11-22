//! Lines drawn into a picture on the CPU. GPUI draws paths through a window-sized multisampled
//! texture, about 27 MB of GPU memory once any path is drawn; a picture of the lines costs only
//! its own size, and is drawn again only when the lines change.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use gpui::{App, Bounds, Corners, Hsla, Pixels, Point, RenderImage, Rgba, Size, Window};
use image::{Frame, RgbaImage};
use resvg::tiny_skia::{Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform};

/// The dash and gap lengths of a dashed line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dash {
    pub(crate) dash: Pixels,
    pub(crate) gap: Pixels,
}

/// A line through some points.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Line {
    /// From the top left of the bounds the lines are painted in.
    pub(crate) points: Vec<Point<Pixels>>,
    pub(crate) width: Pixels,
    pub(crate) color: Hsla,
    /// Dash and gap lengths, for a dashed line.
    pub(crate) dash: Option<Dash>,
}

#[derive(Debug)]
struct Picture {
    lines: Vec<Line>,
    /// The size of the bounds the lines were drawn for, and the display's scale then.
    size: Size<Pixels>,
    scale: f32,
    /// The part of those bounds the picture covers: just around the lines.
    area: Bounds<Pixels>,
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "GPUI's paint_image and drop_image take a picture as an Arc<RenderImage>"
        )
    )]
    image: Arc<RenderImage>,
}

/// The picture last drawn of some lines, kept until they change.
#[derive(Debug, Default)]
pub(crate) struct Drawn {
    last: RefCell<Option<Picture>>,
    /// Whether painting failed since the picture was last drawn. The canvas repaints every frame
    /// while a clip plays, so a failure that lasts is passed on once per picture, not per frame.
    failed: Cell<bool>,
}

impl Drawn {
    /// Paints `lines` over `bounds`, drawing them into a new picture if they or the size changed.
    /// When freeing the previous picture fails, the new one is still drawn and painted.
    ///
    /// # Errors
    ///
    /// GPUI's error when the window can't free the previous picture or take this one into its
    /// sprite atlas: once per picture, as [`Drawn::once`] passes it on.
    pub(crate) fn paint(
        &self,
        bounds: Bounds<Pixels>,
        lines: Vec<Line>,
        window: &mut Window,
    ) -> anyhow::Result<()> {
        let scale = window.scale_factor();
        let mut last = self.last.borrow_mut();
        #[expect(
            clippy::float_cmp,
            reason = "the picture is good only for the exact scale it was drawn at, and any change \
                      at all redraws it"
        )]
        let unchanged = last
            .as_ref()
            .is_some_and(|p| p.size == bounds.size && p.scale == scale && p.lines == lines);
        let mut freed = Ok(());
        if !unchanged {
            self.failed.set(false);
            if let Some(old) = last.take() {
                freed = window.drop_image(old.image);
            }
            *last = area(&lines, bounds.size, scale).and_then(|area| {
                let image = draw(&lines, area.from, area.size, scale)?;
                Some(Picture {
                    lines,
                    size: bounds.size,
                    scale,
                    area: area.bounds,
                    image,
                })
            });
        }
        let painted = last.as_ref().map_or(Ok(()), |picture| {
            let at = Bounds::new(bounds.origin + picture.area.origin, picture.area.size);
            window.paint_image(
                bounds,
                at,
                Corners::default(),
                picture.image.clone(),
                0,
                false,
            )
        });
        drop(last);
        self.once(freed.and(painted))
    }

    /// Passes on `result`'s error only if no painting failed since the picture was last drawn,
    /// for what a canvas paints beside the picture too.
    ///
    /// # Errors
    ///
    /// `result`'s error, the first time since the picture was last drawn.
    pub(crate) fn once(&self, result: anyhow::Result<()>) -> anyhow::Result<()> {
        match result {
            Err(_) if self.failed.replace(true) => Ok(()),
            result => result,
        }
    }

    /// Frees the picture, before the owner is dropped. Call outside any window's update.
    pub(crate) fn release(&self, cx: &mut App) {
        if let Some(picture) = self.last.take() {
            cx.drop_image(picture.image, None);
        }
    }
}

/// A device pixel, from the top left of the bounds the lines are painted in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DevicePoint {
    x: u32,
    y: u32,
}

/// A size in device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeviceSize {
    width: u32,
    height: u32,
}

/// Where a picture of lines goes, on whole device pixels.
#[derive(Debug)]
struct Area {
    /// From the top left of the bounds the lines are painted in.
    bounds: Bounds<Pixels>,
    /// The device pixel it starts at, and its size in device pixels.
    from: DevicePoint,
    size: DeviceSize,
}

/// The part of bounds `size` big that `lines` touch.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "each edge is clamped into the bounds in device pixels and then floored or ceiled, \
              so it is a whole number from 0 to the window's size"
)]
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
            gpui::point(gpui::px(x0 / scale), gpui::px(y0 / scale)),
            gpui::size(gpui::px((x1 - x0) / scale), gpui::px((y1 - y0) / scale)),
        ),
        from: DevicePoint {
            x: x0 as u32,
            y: y0 as u32,
        },
        size: DeviceSize {
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        },
    })
}

/// Draws `lines` into a picture `size` device pixels big whose top left is device pixel `from`.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        shared_ownership,
        reason = "GPUI's paint_image and drop_image take a picture as an Arc<RenderImage>"
    )
)]
#[expect(
    clippy::cast_precision_loss,
    reason = "device pixel offsets within a window are far below 2^24, where f32 starts to round"
)]
fn draw(
    lines: &[Line],
    from: DevicePoint,
    size: DeviceSize,
    scale: f32,
) -> Option<Arc<RenderImage>> {
    let mut pixmap = Pixmap::new(size.width, size.height)?;
    for line in lines {
        let mut points = line.points.iter().map(|p| {
            (
                f32::from(p.x) * scale - from.x as f32,
                f32::from(p.y) * scale - from.y as f32,
            )
        });
        let mut path = PathBuilder::new();
        let (x, y) = points.next()?;
        path.move_to(x, y);
        for (x, y) in points {
            path.line_to(x, y);
        }
        let Some(path) = path.finish() else { continue };
        let color = Rgba::from(line.color);
        let mut paint = Paint::default();
        paint.set_color_rgba8(
            channel(color.r),
            channel(color.g),
            channel(color.b),
            channel(color.a),
        );
        paint.anti_alias = true;
        let stroke = Stroke {
            width: f32::from(line.width) * scale,
            dash: line.dash.and_then(|Dash { dash, gap }| {
                StrokeDash::new(vec![f32::from(dash) * scale, f32::from(gap) * scale], 0.)
            }),
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
    let mut pixels = pixmap.take();
    straight_bgra(&mut pixels);
    let image = RgbaImage::from_raw(size.width, size.height, pixels)?;
    Some(Arc::new(RenderImage::new([Frame::new(image)])))
}

/// Turns tiny-skia's premultiplied RGBA into the straight BGRA GPUI takes.
fn straight_bgra(pixels: &mut [u8]) {
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a == 0 {
            continue;
        }
        if a != 255 {
            for c in &mut px[..3] {
                *c = u8::try_from(u16::from(*c) * 255 / u16::from(a)).unwrap_or(u8::MAX);
            }
        }
        px.swap(0, 2);
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..=1 and scaled to 0..=255 before it is rounded"
)]
fn channel(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}

#[cfg(test)]
mod tests {
    use std::slice;

    use super::*;

    #[test]
    fn draws_a_line_in_straight_bgra() {
        let line = Line {
            points: vec![
                gpui::point(gpui::px(0.), gpui::px(2.)),
                gpui::point(gpui::px(8.), gpui::px(2.)),
            ],
            width: gpui::px(2.),
            color: gpui::rgba(0xff_00_00_80).into(),
            dash: None,
        };
        let image = draw(
            &[line],
            DevicePoint { x: 0, y: 0 },
            DeviceSize {
                width: 8,
                height: 4,
            },
            1.,
        )
        .unwrap();
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
            points: vec![
                gpui::point(gpui::px(10.), gpui::px(20.)),
                gpui::point(gpui::px(30.), gpui::px(25.)),
            ],
            width: gpui::px(2.),
            color: gpui::white(),
            dash: None,
        };
        let size = gpui::size(gpui::px(100.), gpui::px(100.));
        let at = area(slice::from_ref(&line), size, 1.5).unwrap();
        assert_eq!(
            at.from,
            DevicePoint { x: 10, y: 25 },
            "padded by half the width and two pixels, on whole device pixels at a scale of 1.5"
        );
        assert_eq!(
            at.size,
            DeviceSize {
                width: 40,
                height: 17
            }
        );
        assert_eq!(
            at.bounds.origin,
            gpui::point(gpui::px(10. / 1.5), gpui::px(25. / 1.5))
        );
        let edge = Line {
            points: vec![
                gpui::point(gpui::px(0.), gpui::px(0.)),
                gpui::point(gpui::px(200.), gpui::px(0.)),
            ],
            ..line
        };
        let at = area(&[edge], size, 1.).unwrap();
        assert_eq!(
            (at.from, at.size),
            (
                DevicePoint { x: 0, y: 0 },
                DeviceSize {
                    width: 100,
                    height: 3
                }
            ),
            "clipped to the bounds"
        );
        assert!(area(&[], size, 1.).is_none());
    }

    #[test]
    fn a_dashed_line_has_gaps() {
        let line = Line {
            points: vec![
                gpui::point(gpui::px(0.), gpui::px(1.)),
                gpui::point(gpui::px(20.), gpui::px(1.)),
            ],
            width: gpui::px(2.),
            color: gpui::white(),
            dash: Some(Dash {
                dash: gpui::px(4.),
                gap: gpui::px(6.),
            }),
        };
        let image = draw(
            &[line],
            DevicePoint { x: 0, y: 0 },
            DeviceSize {
                width: 20,
                height: 2,
            },
            1.,
        )
        .unwrap();
        let bytes = image.as_bytes(0).unwrap();
        let alpha = |x: usize| bytes[(20 + x) * 4 + 3];
        assert!(alpha(1) > 200, "in a dash");
        assert_eq!(alpha(7), 0, "in a gap");
    }
}
