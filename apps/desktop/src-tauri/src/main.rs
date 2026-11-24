// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use djcast_app::service::{Station, StationStatus};
use djcast_app::{AppConfig, user_config_path};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager, State, WindowEvent};

mod logging;
mod tray;

/// Argument the autostart entry passes, so a system start can be told from a manual one.
const AUTOSTART_ARG: &str = "--autostart";

/// Status pushes to the UI per second.
const STATUS_HZ: u64 = 20;

struct AppState {
    station: Arc<Station>,
    config_path: PathBuf,
    logs_dir: PathBuf,
}

const REPO_URL: &str = "https://github.com/danmarinescu79/djcast";

/// Folders the About screen can open.
#[derive(Serialize)]
struct Folders {
    config: String,
    logs: String,
}

#[tauri::command]
fn app_folders(app: State<'_, AppState>) -> Folders {
    Folders {
        config: app.config_path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
        logs: app.logs_dir.display().to_string(),
    }
}

/// Open a page of the project's GitHub repository in the default browser.
#[tauri::command]
fn open_project_page(path: String) -> CmdResult<()> {
    // Only our own repository: the UI never passes arbitrary URLs.
    let path = path.trim_start_matches('/');
    if !path.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.#".contains(c)) {
        return Err("invalid path".into());
    }
    let url = if path.is_empty() { REPO_URL.to_string() } else { format!("{REPO_URL}/{path}") };
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &url]).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(&url).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
fn get_config(app: State<'_, AppState>) -> AppConfig {
    app.station.config()
}

/// Run blocking work (network, audio devices, file IO) off the UI thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> CmdResult<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// Check the fields every server type needs before connecting or saving.
fn validate_server(s: &djcast_core::config::ServerConfig) -> CmdResult<()> {
    if s.host.trim().is_empty() {
        return Err("Enter the server address.".into());
    }
    if s.port == 0 {
        return Err("Enter the server port.".into());
    }
    if s.password.is_empty() {
        return Err("Enter the password.".into());
    }
    Ok(())
}

/// Every server that will be streamed to must be complete.
fn validate_outputs(config: &AppConfig) -> CmdResult<()> {
    for o in config.live_outputs() {
        validate_server(&o.server).map_err(|e| format!("Profile \"{}\": {e}", o.name))?;
    }
    Ok(())
}

#[tauri::command]
async fn save_config(app: State<'_, AppState>, mut config: AppConfig) -> CmdResult<AppConfig> {
    config.normalize();
    validate_outputs(&config)?;
    let (station, path) = (app.station.clone(), app.config_path.clone());
    blocking(move || {
        let old = station.config();
        config.save(&path).map_err(err)?;
        AppConfig::forget_removed_profiles(&old, &config);
        station.set_config(config.clone());
        Ok(config)
    })
    .await
}

#[tauri::command]
fn get_config_path(app: State<'_, AppState>) -> String {
    app.config_path.display().to_string()
}

#[tauri::command]
async fn go_live(app: State<'_, AppState>) -> CmdResult<()> {
    validate_outputs(&app.station.config())?;
    let station = app.station.clone();
    blocking(move || station.go_live().map_err(err)).await
}

#[tauri::command]
async fn stop(app: State<'_, AppState>) -> CmdResult<()> {
    let station = app.station.clone();
    blocking(move || {
        station.stop();
        Ok(())
    })
    .await
}

#[tauri::command]
fn set_title(app: State<'_, AppState>, title: String) {
    app.station.set_manual_title(&title);
}

#[tauri::command]
fn status(app: State<'_, AppState>) -> StationStatus {
    app.station.status()
}

/// Open a source connection with the given settings and close it again.
#[tauri::command]
async fn test_connection(config: AppConfig) -> CmdResult<String> {
    use djcast_core::config::Protocol;
    use djcast_core::icecast::{AudioInfo, SourceConnection};
    let s = config.stream.server.clone();
    validate_server(&s)?;
    let bitrate = config.stream.encoder.bitrate;
    blocking(move || {
        let info = AudioInfo { sample_rate: 44_100, bitrate, channels: 2 };
        SourceConnection::connect(&s, "audio/mpeg", info).map_err(err)?;
        let where_ = match s.protocol {
            Protocol::Icecast => s.mount.clone(),
            Protocol::Shoutcast2 => format!(" (stream {})", s.stream_id),
            Protocol::Shoutcast1 => String::new(),
        };
        Ok(format!("{}:{}{where_} accepted the login", s.host, s.port))
    })
    .await
}

/// Open a folder in Explorer / Finder. Relative paths are next to the config file
/// (the app's working folder); the folder is created if missing.
#[tauri::command]
fn open_folder(path: String) -> CmdResult<()> {
    let dir = std::env::current_dir().map_err(|e| e.to_string())?.join(path.trim());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program).arg(&dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// The operating system's language, e.g. "ro-RO" (for the "Automatic" language setting).
#[tauri::command]
fn system_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en".into())
}

#[tauri::command]
async fn list_midi_inputs() -> CmdResult<Vec<String>> {
    blocking(|| Ok(djcast_nowplaying::live::midi_inputs())).await
}

#[derive(Serialize)]
struct Device {
    name: String,
    input: bool,
    output: bool,
}

#[tauri::command]
async fn list_devices() -> CmdResult<Vec<Device>> {
    blocking(|| {
        Ok(djcast_core::audio::list_devices()
            .map_err(err)?
            .into_iter()
            .map(|d| Device { name: d.name, input: d.is_input, output: d.is_output })
            .collect())
    })
    .await
}

fn main() {
    let config_path = user_config_path();
    let logs_dir = logging::init(config_path.parent().unwrap_or(std::path::Path::new(".")));
    log::info!("djcast {} starting", env!("CARGO_PKG_VERSION"));
    let config = AppConfig::load_or_default(&config_path).unwrap_or_else(|e| {
        log::error!("{e:#}; starting with defaults");
        AppConfig::default()
    });
    log::info!("config: {}", config_path.display());
    // Move plain-text passwords from the file into the OS credential store.
    if config.plaintext_passwords_on_disk && djcast_app::secrets::available() {
        match config.save(&config_path) {
            Ok(()) => log::info!("server passwords moved to the system credential store"),
            Err(e) => log::warn!("could not move passwords to the credential store: {e:#}"),
        }
    }
    // Relative paths in the config (OBS files, recordings) live next to the config file.
    if let Some(dir) = config_path.parent() {
        let _ = std::fs::create_dir_all(dir);
        if let Err(e) = std::env::set_current_dir(dir) {
            log::warn!("cannot use {} as working folder: {e}", dir.display());
        }
    }
    let prefs = config.app.clone();
    let started_by_system = std::env::args().any(|a| a == AUTOSTART_ARG);
    let station = Station::new(config);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .manage(AppState { station: station.clone(), config_path, logs_dir })
        .setup(move |app| {
            let tray = tray::build(app, station.clone())?;

            if let Some(win) = app.get_webview_window("main") {
                // Started with the system: stay in the tray if the user asked for that.
                if started_by_system && prefs.start_minimized {
                    let _ = win.hide();
                }
                // Close button: hide to the tray when asked to, and always while live
                // (a stream is never cut by closing the window).
                let (st, w) = (station.clone(), win.clone());
                win.on_window_event(move |e| {
                    if let WindowEvent::CloseRequested { api, .. } = e
                        && (st.is_live() || st.config().app.close_to_tray)
                    {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            if prefs.auto_live {
                let st = station.clone();
                std::thread::spawn(move || {
                    // Give the audio monitor and now-playing a moment to start.
                    std::thread::sleep(Duration::from_secs(2));
                    match st.go_live() {
                        Ok(()) => log::info!("went live automatically on start"),
                        Err(e) => log::error!("automatic go live failed: {e:#}"),
                    }
                });
            }

            let handle = app.handle().clone();
            std::thread::Builder::new().name("status-events".into()).spawn(move || {
                let mut tick = 0u64;
                loop {
                    let status = station.status();
                    if tick.is_multiple_of(STATUS_HZ) {
                        tray.update(&status, &station.config().language);
                    }
                    let _ = handle.emit("status", status);
                    tick += 1;
                    std::thread::sleep(Duration::from_millis(1000 / STATUS_HZ));
                }
            })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_config_path,
            go_live,
            stop,
            set_title,
            status,
            list_devices,
            list_midi_inputs,
            system_locale,
            open_folder,
            app_folders,
            open_project_page,
            test_connection
        ])
        .run(tauri::generate_context!())
        .expect("error while running djcast");
}
