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

**The invariants (from `plans/EXPLORATION_MACULA_DESKTOP.md`):**

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

**Walking skeleton.** What works end-to-end today:

- the five-tab shell (Mesh / Chat / Teams / Services / Realms),
- the invoke-only IPC path,
- a real mesh link in the Rust core: puzzle-hardened identity, QUIC
  handshake to `station-de-frankfurt.macula.io:4433`, session held for
  the app's lifetime, live status in the Mesh tab.

Chat over rooms, the Teams board, the services catalog, and realm
membership are the next layers on top of this link; their tabs say so
honestly.

## Install

**Linux / macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/macula-io/macula-desktop/main/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/macula-io/macula-desktop/main/install.ps1 | iex
```

Both pull the release archive matching your OS/arch from
[GitHub Releases](https://github.com/macula-io/macula-desktop/releases),
verify it against the release's own `checksums.txt`, and install
`macula-desktop` (`$HOME/.local/bin` on Linux/macOS,
`%LOCALAPPDATA%\macula-desktop` on Windows — override with
`MACULA_DESKTOP_INSTALL_DIR`).

To remove it again: `curl -fsSL .../uninstall.sh | bash` (or
`irm .../uninstall.ps1 | iex` on Windows) — same repo path,
`uninstall.sh`/`uninstall.ps1` instead of `install`.

*Releases arrive with the first tagged build; until then, build from
source:*

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
explicitly registered function. Full design and rationale:
[`plans/EXPLORATION_MACULA_DESKTOP.md`](plans/EXPLORATION_MACULA_DESKTOP.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE](LICENSE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
