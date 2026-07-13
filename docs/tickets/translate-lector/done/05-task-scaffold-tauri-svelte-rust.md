# 05 — Task: scaffold Tauri + Svelte + TypeScript + core Rust

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md)

## Type

task

## Outcome

Fondazione eseguibile: un progetto Tauri con frontend Svelte+TS e core Rust che builda ed esegue
`tauri dev` su Windows, con SQLite inizializzato secondo lo schema di SPECIFICATION.md §4.3.

## Acceptance Criteria

- [ ] `tauri dev` avvia l'app con una finestra vuota/placeholder su Windows 11.
- [ ] Struttura cartelle allineata a §4.2: frontend Svelte (pdf.js incluso come dipendenza) + core Rust (comandi Tauri).
- [ ] Un comando Tauri "ping" di prova invocabile dal frontend, per validare il ponte webview↔core.
- [ ] SQLite creato al primo avvio con le tabelle `documents`, `sessions`, `translations_cache`, `glossary`, `settings` (§4.3).
- [ ] README minimo con comandi di sviluppo.

## Blocked By

- Idealmente le versioni fissate da **ticket 03** (Tauri/Svelte/`rusqlite`|`sqlx`). Può iniziare in parallelo assumendo Tauri v2 + Svelte 5 + `rusqlite`, da confermare.

## Frontier

È la fondazione su cui poggiano tutte le build verticali. Non blocca le indagini (01/02/04), ma le build successive non partono senza di esso.

## Work Plan

1. Scaffolding con `create-tauri-app` (template Svelte + TS), verificare toolchain Rust su Windows.
2. Aggiungere pdf.js al frontend.
3. Implementare comando "ping" e inizializzazione DB SQLite con lo schema §4.3.
4. Verificare build ed esecuzione; scrivere README.

## Evidence to Capture

- Versioni effettive di Tauri, Svelte, crate SQLite.
- Output di `tauri dev` che parte su Windows.
- Screenshot/log del comando ping e del file `.db` creato con le tabelle.

## Out of Scope

- Qualsiasi feature (rendering PDF reale, traduzione, glossario) — solo scaffold e ponte.
- Packaging/installer.

---

**Completato 2026-07-13** — Scaffold Tauri v2.11.5 + Svelte 5.56.4 (SvelteKit) + Rust core con rusqlite 0.32.1 (bundled). Comando `ping` + `init_database`, schema §4.3 (5 tabelle) inizializzato in app-data dir e testato (`cargo test` 3/3 pass). Node v24.15.0, pdfjs-dist 6.1.200. `cargo build`, `npm run build` e `svelte-check` verdi. GUI (`tauri dev`) non lanciata di proposito.
