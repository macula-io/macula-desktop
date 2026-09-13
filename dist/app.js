// The webview's only data path: registered Tauri commands over the
// built-in invoke IPC. No fetch, no sockets, no ports.

const { invoke } = window.__TAURI__.core;

const TAB_KEYS = { c: "chat", g: "agents", t: "teams", o: "rooms", m: "mesh", s: "services", r: "realms", l: "apps-local" };

function selectTab(name) {
  document.querySelectorAll(".tab").forEach((b) => {
    b.classList.toggle("active", b.dataset.tab === name);
  });
  document.querySelectorAll(".pane").forEach((p) => {
    p.classList.toggle("hidden", p.id !== `tab-${name}`);
  });
  if (name === "apps-local" || name === "apps-mesh") refreshApps();
  if (name === "agents") refreshRoster();
  if (name === "rooms") refreshRooms();
}

document.querySelectorAll(".tab").forEach((b) => {
  b.addEventListener("click", () => selectTab(b.dataset.tab));
});

// Keyboard-first (same letters as the terminal UI): c/t/m/s/r switch
// tabs, l goes to Local Applications, a toggles the Applications
// section. Ignored while typing in an input, so shortcuts never fight
// text entry.
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey || e.altKey || e.metaKey) return;
  const target = e.target;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) return;
  if (e.key.toLowerCase() === "a") {
    openApps();
    return;
  }
  const tab = TAB_KEYS[e.key.toLowerCase()];
  if (tab) {
    if (tab.startsWith("apps-")) openApps();
    selectTab(tab);
    document.querySelector(`.tab[data-tab="${tab}"]`).focus();
  }
});

// --- Applications section ------------------------------------------------

const APP_GLYPH =
  '<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">' +
  '<rect x="2" y="2" width="5" height="5" rx="1"/><rect x="9" y="2" width="5" height="5" rx="1"/>' +
  '<rect x="2" y="9" width="5" height="5" rx="1"/><rect x="9" y="9" width="5" height="5" rx="1"/></svg>';

const MESH_GLYPH =
  '<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">' +
  '<line x1="3" y1="5" x2="13" y2="5"/><line x1="3" y1="11" x2="13" y2="11"/>' +
  '<line x1="6" y1="3" x2="6" y2="13"/><line x1="10" y1="3" x2="10" y2="13"/>' +
  '<circle cx="3" cy="5" r="1.6"/><circle cx="13" cy="5" r="1.6"/>' +
  '<circle cx="6" cy="11" r="1.6"/><circle cx="10" cy="11" r="1.6"/></svg>';

const REMOVE_GLYPH =
  '<svg width="12" height="12" viewBox="0 0 12 12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><line x1="2.5" y1="2.5" x2="9.5" y2="9.5"/><line x1="9.5" y1="2.5" x2="2.5" y2="9.5"/></svg>';

let appsState = { local: [], mesh: [] };

function openApps() {
  const shell = document.querySelector(".shell");
  const collapsed = shell.classList.contains("sidebar-collapsed");
  if (collapsed) setCollapsed(false);
  const sub = document.getElementById("apps-sub");
  const toggle = document.getElementById("apps-toggle");
  sub.classList.remove("hidden");
  toggle.classList.add("open");
}

function toggleApps() {
  const sub = document.getElementById("apps-sub");
  const toggle = document.getElementById("apps-toggle");
  const opening = sub.classList.contains("hidden");
  sub.classList.toggle("hidden", !opening);
  toggle.classList.toggle("open", opening);
  if (opening) refreshApps();
}

document.getElementById("apps-toggle").addEventListener("click", toggleApps);
document.getElementById("apps-toggle").addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    toggleApps();
  }
});

function emptyState(listEl, kind, path) {
  listEl.innerHTML =
    '<div class="empty">' +
    '<div class="title">No ' + kind + ' applications yet</div>' +
    '<div class="hint">Add your first one with the button below — entries persist to <span class="value">' +
    escapeHtml(path) +
    "</span>.</div></div>";
}

function renderApps() {
  const localList = document.getElementById("app-list-local");
  const meshList = document.getElementById("app-list-mesh");

  if (appsState.local.length === 0) {
    configPathText().then((p) => emptyState(localList, "local", p));
  } else {
    localList.innerHTML = appsState.local
      .map(
        (a, i) =>
          '<div class="app-card app-openable" data-url="' + escapeHtml(a.url) + '" data-name="' + escapeHtml(a.name) + '" data-sandboxed="' + (a.sandboxed ? 1 : 0) + '">' +
          '<div class="app-glyph">' + APP_GLYPH + "</div>" +
          '<div class="app-info">' +
          '<div class="app-name">' + escapeHtml(a.name) + "</div>" +
          '<div class="app-desc">' + escapeHtml(a.description || "") + "</div>" +
          '<div class="app-url">' + escapeHtml(a.url) + "</div></div>" +
          '<button class="sandbox-toggle ' + (a.sandboxed ? "sandboxed" : "trusted") + '" data-kind="local" data-index="' + i + '" title="Toggle sandbox — sandboxed apps cannot keep data between sessions">' +
          (a.sandboxed ? "sandboxed" : "trusted") + "</button>" +
          '<button class="open-btn" data-url="' + escapeHtml(a.url) + '">Open <span aria-hidden="true">↗</span></button>' +
          '<button class="remove-btn" data-kind="local" data-index="' + i + '" title="Remove ' + escapeHtml(a.name) + '" aria-label="Remove ' + escapeHtml(a.name) + '">' + REMOVE_GLYPH + "</button>" +
          "</div>"
      )
      .join("");
    wireLocalCards(localList);
  }

  if (appsState.mesh.length === 0) {
    configPathText().then((p) => emptyState(meshList, "mesh", p));
  } else {
    meshList.innerHTML = appsState.mesh
      .map(
        (a, i) =>
          '<div class="app-card">' +
          '<div class="app-glyph">' + MESH_GLYPH + "</div>" +
          '<div class="app-info">' +
          '<div class="app-name">' + escapeHtml(a.name) + "</div>" +
          '<div class="app-desc">' + escapeHtml(a.description || "") + "</div>" +
          '<div class="app-url">' + escapeHtml(a.mri) + "</div></div>" +
          '<button class="sandbox-toggle ' + (a.sandboxed ? "sandboxed" : "trusted") + '" data-kind="mesh" data-index="' + i + '" title="Toggle sandbox — sandboxed apps cannot keep data between sessions">' +
          (a.sandboxed ? "sandboxed" : "trusted") + "</button>" +
          '<span class="mesh-tag">resolves with the DHT slice</span>' +
          '<button class="remove-btn" data-kind="mesh" data-index="' + i + '" title="Remove ' + escapeHtml(a.name) + '" aria-label="Remove ' + escapeHtml(a.name) + '">' + REMOVE_GLYPH + "</button>" +
          "</div>"
      )
      .join("");
  }

  localList.querySelectorAll(".remove-btn").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      appsState.local.splice(Number(btn.dataset.index), 1);
      persistApps();
    });
  });
  meshList.querySelectorAll(".remove-btn").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      appsState.mesh.splice(Number(btn.dataset.index), 1);
      persistApps();
    });
  });

  // The sandbox toggle flips one app's trust posture and persists.
  localList.querySelectorAll(".sandbox-toggle").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      appsState.local[Number(btn.dataset.index)].sandboxed = !appsState.local[Number(btn.dataset.index)].sandboxed;
      persistApps();
    });
  });
  meshList.querySelectorAll(".sandbox-toggle").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      appsState.mesh[Number(btn.dataset.index)].sandboxed = !appsState.mesh[Number(btn.dataset.index)].sandboxed;
      persistApps();
    });
  });
}

function wireLocalCards(list) {
  list.querySelectorAll(".open-btn").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      invoke("open_external", { url: btn.dataset.url });
    });
  });
  list.querySelectorAll(".app-openable").forEach((card) => {
    card.addEventListener("click", () => openEmbedded(card.dataset.name, card.dataset.url, card.dataset.sandboxed === "1"));
  });
}

async function persistApps() {
  try {
    await invoke("save_apps", { config: appsState });
    renderApps();
  } catch (e) {
    // Re-render from the still-authoritative local state; the error is
    // surfaced in the form if it came from there, or ignored for remove.
    renderApps();
  }
}

let configPathCache = null;
async function configPathText() {
  if (configPathCache) return configPathCache;
  configPathCache = await invoke("config_path_display");
  return configPathCache;
}

async function refreshApps() {
  try {
    appsState = await invoke("apps_config");
  } catch (e) {
    appsState = { local: [], mesh: [] };
  }
  configPathText().then((p) => {
    const el = document.getElementById("apps-config-path");
    if (el) el.textContent = p;
  });
  renderApps();
}

// --- add/remove forms -----------------------------------------------------

function wireForm(kind) {
  const btn = document.getElementById(`add-${kind}`);
  const form = document.getElementById(`form-${kind}`);
  const save = document.getElementById(`f-${kind}-save`);
  const cancel = document.getElementById(`f-${kind}-cancel`);
  const err = document.getElementById(`f-${kind}-err`);

  btn.addEventListener("click", () => {
    form.classList.remove("hidden");
    btn.classList.add("hidden");
    form.querySelector("input").focus();
  });

  cancel.addEventListener("click", () => {
    form.classList.add("hidden");
    btn.classList.remove("hidden");
    err.classList.add("hidden");
  });

  save.addEventListener("click", async () => {
    const name = document.getElementById(`f-${kind}-name`).value.trim();
    const desc = document.getElementById(`f-${kind}-desc`).value.trim();
    const key = kind === "local" ? "url" : "mri";
    const value = document.getElementById(`f-${kind}-${key}`).value.trim();
    const sandboxed = document.getElementById(`f-${kind}-sandbox`).checked;
    const entry = kind === "local" ? { name, description: desc, url: value, sandboxed } : { name, description: desc, mri: value, sandboxed };
    try {
      const merged = { local: appsState.local, mesh: appsState.mesh };
      merged[kind] = [...merged[kind], entry];
      await invoke("save_apps", { config: merged });
      appsState = merged;
      renderApps();
      form.classList.add("hidden");
      btn.classList.remove("hidden");
      err.classList.add("hidden");
      ["name", "desc", key].forEach((f) => (document.getElementById(`f-${kind}-${f}`).value = ""));
      document.getElementById(`f-${kind}-sandbox`).checked = kind === "mesh";
    } catch (e) {
      err.textContent = String(e);
      err.classList.remove("hidden");
    }
  });

  form.addEventListener("keydown", (e) => {
    if (e.key === "Enter") save.click();
    if (e.key === "Escape") cancel.click();
  });
}

wireForm("local");
wireForm("mesh");

// --- embedded application view -------------------------------------------

let embeddedUrl = "";

function openEmbedded(name, url, sandboxed) {
  embeddedUrl = url;
  document.getElementById("embed-title").textContent = name;
  document.getElementById("embed-url").textContent = url;
  const frame = document.getElementById("embed-frame");
  if (sandboxed) {
    frame.setAttribute("sandbox", "allow-scripts allow-same-origin allow-forms allow-popups allow-downloads");
  } else {
    frame.removeAttribute("sandbox");
  }
  frame.src = url;
  document.body.classList.add("app-open");
  document.getElementById("embedded-app").classList.remove("hidden");
}

function closeEmbedded() {
  document.body.classList.remove("app-open");
  document.getElementById("embedded-app").classList.add("hidden");
  document.getElementById("embed-frame").src = "about:blank";
  embeddedUrl = "";
}

document.getElementById("embed-back").addEventListener("click", closeEmbedded);
document.getElementById("embed-reload").addEventListener("click", () => {
  const frame = document.getElementById("embed-frame");
  frame.src = embeddedUrl;
});
document.getElementById("embed-external").addEventListener("click", () => {
  if (embeddedUrl) invoke("open_external", { url: embeddedUrl });
});

// Esc leaves the embedded app first (if one is open), before anything
// else claims the key.
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && document.body.classList.contains("app-open")) {
    closeEmbedded();
  }
});

function escapeHtml(s) {
  return String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

// --- chat -----------------------------------------------------------------

const { listen } = window.__TAURI__.event;

let assistantBubble = null; // the bubble being streamed into

const chatLog = document.getElementById("chat-log");
const chatInput = document.getElementById("chat-input");
const chatSend = document.getElementById("chat-send");
const chatStop = document.getElementById("chat-stop");

let turning = false;

function setTurning(on) {
  turning = on;
  chatSend.classList.toggle("hidden", on);
  chatStop.classList.toggle("hidden", !on);
}

function addUserMessage(text) {
  const wrap = document.createElement("div");
  wrap.className = "msg user";
  wrap.innerHTML =
    '<div class="role">you</div><div class="bubble">' + escapeHtml(text) + "</div>";
  chatLog.appendChild(wrap);
  chatLog.scrollTop = chatLog.scrollHeight;
}

function addAssistantBubble() {
  const wrap = document.createElement("div");
  wrap.className = "msg assistant";
  wrap.innerHTML =
    '<div class="role">agent</div><div class="bubble"></div>';
  chatLog.appendChild(wrap);
  assistantBubble = wrap.querySelector(".bubble");
  assistantBubble.innerHTML = '<span class="cursor"></span>';
  chatLog.scrollTop = chatLog.scrollHeight;
  return wrap;
}

function finishAssistant() {
  if (!assistantBubble) return;
  assistantBubble.querySelector(".cursor")?.remove();
  const text = assistantBubble.textContent;
  assistantBubble.innerHTML = marked.parse(text);
  assistantBubble = null;
  chatLog.scrollTop = chatLog.scrollHeight;
}

function sendChat() {
  const text = chatInput.value.trim();
  if (!text || turning) return;
  chatInput.value = "";
  chatInput.style.height = "auto";
  document.querySelector(".chat-empty")?.remove();
  addUserMessage(text);
  addAssistantBubble();
  setTurning(true);
  invoke("chat_send", { content: text });
}

chatSend.addEventListener("click", sendChat);
chatStop.addEventListener("click", () => invoke("chat_interrupt"));
chatInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendChat();
  }
});
chatInput.addEventListener("input", () => {
  chatInput.style.height = "auto";
  chatInput.style.height = Math.min(chatInput.scrollHeight, 160) + "px";
});

listen("chat-delta", (e) => {
  if (!assistantBubble) return;
  assistantBubble.querySelector(".cursor")?.remove();
  assistantBubble.appendChild(document.createTextNode(e.payload));
  assistantBubble.appendChild(document.createElement("span")).className = "cursor";
  chatLog.scrollTop = chatLog.scrollHeight;
});

listen("chat-error", (e) => {
  if (assistantBubble) {
    assistantBubble.innerHTML =
      '<span class="error-note">error: ' + escapeHtml(String(e.payload)) + "</span>";
    assistantBubble = null;
  }
  setTurning(false);
});

// A tool the agent called on the mesh, rendered as a transparent note
// between turns -- the operator sees exactly what the agent did.
listen("chat-tool", (e) => {
  const wrap = document.createElement("div");
  wrap.className = "msg assistant tool";
  wrap.innerHTML =
    '<div class="role">tool</div>' +
    '<div class="bubble tool-note">' +
    escapeHtml(e.payload.name) +
    " → " +
    '<span class="tool-result">' + escapeHtml(truncate(String(e.payload.result), 200)) + "</span></div>";
  chatLog.appendChild(wrap);
  chatLog.scrollTop = chatLog.scrollHeight;
});

// A subscribed topic's delivery, rendered as a system line: the agent
// sees these too, via its grounding, and can react to them in its next
// turn.
listen("mesh-event", (e) => {
  document.querySelector(".chat-empty")?.remove();
  const wrap = document.createElement("div");
  wrap.className = "msg assistant event";
  wrap.innerHTML =
    '<div class="role">mesh · ' + escapeHtml(e.payload.topic) + "</div>" +
    '<div class="bubble event-note">' +
    escapeHtml(truncate(String(e.payload.payload), 300)) +
    '<span class="event-publisher">' + escapeHtml(e.payload.publisher_petname || e.payload.publisher) + " · seq " + e.payload.seq + "</span></div>";
  chatLog.appendChild(wrap);
  chatLog.scrollTop = chatLog.scrollHeight;
});

function truncate(s, n) {
  return s.length <= n ? s : s.slice(0, n) + "…";
}

listen("chat-interrupted", () => {
  if (assistantBubble) {
    assistantBubble.querySelector(".cursor")?.remove();
    const text = assistantBubble.textContent;
    assistantBubble.innerHTML =
      (text ? marked.parse(text) + "<br>" : "") +
      '<span class="error-note">interrupted</span>';
    assistantBubble = null;
  }
  setTurning(false);
});

listen("chat-done", () => {
  finishAssistant();
  setTurning(false);
});

document.getElementById("autoreact-toggle").addEventListener("change", () => {
  syncChatSettings();
});
document.getElementById("memory-toggle").addEventListener("change", () => {
  const on = document.getElementById("memory-toggle").checked;
  const realmInput = document.getElementById("memory-realm");
  realmInput.classList.toggle("hidden", !on);
  if (on) realmInput.focus();
  syncChatSettings();
});
document.getElementById("memory-realm").addEventListener("change", () => syncChatSettings());

function syncChatSettings() {
  const realm = document.getElementById("memory-realm").value.trim();
  invoke("set_chat_settings", {
    autoReact: document.getElementById("autoreact-toggle").checked,
    memory: document.getElementById("memory-toggle").checked,
    memoryRealm: realm,
  }).catch((e) => {
    // The most likely cause: memory on without a valid realm -- which
    // is exactly the gate, so un-check rather than fail silently.
    document.getElementById("memory-toggle").checked = false;
    document.getElementById("memory-realm").classList.add("hidden");
    console.warn("chat settings rejected:", e);
  });
}

// A gated tool the agent wants to run: the operator decides, in the
// chat, before anything reaches the mesh. Matches lazymesh's asklist
// discipline.
listen("chat-approval", (e) => {
  document.querySelector(".chat-empty")?.remove();
  const wrap = document.createElement("div");
  wrap.className = "msg assistant approval";
  const args = truncate(String(e.payload.arguments || ""), 200);
  wrap.innerHTML =
    '<div class="role">approval required</div>' +
    '<div class="bubble approval-note">' +
    'The agent wants to run <span class="approval-tool">' + escapeHtml(e.payload.name) + "</span>" +
    (args ? ' <span class="approval-args">' + escapeHtml(args) + "</span>" : "") +
    '<div class="approval-actions">' +
    '<button class="approve-btn ok" data-id="' + escapeHtml(e.payload.id) + '" data-approved="true">Approve</button>' +
    '<button class="approve-btn deny" data-id="' + escapeHtml(e.payload.id) + '" data-approved="false">Deny</button>' +
    '<button class="approve-btn all" id="approve-all-btn">Approve all this turn</button>' +
    "</div></div>";
  chatLog.appendChild(wrap);
  chatLog.scrollTop = chatLog.scrollHeight;
});

document.getElementById("approve-all-btn")?.addEventListener("click", () => {
  invoke("set_chat_approve_all");
  document.getElementById("approve-all-btn").textContent = "approved ✓";
  document.getElementById("approve-all-btn").disabled = true;
});

document.addEventListener("click", (e) => {
  const btn = e.target.closest(".approve-btn");
  if (!btn || btn.id === "approve-all-btn") return;
  invoke("approve_tool", { id: btn.dataset.id, approved: btn.dataset.approved === "true" });
  const note = btn.closest(".approval-note");
  note.querySelector(".approval-actions").innerHTML =
    '<span class="approval-answered">' + (btn.dataset.approved === "true" ? "approved ✓" : "denied") + "</span>";
});

// --- agents roster ---------------------------------------------------------

function avatarHue(nodeId) {
  let h = 0;
  for (let i = 0; i < nodeId.length; i++) h = (h * 31 + nodeId.charCodeAt(i)) >>> 0;
  return h % 360;
}

function agoShort(seconds) {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h`;
}

async function refreshRoster() {
  const list = document.getElementById("roster-list");
  let entries;
  try {
    entries = await invoke("roster");
  } catch (e) {
    return;
  }
  if (!entries || entries.length === 0) {
    list.innerHTML =
      '<div class="empty"><div class="title">No agents heard yet</div>' +
      '<div class="hint">Heartbeats arrive within a minute — agents on the shared mesh appear here, this app included.</div></div>';
    return;
  }
  const now = Date.now();
  list.innerHTML = entries
    .map((a) => {
      const hue = avatarHue(a.nodeId);
      const seen = agoShort(Math.max(0, Math.floor((now - a.lastSeenMs) / 1000)));
      const sub = [a.operatorName, a.model].filter(Boolean).join(" · ");
      return (
        '<div class="roster-card' + (a.isSelf ? " self" : "") + '">' +
        '<div class="roster-avatar" style="background:hsl(' + hue + ', 55%, 45%); color:#fff">' +
        escapeHtml(a.petname.charAt(0).toUpperCase()) +
        "</div>" +
        '<div class="roster-info">' +
        '<div class="roster-name">' + escapeHtml(a.petname) + (a.isSelf ? ' <span class="self-tag">you</span>' : "") + "</div>" +
        (sub ? '<div class="roster-sub">' + escapeHtml(sub) + "</div>" : "") +
        '<div class="roster-id">' + escapeHtml(a.nodeId.slice(0, 12)) + "…</div></div>" +
        '<div class="roster-seen">' + seen + " ago</div>" +
        "</div>"
      );
    })
    .join("");
}

// --- rooms ----------------------------------------------------------------

function shortTopic(t) {
  return t.replace("agents.room.", "").slice(0, 8);
}

async function refreshRooms() {
  try {
    const publicRooms = await invoke("public_rooms_command");
    const joined = await invoke("joined_rooms_command");
    renderPublicRooms(publicRooms, joined);
    renderJoinedRooms(joined);
  } catch (e) {
    /* the pane retries on next visit */
  }
}

function renderPublicRooms(publicRooms, joined) {
  const list = document.getElementById("public-room-list");
  const joinedTopics = new Set(joined.map((r) => r.topic));
  if (publicRooms.length === 0) {
    list.innerHTML =
      '<div class="empty small"><div class="hint">No public rooms announced yet — they appear here as agents open them.</div></div>';
    return;
  }
  list.innerHTML = publicRooms
    .map((r) => {
      const joinedNow = joinedTopics.has(r.topic);
      return (
        '<div class="room-card">' +
        '<div class="room-info">' +
        '<div class="room-purpose">' + escapeHtml(r.purpose || shortTopic(r.topic)) + "</div>" +
        '<div class="room-meta">' + escapeHtml(r.openedByPetname) + " · " + escapeHtml(shortTopic(r.topic)) + "</div></div>" +
        (joinedNow
          ? '<button class="room-btn joined" data-topic="' + escapeHtml(r.topic) + '">joined</button>'
          : '<button class="room-btn join" data-topic="' + escapeHtml(r.topic) + '" data-purpose="' + escapeHtml(r.purpose || "") + '">join</button>') +
        "</div>"
      );
    })
    .join("");
  list.querySelectorAll(".room-btn.join").forEach((btn) => {
    btn.addEventListener("click", async () => {
      await invoke("join_room", { topic: btn.dataset.topic, purpose: btn.dataset.purpose });
      refreshRooms();
    });
  });
}

function renderJoinedRooms(joined) {
  const list = document.getElementById("joined-room-list");
  if (joined.length === 0) {
    list.innerHTML =
      '<div class="empty small"><div class="hint">You have not joined any rooms yet.</div></div>';
    return;
  }
  list.innerHTML = joined
    .map((r) => {
      const last = r.messages.slice(-3)
        .map((m) => {
          const who = m.publisher.length > 12 ? m.publisher.slice(0, 12) + "…" : m.publisher;
          return '<div class="room-msg"><span class="room-msg-who">' + escapeHtml(who) + "</span> " + escapeHtml(truncate(m.payload, 120)) + "</div>";
        })
        .join("");
      return (
        '<div class="room-card">' +
        '<div class="room-info">' +
        '<div class="room-purpose">' + escapeHtml(r.purpose || shortTopic(r.topic)) + "</div>" +
        '<div class="room-meta">' + escapeHtml(shortTopic(r.topic)) + "</div>" +
        (last || '<div class="room-msg dim">no messages yet</div>') +
        "</div>" +
        '<button class="room-btn leave" data-topic="' + escapeHtml(r.topic) + '">leave</button>' +
        "</div>"
      );
    })
    .join("");
  list.querySelectorAll(".room-btn.leave").forEach((btn) => {
    btn.addEventListener("click", async () => {
      await invoke("leave_room", { topic: btn.dataset.topic });
      refreshRooms();
    });
  });
}

// --- titlebar window controls -------------------------------------------

document.getElementById("win-min").addEventListener("click", () => invoke("window_minimize"));
document.getElementById("win-max").addEventListener("click", () => invoke("window_toggle_maximize"));
document.getElementById("win-close").addEventListener("click", () => invoke("window_close"));

// Double-click on the titlebar = maximize/restore, the platform
// convention the custom titlebar has to re-implement itself.
document.querySelector(".titlebar").addEventListener("dblclick", (e) => {
  if (e.target.closest(".winbtn")) return;
  invoke("window_toggle_maximize");
});

// --- sidebar collapse ----------------------------------------------------

function setCollapsed(collapsed) {
  document.querySelector(".shell").classList.toggle("sidebar-collapsed", collapsed);
  const collapseBtn = document.getElementById("collapse-btn");
  collapseBtn.title = collapsed ? "Expand sidebar (Ctrl+B)" : "Collapse sidebar (Ctrl+B)";
  document.querySelectorAll(".tab").forEach((b) => {
    b.title = collapsed ? `${b.dataset.tab[0].toUpperCase()}${b.dataset.tab.slice(1)} (${TAB_KEYS_INV[b.dataset.tab]})` : "";
  });
}

const TAB_KEYS_INV = { chat: "c", teams: "t", mesh: "m", services: "s", realms: "r" };
document.querySelectorAll(".tab").forEach((b) => {
  b.title = `${b.dataset.tab[0].toUpperCase()}${b.dataset.tab.slice(1)} (${TAB_KEYS_INV[b.dataset.tab]})`;
});

document.getElementById("collapse-btn").addEventListener("click", () => {
  const shell = document.querySelector(".shell");
  setCollapsed(!shell.classList.contains("sidebar-collapsed"));
});

// Ctrl+B: the VS Code convention for the sidebar.
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey && !e.altKey && !e.metaKey && e.key.toLowerCase() === "b") {
    e.preventDefault();
    const shell = document.querySelector(".shell");
    setCollapsed(!shell.classList.contains("sidebar-collapsed"));
  }
});

// --- mesh status -------------------------------------------------------

function setPill(pill, dot, state, text) {
  pill.className = `pill ${state}`;
  pill.textContent = text;
  dot.className = `dot ${state}`;
}

function fmtUptime(ms) {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

let nodeId = "";
let connectedAtMs = null;
let clockAtRefresh = 0;

async function refreshMesh() {
  try {
    const s = await invoke("mesh_status");
    clockAtRefresh = Date.now();

    document.getElementById("station").textContent = s.station;
    document.getElementById("conn-station").textContent = s.station;

    if (s.identityGenerated) {
      nodeId = s.nodeId;
      const name = s.petname ? s.petname + " · " : "";
      document.getElementById("identity").textContent = name + nodeId;
      document.getElementById("copy-node").disabled = false;
    }

    connectedAtMs = s.connectedAtMs;

    const status = document.getElementById("status");
    const statusDot = document.getElementById("status-dot");
    const connLabel = document.getElementById("conn-label");
    const connDot = document.getElementById("conn-dot");
    const errorBox = document.getElementById("error");
    const titlebarStatus = document.getElementById("titlebar-status");

    if (s.connected) {
      setPill(status, statusDot, "ok", "connected");
      connLabel.textContent = "connected";
      connDot.className = "dot ok";
      titlebarStatus.textContent = "connected";
      titlebarStatus.className = "titlebar-status ok";
      errorBox.classList.add("hidden");
    } else if (s.error) {
      setPill(status, statusDot, "bad", "failed");
      connLabel.textContent = "failed";
      connDot.className = "dot bad";
      titlebarStatus.textContent = "failed";
      titlebarStatus.className = "titlebar-status bad";
      errorBox.textContent = s.error;
      errorBox.classList.remove("hidden");
    } else {
      setPill(status, statusDot, "connecting", "connecting…");
      connLabel.textContent = "connecting…";
      connDot.className = "dot connecting";
      titlebarStatus.textContent = "connecting…";
      titlebarStatus.className = "titlebar-status connecting";
      errorBox.classList.add("hidden");
    }
  } catch (e) {
    document.getElementById("error").textContent = `IPC error: ${e}`;
    document.getElementById("error").classList.remove("hidden");
  }
}

// Live uptime: recompute from the last status's connected_at without
// re-polling the Rust core every second.
function tickUptime() {
  if (connectedAtMs == null) return;
  const elapsed = Date.now() - clockAtRefresh + (clockAtRefresh - connectedAtMs);
  document.getElementById("uptime").textContent = fmtUptime(elapsed);
}

document.getElementById("copy-node").addEventListener("click", async () => {
  if (!nodeId) return;
  await navigator.clipboard.writeText(nodeId);
  const btn = document.getElementById("copy-node");
  btn.textContent = "copied ✓";
  setTimeout(() => (btn.textContent = "copy node_id"), 1500);
});

refreshMesh();
refreshApps();
setInterval(refreshMesh, 2000);

(async () => {
  const s = await invoke("chat_settings");
  document.getElementById("autoreact-toggle").checked = s.autoReact;
  document.getElementById("memory-toggle").checked = s.memory;
  document.getElementById("memory-realm").value = s.memoryRealm || "";
  document.getElementById("memory-realm").classList.toggle("hidden", !s.memory);
})();
setInterval(tickUptime, 1000);
