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

## What NOT to do

- No dependency on macula-lazymesh, macula-cli, or their unix-socket
  control plane.
- No porting of hecate-web code — archived art stays archived.
- No web-server deployment mode for v1 — self-contained executable is
  the point.
- No TCP listener of any kind, in any mode — IPC is Tauri invoke only.
