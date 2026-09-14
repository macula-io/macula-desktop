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
    content,
    frame::{self, CallResponse},
    identity::KeyPair,
    transport::Trust,
};
use serde::Serialize;
use tauri::Emitter;
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
    /// The deterministic mesh-wide label for node_id, same derivation
    /// as every other macula tool.
    pub petname: String,
    pub connected: bool,
    pub error: Option<String>,
    /// Unix ms of the moment the link came up -- lets the UI show a
    /// live session age without any clock logic in the web layer.
    pub connected_at_ms: Option<u64>,
}

/// MeshEvent is one pub/sub delivery the mesh thread captured while the
/// app holds a subscription: surfaced in the chat UI and fed into the
/// agent's grounding.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshEvent {
    pub topic: String,
    pub payload: String,
    pub publisher: String,
    pub seq: u64,
}

/// RosterEntry is one agent the desktop has heard a heartbeat from.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterEntry {
    pub node_id: String,
    pub petname: String,
    pub operator_name: String,
    pub model: String,
    pub last_seen_ms: u64,
    pub is_self: bool,
}

/// HELLO_TOPIC is the shared presence channel every macula tool beats on.
const HELLO_TOPIC: &str = "agent.hello";

/// LOBBY_TOPIC is central: public room announcements land here.
const LOBBY_TOPIC: &str = "agents.lobby";

/// Roster staleness: an agent unheard from for this long is pruned.
const ROSTER_TTL_MS: u64 = 15 * 60 * 1000;

/// PublicRoom is one room announced on central.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicRoom {
    pub topic: String,
    pub purpose: String,
    pub opened_by_petname: String,
    pub seen_at_ms: u64,
}

/// JoinedRoom is a room this app subscribed to, with its recent
/// messages.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinedRoom {
    pub topic: String,
    pub purpose: String,
    pub messages: Vec<MeshEvent>,
}

/// MeshCommand is a request the rest of the app can send into the mesh
/// thread: the thread owns the Session, commands travel over a channel.
pub enum MeshCommand {
    /// Call a procedure on the mesh, JSON args in, JSON result out,
    /// in the given realm.
    Call {
        procedure: String,
        args_json: String,
        realm: [u8; 32],
    },
    /// Publish a fact to a topic (realm = the zero realm).
    Publish { topic: String, payload_json: String },
    /// Subscribe to a topic; deliveries arrive as MeshEvents.
    Subscribe { topic: String },
    /// Stop receiving a topic's deliveries.
    Unsubscribe { topic: String },
    /// Join a room: subscribe to its topic and track it locally.
    JoinRoom { topic: String, purpose: String },
    /// Leave a room: unsubscribe and drop local tracking.
    LeaveRoom { topic: String },
    /// Switch the operating realm: resubscribes presence and the
    /// lobby under the new tag and clears realm-scoped state.
    SetRealm { tag: [u8; 32] },
    /// Store bytes as content; returns the MCID hex.
    ContentPut { data_b64: String, name: String },
    /// Fetch content by MCID hex; returns it as text when it is text.
    ContentGet { mcid_hex: String },
}

/// MeshLink is the tauri-managed handle to the background mesh thread.
pub struct MeshLink {
    status: Arc<Mutex<Status>>,
    tx: mpsc::Sender<(MeshCommand, oneshot::Sender<Result<String, String>>)>,
    /// Subscribed topics' deliveries, newest last, capped: the chat
    /// module grounds the agent in them and the UI shows them. Presence
    /// traffic never lands here.
    events: std::sync::Arc<Mutex<std::collections::VecDeque<MeshEvent>>>,
    /// The roster: every agent.hello heard, keyed by node_id, pruned on
    /// read. Written by the mesh thread only.
    roster: std::sync::Arc<Mutex<std::collections::HashMap<String, RosterEntry>>>,
    /// Public rooms announced on central, keyed by topic.
    public_rooms: std::sync::Arc<Mutex<std::collections::HashMap<String, PublicRoom>>>,
    /// Rooms this app joined, keyed by topic, with recent messages.
    joined_rooms: std::sync::Arc<Mutex<std::collections::HashMap<String, JoinedRoom>>>,
    /// help broadcasts from the lobby, newest last, capped.
    help: std::sync::Arc<Mutex<std::collections::VecDeque<MeshEvent>>>,
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

    /// recent_events returns the captured deliveries, oldest first.
    pub fn recent_events(&self) -> Vec<MeshEvent> {
        self.events
            .lock()
            .expect("mesh events lock")
            .iter()
            .cloned()
            .collect()
    }

    /// roster returns the presence list, most recently seen first, with
    /// this app's own node marked. Stale entries are pruned on read.
    pub fn roster(&self) -> Vec<RosterEntry> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let own_id = self.snapshot().node_id;
        let mut entries: Vec<RosterEntry> = self
            .roster
            .lock()
            .expect("roster lock")
            .values()
            .filter(|e| now.saturating_sub(e.last_seen_ms) < ROSTER_TTL_MS)
            .cloned()
            .map(|mut e| {
                e.is_self = e.node_id == own_id;
                e
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.last_seen_ms));
        entries
    }

    /// public_rooms returns the rooms announced on central, newest
    /// first.
    pub fn public_rooms(&self) -> Vec<PublicRoom> {
        let mut rooms: Vec<PublicRoom> = self
            .public_rooms
            .lock()
            .expect("public rooms lock")
            .values()
            .cloned()
            .collect();
        rooms.sort_by_key(|r| std::cmp::Reverse(r.seen_at_ms));
        rooms
    }

    /// joined_rooms returns this app's joined rooms with their recent
    /// messages, oldest first per room.
    pub fn joined_rooms(&self) -> Vec<JoinedRoom> {
        let mut rooms: Vec<JoinedRoom> = self
            .joined_rooms
            .lock()
            .expect("joined rooms lock")
            .values()
            .cloned()
            .collect();
        rooms.sort_by(|a, b| a.topic.cmp(&b.topic));
        rooms
    }

    /// snapshot is the current mesh state, shared with the chat module
    /// so the agent can be grounded in the live link.
    pub fn snapshot(&self) -> Status {
        self.status.lock().expect("mesh status lock").clone()
    }

    /// spawn starts the mesh thread: generate a puzzle-hardened
    /// identity, connect to the station, then serve commands for the
    /// app's lifetime. Dropping the session (when the process exits)
    /// closes the connection. `app` is only used to surface event
    /// deliveries in the UI.
    pub fn spawn(station: String, app: tauri::AppHandle, initial_realm: [u8; 32]) -> Self {
        let status = Arc::new(Mutex::new(Status {
            station: station.clone(),
            identity_generated: false,
            node_id: String::new(),
            petname: String::new(),
            connected: false,
            error: None,
            connected_at_ms: None,
        }));
        let (tx, mut rx) =
            mpsc::channel::<(MeshCommand, oneshot::Sender<Result<String, String>>)>(16);
        let thread_status = status.clone();
        let events: std::sync::Arc<Mutex<std::collections::VecDeque<MeshEvent>>> =
            std::sync::Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let thread_events = events.clone();
        let roster: std::sync::Arc<Mutex<std::collections::HashMap<String, RosterEntry>>> =
            std::sync::Arc::new(Mutex::new(std::collections::HashMap::new()));
        let thread_roster = roster.clone();
        let public_rooms: std::sync::Arc<Mutex<std::collections::HashMap<String, PublicRoom>>> =
            std::sync::Arc::new(Mutex::new(std::collections::HashMap::new()));
        let thread_public_rooms = public_rooms.clone();
        let joined_rooms: std::sync::Arc<Mutex<std::collections::HashMap<String, JoinedRoom>>> =
            std::sync::Arc::new(Mutex::new(std::collections::HashMap::new()));
        let thread_joined_rooms = joined_rooms.clone();
        let help: std::sync::Arc<Mutex<std::collections::VecDeque<MeshEvent>>> =
            std::sync::Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let thread_help = help.clone();
        std::thread::spawn(move || {
            let runtime =
                tokio::runtime::Runtime::new().expect("build tokio runtime for the mesh link");
            runtime.block_on(async move {
                // Puzzle-hardened identities are required: an unhardened
                // one fails the handshake silently (HELLO never accepts).
                let identity = KeyPair::generate_with_default_puzzle();
                {
                    let mut s = thread_status.lock().expect("mesh status lock");
                    s.identity_generated = true;
                    s.node_id = hex(&identity.node_id());
                    s.petname = crate::petname::petname(&s.node_id);
                }
                let current: std::sync::Arc<Mutex<[u8; 32]>> =
                    std::sync::Arc::new(Mutex::new(initial_realm));
                let own_node = hex(&identity.node_id());
                let own_petname = crate::petname::petname(&own_node);

                // The link is reconnect-capable: the operating realm is
                // claimed AT CONNECT TIME (pub/sub delivery is
                // realm-scoped at the connection level), so a realm
                // switch drops the session and dials again with the new
                // membership. Each generation owns its session and its
                // subscriptions; the stores survive across generations
                // except the realm-scoped ones, which the switch clears.
                'outer: loop {
                    let realm = *current.lock().expect("realm lock");
                    let mut session =
                        match connection::connect_in_realm(&station, 4433, Trust::WebPki, &identity, &[realm])
                            .await
                        {
                            Ok(session) => {
                                {
                                    let mut s = thread_status.lock().expect("mesh status lock");
                                    s.connected = true;
                                    s.error = None;
                                    s.connected_at_ms = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .ok()
                                        .map(|d| d.as_millis() as u64);
                                }
                                session
                            }
                            Err(e) => {
                                let mut s = thread_status.lock().expect("mesh status lock");
                                s.connected = false;
                                s.error = Some(e.to_string());
                                return;
                            }
                        };

                    // One event channel for every subscription: hello,
                    // the lobby, and the rooms wildcard (the station's
                    // wildcard rule matches agents.room.* with one
                    // subscription; route_event filters to JOINED rooms
                    // locally). Agent-tool subscriptions spawn their own
                    // forwarding tasks on demand.
                    let (ev_tx, mut ev_rx) = mpsc::channel::<frame::EventInfo>(512);
                    // The entire social layer -- presence, central,
                    // rooms -- lives on the zero realm (macula-mcp
                    // publishes and receives it all there; the stations
                    // rebroadcast hellos and the lobby is "the one topic
                    // everyone keeps watching"). The operating realm
                    // gates memory and realm-scoped services, not the
                    // social layer.
                    let core_topics = [
                        (HELLO_TOPIC, [0u8; 32]),
                        (LOBBY_TOPIC, [0u8; 32]),
                        ("agents.room.*", [0u8; 32]),
                    ];
                    for (topic, topic_realm) in core_topics {
                        let spec =
                            frame::SubscribeSpec::new(topic, topic_realm, identity.node_id());
                        if let Ok(sub) = session.subscribe(&spec, &identity).await {
                            let tx = ev_tx.clone();
                            tokio::spawn(drain_subscription(sub, tx));
                        }
                    }
                    let mut agent_subs: std::collections::HashMap<
                        String,
                        tokio::task::JoinHandle<()>,
                    > = std::collections::HashMap::new();

                    let mut seq: u64 = 0;
                    let mut heartbeat = tokio::time::interval(Duration::from_secs(60));
                    loop {
                        tokio::select! {
                            Some((cmd, reply_tx)) = rx.recv() => {
                                let result = match cmd {
                                    MeshCommand::Call { procedure, args_json, realm } => {
                                        mesh_call(&mut session, &identity, &procedure, &args_json, realm).await
                                    }
                                    MeshCommand::Publish { topic, payload_json } => {
                                        seq += 1;
                                        mesh_publish(&mut session, &identity, &topic, &payload_json, seq).await
                                    }
                                    MeshCommand::Subscribe { topic } => {
                                        let spec = frame::SubscribeSpec::new(&topic, realm, identity.node_id());
                                        match session.subscribe(&spec, &identity).await {
                                            Ok(sub) => {
                                                let tx = ev_tx.clone();
                                                agent_subs.insert(topic.clone(), tokio::spawn(drain_subscription(sub, tx)));
                                                Ok(format!("subscribed to {topic}"))
                                            }
                                            Err(e) => Err(format!("subscribe failed: {e}")),
                                        }
                                    }
                                    MeshCommand::Unsubscribe { topic } => {
                                        if let Some(handle) = agent_subs.remove(&topic) {
                                            handle.abort();
                                        }
                                        Ok(format!("unsubscribed from {topic}"))
                                    }
                                    MeshCommand::JoinRoom { topic, purpose } => {
                                        thread_joined_rooms.lock().expect("joined rooms lock").entry(topic.clone()).or_insert(JoinedRoom {
                                            topic: topic.clone(),
                                            purpose,
                                            messages: Vec::new(),
                                        });
                                        Ok(format!("joined room {topic}"))
                                    }
                                    MeshCommand::LeaveRoom { topic } => {
                                        thread_joined_rooms.lock().expect("joined rooms lock").remove(&topic);
                                        Ok(format!("left room {topic}"))
                                    }
                                    MeshCommand::SetRealm { tag } => {
                                        *current.lock().expect("realm lock") = tag;
                                        thread_roster.lock().expect("roster lock").clear();
                                        thread_public_rooms.lock().expect("public rooms lock").clear();
                                        thread_joined_rooms.lock().expect("joined rooms lock").clear();
                                        thread_events.lock().expect("mesh events lock").clear();
                                        thread_help.lock().expect("help lock").clear();
                                        emit_board_changed(&app);
                                        let _ = reply_tx.send(Ok(format!("operating realm switched (tag {})", hex(&tag))));
                                        break; // reconnect under the new realm
                                    }
                                    MeshCommand::ContentPut { data_b64, name } => {
                                        mesh_content_put(&mut session, &identity, &data_b64, &name).await
                                    }
                                    MeshCommand::ContentGet { mcid_hex } => {
                                        mesh_content_get(&mut session, &identity, &mcid_hex).await
                                    }
                                };
                                let _ = reply_tx.send(result);
                            }
                            _ = heartbeat.tick() => {
                                seq += 1;
                                let now_ms = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0);
                                let payload = json_to_cbor(Some(serde_json::json!({
                                    "node_id": own_node,
                                    "citizen_did": own_node,
                                    "petname": own_petname,
                                    "connected_via": "macula-desktop",
                                    "interval_seconds": 60,
                                    "at": now_ms,
                                })))
                                .unwrap_or(Value::Null);
                                let spec = frame::PublishSpec::new(
                                    HELLO_TOPIC, [0u8; 32], identity.node_id(), seq, payload, now_ms,
                                );
                                let _ = session.publish(&spec, &identity).await;
                            }
                            delivery = ev_rx.recv() => {
                                if let Some(info) = delivery {
                                    let stores = EventStores {
                                        roster: thread_roster.clone(),
                                        public_rooms: thread_public_rooms.clone(),
                                        joined: thread_joined_rooms.clone(),
                                        events: thread_events.clone(),
                                        help: thread_help.clone(),
                                    };
                                    route_event(&info, &app, &own_node, &stores);
                                }
                            }
                            else => break 'outer,
                        }
                    }
                    // The session drops here; a SetRealm break re-enters
                    // the outer loop and reconnects under the new realm.
                }
            });
        });
        MeshLink {
            status,
            tx,
            events,
            roster,
            public_rooms,
            joined_rooms,
            help,
        }
    }
}

/// EventStores bundles the mesh thread's realm-scoped stores so the
/// routing functions take one context instead of five arguments.
pub struct EventStores {
    pub roster: std::sync::Arc<Mutex<std::collections::HashMap<String, RosterEntry>>>,
    pub public_rooms: std::sync::Arc<Mutex<std::collections::HashMap<String, PublicRoom>>>,
    pub joined: std::sync::Arc<Mutex<std::collections::HashMap<String, JoinedRoom>>>,
    pub events: std::sync::Arc<Mutex<std::collections::VecDeque<MeshEvent>>>,
    pub help: std::sync::Arc<Mutex<std::collections::VecDeque<MeshEvent>>>,
}

/// route_event sends one delivery to its one true store: lobby
/// announcements to public rooms, room traffic to joined rooms,
/// heartbeats to the roster, everything else to the chat feed.
fn route_event(
    info: &frame::EventInfo,
    app: &tauri::AppHandle,
    own_node: &str,
    stores: &EventStores,
) {
    let publisher = hex(&info.publisher);
    if info.topic == LOBBY_TOPIC {
        note_lobby(info, &publisher, &stores.public_rooms, &stores.help);
        emit_board_changed(app);
        return;
    }
    if info.topic.starts_with("agents.room.") {
        note_room_message(info, &publisher, &stores.joined);
        emit_board_changed(app);
        return;
    }
    if info.topic == HELLO_TOPIC {
        note_roster_entry(info, &publisher, own_node, &stores.roster);
        emit_board_changed(app);
        return;
    }
    note_chat_event(info, &publisher, &stores.events, app);
}

/// emit_board_changed tells the webview that a board store changed --
/// the UI refreshes the visible pane, no polling anywhere.
fn emit_board_changed(app: &tauri::AppHandle) {
    let _ = app.emit("mesh-board", ());
}

/// note_lobby handles central traffic: room_opened announcements go to
/// the public-rooms list, help broadcasts to the help store.
fn note_lobby(
    info: &frame::EventInfo,
    publisher: &str,
    public_rooms: &std::sync::Arc<Mutex<std::collections::HashMap<String, PublicRoom>>>,
    help: &std::sync::Arc<Mutex<std::collections::VecDeque<MeshEvent>>>,
) {
    let payload = event_json(info);
    let kind = payload["kind"].as_str().unwrap_or("");
    eprintln!(
        "note_lobby: kind={kind:?} payload={}",
        event_payload(info).chars().take(120).collect::<String>()
    );
    if kind == "room_opened" {
        let topic = payload["room_topic"].as_str().unwrap_or("").to_string();
        if topic.is_empty() {
            return;
        }
        public_rooms.lock().expect("public rooms lock").insert(
            topic.clone(),
            PublicRoom {
                purpose: payload["purpose"].as_str().unwrap_or("").to_string(),
                topic,
                opened_by_petname: crate::petname::petname(publisher),
                seen_at_ms: now_ms(),
            },
        );
        return;
    }
    if kind == "help_requested" || kind == "help_offered" {
        let event = MeshEvent {
            topic: info.topic.clone(),
            payload: event_payload(info),
            publisher: publisher.to_string(),
            seq: info.seq,
        };
        let mut q = help.lock().expect("help lock");
        q.push_back(event);
        while q.len() > 20 {
            q.pop_front();
        }
    }
}

/// note_room_message appends one delivery to its joined room.
fn note_room_message(
    info: &frame::EventInfo,
    publisher: &str,
    joined: &std::sync::Arc<Mutex<std::collections::HashMap<String, JoinedRoom>>>,
) {
    let mut rooms = joined.lock().expect("joined rooms lock");
    let Some(room) = rooms.get_mut(&info.topic) else {
        return;
    };
    room.messages.push(MeshEvent {
        topic: info.topic.clone(),
        payload: event_payload(info),
        publisher: publisher.to_string(),
        seq: info.seq,
    });
    while room.messages.len() > 50 {
        room.messages.remove(0);
    }
}

/// note_roster_entry folds one heartbeat into the presence roster.
fn note_roster_entry(
    info: &frame::EventInfo,
    publisher: &str,
    own_node: &str,
    roster: &std::sync::Arc<Mutex<std::collections::HashMap<String, RosterEntry>>>,
) {
    let payload = event_json(info);
    roster.lock().expect("roster lock").insert(
        publisher.to_string(),
        RosterEntry {
            node_id: publisher.to_string(),
            petname: crate::petname::petname(publisher),
            operator_name: payload["operator_name"].as_str().unwrap_or("").to_string(),
            model: payload["model"].as_str().unwrap_or("").to_string(),
            last_seen_ms: now_ms(),
            is_self: publisher == own_node,
        },
    );
}

/// note_chat_event buffers one delivery for the chat feed and wakes the
/// auto-react policy.
fn note_chat_event(
    info: &frame::EventInfo,
    publisher: &str,
    events: &std::sync::Arc<Mutex<std::collections::VecDeque<MeshEvent>>>,
    app: &tauri::AppHandle,
) {
    let event = MeshEvent {
        topic: info.topic.clone(),
        payload: event_payload(info),
        publisher: publisher.to_string(),
        seq: info.seq,
    };
    {
        let mut q = events.lock().expect("mesh events lock");
        q.push_back(event.clone());
        while q.len() > 50 {
            q.pop_front();
        }
    }
    let _ = app.emit(
        "mesh-event",
        serde_json::json!({
            "topic": event.topic,
            "payload": event.payload,
            "publisher": event.publisher,
            "publisher_petname": crate::petname::petname(&event.publisher),
            "seq": event.seq,
        }),
    );
    crate::chat::maybe_react(app);
}

/// event_payload renders a delivery's payload as a JSON string.
fn event_payload(info: &frame::EventInfo) -> String {
    cbor_to_json(&info.payload).unwrap_or_else(|_| "{}".to_string())
}

/// event_json parses a delivery's payload as a JSON value.
fn event_json(info: &frame::EventInfo) -> serde_json::Value {
    serde_json::from_str(&event_payload(info)).unwrap_or(serde_json::Value::Null)
}

/// now_ms is the current epoch time in milliseconds.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// drain_subscription forwards one subscription's deliveries into the
/// thread's single event channel until the subscription ends.
async fn drain_subscription(mut sub: connection::Subscription, tx: mpsc::Sender<frame::EventInfo>) {
    while let Ok(event) = sub.recv_event(Duration::from_secs(3600)).await {
        if tx.send(event).await.is_err() {
            return;
        }
    }
}

/// mesh_publish sends one fact to a topic. Fire-and-forget by protocol:
/// success means the frame went out signed, not that anyone heard.
async fn mesh_publish(
    session: &mut Session,
    identity: &KeyPair,
    topic: &str,
    payload_json: &str,
    seq: u64,
) -> Result<String, String> {
    if topic.trim().is_empty() {
        return Err("publish requires a topic".to_string());
    }
    let payload = json_to_cbor(serde_json::from_str::<serde_json::Value>(payload_json).ok())
        .map_err(|e| format!("invalid payload: {e}"))?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let spec = frame::PublishSpec::new(topic, [0u8; 32], identity.node_id(), seq, payload, now_ms);
    session
        .publish(&spec, identity)
        .await
        .map_err(|e| format!("publish failed: {e}"))?;
    Ok(format!("published to {topic} (seq {seq})"))
}

/// mesh_content_put stores bytes as content and returns the MCID.
async fn mesh_content_put(
    session: &mut Session,
    identity: &KeyPair,
    data_b64: &str,
    name: &str,
) -> Result<String, String> {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|e| format!("bad base64: {e}"))?;
    let mcid = content::put(session, &data, name, identity)
        .await
        .map_err(|e| format!("content put failed: {e}"))?;
    Ok(mcid.iter().map(|b| format!("{b:02x}")).collect())
}

/// mesh_content_get fetches content by MCID hex; text comes back as
/// text, anything else reports its size.
async fn mesh_content_get(
    session: &mut Session,
    identity: &KeyPair,
    mcid_hex: &str,
) -> Result<String, String> {
    let bytes = hex_decode(mcid_hex)?;
    let mut mcid = [0u8; 34];
    if bytes.len() != 34 {
        return Err("an MCID is 34 bytes (68 hex chars)".to_string());
    }
    mcid.copy_from_slice(&bytes);
    let data = content::get(session, mcid, identity)
        .await
        .map_err(|e| format!("content get failed: {e}"))?;
    match String::from_utf8(data) {
        Ok(text) => Ok(text),
        Err(e) => {
            let n = e.into_bytes().len();
            Ok(format!("binary content, {n} bytes"))
        }
    }
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return Err("hex string must have even length".to_string());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("bad hex: {e}")))
        .collect()
}

/// mesh_call invokes one procedure through the live session: JSON args
/// in, JSON result (or a BOLT error) out, 30s deadline, in the given
/// realm.
async fn mesh_call(
    session: &mut Session,
    identity: &KeyPair,
    procedure: &str,
    args_json: &str,
    realm: [u8; 32],
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
        .call(
            procedure,
            realm,
            payload,
            deadline,
            identity,
            Duration::from_secs(30),
        )
        .await
    {
        Ok(CallResponse::Result { payload, .. }) => {
            cbor_to_json(&payload).map_err(|e| format!("decode result: {e}"))
        }
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
            Value::Bytes(b) => {
                serde_json::Value::String(b.iter().map(|x| format!("{x:02x}")).collect())
            }
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

/// teams_board_command derives the team board from the joined rooms
/// and the lobby's help broadcasts.
#[tauri::command]
pub fn teams_board_command(link: tauri::State<'_, MeshLink>) -> crate::teams::TeamBoard {
    let joined = link.joined_rooms();
    let help: Vec<MeshEvent> = link
        .help
        .lock()
        .expect("help lock")
        .iter()
        .cloned()
        .collect();
    crate::teams::board(&joined, &help)
}

/// roster is the presence list, most recently seen first, self marked.
#[tauri::command]
pub fn roster(link: tauri::State<'_, MeshLink>) -> Vec<RosterEntry> {
    link.roster()
}

/// public_rooms_command lists the rooms announced on central.
#[tauri::command]
pub fn public_rooms_command(link: tauri::State<'_, MeshLink>) -> Vec<PublicRoom> {
    link.public_rooms()
}

/// joined_rooms_command lists this app's joined rooms with messages.
#[tauri::command]
pub fn joined_rooms_command(link: tauri::State<'_, MeshLink>) -> Vec<JoinedRoom> {
    link.joined_rooms()
}

/// join_room subscribes the session to a room topic and tracks it.
#[tauri::command]
pub async fn join_room(
    link: tauri::State<'_, MeshLink>,
    topic: String,
    purpose: String,
) -> Result<(), String> {
    link.request(MeshCommand::JoinRoom { topic, purpose })
        .await
        .map(|_| ())
}

/// leave_room unsubscribes and drops local tracking.
#[tauri::command]
pub async fn leave_room(link: tauri::State<'_, MeshLink>, topic: String) -> Result<(), String> {
    link.request(MeshCommand::LeaveRoom { topic })
        .await
        .map(|_| ())
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
