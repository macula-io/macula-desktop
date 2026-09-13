//! macula-desktop: a Tauri shell whose Rust core IS the mesh client,
//! built on the published macula-rust SDK crate. The webview never
//! touches the network -- every fact on screen arrives through a
//! registered `invoke` command (no TCP listener of any kind; see the
//! exploration doc's IPC security posture).
//!
//! Walking-skeleton scope (2026-09-13): the shell renders its five tabs,
//! the IPC round-trip works, and the Rust core really connects to the
//! mesh -- identity, handshake, and a live session held for the app's
//! lifetime -- reporting its own node_id and connection state to the
//! window. Chat/teams/services/realms content comes next; their tabs
//! state that honestly.

mod mesh;

use tauri::Manager;

/// run starts the application.
pub fn run() {
    let mesh_link = mesh::MeshLink::spawn(
        "station-de-frankfurt.macula.io".to_string(),
    );
    tauri::Builder::default()
        .setup(move |app| {
            app.manage(mesh_link);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![mesh::mesh_status])
        .run(tauri::generate_context!())
        .expect("error while running macula-desktop");
}
