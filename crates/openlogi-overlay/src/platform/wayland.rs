//! Wayland has no global cursor query or absolute toplevel placement. Cover
//! each display and let its pointer-enter event anchor one panel in local space.

use std::sync::Arc;

use anyhow::{Result, ensure};
use gpui::{AppContext as _, WindowBounds, WindowDecorations, WindowOptions};
use openlogi_ipc::ActionRingInvocation;
use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    ipc::OverlayCommand,
    ring::{PanelPlacement, RingView, ring_window_options},
    session::ShowingRing,
};

pub(super) fn open_ring_windows(
    cx: &mut gpui::App,
    invocation: &ActionRingInvocation,
    commands: &mpsc::UnboundedSender<OverlayCommand>,
    showing: &Arc<ShowingRing>,
) -> Result<()> {
    let placement = PanelPlacement::default();
    let mut opened = false;
    for display in cx.displays() {
        // Maximize through the compositor: wl_output's integer-scaled display
        // bounds can underestimate the fractional logical size. Layer-shell
        // cannot use the protocol's zero-size stretch request in GPUI 0.3.4:
        // the backend sends a zero wp_viewport destination before configure.
        let options = WindowOptions {
            display_id: Some(display.id()),
            window_bounds: Some(WindowBounds::Maximized(display.bounds())),
            window_decorations: Some(WindowDecorations::Client),
            ..ring_window_options()
        };
        let result = cx.open_window(options, |_, cx| {
            cx.new(|_| {
                RingView::new(
                    invocation.clone(),
                    commands.clone(),
                    Arc::clone(showing),
                    placement.clone(),
                )
            })
        });
        match result {
            Ok(_) => opened = true,
            Err(error) => warn!(%error, "could not open Actions Ring on display"),
        }
    }
    ensure!(opened, "no display could host the Actions Ring");
    Ok(())
}
