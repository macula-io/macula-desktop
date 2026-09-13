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
use tauri::Emitter;

/// ChatMessage is one turn of the conversation handed to the model.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// chat_send fires one completion request in the background: it returns
/// immediately, then emits `chat-delta` events for every streamed
/// fragment, `chat-error` on failure, and a final `chat-done`. The
/// request is grounded in the app's live mesh state: a system message
/// describes who this agent is (its node identity) and what it is
/// connected to, built fresh at send time.
#[tauri::command]
pub fn chat_send(
    app: tauri::AppHandle,
    link: tauri::State<'_, crate::mesh::MeshLink>,
    messages: Vec<ChatMessage>,
) -> Result<(), String> {
    if messages.is_empty() {
        return Err("no messages to send".to_string());
    }
    let grounded = ground_messages(&link.snapshot(), messages);
    std::thread::spawn(move || run_chat(app, grounded));
    Ok(())
}

/// ground_messages prepends the system prompt derived from the live
/// mesh snapshot: the agent answers AS the node it actually is, about
/// the connection it actually has.
fn ground_messages(status: &crate::mesh::Status, messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
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
    let system = format!(
        "You are the operator's personal agent inside macula-desktop, a desktop app for the Macula mesh. \
The app's Rust core holds a live mesh connection, and you are grounded in it: \
you are node {identity} dialing station {station}; the link is currently {connection}. \
When asked about your connection or the mesh, answer from THIS state, not from general knowledge: \
you genuinely are this node on this mesh, through this app. \
Do not invent mesh facts beyond what you are given here; if asked for something you cannot know, say so.",
        identity = identity,
        station = status.station,
        connection = connection,
    );
    let mut grounded = Vec::with_capacity(messages.len() + 1);
    grounded.push(ChatMessage { role: "system".to_string(), content: system });
    grounded.extend(messages);
    grounded
}

fn run_chat(app: tauri::AppHandle, messages: Vec<ChatMessage>) {
    let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime for the chat link");
    runtime.block_on(async {
        if let Err(e) = stream_chat(&app, &messages).await {
            let _ = app.emit("chat-error", e.to_string());
        }
        let _ = app.emit("chat-done", ());
    });
}

async fn stream_chat(
    app: &tauri::AppHandle,
    messages: &[ChatMessage],
) -> Result<(), Box<dyn std::error::Error>> {
    let (key, base_url, model) = llm_settings()?;

    #[derive(Serialize)]
    struct Request<'a> {
        model: &'a str,
        messages: &'a [ChatMessage],
        stream: bool,
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let response = client
        .post(format!("{base_url}/chat/completions"))
        .bearer_auth(key)
        .json(&Request { model: &model, messages, stream: true })
        .send()
        .await?
        .error_for_status()?;

    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        // SSE frames are separated by blank lines; drain complete lines.
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data == "[DONE]" {
                    return Ok(());
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
                        let _ = app.emit("chat-delta", delta);
                    }
                }
            }
        }
    }
    Ok(())
}

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
