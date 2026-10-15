//! Optional compositor effects. GPUI keeps the window transparent; this owner
//! applies blur only to the disk and releases protocol objects on window close.

use gpui::{Global, Pixels, Point, Window};
use raw_window_handle::{
    HasDisplayHandle as _, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use tracing::warn;

use crate::ring::{RingPalette, panel_bounds};

mod kde;
mod wayland;

pub(super) fn palette() -> RingPalette {
    kde::palette().unwrap_or_default()
}

#[derive(Default)]
struct Materials(Option<wayland::Effects>);
impl Global for Materials {}

/// Bind the optional effect protocols during window setup, before rendering.
pub(super) fn prepare(window: &Window, cx: &mut gpui::App) {
    let Ok(display) = window.display_handle() else {
        return;
    };
    let RawDisplayHandle::Wayland(display) = display.as_raw() else {
        return;
    };
    if !cx.has_global::<Materials>() {
        let effects = match wayland::Effects::new(display) {
            Ok(effects) => effects,
            Err(error) => {
                warn!(%error, "native ring blur unavailable");
                None
            }
        };
        cx.set_global(Materials(effects));
        cx.on_window_closed(|cx, id| {
            if let Some(effects) = &mut cx.global_mut::<Materials>().0 {
                effects.remove(id);
            }
        })
        .detach();
        // Guest Wayland backends must die before GPUI disconnects its display.
        // Keep them out of views, which external entity handles can retain after
        // their native windows (and even the application) have been closed.
        cx.on_app_quit(|cx| {
            cx.global_mut::<Materials>().0.take();
            async {}
        })
        .detach();
    }
}

/// Synchronize surface-local geometry before GPUI's next surface commit.
/// Keep the covering window transparent; whole-window blur is never requested.
pub(crate) fn update(window: &Window, cx: &mut gpui::App, origin: Option<Point<Pixels>>) {
    let Some(origin) = origin else { return };
    if !cx.has_global::<Materials>() {
        return;
    }
    let Ok(surface) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Wayland(surface) = surface.as_raw() else {
        return;
    };
    if let Some(effects) = &mut cx.global_mut::<Materials>().0
        && let Err(error) = effects.update(
            window.window_handle().window_id(),
            surface,
            panel_bounds(origin),
        )
    {
        warn!(%error, "could not update ring blur region");
    }
}
