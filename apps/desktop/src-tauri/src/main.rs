// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use djcast_app::service::{Station, StationStatus};
use djcast_app::{AppConfig, user_config_path};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, State};

/// Status pushes to the UI per second.
const STATUS_HZ: u64 = 20;

struct AppState {
    station: Arc<Station>,
    config_path: PathBuf,
}

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
fn get_config(app: State<'_, AppState>) -> AppConfig {
    app.station.config()
}

#[tauri::command]
fn save_config(app: State<'_, AppState>, config: AppConfig) -> CmdResult<()> {
    config.save(&app.config_path).map_err(err)?;
    app.station.set_config(config);
    Ok(())
}

#[tauri::command]
fn get_config_path(app: State<'_, AppState>) -> String {
    app.config_path.display().to_string()
}

#[tauri::command]
fn go_live(app: State<'_, AppState>) -> CmdResult<()> {
    app.station.go_live().map_err(err)
}

#[tauri::command]
fn stop(app: State<'_, AppState>) {
    app.station.stop();
}

#[tauri::command]
fn set_title(app: State<'_, AppState>, title: String) {
    app.station.set_manual_title(&title);
}

#[tauri::command]
fn status(app: State<'_, AppState>) -> StationStatus {
    app.station.status()
}

#[derive(Serialize)]
struct Device {
    name: String,
    input: bool,
    output: bool,
}

#[tauri::command]
fn list_devices() -> CmdResult<Vec<Device>> {
    Ok(djcast_core::audio::list_devices()
        .map_err(err)?
        .into_iter()
        .map(|d| Device { name: d.name, input: d.is_input, output: d.is_output })
        .collect())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config_path = user_config_path();
    let config = AppConfig::load_or_default(&config_path).unwrap_or_else(|e| {
        log::error!("{e:#}; starting with defaults");
        AppConfig::default()
    });
    log::info!("config: {}", config_path.display());
    // Relative paths in the config (OBS files, recordings) live next to the config file.
    if let Some(dir) = config_path.parent() {
        let _ = std::fs::create_dir_all(dir);
        if let Err(e) = std::env::set_current_dir(dir) {
            log::warn!("cannot use {} as working folder: {e}", dir.display());
        }
    }
    let station = Station::new(config);

    tauri::Builder::default()
        .manage(AppState { station: station.clone(), config_path })
        .setup(move |app| {
            let handle = app.handle().clone();
            std::thread::Builder::new().name("status-events".into()).spawn(move || {
                loop {
                    let _ = handle.emit("status", station.status());
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
            list_devices
        ])
        .run(tauri::generate_context!())
        .expect("error while running djcast");
}
