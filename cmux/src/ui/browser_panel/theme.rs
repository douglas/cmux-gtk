use webkit6::prelude::*;

/// Apply a dark-mode user stylesheet if the system prefers dark.
pub(super) fn apply_dark_mode(web_view: &webkit6::WebView) {
    let style_manager = libadwaita::StyleManager::default();
    let is_dark = style_manager.is_dark();

    if is_dark {
        inject_dark_stylesheet(web_view);
    }

    // React to theme changes at runtime
    let wv = web_view.clone();
    style_manager.connect_dark_notify(move |sm: &libadwaita::StyleManager| {
        if let Some(ucm) = wv.user_content_manager() {
            ucm.remove_all_style_sheets();
        }
        if sm.is_dark() {
            inject_dark_stylesheet(&wv);
        }
    });
}

pub(super) fn inject_dark_stylesheet(web_view: &webkit6::WebView) {
    let dark_css = r#"
        @media (prefers-color-scheme: light) {
            :root {
                color-scheme: dark;
            }
            html {
                filter: invert(0.88) hue-rotate(180deg);
            }
            img, video, canvas, svg, [style*="background-image"] {
                filter: invert(1) hue-rotate(180deg);
            }
        }
    "#;

    let stylesheet = webkit6::UserStyleSheet::new(
        dark_css,
        webkit6::UserContentInjectedFrames::AllFrames,
        webkit6::UserStyleLevel::User,
        &[],
        &[],
    );

    if let Some(ucm) = web_view.user_content_manager() {
        ucm.add_style_sheet(&stylesheet);
    }
}
