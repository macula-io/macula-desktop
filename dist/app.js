// The webview's only data path: registered Tauri commands over the
// built-in invoke IPC. No fetch, no sockets, no ports.

const { invoke } = window.__TAURI__.core;

const TAB_KEYS = { c: "chat", t: "teams", m: "mesh", s: "services", r: "realms", l: "apps-local" };

function selectTab(name) {
  document.querySelectorAll(".tab").forEach((b) => {
    b.classList.toggle("active", b.dataset.tab === name);
  });
  document.querySelectorAll(".pane").forEach((p) => {
    p.classList.toggle("hidden", p.id !== `tab-${name}`);
  });
  if (name === "apps-local") refreshLocalApps();
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
  if (opening) refreshLocalApps();
}

document.getElementById("apps-toggle").addEventListener("click", toggleApps);
document.getElementById("apps-toggle").addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    toggleApps();
  }
});

const APP_GLYPH =
  '<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">' +
  '<rect x="2" y="2" width="5" height="5" rx="1"/><rect x="9" y="2" width="5" height="5" rx="1"/>' +
  '<rect x="2" y="9" width="5" height="5" rx="1"/><rect x="9" y="9" width="5" height="5" rx="1"/></svg>';

async function refreshLocalApps() {
  const list = document.getElementById("app-list");
  let apps;
  try {
    apps = await invoke("local_apps");
  } catch (e) {
    list.innerHTML = "";
    return;
  }
  if (!apps || apps.length === 0) {
    const path = await invoke("config_path_display");
    list.innerHTML =
      '<div class="empty">' +
      '<div class="title">No local applications yet</div>' +
      '<div class="hint">Declare your LAN admin UIs in <span class="value">' +
      escapeHtml(path) +
      "</span> — a JSON file with a <span class=\"value\">local</span> array of " +
      '<span class="value">{ name, description, url }</span> entries.</div></div>';
    return;
  }
  list.innerHTML = apps
    .map(
      (a) =>
        '<div class="app-card">' +
        '<div class="app-glyph">' + APP_GLYPH + "</div>" +
        '<div class="app-info">' +
        '<div class="app-name">' + escapeHtml(a.name) + "</div>" +
        '<div class="app-desc">' + escapeHtml(a.description || "") + "</div>" +
        '<div class="app-url">' + escapeHtml(a.url) + "</div></div>" +
        '<button class="open-btn" data-url="' + escapeHtml(a.url) + '">' +
        "Open <span aria-hidden=\"true\">↗</span></button></div>"
    )
    .join("");
  list.querySelectorAll(".open-btn").forEach((btn) => {
    btn.addEventListener("click", () => invoke("open_external", { url: btn.dataset.url }));
  });
}

function escapeHtml(s) {
  return String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
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
      document.getElementById("identity").textContent = nodeId;
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
setInterval(refreshMesh, 2000);
setInterval(tickUptime, 1000);
