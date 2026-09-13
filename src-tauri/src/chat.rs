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
    grounded.push(ChatMessage {
        role: "system".to_string(),
        content: system,
        tool_calls: None,
        tool_call_id: None,
    });
    grounded.extend(messages);
    grounded
}

fn run_chat(app: tauri::AppHandle, messages: Vec<ChatMessage>) {
    let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime for the chat link");
    runtime.block_on(async {
        if let Err(e) = run_tool_loop(&app, messages).await {
            let _ = app.emit("chat-error", e.to_string());
        }
        let _ = app.emit("chat-done", ());
    });
}

/// run_tool_loop drives one or more completion requests: when the model
/// answers with tool calls, the Rust core executes them against the live
/// mesh link and sends the results back, until the model produces a
/// final answer (or the round cap trips).
async fn run_tool_loop(
    app: &tauri::AppHandle,
    messages: Vec<ChatMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    const MAX_ROUNDS: usize = 6;
    let link = app.state::<crate::mesh::MeshLink>();
    let mut conversation = messages;
    for _round in 0..MAX_ROUNDS {
        let tool_calls = stream_chat(app, &conversation).await?;
        if tool_calls.is_empty() {
            return Ok(()); // the model answered in prose
        }
        conversation.push(ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
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
) -> Result<Vec<ToolCall>, Box<dyn std::error::Error + Send + Sync>> {
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
    Ok(tool_calls)
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
