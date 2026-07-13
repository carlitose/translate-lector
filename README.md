# translate-lector

Desktop PDF reader with side-by-side AI translation, context perception
(rolling summary + dynamic glossary) and full session restore.

See [SPECIFICATION.md](./SPECIFICATION.md) for the product/design contract and
[docs/specs/translate-lector-wayfinder.md](./docs/specs/translate-lector-wayfinder.md)
for the wayfinding map.

## Stack

| Layer     | Tech |
|-----------|------|
| App shell | Tauri v2 |
| Frontend  | Svelte 5 + TypeScript (SvelteKit, SPA/static adapter) in the webview |
| PDF       | pdf.js (`pdfjs-dist`) |
| Core      | Rust (Tauri commands) |
| Storage   | SQLite via `rusqlite` (bundled), single `.db` file in the OS app-data dir |

## Prerequisites

- Node.js 20+ and npm
- Rust stable toolchain (target `x86_64-pc-windows-msvc` on Windows)
- Tauri v2 platform prerequisites (WebView2 runtime + MSVC build tools on Windows).
  See https://v2.tauri.app/start/prerequisites/

## Setup

```bash
npm install
```

## Dev commands

```bash
npm run tauri dev     # run the desktop app (webview + Rust core, hot reload)
npm run dev           # run only the Vite/SvelteKit frontend in a browser
npm run build         # build the frontend into ./build (embedded by Tauri)
npm run check         # svelte-check / TypeScript type check
npm run tauri build   # produce a release bundle/installer
```

### Rust core

```bash
cd src-tauri
cargo build           # compile the core
cargo test            # unit tests (includes SQLite schema init test)
```

## Project layout

```
src/                  Svelte 5 + TS frontend (SvelteKit routes)
static/               Static frontend assets
src-tauri/            Rust core (Tauri v2)
  src/lib.rs          Tauri commands: `ping`, `init_database`; DB init on setup
  src/db.rs           SQLite schema (SPECIFICATION.md §4.3) + tests
  Cargo.toml          Rust deps (tauri, rusqlite bundled, serde)
  tauri.conf.json     Tauri config
docs/                 Specs and tickets
SPECIFICATION.md      Product & system design source of truth
```

## Bridge & storage notes

- `ping` — Tauri command returning a string from Rust, used to validate the
  webview↔core bridge (`invoke("ping")`).
- `init_database` — creates/opens `translate-lector.db` in the OS app-data dir
  and initialises the schema. The DB is also initialised automatically at
  startup in the Tauri `setup` hook.
- Tables (SPECIFICATION.md §4.3): `documents`, `sessions`, `translations_cache`,
  `glossary`, `settings`.
