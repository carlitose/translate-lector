# 05 — Task: gestione del modello GGUF

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Type

task

## Decisioni dal grilling 03

Vedi [decision-brief-llamacpp-direct-03.md](../../specs/decision-brief-llamacpp-direct-03.md).
Rilevanti qui: **D0** (uso personale, niente download-manager), **D2** (due path espliciti con
default precompilati, errore azionabile), + assunzione 1 (casa stabile del binario ufficiale).

## Outcome

L'app conosce **due path locali** — il binario llama-server e il file GGUF — da passare allo
spawner del ticket 04. Path espliciti in ⚙️, precompilati ai valori che funzionano; errore azionabile
se un file manca. Nessun download gestito (D0: i file sono già sul disco).

## Acceptance Criteria

- [ ] **Due impostazioni nuove** per il provider `llamaserver`: `provider.llamaserver.binary_path`
      e `provider.llamaserver.model_path` (o equivalenti), con lo stesso meccanismo di override di
      `base_url`/`n_ctx`/`timeout_secs` già in `settings.rs`.
- [ ] **Default del binario = casa stabile** (assunzione 1): la release ufficiale llama.cpp
      installata in una dir fissa nota; **non** puntare al build Unsloth (dipende dalle DLL del venv
      di Studio). Documentare la dir e come popolarla.
- [ ] **Default del modello** = il GGUF gemma-4-E2B-it-qat-UD-Q4_K_XL già in cache HF (path esplicito,
      non auto-glob — D2 esclude l'auto-detect per fragilità dell'hash di snapshot).
- [ ] **File mancante → errore azionabile** ("imposta il path del binario/modello in ⚙️"), non uno
      spawn opaco.
- [ ] **UI ⚙️**: due campi path (con eventuale file-picker) e stato "trovato/mancante".
- [ ] Test unitari sulla risoluzione del path (esiste / manca / override); suite verde.

## Blocked By

- [03-grilling-llamacpp-direct-decisions.md](./done/03-grilling-llamacpp-direct-decisions.md) →
  **done**. Sbloccato. Coordinare col ticket 04 (lo spawner consuma questi due path).

## Frontier

Il sidecar (04) non parte senza un `-m <path>` e un binario validi: questa è la sua unica dipendenza
dati.

## Work Plan

1. Aggiungere le due chiavi di override in `settings.rs` (riuso di `resolve_*_override`) e i default.
2. Installare/collocare la release ufficiale llama.cpp in una dir stabile; default del binary_path lì.
3. TDD sulla risoluzione del path (esiste/manca/override) e sul messaggio d'errore.
4. UI ⚙️ per i due path; wiring col ticket 04.

## Evidence to Capture

- Path risolti nei tre casi; screenshot ⚙️.
- Dir stabile scelta per il binario ufficiale.

## Out of Scope

- Download gestito di modelli/binari (D0: fuori scope, uso personale).
- Model picker generale / modelli diversi da gemma-4.
