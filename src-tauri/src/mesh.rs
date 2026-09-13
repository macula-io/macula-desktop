//! The in-app mesh daemon: one background thread holding the macula-rust
//! SDK connection for the app's whole lifetime, exposing its state to
//! the webview through the `mesh_status` command.
//!
//! The session is held deliberately: dropping the last Session handle
//! closes the connection at once (see the SDK's own Session doc). The
//! thread parks its runtime on a never-ready future so the session
//! stays alive until the process exits.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use macula_rust::{
    cbor::Value,
    connection::{self, Session},
    frame::CallResponse,
    identity::KeyPair,
    transport::Trust,
};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

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

/// MeshCommand is a request the rest of the app can send into the mesh
/// thread: the thread owns the Session, commands travel over a channel.
pub enum MeshCommand {
    /// Call a procedure on the mesh, JSON args in, JSON result out.
    Call { procedure: String, args_json: String },
}

/// MeshLink is the tauri-managed handle to the background mesh thread.
pub struct MeshLink {
    status: Arc<Mutex<Status>>,
    tx: mpsc::Sender<(MeshCommand, oneshot::Sender<Result<String, String>>)>,
}

impl MeshLink {
    /// request sends one command into the mesh thread and waits for its
    /// answer. The thread owns the session; nobody else touches it.
    pub async fn request(&self, cmd: MeshCommand) -> Result<String, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send((cmd, reply_tx))
            .await
            .map_err(|_| "mesh link is gone".to_string())?;
        reply_rx
            .await
            .map_err(|_| "mesh link dropped the request".to_string())?
    }

    /// snapshot is the current mesh state, shared with the chat module
    /// so the agent can be grounded in the live link.
    pub fn snapshot(&self) -> Status {
        self.status.lock().expect("mesh status lock").clone()
    }

    /// spawn starts the mesh thread: generate a puzzle-hardened
    /// identity, connect to the station, then serve commands for the
    /// app's lifetime. Dropping the session (when the process exits)
    /// closes the connection.
    pub fn spawn(station: String) -> Self {
        let status = Arc::new(Mutex::new(Status {
            station: station.clone(),
            identity_generated: false,
            node_id: String::new(),
            connected: false,
            error: None,
            connected_at_ms: None,
        }));
        let (tx, mut rx) = mpsc::channel::<(MeshCommand, oneshot::Sender<Result<String, String>>)>(16);
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
                let mut session = match connection::connect(&station, 4433, Trust::WebPki, &identity).await {
                    Ok(session) => {
                        {
                            let mut s = thread_status.lock().expect("mesh status lock");
                            s.connected = true;
                            s.connected_at_ms = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .ok()
                                .map(|d| d.as_millis() as u64);
                        }
                        session
                    }
                    Err(e) => {
                        let mut s = thread_status.lock().expect("mesh status lock");
                        s.error = Some(e.to_string());
                        return;
                    }
                };

                // The session lives here, in this loop, until the process
                // exits: serve commands, one at a time, on the thread that
                // owns the connection.
                while let Some((cmd, reply_tx)) = rx.recv().await {
                    let result = match cmd {
                        MeshCommand::Call { procedure, args_json } => {
                            mesh_call(&mut session, &identity, &procedure, &args_json).await
                        }
                    };
                    let _ = reply_tx.send(result);
                }
            });
        });
        MeshLink { status, tx }
    }
}

/// mesh_call invokes one procedure through the live session: JSON args
/// in, JSON result (or a BOLT error) out, 30s deadline.
async fn mesh_call(
    session: &mut Session,
    identity: &KeyPair,
    procedure: &str,
    args_json: &str,
) -> Result<String, String> {
    let payload = match json_to_cbor(serde_json::from_str::<serde_json::Value>(args_json).ok()) {
        Ok(v) => v,
        Err(e) => return Err(format!("invalid arguments: {e}")),
    };
    let deadline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i128 + 30_000)
        .unwrap_or(30_000);
    match session
        .call(procedure, [0u8; 32], payload, deadline, identity, Duration::from_secs(30))
        .await
    {
        Ok(CallResponse::Result { payload, .. }) => cbor_to_json(&payload)
            .map_err(|e| format!("decode result: {e}")),
        Ok(CallResponse::Error { name, detail, .. }) => {
            Err(format!("{name}: {}", detail.unwrap_or_default()))
        }
        Err(e) => Err(format!("call failed: {e}")),
    }
}

/// json_to_cbor maps a parsed JSON value onto the SDK's cbor::Value so
/// procedure args can be structured, not just strings.
fn json_to_cbor(v: Option<serde_json::Value>) -> Result<Value, String> {
    match v {
        None | Some(serde_json::Value::Null) => Ok(Value::Null),
        Some(serde_json::Value::Bool(b)) => Ok(if b { Value::Int(1) } else { Value::Int(0) }),
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .map(|i| Value::Int(i as i128))
            .or_else(|| n.as_f64().map(Value::Float))
            .ok_or_else(|| "unsupported number".to_string()),
        Some(serde_json::Value::String(s)) => Ok(Value::Text(s)),
        Some(serde_json::Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(json_to_cbor(Some(item))?);
            }
            Ok(Value::List(out))
        }
        Some(serde_json::Value::Object(map)) => {
            let mut out = Vec::with_capacity(map.len());
            for (k, v) in map {
                out.push((Value::Text(k), json_to_cbor(Some(v))?));
            }
            Ok(Value::Map(out))
        }
    }
}

/// cbor_to_json renders a procedure result as JSON for the agent.
fn cbor_to_json(v: &Value) -> Result<String, String> {
    fn inner(v: &Value) -> Result<serde_json::Value, String> {
        Ok(match v {
            Value::Null => serde_json::Value::Null,
            Value::Int(i) => serde_json::Number::from_i128(*i)
                .map(serde_json::Value::Number)
                .ok_or_else(|| "integer out of JSON range".to_string())?,
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .ok_or_else(|| "non-finite float".to_string())?,
            Value::Text(s) => serde_json::Value::String(s.clone()),
            Value::Bytes(b) => serde_json::Value::String(
                b.iter().map(|x| format!("{x:02x}")).collect(),
            ),
            Value::List(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(inner(item)?);
                }
                serde_json::Value::Array(out)
            }
            Value::Map(pairs) => {
                let mut map = serde_json::Map::new();
                for (k, val) in pairs {
                    let key = match k {
                        Value::Text(s) => s.clone(),
                        Value::Int(i) => i.to_string(),
                        other => format!("{other:?}"),
                    };
                    map.insert(key, inner(val)?);
                }
                serde_json::Value::Object(map)
            }
        })
    }
    serde_json::to_string(&inner(v)?).map_err(|e| e.to_string())
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
