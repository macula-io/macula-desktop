# macula-desktop

Desktop GUI for the Macula mesh: a Tauri shell whose Rust core is the
mesh client, built on the published `macula-rust` SDK crate. See
`plans/EXPLORATION_MACULA_DESKTOP.md` for the architecture and
constraints (SDK-only dependency, no lazymesh/macula-cli, no TCP ports,
self-contained executable).

## Walking skeleton (current state)

What exists end-to-end today:

- the five-tab shell (Mesh / Chat / Teams / Services / Realms),
- Tauri IPC (`invoke` only, no network from the webview),
- a Rust-core mesh link that really dials the fleet: it generates a
  puzzle-hardened identity, connects to
  `station-de-frankfurt.macula.io:4433` over QUIC, and holds the session
  while the app runs.

Chat/teams/services/realms content is the next layer on top of this
link; their tabs say so honestly.

## Run

```sh
cd src-tauri
cargo tauri dev --no-dev-server
```

or, for a plain debug binary:

```sh
cd src-tauri
cargo build
./target/debug/macula-desktop
```
