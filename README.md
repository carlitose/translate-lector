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

## Translation providers

Translation runs through a selectable **provider** (configured in ⚙️). The
**default on a clean install is the direct llama.cpp local provider**
(`llamaserver`), so translation works locally out-of-the-box; OpenRouter (cloud)
and the other local providers stay selectable. An existing choice is always kept.

### Direct llama.cpp provider (`llamaserver`, default)

The app runs a local [`llama-server`](https://github.com/ggml-org/llama.cpp)
itself — no Unsloth Studio proxy, no terminal to babysit:

- **Spawns on demand**: `llama-server` is started at the *first* local
  translation (not at app launch, so opening a PDF you only want to re-read never
  loads the model), reused while healthy, and **killed automatically when the app
  exits**. A server orphaned by a hard crash is reaped on the next launch.
- **Two paths in ⚙️** (each with a precompiled default, both overridable). The
  shipped defaults are literally one machine's absolute paths (personal-use scope,
  see below) — on any other machine you must point them at your own files in ⚙️:
  - **Binary path** — the official `llama-server.exe` (+ its sibling CUDA DLLs).
    Default: `C:\Users\CGS03\.translate-lector\llama.cpp\llama-server.exe`.
  - **GGUF model path** — an explicit path to the model file (no auto-glob, so it
    is always clear which GGUF is in use). Default points at the gemma model in
    the HuggingFace cache (a fixed snapshot path under `C:\Users\CGS03\...`).
- **Server flags** (wired at spawn, D4): port `8080`, `-ngl 99` (full GPU
  offload), `-c 4096` (context; kept in sync with the provider `n_ctx`),
  `--reasoning off` (suppresses chain-of-thought — the whole point of this
  provider), `--parallel 1`.
- **GPU required**: full offload (`-ngl 99`) targets a CUDA GPU. The reference
  setup is an RTX 500 Ada (4 GB); this model fits (~1.5 GB used). A build without
  a working CUDA runtime beside the binary will not start.

> Personal-use build (decision D0): the binary and model are **not** bundled in
> an installer and nothing is downloaded — the app only launches an external
> binary already on disk at the configured path.

#### Troubleshooting

- **Server does not start / "Binario llama-server non trovato" or "Modello GGUF
  non trovato"** → the configured path is blank or missing; fix the binary/GGUF
  path in ⚙️. The app surfaces the actionable message instead of an opaque spawn.
- **"avviato ma non ha risposto in tempo"** (started but no reply within ~30 s)
  → check the GGUF path in ⚙️ and that the port is not already occupied.
- **"base_url … senza porta esplicita"** → the provider `base_url` must carry an
  explicit port (the shipped `http://127.0.0.1:8080/...` does); set it in ⚙️.
- Prefer running a single app instance: the managed server is tracked by a shared
  PID file.

### Unsloth Studio (`unsloth`) — alternative

Unsloth Studio remains a normal, first-class provider (it is user-launched, not
app-managed). Keep it if you use Studio for other work; select it in ⚙️ and point
its `base_url` at Studio's port (default `8888`).

## Bridge & storage notes

- `ping` — Tauri command returning a string from Rust, used to validate the
  webview↔core bridge (`invoke("ping")`).
- `init_database` — creates/opens `translate-lector.db` in the OS app-data dir
  and initialises the schema. The DB is also initialised automatically at
  startup in the Tauri `setup` hook.
- Tables (SPECIFICATION.md §4.3): `documents`, `sessions`, `translations_cache`,
  `glossary`, `settings`.
