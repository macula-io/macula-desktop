// The webview's only data path: registered Tauri commands over the
// built-in invoke IPC. No fetch, no sockets, no ports.

const { invoke } = window.__TAURI__.core;

const TAB_KEYS = { c: "chat", t: "teams", m: "mesh", s: "services", r: "realms" };

function selectTab(name) {
  document.querySelectorAll(".tab").forEach((b) => {
    b.classList.toggle("active", b.dataset.tab === name);
  });
  document.querySelectorAll(".pane").forEach((p) => {
    p.classList.toggle("hidden", p.id !== `tab-${name}`);
  });
}

document.querySelectorAll(".tab").forEach((b) => {
  b.addEventListener("click", () => selectTab(b.dataset.tab));
});

// Keyboard-first (same letters as the terminal UI): c/t/m/s/r switch
// tabs. Ignored while typing in an input, so shortcuts never fight text
// entry.
document.addEventListener("keydown", (e) => {
  if (e.ctrlKey || e.altKey || e.metaKey) return;
  const target = e.target;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) return;
  const tab = TAB_KEYS[e.key.toLowerCase()];
  if (tab) {
    selectTab(tab);
    document.querySelector(`.tab[data-tab="${tab}"]`).focus();
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

    if (s.connected) {
      setPill(status, statusDot, "ok", "connected");
      connLabel.textContent = "connected";
      connDot.className = "dot ok";
      errorBox.classList.add("hidden");
    } else if (s.error) {
      setPill(status, statusDot, "bad", "failed");
      connLabel.textContent = "failed";
      connDot.className = "dot bad";
      errorBox.textContent = s.error;
      errorBox.classList.remove("hidden");
    } else {
      setPill(status, statusDot, "connecting", "connecting…");
      connLabel.textContent = "connecting…";
      connDot.className = "dot connecting";
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
