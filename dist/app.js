// The webview's only data path: registered Tauri commands over the
// built-in invoke IPC. No fetch, no sockets, no ports.
const { invoke } = window.__TAURI__.core;

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

async function refreshMesh() {
  try {
    const s = await invoke("mesh_status");
    document.getElementById("station").textContent = s.station;
    document.getElementById("identity").textContent = s.identityGenerated
      ? s.nodeId
      : "generating puzzle-hardened keypair…";
    const statusEl = document.getElementById("status");
    if (s.connected) {
      statusEl.textContent = "connected";
      statusEl.className = "pill ok";
    } else if (s.error) {
      statusEl.textContent = "failed";
      statusEl.className = "pill bad";
      document.getElementById("error").textContent = s.error;
    } else {
      statusEl.textContent = "connecting…";
      statusEl.className = "pill connecting";
    }
  } catch (e) {
    document.getElementById("error").textContent = `IPC error: ${e}`;
  }
}

refreshMesh();
setInterval(refreshMesh, 1000);
