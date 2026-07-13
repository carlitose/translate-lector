> ✅ Completato il 2026-07-13 — pannello ⚙️ (API key mascherata + modello) con comandi core `has_api_key`/`get_setting`/`set_setting`/`get_model`; 15 test Rust + 10 vitest verdi, check/build ok.

# 07 — Configurazione minima provider (API key + modello)

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md) · design: [SPECIFICATION.md](../../../SPECIFICATION.md) §3.5, FR11/FR12, UC05, NFR07

## What to Build

Slice minima che permette all'app di **avere le credenziali** per chiamare l'LLM: un pannellino (accessibile da ⚙️, versione ridotta della futura schermata Impostazioni) dove l'utente **inserisce/aggiorna/cancella la API key OpenRouter** (salvata nel keychain, comandi già esistenti `store/load/clear_api_key`) e **sceglie il modello** (salvato in `settings`, default `anthropic/claude-sonnet-5`, D5). Mostra se una key è già presente (senza rivelarla). È il prerequisito perché il ticket 08 possa tradurre davvero.

## Acceptance Criteria

- [ ] Da ⚙️ si apre un pannello con: campo API key (mascherato), campo/dropdown modello, pulsanti Salva ed Elimina key.
- [ ] Salvare la key la scrive nel keychain (via `store_api_key`); il pannello poi indica "key presente" senza mostrarla.
- [ ] Il modello scelto è persistito in `settings` (key `model`); default `anthropic/claude-sonnet-5` se assente.
- [ ] Eliminare la key la rimuove dal keychain (`clear_api_key`) e il pannello torna a "nessuna key".
- [ ] Nessuna API key transita/è loggata nella webview oltre l'input momentaneo (resta nel core).

## Blocked By

- None - can start immediately (comandi keychain e tabella `settings` già esistono).

## Frontier

**Ready now.** Indipendente da 06; eseguibile in parallelo.

## Step-by-Step Implementation Plan

1. **Comandi settings nel core** (`src-tauri`): aggiungi `get_setting(key)`/`set_setting(key,value)` su tabella `settings` (con default per `model`). *Affects*: modulo `db.rs`/nuovo `settings.rs`; unit test get/set/default.
2. **Comando `has_api_key`**: ritorna bool derivato da `load_api_key().is_some()` per popolare la UI senza esporre il segreto. *Perché*: la UI non deve mai ricevere la key in chiaro se non durante l'inserimento.
3. **Frontend — pannello config** (`src/`): form con API key (type=password), modello (dropdown dei più comuni + campo libero, D5), stato "key presente/assente". Bottoni Salva/Elimina invocano i comandi. *Verifica*: `npm run check` pulito.
4. **Validazione leggera**: key non vuota prima di salvare; modello non vuoto (fallback al default). *Pitfall*: non validare la key con una vera chiamata qui (è compito di 08); evitare falsi gate.

## Testing Plan

- **Rust unit**: `get_setting`/`set_setting` con default `model`; `has_api_key` riflette lo stato del keychain (usa account throwaway come nel test di `secrets.rs`).
- **Manuale**: salvare una key fittizia → "presente"; riavviare → resta presente (keychain persistente); eliminare → "assente".

## Out of Scope

- La schermata Impostazioni completa (lingua default, prefetch, limite summary, cartella dati, svuota cache) → ticket 13.
- Qualsiasi chiamata reale all'LLM → ticket 08.
