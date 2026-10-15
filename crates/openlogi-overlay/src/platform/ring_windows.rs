//! Open one session's native window or Wayland display-covering window set.

use std::sync::Arc;

use anyhow::Result;
use gpui::AppContext as _;
use openlogi_ipc::ActionRingInvocation;
use tokio::sync::mpsc;

use super::RingPlacement;
use crate::{
    ipc::OverlayCommand,
    ring::{PanelPlacement, RingView},
    session::{ShowingRing, close_ring_windows},
};

pub(crate) fn open_ring_windows(
    cx: &mut gpui::App,
    invocation: ActionRingInvocation,
    commands: mpsc::UnboundedSender<OverlayCommand>,
    showing: Arc<ShowingRing>,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    if gpui::guess_compositor() == "Wayland" {
        return super::wayland::open_ring_windows(cx, &invocation, &commands, &showing);
    }
    let placement = RingPlacement::capture(cx)?;
    let session_id = invocation.session_id;
    let handle = cx.open_window(placement.window_options(), |_, cx| {
        cx.new(|_| RingView::new(invocation, commands, showing, PanelPlacement::Window))
    })?;
    let result = handle
        .update(cx, |_, window, cx| {
            // Resync GPUI after Windows' native bootstrap resize and DPI transition.
            placement.show(window).map(|()| window.bounds_changed(cx))
        })
        .and_then(std::convert::identity);
    if result.is_err() {
        close_ring_windows(cx, session_id);
    }
    result
}
