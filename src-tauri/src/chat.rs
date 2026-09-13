//! Chat: the desktop's own personal-agent loop. The Rust core makes
//! the LLM calls (OpenAI-compatible endpoint) and streams deltas to the
//! webview as Tauri events -- the webview itself still never touches
//! the network, same posture as the mesh link.
//!
//! Credentials follow the workstation's existing convention: the API
//! key lives in ~/.ai-api-keys/.deepseek-api-keys/macula-desktop (never
//! in the repo), with the DEEPSEEK_API_KEY environment variable as a
//! fallback. Base URL and model come from MACULA_LLM_BASE_URL /
//! MACULA_LLM_MODEL with DeepSeek defaults.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{Emitter, Manager};

/// ChatMessage is one turn of the conversation handed to the model.
/// Tool plumbing rides along: tool_calls the model requested (assistant
/// role) and tool_call_id linking a tool result to its call.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

/// ChatState is the conversation and the agent's standing policy,
/// shared between the user-driven path and the mesh-event reactor. The
/// history is authoritative HERE: the webview sends only the newest
/// user message, and reactor-driven turns append without the UI.
pub struct ChatState {
    pub history: std::sync::Mutex<Vec<ChatMessage>>,
    /// auto_react: mesh events may wake the agent without a user turn.
    pub auto_react: std::sync::Mutex<bool>,
    /// in_flight: one turn at a time, user-driven or reactor-driven.
    pub in_flight: std::sync::Mutex<bool>,
}

impl Default for ChatState {
    fn default() -> Self {
        ChatState {
            history: std::sync::Mutex::new(Vec::new()),
            auto_react: std::sync::Mutex::new(false),
            in_flight: std::sync::Mutex::new(false),
        }
    }
}

/// Settings is the persisted chat policy.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub auto_react: bool,
}

fn settings_path() -> Option<PathBuf> {
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
    path.push("settings.json");
    Some(path)
}

/// chat_settings returns the persisted policy (defaults when absent).
#[tauri::command]
pub fn chat_settings() -> Settings {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or(Settings { auto_react: false })
}

/// set_chat_settings persists the policy and applies it immediately:
/// the reactor reads this same value for every event.
#[tauri::command]
pub fn set_chat_settings(
    state: tauri::State<'_, ChatState>,
    auto_react: bool,
) -> Result<(), String> {
    *state.auto_react.lock().expect("chat policy lock") = auto_react;
    let Some(path) = settings_path() else {
        return Err("no config directory could be determined".to_string());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create config directory: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&Settings { auto_react })
        .map_err(|e| format!("encode settings: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write settings: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("commit settings: {e}"))?;
    Ok(())
}

/// chat_send takes the operator's NEWEST message (the history lives in
/// ChatState), runs the agent turn in the background, and streams
/// deltas as before.
#[tauri::command]
pub fn chat_send(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatState>,
    content: String,
) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err("empty message".to_string());
    }
    state
        .history
        .lock()
        .expect("chat history lock")
        .push(ChatMessage { role: "user".to_string(), content, tool_calls: None, tool_call_id: None });
    std::thread::spawn(move || run_user_turn(app));
    Ok(())
}

/// run_user_turn executes the operator-initiated turn and records the
/// assistant's answer back into the shared history.
fn run_user_turn(app: tauri::AppHandle) {
    let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime for the chat link");
    runtime.block_on(async {
        let state = app.state::<ChatState>();
        let link = app.state::<crate::mesh::MeshLink>();
        let history: Vec<ChatMessage> = state
            .history
            .lock()
            .expect("chat history lock")
            .iter()
            .cloned()
            .collect();
        let result = run_turn(&app, &link, history).await;
        let _ = app.emit("chat-done", ());
        record_answer(&state, result);
    });
}

fn record_answer(state: &ChatState, result: Result<String, Box<dyn std::error::Error + Send + Sync>>) {
    match result {
        Ok(text) => {
            if !text.is_empty() {
                state.history.lock().expect("chat history lock").push(ChatMessage {
                    role: "assistant".to_string(),
                    content: text,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
        Err(_) => {} // the error was already emitted from run_turn
    }
}

/// maybe_react is the mesh-event reactor: when auto_react is on and no
/// turn is in flight, a fresh event wakes the agent to process what it
/// heard. Events arriving while a turn runs simply wait in the buffer --
/// the next event that finds the agent idle wakes it again.
pub fn maybe_react(app: &tauri::AppHandle) {
    let app = app.clone();
    {
        let state = app.state::<ChatState>();
        let auto = *state.auto_react.lock().expect("chat policy lock");
        if !auto {
            return;
        }
        let mut inflight = state.in_flight.lock().expect("chat inflight lock");
        if *inflight {
            return;
        }
        *inflight = true;
    }
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime for the reactor");
        runtime.block_on(async {
            let state = app.state::<ChatState>();
            let link = app.state::<crate::mesh::MeshLink>();
            let mut history: Vec<ChatMessage> = state
                .history
                .lock()
                .expect("chat history lock")
                .iter()
                .cloned()
                .collect();
            history.push(ChatMessage {
                role: "user".to_string(),
                content: "A new mesh event just arrived on one of your subscribed topics (see your context). React as appropriate -- observe, act with your mesh tools if the event calls for it, or note it briefly.".to_string(),
                tool_calls: None,
                tool_call_id: None,
            });
            let result = run_turn(&app, &link, history).await;
            let _ = app.emit("chat-done", ());
            record_answer(&state, result);
            *state.in_flight.lock().expect("chat inflight lock") = false;
        });
    });
}

/// ground_messages prepends the system prompt derived from the live
/// mesh snapshot AND the captured event deliveries: the agent answers
/// as the node it actually is, about the connection it actually has,
/// with the events its subscriptions actually received.
fn ground_messages(
    link: &crate::mesh::MeshLink,
    messages: Vec<ChatMessage>,
) -> Vec<ChatMessage> {
    let status = link.snapshot();
    let identity = if status.identity_generated {
        status.node_id.clone()
    } else {
        "still generating".to_string()
    };
    let connection = if status.connected {
        format!("connected, session held since {:?} ms after the Unix epoch", status.connected_at_ms)
    } else {
        "not connected".to_string()
    };
    let events = link.recent_events();
    let events_text = if events.is_empty() {
        "You have not received any subscribed events yet.".to_string()
    } else {
        let mut lines: Vec<String> = Vec::with_capacity(events.len());
        for e in events.iter().rev().take(20).rev() {
            lines.push(format!(
                "- topic {} from {} (seq {}): {}",
                e.topic, e.publisher, e.seq, e.payload
            ));
        }
        format!(
            "Deliveries your subscriptions have received (oldest last):\n{}",
            lines.join("\n")
        )
    };
    let system = format!(
        "You are the operator's personal agent inside macula-desktop, a desktop app for the Macula mesh. \
The app's Rust core holds a live mesh connection, and you are grounded in it: \
you are node {identity} dialing station {station}; the link is currently {connection}. \
{events} \
When asked about your connection or the mesh, answer from THIS state, not from general knowledge: \
you genuinely are this node on this mesh, through this app. \
Use your mesh tools (call, publish, subscribe, unsubscribe, content get/put) to act on the mesh when the task needs it. \
Do not invent mesh facts beyond what you are given here; if asked for something you cannot know, say so.",
        identity = identity,
        station = status.station,
        connection = connection,
        events = events_text,
    );
    let mut grounded = Vec::with_capacity(messages.len() + 1);
    grounded.push(ChatMessage {
        role: "system".to_string(),
        content: system,
        tool_calls: None,
        tool_call_id: None,
    });
    grounded.extend(messages);
    grounded
}

/// run_turn grounds the conversation in the live mesh state and drives
/// the tool loop; it returns the assistant's final prose answer.
async fn run_turn(
    app: &tauri::AppHandle,
    link: &crate::mesh::MeshLink,
    messages: Vec<ChatMessage>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    const MAX_ROUNDS: usize = 6;
    let mut conversation = ground_messages(link, messages);
    let mut answer = String::new();
    for _round in 0..MAX_ROUNDS {
        let (text, tool_calls) = stream_chat(app, &conversation).await?;
        answer.push_str(&text);
        if tool_calls.is_empty() {
            return Ok(answer);
        }
        conversation.push(ChatMessage {
            role: "assistant".to_string(),
            content: text,
            tool_calls: Some(tool_calls.clone()),
            tool_call_id: None,
        });
        for call in &tool_calls {
            let result: Result<String, String> = match call.function.name.as_str() {
                "mesh_status" => serde_json::to_string(&link.snapshot())
                    .map_err(|e| format!("encode status: {e}")),
                "mesh_call" => {
                    let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                        .map_err(|e| format!("bad arguments: {e}"))?;
                    let procedure = args["procedure"].as_str().unwrap_or("").to_string();
                    let args_json = args["args_json"].as_str().unwrap_or("").to_string();
                    if procedure.is_empty() {
                        Err("mesh_call requires a procedure".to_string())
                    } else {
                        link.request(crate::mesh::MeshCommand::Call { procedure, args_json }).await
                    }
                }
                "mesh_publish" => {
                    let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                        .map_err(|e| format!("bad arguments: {e}"))?;
                    let topic = args["topic"].as_str().unwrap_or("").to_string();
                    let payload_json = args["payload_json"].as_str().unwrap_or("").to_string();
                    link.request(crate::mesh::MeshCommand::Publish { topic, payload_json }).await
                }
                "mesh_subscribe" => {
                    let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                        .map_err(|e| format!("bad arguments: {e}"))?;
                    let topic = args["topic"].as_str().unwrap_or("").to_string();
                    link.request(crate::mesh::MeshCommand::Subscribe { topic }).await
                }
                "mesh_unsubscribe" => {
                    let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                        .map_err(|e| format!("bad arguments: {e}"))?;
                    let topic = args["topic"].as_str().unwrap_or("").to_string();
                    link.request(crate::mesh::MeshCommand::Unsubscribe { topic }).await
                }
                "content_get" => {
                    let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                        .map_err(|e| format!("bad arguments: {e}"))?;
                    let mcid_hex = args["mcid"].as_str().unwrap_or("").to_string();
                    if mcid_hex.is_empty() {
                        Err("content_get requires an mcid".to_string())
                    } else {
                        link.request(crate::mesh::MeshCommand::ContentGet { mcid_hex }).await
                    }
                }
                "content_put" => {
                    let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                        .map_err(|e| format!("bad arguments: {e}"))?;
                    let data = args["data"].as_str().unwrap_or("").to_string();
                    let name = args["name"].as_str().unwrap_or("").to_string();
                    use base64::Engine;
                    let data_b64 = base64::engine::general_purpose::STANDARD.encode(data.as_bytes());
                    link.request(crate::mesh::MeshCommand::ContentPut { data_b64, name }).await
                }
                other => Err(format!("unknown tool {other}")),
            };
            let content = match result {
                Ok(ok) => ok,
                Err(e) => format!("error: {e}"),
            };
            let _ = app.emit(
                "chat-tool",
                serde_json::json!({ "name": call.function.name, "result": content }),
            );
            conversation.push(ChatMessage {
                role: "tool".to_string(),
                content,
                tool_call_id: Some(call.id.clone()),
                tool_calls: None,
            });
        }
    }
    Err("the model kept calling tools past the round cap".into())
}

async fn stream_chat(
    app: &tauri::AppHandle,
    messages: &[ChatMessage],
) -> Result<(String, Vec<ToolCall>), Box<dyn std::error::Error + Send + Sync>> {
    let (key, base_url, model) = llm_settings()?;

    #[derive(Serialize)]
    struct Request<'a> {
        model: &'a str,
        messages: &'a [ChatMessage],
        stream: bool,
        tools: &'a [serde_json::Value],
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let response = client
        .post(format!("{base_url}/chat/completions"))
        .bearer_auth(key)
        .json(&Request { model: &model, messages, stream: true, tools: &TOOLS })
        .send()
        .await?
        .error_for_status()?;

    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buf = String::new();
    let mut text = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data == "[DONE]" {
                    break;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    let choice = &v["choices"][0];
                    if let Some(delta) = choice["delta"]["content"].as_str() {
                        text.push_str(delta);
                        let _ = app.emit("chat-delta", delta);
                    }
                    if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
                        for call in calls {
                            let index = call["index"].as_u64().unwrap_or(0) as usize;
                            while tool_calls.len() <= index {
                                tool_calls.push(ToolCall {
                                    id: String::new(),
                                    kind: "function".to_string(),
                                    function: ToolCallFunction { name: String::new(), arguments: String::new() },
                                });
                            }
                            let tc = &mut tool_calls[index];
                            if let Some(id) = call["id"].as_str() {
                                tc.id = id.to_string();
                            }
                            if let Some(name) = call["function"]["name"].as_str() {
                                tc.function.name.push_str(name);
                            }
                            if let Some(args) = call["function"]["arguments"].as_str() {
                                tc.function.arguments.push_str(args);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok((text, tool_calls))
}

/// TOOLS is the agent's native toolbox: mesh operations executed by
/// the Rust core against its own live session. No MCP client, no
/// external server -- the SDK primitives are right here.
static TOOLS: std::sync::LazyLock<Vec<serde_json::Value>> = std::sync::LazyLock::new(|| {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mesh_status",
                "description": "Report this app's live mesh link: the node's own identity, the station it dials, and whether the connection is up. Use this whenever asked about the connection or the app's place on the mesh.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mesh_call",
                "description": "Invoke a procedure on the Macula mesh through this app's live connection. procedure is the procedure name; args_json is an optional JSON object of arguments (empty string for none). Returns the procedure's JSON result or a BOLT error.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "procedure": { "type": "string", "description": "The procedure name to call." },
                        "args_json": { "type": "string", "description": "JSON object of arguments, or empty string." }
                    },
                    "required": ["procedure"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mesh_publish",
                "description": "Publish a fact to a mesh topic (fire-and-forget: success means the signed frame went out, not that anyone heard). topic is the topic name; payload_json is a JSON object or value.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "description": "The topic to publish to." },
                        "payload_json": { "type": "string", "description": "JSON payload." }
                    },
                    "required": ["topic", "payload_json"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mesh_subscribe",
                "description": "Subscribe this app to a mesh topic: deliveries arrive live in the chat and in your context. Use it to watch for events you need to react to.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "description": "The topic to subscribe to." }
                    },
                    "required": ["topic"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mesh_unsubscribe",
                "description": "Stop receiving deliveries for a topic this app subscribed to.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "description": "The topic to unsubscribe from." }
                    },
                    "required": ["topic"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "content_get",
                "description": "Fetch content-addressed data from the mesh by its MCID (68 hex chars). Text content returns as text; binary content reports its size. Integrity is verified against the MCID itself.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "mcid": { "type": "string", "description": "The 68-hex-char MCID." }
                    },
                    "required": ["mcid"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "content_put",
                "description": "Store text on the mesh as content-addressed data; returns the MCID anyone can later fetch it by.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "data": { "type": "string", "description": "The text to store." },
                        "name": { "type": "string", "description": "A name attached to chunked manifests; ignored for small content." }
                    },
                    "required": ["data"]
                }
            }
        }),
    ]
});

fn llm_settings() -> Result<(String, String, String), String> {
    let key = api_key_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("DEEPSEEK_API_KEY").ok().filter(|s| !s.is_empty()))
        .ok_or_else(|| {
            "no API key: put it in ~/.ai-api-keys/.deepseek-api-keys/macula-desktop or set DEEPSEEK_API_KEY".to_string()
        })?;
    let base_url = std::env::var("MACULA_LLM_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://api.deepseek.com".to_string());
    let model = std::env::var("MACULA_LLM_MODEL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "deepseek-chat".to_string());
    Ok((key, base_url, model))
}

fn api_key_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut path = PathBuf::from(home);
    path.push(".ai-api-keys");
    path.push(".deepseek-api-keys");
    path.push("macula-desktop");
    Some(path)
}
