//! A guest connection to GPUI's libwayland display, with its own event queue.
//! Never dispatch or take ownership of GPUI's surface or event queue.

use anyhow::Result;
use gpui::{Bounds, Pixels, WindowId};
use raw_window_handle::{WaylandDisplayHandle, WaylandWindowHandle};
use std::collections::HashMap;
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_compositor::WlCompositor,
        wl_region::WlRegion,
        wl_registry::{self, WlRegistry},
        wl_surface::WlSurface,
    },
};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1,
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};
use wayland_protocols_plasma::blur::client::{
    org_kde_kwin_blur::OrgKdeKwinBlur, org_kde_kwin_blur_manager::OrgKdeKwinBlurManager,
};

struct State;
impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(State: ignore WlCompositor);
delegate_noop!(State: ignore WlRegion);
delegate_noop!(State: ignore ExtBackgroundEffectManagerV1);
delegate_noop!(State: ignore ExtBackgroundEffectSurfaceV1);
delegate_noop!(State: ignore OrgKdeKwinBlurManager);
delegate_noop!(State: ignore OrgKdeKwinBlur);

enum Effect {
    Standard(ExtBackgroundEffectSurfaceV1),
    Kde(OrgKdeKwinBlur),
}

enum Manager {
    Standard(ExtBackgroundEffectManagerV1),
    Kde(OrgKdeKwinBlurManager),
}

pub(super) struct Effects {
    connection: Connection,
    queue: EventQueue<State>,
    compositor: WlCompositor,
    manager: Manager,
    windows: HashMap<WindowId, (Effect, Bounds<Pixels>)>,
}

impl Effects {
    #[expect(
        unsafe_code,
        reason = "GPUI exposes its live libwayland display through a borrowed raw-window-handle"
    )]
    pub(super) fn new(display: WaylandDisplayHandle) -> Result<Option<Self>> {
        // SAFETY: called only with a handle borrowed from a live GPUI Window.
        // Materials clears the guest backend on app quit, before GPUI drops its
        // display. The guest never disconnects or dispatches GPUI's event queue.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<State>(&connection)?;
        let qh = queue.handle();
        let compositor: WlCompositor = globals.bind(&qh, 1..=1, ())?;
        let manager = if let Ok(manager) =
            globals.bind::<ExtBackgroundEffectManagerV1, _, _>(&qh, 1..=1, ())
        {
            Manager::Standard(manager)
        } else if let Ok(manager) = globals.bind::<OrgKdeKwinBlurManager, _, _>(&qh, 1..=1, ()) {
            Manager::Kde(manager)
        } else {
            return Ok(None);
        };
        Ok(Some(Self {
            connection,
            queue,
            compositor,
            manager,
            windows: HashMap::new(),
        }))
    }

    #[expect(
        unsafe_code,
        reason = "borrow GPUI's wl_surface for this call without destroying it or changing its queue"
    )]
    pub(super) fn update(
        &mut self,
        window: WindowId,
        surface: WaylandWindowHandle,
        bounds: Bounds<Pixels>,
    ) -> Result<()> {
        self.queue.dispatch_pending(&mut State)?;
        if self
            .windows
            .get(&window)
            .is_some_and(|(_, previous)| *previous == bounds)
        {
            return Ok(());
        }
        // SAFETY: the pointer is GPUI's live wl_surface, borrowed from Window.
        // This proxy is only used in this call and is never destroyed or retained.
        let id =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.surface.as_ptr().cast())? };
        let surface = WlSurface::from_id(&self.connection, id)?;
        let qh = self.queue.handle();
        let (effect, previous) = self.windows.entry(window).or_insert_with(|| {
            let effect = match &self.manager {
                Manager::Standard(manager) => {
                    Effect::Standard(manager.get_background_effect(&surface, &qh, ()))
                }
                Manager::Kde(manager) => Effect::Kde(manager.create(&surface, &qh, ())),
            };
            (effect, bounds)
        });
        let region = self.compositor.create_region(&qh, ());
        for [x, y, width, height] in circle_rows(bounds) {
            region.add(x, y, width, height);
        }
        match effect {
            Effect::Standard(effect) => effect.set_blur_region(Some(&region)),
            Effect::Kde(effect) => {
                effect.set_region(Some(&region));
                effect.commit();
            }
        }
        region.destroy();
        // Surface-local coordinates follow GPUI's viewport transform, including
        // fractional scaling. GPUI remains the sole owner of surface.commit.
        *previous = bounds;
        Ok(())
    }

    pub(super) fn remove(&mut self, window: WindowId) {
        self.windows.remove(&window);
        let _ = self.connection.flush();
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        match self {
            Self::Standard(effect) => effect.destroy(),
            Self::Kde(effect) => effect.release(),
        }
    }
}

impl Drop for Effects {
    fn drop(&mut self) {
        self.windows.clear();
        if let Manager::Standard(manager) = &self.manager {
            manager.destroy();
        }
        let _ = self.connection.flush();
    }
}

/// Inscribe integer scanlines in the disk. No rectangle extends outside the
/// painted circle, including fractional origins and the antialiased edge.
#[expect(
    clippy::cast_possible_truncation,
    reason = "validated surface coordinates fit i32; ceil/floor intentionally quantize Wayland regions"
)]
fn circle_rows(bounds: Bounds<Pixels>) -> Vec<[i32; 4]> {
    let center = bounds.center();
    let (cx, cy, radius) = (
        f64::from(f32::from(center.x)),
        f64::from(f32::from(center.y)),
        f64::from(f32::from(bounds.size.width)) / 2.0,
    );
    if ![cx, cy, radius].into_iter().all(f64::is_finite)
        || radius <= 0.0
        || cx.abs().max(cy.abs()) + radius > 16_000_000.0
    {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for y in (cy - radius).ceil() as i32..(cy + radius).floor() as i32 {
        let dy = (f64::from(y) - cy)
            .abs()
            .max((f64::from(y) + 1.0 - cy).abs());
        let half = (radius * radius - dy * dy).max(0.0).sqrt();
        let x = (cx - half).ceil() as i32;
        let right = (cx + half).floor() as i32;
        if right > x {
            rows.push([x, y, right - x, 1]);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::panel_bounds;
    use gpui::{Point, point, px};

    #[test]
    fn blur_is_confined_to_the_visible_disk_at_center_edges_and_fractional_origins() {
        for origin in [
            Point::default(),
            point(px(1020.0), px(620.0)),
            point(px(207.35), px(91.8)),
        ] {
            let bounds = panel_bounds(origin);
            let rows = circle_rows(bounds);
            assert!(rows.len() > 300);
            let center = bounds.center();
            let (cx, cy) = (
                f64::from(f32::from(center.x)),
                f64::from(f32::from(center.y)),
            );
            let radius = f64::from(f32::from(bounds.size.width)) / 2.0;
            for [x, y, width, height] in rows {
                for x in [x, x + width] {
                    for y in [y, y + height] {
                        assert!((f64::from(x) - cx).hypot(f64::from(y) - cy) <= radius + 0.001);
                    }
                }
            }
        }
    }
}
