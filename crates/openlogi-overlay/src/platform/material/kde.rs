//! KDE semantic color roles. Read once per invocation, so a new ring follows
//! changes made in System Settings without restarting the overlay.

use crate::ring::RingPalette;
use gpui::{Hsla, Rgba};
use ini::Ini;

pub(super) fn palette() -> Option<RingPalette> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok()?;
    if !desktop
        .split(':')
        .any(|name| name.eq_ignore_ascii_case("KDE"))
    {
        return None;
    }
    let path = openlogi_core::paths::xdg_config_home()
        .ok()?
        .join("kdeglobals");
    let contents = std::fs::read_to_string(path).ok()?;
    from_config(&contents)
}

fn from_config(contents: &str) -> Option<RingPalette> {
    let config = Ini::load_from_str(contents).ok()?;
    let color = |group, key| rgb(config.section(Some(group))?.get(key)?);
    let text = color("Colors:Window", "ForegroundNormal")?;
    let button = color("Colors:Button", "BackgroundNormal")?;
    let mut panel = color("Colors:Window", "BackgroundNormal")?;
    // Keep enough tint for legibility when the user disables compositor blur.
    panel.a = 0.88;
    Some(RingPalette {
        panel,
        button,
        cancel: button,
        text,
        button_text: color("Colors:Button", "ForegroundNormal").unwrap_or(text),
        muted_text: color("Colors:Window", "ForegroundInactive").unwrap_or(text),
        selected: color("Colors:Selection", "BackgroundNormal")?,
        selected_text: color("Colors:Selection", "ForegroundNormal")?,
        focus: color("Colors:Button", "DecorationFocus")?,
    })
}

fn rgb(value: &str) -> Option<Hsla> {
    let channels = value
        .split(',')
        .map(|part| part.trim().parse::<u8>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let [r, g, b] = channels.as_slice() else {
        return None;
    };
    Some(
        Rgba {
            r: f32::from(*r) / 255.0,
            g: f32::from(*g) / 255.0,
            b: f32::from(*b) / 255.0,
            a: 1.0,
        }
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(background: &str, foreground: &str) -> String {
        format!(
            "[Colors:Window]\nBackgroundNormal={background}\nForegroundNormal={foreground}\n[Colors:Button]\nBackgroundNormal={background}\nDecorationFocus=61,174,233\n[Colors:Selection]\nBackgroundNormal=61,174,233\nForegroundNormal=255,255,255\n[Colors:Header][Inactive]\nBackgroundNormal=0,0,0\n"
        )
    }

    #[test]
    fn follows_light_and_dark_kde_schemes_and_preserves_selected_text() {
        for (background, foreground) in [("239,240,241", "35,38,41"), ("35,38,41", "239,240,241")] {
            let palette = from_config(&config(background, foreground)).unwrap();
            let mut expected = rgb(background).unwrap();
            expected.a = 0.88;
            assert_eq!(palette.panel, expected);
            assert_eq!(palette.text, rgb(foreground).unwrap());
            assert_eq!(palette.selected_text, rgb("255,255,255").unwrap());
        }
    }

    #[test]
    fn malformed_or_incomplete_config_keeps_the_fallback_palette() {
        assert!(from_config("[Colors:Window]\nBackgroundNormal=0,0,0").is_none());
        for color in ["256,0,0", "-1,0,0", "1,2", "1,2,3,4", "bad"] {
            assert!(from_config(&config(color, "255,255,255")).is_none());
        }
    }
    #[gpui::test]
    fn configured_colors_are_painted_by_the_production_ring(cx: &mut gpui::TestAppContext) {
        use crate::{
            ring::{PanelPlacement, RingView, panel_bounds},
            session::ClickAwaySession,
        };
        use gpui::{AppContext as _, px};
        use std::{collections::BTreeMap, sync::Arc};
        let palette = from_config(&config("239,240,241", "35,38,41")).unwrap();
        let live = Arc::new(ClickAwaySession::new());
        let (commands, _received) = tokio::sync::mpsc::unbounded_channel();
        let handle = cx.add_window(|_, _| {
            RingView::new(
                openlogi_ipc::ActionRingInvocation {
                    session_id: 1,
                    slots: BTreeMap::new(),
                    language: None,
                },
                commands,
                Arc::new(live.showing(1)),
                PanelPlacement::Window,
            )
            .with_palette(palette)
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            let disk = panel_bounds(gpui::Point::default()).scale(window.scale_factor());
            let quads = window.painted_quads();
            let panel = quads.iter().find(|quad| quad.bounds == disk).unwrap();
            assert_eq!(panel.background.as_solid(), Some(palette.panel));
            assert_eq!(panel.corner_radii.top_left, disk.size.width / 2.0);
            let cancel = quads
                .iter()
                .find(|quad| quad.bounds.size.width == px(48.0).scale(window.scale_factor()))
                .unwrap();
            assert_eq!(cancel.background.as_solid(), Some(palette.cancel));
        })
        .unwrap();
    }
}
