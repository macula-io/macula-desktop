# macula-desktop — Desktop GUI for the Macula mesh (Exploration)

**Status:** Exploration — direction set by Raf 2026-09-13 (revised); no repo, no code
**Created:** 2026-09-13
**Last Updated:** 2026-09-13

## End goal

> This exploration exists so macula-desktop can give the Macula mesh a
> real GUI — chat over rooms, the team board, mesh state, realms — as a
> self-contained installable/executable that is itself a first-class
> mesh client on the macula SDK, with no dependency on lazymesh,
> macula-cli, or any prior art.

**Classification:** Exploration. The build itself is a BUILD.

## Raf's constraints (2026-09-13, verbatim intent)

- **Self-contained installable/executable.** No runtime dependencies.
- **SDKs only.** macula-desktop depends on the macula SDK — the choice
  of six stacks (Rust, Go, TypeScript, PHP, .NET, Python) — and nothing
  else from the macula-io ecosystem.
- **NO dependency on macula-lazymesh or macula-cli.** The desktop is
  not a renderer over lazymesh's daemon; it does not bundle, spawn, or
  talk to it.
- **No prior art.** hecate-web lives archived on GitHub; macula-desktop
  starts clean.
- **Full keyboard capability** (2026-09-13): every action must be
  reachable from the keyboard, not just the mouse. The terminal TUI
  died partly from keyboard limits; the desktop must not regress — tabs,
  focus traversal, message sending, approvals and table filtering all
  get keys. This is a first-class requirement, not a nicety, so the
  shell's keymap is designed alongside the mouse affordances from the
  first slice onward.
- **Cross-platform** (2026-09-13): the binary must run on Linux,
  Windows and macOS. Tauri gives this natively (WebKitGTK / WebView2 /
  WKWebView), but it is a real commitment: CI builds for all three
  targets, and every platform-conditional in the Rust core is earned.
- **Familiar installation** (2026-09-13): the well-known
  `curl -fsSL https://…/install.sh | bash` shape for Linux and macOS,
  with PowerShell equivalents for Windows — always paired with an
  uninstaller of the same shape, checksum-verified downloads, and a
  user-local install (no sudo) as the default.

## What this means architecturally

The macula SDK already defines the client role: "SDK provides: client
transport, wire protocol, identity (Ed25519/UCAN/DID NIFs), MRI resource
identifiers, cert system" — and consumers are instructed to never
re-implement these, only pull them from the SDK. macula-desktop is a
**future SDK client**: a peer on the mesh exactly like lazymesh is,
using the same identity, rooms, DHT, pub/sub and RPC — but its own
process, its own surface, its own lifecycle.

```
┌────────────────────────────────────────────────────────────┐
│ macula-desktop (Tauri, one binary)                         │
│                                                            │
│   webview UI: markdown, selection, tables, tabs            │
│        ▲ Tauri IPC (commands + events)                     │
│   Rust core (the in-app "daemon"):                         │
│     ── macula-rust SDK ──                                  │
│     identity, rooms, pubsub, RPC, DHT, realm membership,   │
│     mesh state in memory                                   │
└───────────────────────────┬────────────────────────────────┘
                            │ QUIC (post-quantum per the SDK's timeline)
                       the Macula mesh
```

- **Tauri** satisfies the self-contained constraint: one small binary,
  no Node, no browser server. The Rust shell makes the **macula-rust
  SDK** the natural pick.
- **The Tauri backend IS the daemon** (Raf, 2026-09-13): the Rust core
  owns the mesh connection, identity, subscriptions and in-memory mesh
  state, and feeds the webview over Tauri IPC. The webview never
  touches the mesh directly — every fact on screen came through the
  Rust core.
- Because the Tauri backend outlives the window (tray mode), the app
  can hold mesh presence, ring-answering and subscriptions while the
  window is closed — the desktop equivalent of what a daemon does, with
  no separate process to install or supervise.

## IPC security posture (Raf, 2026-09-13)

- **The webview talks to the Rust core exclusively over Tauri's built-in
  invoke IPC** — a custom protocol handler bound inside the webview
  process (no TCP listener, nothing network-reachable). Each command is
  an explicitly registered Rust function, so the webview can call only
  what the app whitelists — the mesh connection itself is never
  exposed to the web layer.
- **Tauri never opens a TCP port, ever.** No localhost HTTP server, no
  socket listener, no port to scan: the only network traffic in or out
  of the app is the Rust core's own QUIC connection to the mesh.
- The mesh never serves anything back over a port either: all mesh data
  arrives through the SDK inside the Rust core and is pushed to the
  webview as IPC events, so a compromised webview (XSS) can reach the
  mesh only through the registered command surface.
- The desktop rides the SDK's own post-quantum timeline: it is a Stage
  4-class consumer in `PLAN_POST_QUANTUM_SECURITY.md` (each stack
  against the fleet), not a Stage 5 cutover of anyone else's binary.
- Everything the terminal TUI renders — rooms, rings, presence, team
  board, realms, services — is reachable through the SDK's existing
  surfaces; nothing about them is lazymesh-specific.

## Applications: the desktop as the ecosystem's launcher

Raf's model (2026-09-13): hecate services run on edge boxes in the
operator's LAN and expose two faces — a mesh API to the fleet and a
plain-HTTP admin UI to the LAN (hecate-tube's streaming API vs. its
Admin API being the canonical example). A desktop on that same LAN can
deep-link into both, so the sidebar gains an **Applications** section
with two tabs:

- **Local Applications** — LAN-hosted admin UIs, discovered via **mDNS**
  (local discovery is already in the SDK's toolkit) or explicit config;
  each entry is a name, description, and HTTP URL opened on click.
- **Mesh Applications** — remotely hosted UIs, discovered via **DHT
  `content_announcement` / `procedure_advertisement` records**; each
  entry is an MRI or MCID resolved through the mesh.

Mesh-hosted UIs are already the platform's own design, not an
invention:

- The **content system** is content-addressed: "location stops
  mattering — any host holding the bytes serves the same MCID"
  (`macula/docs/guides/content/CONTENT_GUIDE.md`). A static web UI is
  content: build, `macula_feeder`, announce, `macula_download` with
  integrity self-verified against the MCID.
- **D27** (`PLAN_POST_QUANTUM_SECURITY.md`): the sharing node keeps and
  serves content; stations pass it through.
- **MRIs name it** (`mri:{type}:{realm}/path`) and the relay's surface
  includes a **gateway** alongside content and registry; the transport
  is HTTP/3, so even a plain browser can open a gateway URL.

**Security carve-out (explicit, not accidental):** the main webview
stays IPC-only and never touches the network. Application UIs are the
deliberate exception, in three shapes:

1. **v1 (built 2026-09-13): embedded iframe in the content panel** —
   the app's own JS never fetches anything; the embedded browsing
   context is origin-isolated by the browser engine, the sidebar stays,
   and the system browser is one toolbar click away for sites that
   refuse to be framed.
2. **system browser** — the per-app "Open in browser" button and the
   `open_external` command (refuses anything non-http(s) before
   spawning).
3. **later, only if earned:** dedicated secondary Tauri windows scoped
   to one origin — more complexity than the iframe for little extra
   isolation, given the engine already isolates the frame by origin.

Neither shape ever relaxes the main window's rule: no TCP ports, invoke
only.

## What genuinely beats the TUI (unchanged from the terminal's limits)

- Real markdown rendering (the webview's native job, not hand-rolled ANSI).
- Mouse-native selection and clipboard.
- Filterable tables: services catalog, team board, presence.
- Tabs as real UI; no keybinding scarcity.
- Notifications/tray presence for rings and approvals.

## Open questions

1. ~~Which SDK?~~ **Decided 2026-09-13: macula-rust, with the Tauri
   backend as the mesh-connecting daemon** (webview stays IPC-only).
2. **Agent functionality**: is the desktop a pure mesh client (chat is
   rooms, work is services — agents live on the mesh, e.g. the
   hecate-experts pool), or does it also embed a local personal-agent
   loop (its own LLM calls through the SDK, no lazymesh involved)?
3. **First window**: chat-over-rooms first, or the Teams board first
   (the feature a terminal renders worst)?
4. **Applications first, or mesh features first?** The Applications
   section is the quickest way to real value (LAN services already
   exist); the mesh tabs need more SDK surface. Order is an operator
   decision, not a technical one.
5. **Local-app discovery**: mDNS announcements from hecate services, or
   explicit config in the desktop for v1? (Recommendation: config first
   — it works today with zero changes to the services; mDNS when the
   services announce themselves.)

## What NOT to do

- No dependency on macula-lazymesh, macula-cli, or their unix-socket
  control plane.
- No porting of hecate-web code — archived art stays archived.
- No web-server deployment mode for v1 — self-contained executable is
  the point.
- No TCP listener of any kind, in any mode — IPC is Tauri invoke only.
