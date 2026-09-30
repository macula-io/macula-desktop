# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-09-30

### Changed

- The mesh core is on `macula-rust` 0.6: every station link dials handshake
  v5, bound to its TLS session, and a call to a provider that advertises a
  KEM key is never made in the clear.
- Chat's memory step after each answer runs in its own function, within the
  house nesting limit.

## [0.1.0] - 2026-09-29

The first release: a desktop client that holds a real session on the
Macula fleet.

### Added

- The mesh core on `macula-rust` 0.5.1 from crates.io: a pool linked to the
  six fleet stations, each pinned by the node_id it must prove, under a
  persistent pq_hybrid identity (`node.key` next to the settings, admission
  puzzle solved on first run), trusting the io.macula realm key, retrying
  until a link is up.
- Presence (`agent.hello`), the lobby (`agents.lobby`: room announcements
  and help broadcasts) and joined rooms in io.macula, where macula-mcp
  publishes them.
- Calls by direct dial in the operating realm (io.macula when none is
  chosen), the `mcl-rag/*` memory procedures, and node-served content.
- The five-tab shell (Mesh, Chat, Teams, Services, Realms) over Tauri's
  invoke-only IPC.
- Releases: a plain executable per OS (Linux x64, macOS arm64, Windows x64)
  with `SHA256SUMS`. On Linux the only runtime dependency is the system
  WebKitGTK 4.1.
