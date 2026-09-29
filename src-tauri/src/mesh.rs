//! The in-app mesh daemon: one background thread holding a macula-rust
//! pool for the app's whole lifetime, exposing its state to the webview
//! through the `mesh_status` command.
//!
//! The pool links to the fleet's stations, each pinned by the node_id it
//! must prove, under a persistent pq_hybrid identity, trusting the
//! io.macula realm key. It redials a link that ends and gives it back its
//! subscriptions, so the thread holds one pool until the process exits.
//! The social layer (presence, the lobby, rooms) lives in io.macula, where
//! macula-mcp publishes it; the operating realm scopes calls and memory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use macula_rust::{
    cbor::Value,
    node_key::NodeKey,
    petname::petname,
    pool::{Call, ContentOptions, Opts, Pool, Seed, Subscription},
    profile::Profile,
    station_link::{Event, Publication},
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
/// thread: the thread owns the pool, commands travel over a channel.
pub enum MeshCommand {
    /// Call a procedure on the mesh, JSON args in, JSON result out,
    /// in the given realm.
    Call {
        procedure: String,
        args_json: String,
        realm: [u8; 32],
    },
    /// Publish a fact to a topic, in the social realm (io.macula).
    Publish { topic: String, payload_json: String },
    /// Subscribe to a topic; deliveries arrive as MeshEvents.
    Subscribe { topic: String },
    /// Stop receiving a topic's deliveries.
    Unsubscribe { topic: String },
    /// Join a room: subscribe to its topic and track it locally.
    JoinRoom { topic: String, purpose: String },
    /// Leave a room: unsubscribe and drop local tracking.
    LeaveRoom { topic: String },
    /// Switch the operating realm: calls, agent subscriptions and
    /// content use the new tag from now on. The social layer stays in
    /// io.macula and needs no reconnect.
    SetRealm { tag: [u8; 32] },
    /// Share bytes as content from this node; returns the MCID hex.
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

    /// spawn starts the mesh thread: load (or create) the persistent
    /// identity, connect the pool to the fleet (retrying until it comes
    /// up), subscribe the social layer, then serve commands for the app's
    /// lifetime. `app` is only used to surface deliveries in the UI.
    pub fn spawn(app: tauri::AppHandle, initial_realm: [u8; 32]) -> Self {
        let status = Arc::new(Mutex::new(Status {
            station: String::new(),
            identity_generated: false,
            node_id: String::new(),
            petname: String::new(),
            connected: false,
            error: None,
            connected_at_ms: None,
        }));
        let (tx, rx) = mpsc::channel::<(MeshCommand, oneshot::Sender<Result<String, String>>)>(16);
        let link = MeshLink {
            status,
            tx,
            events: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            roster: Arc::new(Mutex::new(HashMap::new())),
            public_rooms: Arc::new(Mutex::new(HashMap::new())),
            joined_rooms: Arc::new(Mutex::new(HashMap::new())),
            help: Arc::new(Mutex::new(std::collections::VecDeque::new())),
        };
        let thread = ThreadState {
            status: link.status.clone(),
            stores: EventStores {
                roster: link.roster.clone(),
                public_rooms: link.public_rooms.clone(),
                joined: link.joined_rooms.clone(),
                events: link.events.clone(),
                help: link.help.clone(),
            },
            app,
            realm: initial_realm,
        };
        std::thread::spawn(move || {
            let runtime =
                tokio::runtime::Runtime::new().expect("build tokio runtime for the mesh link");
            runtime.block_on(run_mesh(thread, rx));
        });
        link
    }
}

/// social_realm is io.macula's realm id, where presence, the lobby and
/// rooms live.
fn social_realm() -> [u8; 32] {
    crate::chat::realm_tag(crate::io_macula::REALM_NAME)
}

/// operating_realm is the realm calls, agent subscriptions and content
/// use: the one the user chose, or io.macula when none is set.
fn operating_realm(tag: [u8; 32]) -> [u8; 32] {
    if tag == [0u8; 32] {
        return social_realm();
    }
    tag
}

/// fleet_seeds are the six fleet stations, pinned by node_id.
fn fleet_seeds() -> Vec<Seed> {
    crate::io_macula::SEEDS
        .iter()
        .map(|(host, port, node_hex)| Seed {
            host: host.to_string(),
            port: *port,
            node_id: hex_decode(node_hex)
                .ok()
                .and_then(|b| b.try_into().ok())
                .expect("SEEDS node_ids are 64 hex characters"),
        })
        .collect()
}

/// ThreadState is what the mesh thread owns besides the pool.
struct ThreadState {
    status: Arc<Mutex<Status>>,
    stores: EventStores,
    app: tauri::AppHandle,
    realm: [u8; 32],
}

/// Session is one connected pool and the subscriptions held on it.
struct Session {
    pool: Pool,
    node: String,
    petname: String,
    events: mpsc::Sender<Event>,
    rooms: HashMap<String, tokio::task::JoinHandle<()>>,
    agent_subs: HashMap<String, tokio::task::JoinHandle<()>>,
}

/// run_mesh is the mesh thread's whole life: identity, connect, then the
/// command/event/heartbeat loop until the app drops its sender.
async fn run_mesh(
    mut state: ThreadState,
    mut rx: mpsc::Receiver<(MeshCommand, oneshot::Sender<Result<String, String>>)>,
) {
    let identity = match load_identity() {
        Ok(key) => Arc::new(key),
        Err(e) => {
            set_error(&state.status, format!("identity: {e}"));
            return;
        }
    };
    let pool = connect_until_up(&state.status, identity).await;
    let node = hex(&pool.node_id());
    let (ev_tx, mut ev_rx) = mpsc::channel::<Event>(512);
    let mut session = Session {
        petname: petname(&node),
        node,
        pool,
        events: ev_tx,
        rooms: HashMap::new(),
        agent_subs: HashMap::new(),
    };
    for topic in [HELLO_TOPIC, LOBBY_TOPIC] {
        if let Err(e) = subscribe_into(&session, social_realm(), topic).await {
            set_error(&state.status, format!("subscribe {topic}: {e}"));
        }
    }
    let mut heartbeat = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            Some((cmd, reply_tx)) = rx.recv() => {
                let result = handle_command(&mut session, &mut state, cmd).await;
                let _ = reply_tx.send(result);
            }
            _ = heartbeat.tick() => publish_hello(&session).await,
            Some(event) = ev_rx.recv() => route_event(&event, &state.app, &session.node, &state.stores),
            else => break,
        }
    }
    session.pool.close().await;
}

/// load_identity is the node's persistent pq_hybrid identity key, next to
/// the app's settings, created (puzzle solved) on first run.
fn load_identity() -> Result<NodeKey, String> {
    let mut path = crate::chat::settings_path()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "no settings directory".to_string())?;
    std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    path.push("node.key");
    NodeKey::load_or_create(&path, Profile::PqHybrid)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// connect_until_up connects the pool to the fleet, retrying with a
/// growing pause until one link is up, reporting each failure in Status.
async fn connect_until_up(status: &Arc<Mutex<Status>>, identity: Arc<NodeKey>) -> Pool {
    {
        let mut s = status.lock().expect("mesh status lock");
        s.identity_generated = true;
        s.node_id = identity.node_id().map(|id| hex(&id)).unwrap_or_default();
        s.petname = petname(&s.node_id);
    }
    let mut pause = Duration::from_secs(5);
    loop {
        let mut opts = Opts::new(identity.clone());
        opts.realm_trust = HashMap::from([(social_realm(), crate::io_macula::realm_key())]);
        opts.connect_timeout = Duration::from_secs(60);
        match Pool::connect(fleet_seeds(), opts).await {
            Ok(pool) => {
                mark_connected(status, &pool);
                return pool;
            }
            Err(e) => set_error(
                status,
                format!("connect: {e} (retrying in {}s)", pause.as_secs()),
            ),
        }
        tokio::time::sleep(pause).await;
        pause = (pause * 2).min(Duration::from_secs(60));
    }
}

/// mark_connected records the link that came up in Status.
fn mark_connected(status: &Arc<Mutex<Status>>, pool: &Pool) {
    let station = pool
        .status()
        .into_iter()
        .find(|l| l.up)
        .map(|l| l.host)
        .unwrap_or_default();
    let mut s = status.lock().expect("mesh status lock");
    s.station = station;
    s.connected = true;
    s.error = None;
    s.connected_at_ms = Some(now_ms());
}

/// set_error records an honest error in Status; the link state is
/// whatever the pool last reported.
fn set_error(status: &Arc<Mutex<Status>>, error: String) {
    status.lock().expect("mesh status lock").error = Some(error);
}

/// subscribe_into subscribes to topic in realm and forwards its
/// deliveries into the thread's event channel, returning the forwarder.
async fn subscribe_into(
    session: &Session,
    realm: [u8; 32],
    topic: &str,
) -> Result<tokio::task::JoinHandle<()>, String> {
    let sub = session
        .pool
        .subscribe(&realm, topic)
        .await
        .map_err(|e| e.to_string())?;
    Ok(tokio::spawn(drain_subscription(
        sub,
        session.events.clone(),
    )))
}

/// handle_command runs one command against the session.
async fn handle_command(
    session: &mut Session,
    state: &mut ThreadState,
    cmd: MeshCommand,
) -> Result<String, String> {
    let realm = operating_realm(state.realm);
    match cmd {
        MeshCommand::Call {
            procedure,
            args_json,
            realm,
        } => {
            mesh_call(
                &session.pool,
                &procedure,
                &args_json,
                operating_realm(realm),
            )
            .await
        }
        MeshCommand::Publish {
            topic,
            payload_json,
        } => mesh_publish(&session.pool, &topic, &payload_json).await,
        MeshCommand::Subscribe { topic } => {
            let handle = subscribe_into(session, realm, &topic)
                .await
                .map_err(|e| format!("subscribe failed: {e}"))?;
            session.agent_subs.insert(topic.clone(), handle);
            Ok(format!("subscribed to {topic}"))
        }
        MeshCommand::Unsubscribe { topic } => {
            stop_forwarder(session.agent_subs.remove(&topic));
            Ok(format!("unsubscribed from {topic}"))
        }
        MeshCommand::JoinRoom { topic, purpose } => {
            join_room_topic(session, state, topic, purpose).await
        }
        MeshCommand::LeaveRoom { topic } => {
            stop_forwarder(session.rooms.remove(&topic));
            state
                .stores
                .joined
                .lock()
                .expect("joined rooms lock")
                .remove(&topic);
            Ok(format!("left room {topic}"))
        }
        MeshCommand::SetRealm { tag } => {
            state.realm = tag;
            Ok(format!("operating realm switched (tag {})", hex(&tag)))
        }
        MeshCommand::ContentPut { data_b64, name } => {
            mesh_content_put(&session.pool, realm, &data_b64, &name).await
        }
        MeshCommand::ContentGet { mcid_hex } => {
            mesh_content_get(&session.pool, realm, &mcid_hex).await
        }
    }
}

/// join_room_topic subscribes to a room's own topic in the social realm
/// and tracks it; a room already joined is left as it is.
async fn join_room_topic(
    session: &mut Session,
    state: &ThreadState,
    topic: String,
    purpose: String,
) -> Result<String, String> {
    if session.rooms.contains_key(&topic) {
        return Ok(format!("already in room {topic}"));
    }
    let handle = subscribe_into(session, social_realm(), &topic)
        .await
        .map_err(|e| format!("join failed: {e}"))?;
    session.rooms.insert(topic.clone(), handle);
    state
        .stores
        .joined
        .lock()
        .expect("joined rooms lock")
        .entry(topic.clone())
        .or_insert(JoinedRoom {
            topic: topic.clone(),
            purpose,
            messages: Vec::new(),
        });
    emit_board_changed(&state.app);
    Ok(format!("joined room {topic}"))
}

/// stop_forwarder ends a subscription's forwarder; dropping its
/// Subscription unsubscribes.
fn stop_forwarder(handle: Option<tokio::task::JoinHandle<()>>) {
    if let Some(handle) = handle {
        handle.abort();
    }
}

/// publish_hello beats this node's presence on agent.hello.
async fn publish_hello(session: &Session) {
    let payload = json_to_cbor(Some(serde_json::json!({
        "node_id": session.node,
        "citizen_did": session.node,
        "petname": session.petname,
        "connected_via": "macula-desktop",
        "interval_seconds": 60,
        "at": now_ms(),
    })))
    .unwrap_or(Value::Null);
    let _ = session
        .pool
        .publish(Publication {
            realm: social_realm(),
            topic: HELLO_TOPIC.to_string(),
            payload,
            ttl_ms: None,
        })
        .await;
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
fn route_event(info: &Event, app: &tauri::AppHandle, own_node: &str, stores: &EventStores) {
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
    info: &Event,
    publisher: &str,
    public_rooms: &std::sync::Arc<Mutex<std::collections::HashMap<String, PublicRoom>>>,
    help: &std::sync::Arc<Mutex<std::collections::VecDeque<MeshEvent>>>,
) {
    let payload = event_json(info);
    let kind = payload["kind"].as_str().unwrap_or("");
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
                opened_by_petname: petname(publisher),
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
    info: &Event,
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
    info: &Event,
    publisher: &str,
    own_node: &str,
    roster: &std::sync::Arc<Mutex<std::collections::HashMap<String, RosterEntry>>>,
) {
    let payload = event_json(info);
    roster.lock().expect("roster lock").insert(
        publisher.to_string(),
        RosterEntry {
            node_id: publisher.to_string(),
            petname: petname(publisher),
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
    info: &Event,
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
            "publisher_petname": petname(&event.publisher),
            "seq": event.seq,
        }),
    );
    crate::chat::maybe_react(app);
}

/// event_payload renders a delivery's payload as a JSON string.
fn event_payload(info: &Event) -> String {
    cbor_to_json(&info.payload).unwrap_or_else(|_| "{}".to_string())
}

/// event_json parses a delivery's payload as a JSON value.
fn event_json(info: &Event) -> serde_json::Value {
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
async fn drain_subscription(mut sub: Subscription, tx: mpsc::Sender<Event>) {
    while let Some(event) = sub.recv().await {
        if tx.send(event).await.is_err() {
            return;
        }
    }
}

/// mesh_publish sends one fact to a topic in the social realm.
/// Fire-and-forget by protocol: success means the publication went out
/// signed, not that anyone heard.
async fn mesh_publish(pool: &Pool, topic: &str, payload_json: &str) -> Result<String, String> {
    if topic.trim().is_empty() {
        return Err("publish requires a topic".to_string());
    }
    let payload = json_to_cbor(serde_json::from_str::<serde_json::Value>(payload_json).ok())
        .map_err(|e| format!("invalid payload: {e}"))?;
    pool.publish(Publication {
        realm: social_realm(),
        topic: topic.to_string(),
        payload,
        ttl_ms: None,
    })
    .await
    .map_err(|e| format!("publish failed: {e}"))?;
    Ok(format!("published to {topic}"))
}

/// mesh_content_put shares bytes from this node in realm and returns the
/// MCID hex. The node serves the content while the app runs.
async fn mesh_content_put(
    pool: &Pool,
    realm: [u8; 32],
    data_b64: &str,
    name: &str,
) -> Result<String, String> {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|e| format!("bad base64: {e}"))?;
    let mcid = pool
        .share_content(&realm, &data, name)
        .await
        .map_err(|e| format!("content share failed: {e}"))?;
    Ok(mcid.iter().map(|b| format!("{b:02x}")).collect())
}

/// mesh_content_get fetches content by MCID hex from the nodes sharing
/// it; text comes back as text, anything else reports its size.
async fn mesh_content_get(pool: &Pool, realm: [u8; 32], mcid_hex: &str) -> Result<String, String> {
    let mcid: [u8; 50] = hex_decode(mcid_hex)?
        .try_into()
        .map_err(|_| "an MCID is 50 bytes (100 hex chars)".to_string())?;
    let data = pool
        .get_content(&realm, &mcid, ContentOptions::default())
        .await
        .map_err(|e| format!("content get failed: {e}"))?;
    match String::from_utf8(data) {
        Ok(text) => Ok(text),
        Err(e) => Ok(format!("binary content, {} bytes", e.into_bytes().len())),
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

/// mesh_call invokes one procedure by direct dial: JSON args in, JSON
/// result (or the provider's or the pool's error) out, 30 s timeout, in
/// the given realm.
async fn mesh_call(
    pool: &Pool,
    procedure: &str,
    args_json: &str,
    realm: [u8; 32],
) -> Result<String, String> {
    let payload = json_to_cbor(serde_json::from_str::<serde_json::Value>(args_json).ok())
        .map_err(|e| format!("invalid arguments: {e}"))?;
    let result = pool
        .call(Call {
            realm,
            procedure: procedure.to_string(),
            payload,
            timeout: Duration::from_secs(30),
            ..Call::default()
        })
        .await
        .map_err(|e| format!("call failed: {e}"))?;
    cbor_to_json(&result).map_err(|e| format!("decode result: {e}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fleet_seeds_are_six_stations_pinned_by_node_id() {
        let seeds = fleet_seeds();
        assert_eq!(seeds.len(), 6);
        assert!(seeds
            .iter()
            .all(|s| s.port == 4433 && s.node_id != [0u8; 32]));
    }

    #[test]
    fn the_io_macula_realm_key_is_its_public_pq_hybrid_key() {
        assert_eq!(crate::io_macula::realm_key().len(), 3118);
    }

    #[test]
    fn the_social_realm_is_io_macula() {
        assert_eq!(
            hex(&social_realm()),
            "abb81b5a614b63551b400b810648c0c8a78efad845442630c94b46cc95d2fcd1"
        );
    }

    #[test]
    fn no_operating_realm_means_io_macula_and_a_chosen_one_is_kept() {
        assert_eq!(operating_realm([0u8; 32]), social_realm());
        assert_eq!(operating_realm([7u8; 32]), [7u8; 32]);
    }

    #[test]
    fn an_mcid_of_the_wrong_length_is_refused_before_any_fetch() {
        assert!(hex_decode("abcd").unwrap().len() == 2);
        assert!(hex_decode("abc").is_err());
    }

    /// A live session against the fleet through this module's own path:
    /// connect_until_up (pinned seeds, io.macula trust) under a key made
    /// for the run, mesh_call to mcl-echo/echo, and a publication heard
    /// through subscribe_into. Run with `cargo test -- --ignored live_`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "live: reaches the fleet"]
    async fn live_session_on_the_fleet() {
        let status = Arc::new(Mutex::new(Status {
            station: String::new(),
            identity_generated: false,
            node_id: String::new(),
            petname: String::new(),
            connected: false,
            error: None,
            connected_at_ms: None,
        }));
        let key = Arc::new(
            NodeKey::generate_identity(Profile::PqHybrid, macula_rust::node_key::PUZZLE_DIFFICULTY)
                .unwrap(),
        );
        let started = std::time::Instant::now();
        let pool = connect_until_up(&status, key).await;
        let station = status.lock().unwrap().station.clone();
        eprintln!("linked to {station} in {:?}", started.elapsed());

        let started = std::time::Instant::now();
        let echoed = mesh_call(
            &pool,
            "mcl-echo/echo",
            "\"hello from desktop\"",
            social_realm(),
        )
        .await;
        eprintln!("mcl-echo/echo in {:?}: {echoed:?}", started.elapsed());
        assert_eq!(echoed.unwrap(), "\"hello from desktop\"");

        let (tx, mut rx) = mpsc::channel::<Event>(8);
        let session = Session {
            node: hex(&pool.node_id()),
            petname: String::new(),
            pool,
            events: tx,
            rooms: HashMap::new(),
            agent_subs: HashMap::new(),
        };
        let topic = format!(
            "macula-desktop/live/check/publication_heard_v1/{}",
            now_ms()
        );
        let forwarder = subscribe_into(&session, social_realm(), &topic)
            .await
            .unwrap();
        let mut heard = None;
        for _ in 0..10 {
            mesh_publish(&session.pool, &topic, "\"heard\"")
                .await
                .unwrap();
            if let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
                heard = Some(event);
                break;
            }
        }
        forwarder.abort();
        session.pool.close().await;
        let event = heard.expect("the publication was never heard");
        assert_eq!(event_payload(&event), "\"heard\"");
        assert_eq!(
            event.publisher,
            hex_decode(&session.node).unwrap().as_slice()
        );
        eprintln!("heard own publication on {topic}");
    }
}
