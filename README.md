# macula-desktop

[![CI](https://img.shields.io/github/actions/workflow/status/macula-io/macula-desktop/ci.yml?branch=main&label=CI)](https://github.com/macula-io/macula-desktop/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/rust-stable-6E40C9.svg?logo=rust)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB.svg?logo=tauri)](https://tauri.app)
[![GitHub Sponsors](https://img.shields.io/badge/GitHub%20Sponsors-support-ea4aaa.svg?logo=githubsponsors&logoColor=white)](https://github.com/sponsors/rgfaber)

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/macula-desktop-full-dark.svg">
    <img src="assets/macula-desktop-full-light.svg" alt="Macula Desktop" width="320">
  </picture>
</p>

<p align="center">
  <strong>A desktop GUI for the Macula mesh — a window onto the mesh</strong>
</p>

---

## What is macula-desktop?

A **self-contained desktop client** for the Macula mesh: a Tauri app
whose Rust core is itself a first-class mesh citizen, built directly on
the published [`macula-rust`](https://github.com/macula-io/macula-rust)
SDK crate. One installable executable, no runtime dependencies, no
server to start, no TCP ports opened — ever.

It exists because the terminal TUI's ceiling is real: markdown rendered
by hand, keyboard protocols reverse-engineered terminal by terminal,
mouse selection as a reverse-video overlay. A webview does all of that
natively — and a desktop app can hold mesh presence in the tray while
the window is closed, which a terminal cannot do at all.

**The invariants:**

- **SDKs only** — the only macula-io dependency is the published
  `macula-rust` crate; no macula-lazymesh, no macula-cli, no prior art.
- **IPC-only** — the webview talks to the Rust core exclusively over
  Tauri's registered `invoke` commands; the webview never touches the
  network, and Tauri never opens a TCP listener. The app's only network
  traffic is the Rust core's QUIC connection to the mesh.
- **Keyboard-first** — every action reachable from the keyboard, not
  just the mouse.
- **Cross-platform** — Linux, Windows, and macOS.

## Status

**Early.** The Rust core runs on `macula-rust` 0.5.1 (the macula 12/13
wire, handshake v4, which today's stations accept):

- a pool linked to the six fleet stations, each pinned by the node_id it
  must prove, redialing any link that ends, under a persistent pq_hybrid
  identity (`node.key` next to the app's settings, admission puzzle
  solved on first run), trusting the io.macula realm key;
- the social layer in io.macula, where macula-mcp publishes it:
  `agent.hello` presence (a heartbeat every 60 s and the roster), the
  `agents.lobby` room announcements and help broadcasts, and each joined
  room on its own topic;
- calls by direct dial in the operating realm (io.macula when none is
  chosen), including the `mcl-rag/*` memory procedures, and node-served
  content;
- the five-tab shell (Mesh / Chat / Teams / Services / Realms) over the
  invoke-only IPC path.

The core's live path (connect, a call to `mcl-echo/echo`, hearing its own
publication) is checked against the fleet by an ignored test:
`cargo test -- --ignored live_`. Handshake v5 arrives with a later
`macula-rust` release as an ordinary version update.

## Install

Each release carries a plain executable per OS, nothing to install:
`macula-desktop-linux-x64.tar.gz`, `macula-desktop-macos-arm64.tar.gz` and
`macula-desktop-windows-x64.zip`, with `SHA256SUMS`, on
[GitHub Releases](https://github.com/macula-io/macula-desktop/releases).
Unpack and run it from the command line:

```bash
tar xzf macula-desktop-linux-x64.tar.gz && ./macula-desktop
```

**The only runtime dependency** on Linux is the system WebKitGTK 4.1
(Arch: `pacman -S webkit2gtk-4.1`; Debian/Ubuntu: `apt install
libwebkit2gtk-4.1-0`). macOS and Windows ship their webview with the OS.

Or let a script pick the archive for this machine, check it against
`SHA256SUMS` and put `macula-desktop` on your PATH:

```bash
curl -fsSL https://raw.githubusercontent.com/macula-io/macula-desktop/main/install.sh | bash
```

```powershell
irm https://raw.githubusercontent.com/macula-io/macula-desktop/main/install.ps1 | iex
```

They install into `$HOME/.local/bin` (Linux/macOS) or
`%LOCALAPPDATA%\macula-desktop` (Windows); override with
`MACULA_DESKTOP_INSTALL_DIR`, or pin a release with
`MACULA_DESKTOP_VERSION=v0.1.0`.

To remove it again: `curl -fsSL .../uninstall.sh | bash` (or
`irm .../uninstall.ps1 | iex` on Windows) — same repo path,
`uninstall.sh`/`uninstall.ps1` instead of `install`.

## Build from source

Prerequisites: a stable Rust toolchain, and Tauri's per-platform system
dependencies (on Debian/Ubuntu: `libwebkit2gtk-4.1-dev libgtk-3-dev
libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev
libjavascriptcoregtk-4.1-dev`).

```sh
cd src-tauri
cargo tauri dev --no-dev-server   # dev window
cargo build                        # plain debug binary in target/debug/
```

## Architecture

The Rust core is the mesh client (identity, rooms, pub/sub, RPC, DHT,
realm membership via `macula-rust`); the webview renders and never
touches the network; Tauri IPC in between, with each command an
explicitly registered function. Planned work is tracked in
[issues labelled `plan`](https://github.com/macula-io/macula-desktop/issues?q=is%3Aissue+is%3Aopen+label%3Aplan).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE](LICENSE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
