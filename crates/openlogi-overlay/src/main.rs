//! Lightweight GPUI host for the cursor-centred Actions Ring.
//!
//! This process is a pure IPC client. The agent owns HID++, session validation,
//! haptic output, and action execution; the overlay only renders the
//! agent-snapshotted actions and reports hover/activate/cancel interactions.

#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

// `t!` resolves against a backend the invoking crate must generate itself, so
// both binaries expand `i18n!` over the one catalog in `openlogi-ui` — the same
// crate this one already depends on for locale negotiation.
rust_i18n::i18n!("../openlogi-ui/locales", fallback = "en");

mod ipc;
mod platform;
mod ring;
mod session;

use std::sync::Arc;

use anyhow::Result;
use tracing::warn;

use openlogi_core::action_ring::DISPLAY_LIFETIME;

use crate::ipc::OverlayCommand;
use crate::session::{
    ClickAwaySession, claim_the_role, close_all_ring_windows, close_ring_windows,
    spawn_click_away_dismissal,
};

fn main() -> Result<()> {
    openlogi_core::logging::init_stderr();

    // Before anything NSApplication-related, including gpui_platform's own
    // setup below: AppKit only reliably suppresses the Dock icon when the
    // activation policy is set before the app finishes launching, not after
    // (#1630 — a later `-setActivationPolicy:` call, inside `app.run`'s
    // closure, left the overlay showing a Dock icon anyway on some macOS
    // versions despite LSUIElement already being set in Info.plist).
    platform::configure_application();

    openlogi_core::locale::activate(None);
    // Held for the whole run: dropping it hands the role to the replacement.
    let _tenancy = claim_the_role()?;
    let ipc::Handle {
        mut invocations,
        commands,
    } = ipc::spawn();

    let mut app = gpui_platform::application().with_assets(openlogi_ui::action_icons::ActionIcons);
    app = app.with_quit_mode(gpui::QuitMode::Explicit);
    app.run(move |cx| {
        let live_session = Arc::new(ClickAwaySession::new());
        spawn_click_away_dismissal(cx, Arc::clone(&live_session));
        cx.spawn(async move |cx| {
            while let Some(observed) = invocations.recv().await {
                // No ring is what a dismissal looks like: close whatever is
                // showing and open nothing. The agent has already forgotten the
                // session, so there is nothing to acknowledge either.
                let Some(invocation) = observed else {
                    cx.update(|cx| {
                        close_all_ring_windows(cx);
                    });
                    continue;
                };
                openlogi_core::locale::activate(invocation.language.as_deref());
                cx.update(|cx| {
                    close_all_ring_windows(cx);
                    let session_id = invocation.session_id;
                    let showing = Arc::new(live_session.showing(session_id));
                    if let Err(error) =
                        platform::open_ring_windows(cx, invocation, commands.clone(), showing)
                    {
                        warn!(%error, "could not open Actions Ring windows");
                        let _ = commands.send(OverlayCommand::Cancel { session_id });
                        return;
                    }
                    let timeout_commands = commands.clone();
                    cx.spawn(async move |cx| {
                        cx.background_executor().timer(DISPLAY_LIFETIME).await;
                        if cx.update(|cx| close_ring_windows(cx, session_id)) {
                            let _ = timeout_commands.send(OverlayCommand::Cancel { session_id });
                        }
                    })
                    .detach();
                });
            }
        })
        .detach();
    });
    Ok(())
}

#[cfg(test)]
mod tests;
