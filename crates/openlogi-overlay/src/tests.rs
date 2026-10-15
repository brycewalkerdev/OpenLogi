/// The catalog this binary translates against lives in `openlogi-ui` and is
/// reached by the relative path in the `i18n!` in `main.rs`. A wrong path does
/// **not** fail the build: `rust_i18n` compiles it to an empty catalog, and
/// every ring label renders as its semantic key. Resolve one action key so that
/// breakage is loud without pinning any translation wording.
#[test]
fn the_shared_catalog_is_wired_up() {
    const KEY: &str = "actions.left_click";
    assert_ne!(rust_i18n::t!(KEY), KEY);
}

use std::{collections::BTreeMap, sync::Arc};

use gpui::{
    AppContext as _, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    PlatformInput, Point, Quad, TestAppContext, WindowHandle, point, px,
};
use openlogi_ipc::ActionRingInvocation;
use tokio::sync::mpsc;

use crate::{
    ipc::OverlayCommand,
    ring::{PanelPlacement, RingView},
    session::{ClickAwaySession, ShowingRing, close_ring_windows},
};

fn invocation(session_id: u64) -> ActionRingInvocation {
    ActionRingInvocation {
        session_id,
        slots: BTreeMap::new(),
        language: None,
    }
}

fn display_windows(
    cx: &mut TestAppContext,
    invocation: &ActionRingInvocation,
    commands: &mpsc::UnboundedSender<OverlayCommand>,
    showing: &Arc<ShowingRing>,
) -> [WindowHandle<RingView>; 2] {
    let placement = PanelPlacement::default();
    std::array::from_fn(|_| {
        let handle = cx.add_window(|_, _| {
            RingView::new(
                invocation.clone(),
                commands.clone(),
                Arc::clone(showing),
                placement.clone(),
            )
        });
        paint(cx, handle);
        handle
    })
}

fn paint(cx: &mut TestAppContext, handle: WindowHandle<RingView>) -> Vec<Quad> {
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        window.painted_quads()
    })
    .unwrap()
}

fn move_pointer(cx: &mut TestAppContext, handle: WindowHandle<RingView>, position: Point<Pixels>) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
}

fn click(cx: &mut TestAppContext, handle: WindowHandle<RingView>, position: Point<Pixels>) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                position,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                position,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn wayland_pointer_event_places_one_panel_and_outside_click_closes_the_session(
    cx: &mut TestAppContext,
) {
    let live = Arc::new(ClickAwaySession::new());
    let showing = Arc::new(live.showing(7));
    let (commands, mut received) = mpsc::unbounded_channel();
    let windows = display_windows(cx, &invocation(7), &commands, &showing);
    for handle in windows {
        assert!(
            paint(cx, handle).is_empty(),
            "no centered fallback may be painted before pointer-enter"
        );
    }
    move_pointer(cx, windows[0], point(px(700.0), px(400.0)));
    cx.update_window(windows[0].into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        // The 48px cancel button must be centered on the invocation pointer.
        assert!(
            window.painted_quads().iter().any(|quad| {
                quad.bounds.size.width == px(48.0).scale(window.scale_factor())
                    && quad.bounds.origin.x == px(676.0).scale(window.scale_factor())
                    && quad.bounds.origin.y == px(376.0).scale(window.scale_factor())
            }),
            "cancel button must be centered on the pointer"
        );
    })
    .unwrap();
    assert!(
        paint(cx, windows[1]).is_empty(),
        "the other display must remain transparent"
    );
    click(cx, windows[1], point(px(10.0), px(10.0)));
    assert_eq!(
        received.try_recv().unwrap(),
        OverlayCommand::Cancel { session_id: 7 }
    );
    received.try_recv().unwrap_err();
    cx.update(|cx| {
        assert!(
            cx.windows().is_empty(),
            "all transparent display windows must be removed"
        );
        assert!(
            !close_ring_windows(cx, 7),
            "timeout teardown after dismissal is a no-op"
        );
    });
}

#[gpui::test]
fn slot_activation_closes_every_display_and_a_stale_timeout_spares_the_replacement(
    cx: &mut TestAppContext,
) {
    use openlogi_core::binding::{ActionRingIcon, ActionRingSlot};
    let live = Arc::new(ClickAwaySession::new());
    let showing = Arc::new(live.showing(7));
    let (commands, mut received) = mpsc::unbounded_channel();
    let mut invocation = invocation(7);
    invocation.slots.insert(
        ActionRingSlot::Top,
        openlogi_ipc::ActionRingPresentation {
            label: "Copy".into(),
            literal: true,
            icon: ActionRingIcon::Copy,
        },
    );
    let windows = display_windows(cx, &invocation, &commands, &showing);
    move_pointer(cx, windows[0], point(px(700.0), px(400.0)));
    paint(cx, windows[0]);
    let position = point(px(700.0), px(278.0));
    move_pointer(cx, windows[0], position);
    paint(cx, windows[0]);
    click(cx, windows[0], position);
    assert_eq!(
        received.try_recv().unwrap(),
        OverlayCommand::Hover {
            session_id: 7,
            slot: ActionRingSlot::Top
        }
    );
    assert_eq!(
        received.try_recv().unwrap(),
        OverlayCommand::Activate {
            session_id: 7,
            slot: ActionRingSlot::Top
        }
    );
    received.try_recv().unwrap_err();
    cx.read(|cx| assert!(cx.windows().is_empty()));

    let replacement = Arc::new(live.showing(8));
    cx.add_window(|_, _| {
        RingView::new(
            ActionRingInvocation {
                session_id: 8,
                ..invocation
            },
            commands,
            replacement,
            PanelPlacement::Window,
        )
    });
    cx.update(|cx| {
        assert!(!close_ring_windows(cx, 7));
        assert_eq!(
            cx.windows().len(),
            1,
            "a stale timeout must leave the replacement open"
        );
        assert!(close_ring_windows(cx, 8));
        assert!(cx.windows().is_empty());
    });
}
