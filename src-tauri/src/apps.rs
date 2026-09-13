//! Applications: the LAN and mesh faces of hecate services, declared
//! and managed by the operator through the app itself. The webview
//! still never touches the network: clicking a local application hands
//! its URL to the OPERATING SYSTEM's browser (or embeds it, origin-
//! isolated), and mesh applications resolve through the mesh once the
//! DHT-discovery slice lands.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// LocalApp is one LAN application entry: what it is and where its
/// admin UI lives. `sandboxed` embeds the UI in a sandboxed frame --
/// WebKit gives such frames an ephemeral session, so a sandboxed app
/// cannot keep data between sessions. LAN apps are operator-trusted by
/// default (persistent), the toggle exists for the ones that are not.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApp {
    pub name: String,
    pub description: String,
    pub url: String,
    #[serde(default)]
    pub sandboxed: bool,
}

/// MeshApp is one mesh application entry: what it is and the MRI it
/// will resolve through once mesh resolution ships. Mesh apps are
/// remote and therefore NOT trusted by default: sandboxed on.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshApp {
    pub name: String,
    pub description: String,
    pub mri: String,
    #[serde(default = "sandboxed_default")]
    pub sandboxed: bool,
}

fn sandboxed_default() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppsFile {
    #[serde(default)]
    local: Vec<LocalApp>,
    #[serde(default)]
    mesh: Vec<MeshApp>,
}

/// AppsConfig is what the webview reads and writes: both lists.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppsConfig {
    pub local: Vec<LocalApp>,
    pub mesh: Vec<MeshApp>,
}

/// config_path is where applications live:
/// ~/.config/macula-desktop/apps.json on Linux/macOS,
/// %LOCALAPPDATA%\macula-desktop\apps.json on Windows.
fn config_path() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .or_else(|| {
                std::env::var_os("HOME").map(|h| {
                    let mut p = PathBuf::from(&h);
                    p.push(".config");
                    p.into_os_string()
                })
            })
    }?;
    let mut path = PathBuf::from(dir);
    path.push("macula-desktop");
    path.push("apps.json");
    Some(path)
}

/// config_path_display is the same path, stringified for the UI.
#[tauri::command]
pub fn config_path_display() -> String {
    config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "apps.json".to_string())
}

/// apps_config returns the current configuration. A missing or
/// unreadable file is an empty configuration, not an error -- the UI's
/// empty state explains where the file lives.
#[tauri::command]
pub fn apps_config() -> AppsConfig {
    let Some(path) = config_path() else {
        return AppsConfig {
            local: Vec::new(),
            mesh: Vec::new(),
        };
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return AppsConfig {
            local: Vec::new(),
            mesh: Vec::new(),
        };
    };
    serde_json::from_str::<AppsFile>(&text)
        .map(|f| AppsConfig {
            local: f.local,
            mesh: f.mesh,
        })
        .unwrap_or(AppsConfig {
            local: Vec::new(),
            mesh: Vec::new(),
        })
}

/// save_apps persists the configuration, validating before writing:
/// every local URL must be http(s), every mesh MRI must be an MRI, and
/// names may not be empty. The write is atomic (temp file + rename) so
/// a crash can never leave a half-written config.
#[tauri::command]
pub fn save_apps(config: AppsConfig) -> Result<(), String> {
    for app in &config.local {
        if app.name.trim().is_empty() {
            return Err("a local application has an empty name".to_string());
        }
        let lower = app.url.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return Err(format!(
                "local application {}: URL must be http(s), got {}",
                app.name, app.url
            ));
        }
    }
    for app in &config.mesh {
        if app.name.trim().is_empty() {
            return Err("a mesh application has an empty name".to_string());
        }
        if !app.mri.starts_with("mri:") {
            return Err(format!(
                "mesh application {}: {} is not an MRI (expected mri:type:realm/path)",
                app.name, app.mri
            ));
        }
    }

    let Some(path) = config_path() else {
        return Err("no config directory could be determined".to_string());
    };
    let dir = path.parent().expect("config path has a parent");
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("create config directory {}: {e}", dir.display()))?;

    let text = serde_json::to_string_pretty(&AppsFile {
        local: config.local,
        mesh: config.mesh,
    })
    .map_err(|e| format!("encode config: {e}"))?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write config: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("commit config: {e}"))?;
    Ok(())
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
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .status()
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
