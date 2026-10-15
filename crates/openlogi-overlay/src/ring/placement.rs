//! Panel placement in window-local coordinates. A Wayland session shares one
//! anchor across displays so moving onto another display cannot spawn a second ring.

use std::{cell::Cell, rc::Rc};

use gpui::{Bounds, Pixels, Point, Size, WindowId, point, px};

use super::{WINDOW_SIZE, clamp_window_origin};

#[derive(Clone)]
pub(crate) enum PanelPlacement {
    Window,
    Pointer(Rc<Cell<Option<PointerAnchor>>>),
}

#[derive(Clone, Copy)]
pub(crate) struct PointerAnchor {
    window: WindowId,
    center: Point<Pixels>,
}

impl Default for PanelPlacement {
    fn default() -> Self {
        Self::Pointer(Rc::new(Cell::new(None)))
    }
}

impl PanelPlacement {
    /// Latch only the first compositor-delivered position for the whole session.
    pub(crate) fn pointer_entered(&self, window: WindowId, center: Point<Pixels>) -> bool {
        if let Self::Pointer(anchor) = self
            && anchor.get().is_none()
        {
            anchor.set(Some(PointerAnchor { window, center }));
            return true;
        }
        false
    }

    pub(crate) fn origin(&self, window: WindowId, viewport: Size<Pixels>) -> Option<Point<Pixels>> {
        match self {
            Self::Window => Some(Point::default()),
            Self::Pointer(anchor) => {
                let anchor = anchor.get().filter(|anchor| anchor.window == window)?;
                let edge = px(WINDOW_SIZE);
                Some(clamp_window_origin(
                    point(anchor.center.x - edge / 2.0, anchor.center.y - edge / 2.0),
                    Size::new(edge, edge),
                    Bounds::new(Point::default(), viewport),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_a_pointer_then_stays_at_the_invocation_position() {
        let placement = PanelPlacement::default();
        let window = WindowId::from(1);
        let viewport = Size::new(px(1920.0), px(1080.0));
        assert_eq!(placement.origin(window, viewport), None);
        assert!(placement.pointer_entered(window, point(px(700.0), px(400.0))));
        assert_eq!(
            placement.origin(window, viewport),
            Some(point(px(520.0), px(220.0)))
        );
        assert!(!placement.pointer_entered(window, point(px(900.0), px(600.0))));
        assert_eq!(
            placement.origin(window, viewport),
            Some(point(px(520.0), px(220.0)))
        );
    }

    #[test]
    fn a_second_display_cannot_reanchor_or_duplicate_the_ring() {
        let first = PanelPlacement::default();
        let second = first.clone();
        let viewport = Size::new(px(1920.0), px(1080.0));
        assert!(first.pointer_entered(WindowId::from(1), point(px(700.0), px(400.0))));
        assert!(!second.pointer_entered(WindowId::from(2), point(px(500.0), px(300.0))));
        assert_eq!(second.origin(WindowId::from(2), viewport), None);
        assert_eq!(
            first.origin(WindowId::from(1), viewport),
            Some(point(px(520.0), px(220.0)))
        );
    }

    #[test]
    fn clamps_to_the_actual_viewport_including_a_display_smaller_than_the_ring() {
        for (center, viewport, expected) in [
            ((0.0, 0.0), (1920.0, 1080.0), (0.0, 0.0)),
            ((1919.0, 1079.0), (1920.0, 1080.0), (1560.0, 720.0)),
            ((100.0, 80.0), (200.0, 160.0), (0.0, 0.0)),
            ((1200.0, 800.0), (1692.0, 1128.0), (1020.0, 620.0)),
        ] {
            let placement = PanelPlacement::default();
            let window = WindowId::from(1);
            placement.pointer_entered(window, point(px(center.0), px(center.1)));
            assert_eq!(
                placement.origin(window, Size::new(px(viewport.0), px(viewport.1))),
                Some(point(px(expected.0), px(expected.1)))
            );
        }
    }
}
