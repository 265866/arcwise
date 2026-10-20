//! Draws the clip over its place in the panel without being part of the panel. A view's
//! ancestors are drawn again whenever the view changes, so with the player inside the panel,
//! every frame of a clip laid out the whole panel; here the player's only ancestor is the
//! browser, and the panel stays as GPUI drew it last.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Position, Style, Window, canvas, prelude::*,
};

/// Where the clip goes: the bounds of its place in the panel, and the part of them showing
/// through the panel's scrolling.
pub(super) type Slot = Rc<Cell<Option<(Bounds<Pixels>, Bounds<Pixels>)>>>;

/// Marks the clip's place in the panel, as the panel draws.
pub(super) fn place(slot: Slot) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            slot.set(Some((bounds, window.content_mask().bounds)));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// Lays out and draws `child` over the place last marked in `slot`. Drawn after the panel, so
/// the place is this frame's, even while the panel slides.
pub(super) struct Follow {
    slot: Slot,
    child: AnyElement,
}

impl Follow {
    pub fn new(slot: Slot, child: impl IntoElement) -> Self {
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

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
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
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let (place, showing) = self.slot.get()?;
        window.with_content_mask(Some(ContentMask { bounds: showing }), |window| {
            self.child
                .layout_as_root(place.size.map(AvailableSpace::Definite), window, cx);
            self.child.prepaint_at(place.origin, window, cx);
        });
        Some(showing)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
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
