//! Draws the clip over its place in the panel without being part of the panel. A view's
//! ancestors are drawn again whenever the view changes, so with the player inside the panel,
//! every frame of a clip laid out the whole panel; here the player's only ancestor is the
//! browser, and the panel stays as GPUI drew it last.

use std::cell::Cell;
use std::panic::Location;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Position, Style, Window,
};

/// The clip's place in the panel, as the panel last drew it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Place {
    /// The bounds of the place.
    pub(crate) bounds: Bounds<Pixels>,
    /// The part of them showing through the panel's scrolling.
    pub(crate) showing: Bounds<Pixels>,
}

/// Where the clip goes, written by the panel as it draws and read by [`Follow`] after it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        shared_ownership,
        reason = "the panel's canvas and the browser's Follow are separate elements that GPUI \
                  owns and drops each frame, so the place they share can't be borrowed by both"
    )
)]
pub(crate) type Slot = Rc<Cell<Option<Place>>>;

/// Marks the clip's place in the panel, as the panel draws.
pub(crate) fn place(slot: Slot) -> impl IntoElement {
    use gpui::Styled as _;

    gpui::canvas(
        move |bounds, window, _| {
            slot.set(Some(Place {
                bounds,
                showing: window.content_mask().bounds,
            }));
        },
        |_, (), _, _| {},
    )
    .absolute()
    .inset_0()
}

/// Lays out and draws `child` over the place last marked in `slot`. Drawn after the panel, so
/// the place is this frame's, even while the panel slides.
pub(crate) struct Follow {
    slot: Slot,
    child: AnyElement,
}

impl Follow {
    /// `child`, drawn over the place `slot` holds.
    pub(crate) fn new(slot: Slot, child: impl IntoElement) -> Self {
        Follow {
            slot,
            child: child.into_any_element(),
        }
    }
}

impl IntoElement for Follow {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Follow {
    type RequestLayoutState = ();
    /// The part of the place that shows, once the child is laid out in it.
    type PrepaintState = Option<Bounds<Pixels>>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let Place { bounds, showing } = self.slot.get()?;
        window.with_content_mask(Some(ContentMask { bounds: showing }), |window| {
            self.child
                .layout_as_root(bounds.size.map(AvailableSpace::Definite), window, cx);
            self.child.prepaint_at(bounds.origin, window, cx);
        });
        Some(showing)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        showing: &mut Option<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(showing) = *showing {
            window.with_content_mask(Some(ContentMask { bounds: showing }), |window| {
                self.child.paint(window, cx);
            });
        }
    }
}
