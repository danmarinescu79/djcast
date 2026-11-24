//! System tray icon: Go Live / Stop, show the window, quit; tooltip shows the current track.

use djcast_app::service::{Station, StationStatus};
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};

pub struct Tray {
    icon: TrayIcon<Wry>,
    live_item: MenuItem<Wry>,
}

impl Tray {
    /// Refresh the menu label and tooltip (called about once per second).
    pub fn update(&self, st: &StationStatus) {
        let _ = self.live_item.set_text(if st.live { "Stop streaming" } else { "Go Live" });
        let state = match (st.live, st.connected) {
            (false, _) => "offline".to_string(),
            (true, false) => "connecting…".to_string(),
            (true, true) => format!("LIVE {}", clock(st.connected_secs)),
        };
        let track = st.track.as_ref().map(|t| format!("\n{}", t.display)).unwrap_or_default();
        // Windows limits tray tooltips to 127 characters.
        let tip: String = format!("djcast — {state}{track}").chars().take(120).collect();
        let _ = self.icon.set_tooltip(Some(tip));
    }
}

pub fn build(app: &App, station: Arc<Station>) -> tauri::Result<Tray> {
    let live_item = MenuItem::with_id(app, "live", "Go Live", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show djcast", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit djcast", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&live_item, &show, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main").tooltip("djcast").menu(&menu).show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let icon = builder
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "live" => {
                if station.is_live() {
                    station.stop();
                } else if let Err(e) = station.go_live() {
                    log::error!("go live from tray: {e:#}");
                    show_main(app);
                }
            }
            "show" => show_main(app),
            "quit" => {
                station.stop();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(Tray { icon, live_item })
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}
