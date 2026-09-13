//! The in-app mesh daemon: one background thread holding the macula-rust
//! SDK connection for the app's whole lifetime, exposing its state to
//! the webview through the `mesh_status` command.
//!
//! The session is held deliberately: dropping the last Session handle
//! closes the connection at once (see the SDK's own Session doc). The
//! thread parks its runtime on a never-ready future so the session
//! stays alive until the process exits.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use macula_rust::{
    connection,
    identity::KeyPair,
    transport::Trust,
};
use serde::Serialize;

/// Status is the whole mesh state the walking skeleton reports: the
/// identity the app generated, whether the station link is up, and the
/// honest error when it is not. No placeholders: every field is real
/// state from the SDK.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub station: String,
    pub identity_generated: bool,
    pub node_id: String,
    pub connected: bool,
    pub error: Option<String>,
    /// Unix ms of the moment the link came up -- lets the UI show a
    /// live session age without any clock logic in the web layer.
    pub connected_at_ms: Option<u64>,
}

/// MeshLink is the tauri-managed handle to the background mesh thread.
pub struct MeshLink {
    status: Arc<Mutex<Status>>,
}

impl MeshLink {
    /// spawn starts the mesh thread: generate a puzzle-hardened
    /// identity, connect to the station, and hold the session.
    pub fn spawn(station: String) -> Self {
        let status = Arc::new(Mutex::new(Status {
            station: station.clone(),
            identity_generated: false,
            node_id: String::new(),
            connected: false,
            error: None,
            connected_at_ms: None,
        }));
        let thread_status = status.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime for the mesh link");
            runtime.block_on(async move {
                // Puzzle-hardened identities are required: an unhardened
                // one fails the handshake silently (HELLO never accepts).
                let identity = KeyPair::generate_with_default_puzzle();
                {
                    let mut s = thread_status.lock().expect("mesh status lock");
                    s.identity_generated = true;
                    s.node_id = hex(&identity.node_id());
                }
                match connection::connect(&station, 4433, Trust::WebPki, &identity).await {
                    Ok(session) => {
                        {
                            let mut s = thread_status.lock().expect("mesh status lock");
                            s.connected = true;
                            s.connected_at_ms = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .ok()
                                .map(|d| d.as_millis() as u64);
                        }
                        // Hold the session forever: park on a pending
                        // future until the process exits.
                        std::future::pending::<()>().await;
                        let _held = session;
                    }
                    Err(e) => {
                        let mut s = thread_status.lock().expect("mesh status lock");
                        s.error = Some(e.to_string());
                    }
                }
            });
        });
        MeshLink { status }
    }
}

/// mesh_status is the IPC command the webview polls: the current mesh
/// state, from the Rust core's own connection -- never a network call
/// from the web layer.
#[tauri::command]
pub fn mesh_status(link: tauri::State<'_, MeshLink>) -> Status {
    link.status.lock().expect("mesh status lock").clone()
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
