//! The ring itself: the GPUI view, and the window it is drawn in.
//!
//! Placement is the interesting part — the panel is centred on the cursor and
//! clamped to the display it came up on, so a ring raised near a screen edge
//! stays whole instead of being cut off.

use gpui::{Bounds, Pixels, Point, Size, point};
use gpui::{
    Context, Div, InteractiveElement, IntoElement, MouseMoveEvent, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Window, WindowBackgroundAppearance,
    WindowDecorations, WindowKind, WindowOptions, div, prelude::FluentBuilder as _, px, svg,
};
use openlogi_core::binding::{Action, ActionRingSlot};
use openlogi_ipc::ActionRingInvocation;
use openlogi_ui::action_icons::RING_CANCEL_ICON;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::ipc::OverlayCommand;
use crate::session::{ShowingRing, close_ring_windows};

mod placement;
pub(crate) use placement::PanelPlacement;

pub(crate) const WINDOW_SIZE: f32 = 360.0;
pub(crate) const SLOT_SIZE: f32 = 54.0;
pub(crate) const RADIUS: f32 = 122.0;

mod material;
pub(crate) use material::{RingPalette, panel_bounds};

pub(crate) struct RingView {
    invocation: ActionRingInvocation,
    commands: mpsc::UnboundedSender<OverlayCommand>,
    hovered: Option<ActionRingSlot>,
    /// Keeps click-away identity published until the last display view closes.
    _showing: Arc<ShowingRing>,
    placement: PanelPlacement,
    palette: RingPalette,
}

impl RingView {
    /// Open a view on `invocation`, reporting interactions through `commands`.
    pub(crate) fn new(
        invocation: ActionRingInvocation,
        commands: mpsc::UnboundedSender<OverlayCommand>,
        showing: Arc<ShowingRing>,
        placement: PanelPlacement,
    ) -> Self {
        Self {
            invocation,
            commands,
            hovered: None,
            _showing: showing,
            placement,
            palette: RingPalette::default(),
        }
    }

    /// Apply compositor-independent colors resolved for this invocation.
    #[cfg(target_os = "linux")]
    pub(crate) fn with_palette(mut self, palette: RingPalette) -> Self {
        self.palette = palette;
        self
    }

    /// The ring session this view is showing.
    pub(crate) const fn session_id(&self) -> u64 {
        self.invocation.session_id
    }

    /// Report this ring cancelled; the caller closes the session's window set.
    pub(crate) fn cancel(&self) {
        let _ = self.commands.send(OverlayCommand::Cancel {
            session_id: self.invocation.session_id,
        });
    }

    fn slot_element(
        &self,
        slot: ActionRingSlot,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let presentation = self.invocation.slots.get(&slot)?;
        let icon_path = presentation.icon.asset_path();
        let selected = self.hovered == Some(slot);
        let glyph = if selected {
            self.palette.selected_text
        } else {
            self.palette.button_text
        };
        let (left, top) = slot.placement(WINDOW_SIZE, RADIUS, SLOT_SIZE);
        let session_id = self.invocation.session_id;
        let activate = self.commands.clone();
        Some(
            div()
                .id(("ring-slot", slot.index()))
                .absolute()
                .left(px(left))
                .top(px(top))
                .size(px(SLOT_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(if selected {
                    self.palette.selected
                } else {
                    self.palette.button
                })
                .when(selected, |slot| {
                    slot.border_2().border_color(self.palette.focus)
                })
                .shadow_md()
                .text_color(glyph)
                .cursor_pointer()
                .child(svg().path(icon_path).size(px(22.0)).text_color(glyph))
                .on_hover(cx.listener(move |this, hovered, _, cx| {
                    if *hovered && this.hovered != Some(slot) {
                        this.hovered = Some(slot);
                        let _ = this
                            .commands
                            .send(OverlayCommand::Hover { session_id, slot });
                        cx.notify();
                    } else if !*hovered && this.hovered == Some(slot) {
                        this.hovered = None;
                        cx.notify();
                    }
                }))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = activate.send(OverlayCommand::Activate { session_id, slot });
                    cx.defer(move |cx| {
                        close_ring_windows(cx, session_id);
                    });
                })
                .into_any_element(),
        )
    }

    fn panel(&self, cx: &mut Context<Self>) -> Div {
        let surface = panel_bounds(Point::default());
        let session_id = self.invocation.session_id;
        let center_commands = self.commands.clone();
        let hovered_label = self.hovered.and_then(|slot| {
            let presentation = self.invocation.slots.get(&slot)?;
            // User-authored labels render verbatim: passing them through the
            // localization table would translate any label that happens to
            // collide with a known key ("Copy" → "Copier" under fr).
            let label = if presentation.literal {
                presentation.label.clone()
            } else if let Some(key) = Action::translation_key_for_label(&presentation.label) {
                rust_i18n::t!(key).into_owned()
            } else {
                presentation.label.clone()
            };
            Some(SharedString::from(label))
        });
        let slots = ActionRingSlot::ALL
            .into_iter()
            .filter_map(|slot| self.slot_element(slot, cx))
            .collect::<Vec<_>>();

        div()
            .relative()
            .size(px(WINDOW_SIZE))
            .child(
                div()
                    .absolute()
                    .left(surface.origin.x)
                    .top(surface.origin.y)
                    .w(surface.size.width)
                    .h(surface.size.height)
                    .rounded_full()
                    .bg(self.palette.panel)
                    .shadow_lg(),
            )
            .children(slots)
            .child(
                div()
                    .id("ring-cancel")
                    .absolute()
                    .left(px(WINDOW_SIZE / 2.0 - 24.0))
                    .top(px(WINDOW_SIZE / 2.0 - 24.0))
                    .size(px(48.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(self.palette.cancel)
                    .text_color(self.palette.muted_text)
                    .cursor_pointer()
                    .child(svg().path(RING_CANCEL_ICON).size(px(20.0)).flex_none())
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = center_commands.send(OverlayCommand::Cancel { session_id });
                        cx.defer(move |cx| {
                            close_ring_windows(cx, session_id);
                        });
                    }),
            )
            .when_some(hovered_label, |ring, label| {
                ring.child(
                    div()
                        .absolute()
                        .left(px(WINDOW_SIZE / 2.0 - 80.0))
                        .top(px(WINDOW_SIZE / 2.0 + 34.0))
                        .w(px(160.0))
                        .text_center()
                        .text_sm()
                        .text_color(self.palette.text)
                        .child(label),
                )
            })
    }
}

impl Render for RingView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session_id = self.invocation.session_id;
        let commands = self.commands.clone();
        let origin = self
            .placement
            .origin(window.window_handle().window_id(), window.viewport_size());
        #[cfg(target_os = "linux")]
        crate::platform::material::update(window, cx, origin);
        let panel = origin.map(|origin| {
            div()
                .absolute()
                .left(origin.x)
                .top(origin.y)
                .child(self.panel(cx))
        });
        div()
            .id("ring-root")
            .relative()
            .size_full()
            // GPUI synthesizes MouseMove from wl_pointer.enter even when the
            // pointer is stationary. Latch it once, before painting the panel.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if this.placement.pointer_entered(window.window_handle().window_id(), event.position) {
                    cx.notify();
                }
            }))
            .children(panel)
            .on_click(move |_, _, cx| {
                let _ = commands.send(OverlayCommand::Cancel { session_id });
                cx.defer(move |cx| { close_ring_windows(cx, session_id); });
            })
    }
}

/// Appearance shared by the platform-specific placement paths.
pub(crate) fn ring_window_options() -> WindowOptions {
    WindowOptions {
        titlebar: None,
        // Linux negotiates decorations independently of `titlebar`. The ring
        // draws its own surface and must not acquire compositor window chrome.
        window_decorations: Some(WindowDecorations::Client),
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some("openlogi-action-ring".to_string()),
        ..WindowOptions::default()
    }
}

pub(crate) fn clamp_window_origin(
    desired: Point<Pixels>,
    window_size: Size<Pixels>,
    display: Bounds<Pixels>,
) -> Point<Pixels> {
    let max = point(
        (display.right() - window_size.width).max(display.left()),
        (display.bottom() - window_size.height).max(display.top()),
    );
    desired.clamp(&display.origin, &max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_origin_is_clamped_to_the_display() {
        let display = Bounds::new(point(px(100.0), px(50.0)), Size::new(px(800.0), px(600.0)));
        let size = Size::new(px(400.0), px(400.0));
        assert_eq!(
            clamp_window_origin(point(px(-50.0), px(-50.0)), size, display),
            point(px(100.0), px(50.0))
        );
        assert_eq!(
            clamp_window_origin(point(px(700.0), px(500.0)), size, display),
            point(px(500.0), px(250.0))
        );
    }

    #[test]
    fn overlay_origin_stays_cursor_centered_away_from_edges() {
        let display = Bounds::new(Point::default(), Size::new(px(1600.0), px(1000.0)));
        let desired = point(px(600.0), px(300.0));
        assert_eq!(
            clamp_window_origin(desired, Size::new(px(400.0), px(400.0)), display),
            desired
        );
    }
}
