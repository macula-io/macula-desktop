//! Local applications: the LAN face of hecate services, configured by
//! the operator in a small JSON file. The webview still never touches
//! the network: clicking an application hands the URL to the OPERATING
//! SYSTEM's browser through a registered command, and the main window
//! keeps its IPC-only posture (see the exploration doc's security
//! carve-out).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// LocalApp is one LAN application entry: what it is and where its
/// admin UI lives.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApp {
    pub name: String,
    pub description: String,
    pub url: String,
}

#[derive(Deserialize)]
struct AppsFile {
    #[serde(default)]
    local: Vec<LocalApp>,
}

/// config_path is where the operator declares local applications:
/// ~/.config/macula-desktop/apps.json on Linux/macOS,
/// %LOCALAPPDATA%\macula-desktop\apps.json on Windows.
fn config_path() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var_os("HOME").map(|h| {
                let mut p = PathBuf::from(&h);
                p.push(".config");
                p.into_os_string()
            }))
    }?;
    let mut path = PathBuf::from(dir);
    path.push("macula-desktop");
    path.push("apps.json");
    Some(path)
}

/// local_apps returns the configured LAN applications. A missing or
/// unreadable config is an empty list, not an error -- the UI's empty
/// state explains where the file lives.
#[tauri::command]
pub fn local_apps() -> Vec<LocalApp> {
    let Some(path) = config_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<AppsFile>(&text)
        .map(|f| f.local)
        .unwrap_or_default()
}

/// config_path_display is the same path, stringified for the UI's
/// empty state.
#[tauri::command]
pub fn config_path_display() -> String {
    config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "apps.json".to_string())
}

/// open_external hands a URL to the operating system's default
/// browser. Only http/https ever leaves this app; everything else is
/// refused before anything is spawned.
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(format!("refusing to open a non-http URL: {url}"));
    }
    let status = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", &url]).status()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&url).status()
    } else {
        std::process::Command::new("xdg-open").arg(&url).status()
    };
    status
        .map(|s| {
            if s.success() {
                Ok(())
            } else {
                Err(format!("browser exited with {s}"))
            }
        })
        .map_err(|e| format!("failed to launch the system browser: {e}"))?
}
