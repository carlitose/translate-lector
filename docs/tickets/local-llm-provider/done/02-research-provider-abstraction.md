# 02 — Astrazione di provider nell'app (base-URL/key/modello configurabili)

## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)

## Type

research

## Outcome

Un design chiuso per trasformare l'attuale client single-endpoint in un'**astrazione di provider**:
base-URL configurabile, key **opzionale** per-provider, modello per-provider, e un selettore
(OpenRouter | Locale) in impostazioni, con la relativa persistenza. Pronto per le build verticali.

## Acceptance Criteria

- [ ] Design di come rendere configurabile la base-URL oggi hardcoded (`OPENROUTER_URL`,
      `src-tauri/src/llm.rs:13`): campo per-provider vs override globale.
- [ ] Modello di configurazione provider: `{ id, label, base_url, requires_key, model }`; dove si persiste
      (tabella `settings` §4.3 vs nuova tabella), e come si sceglie il provider attivo.
- [ ] Key **opzionale**: adattare `isValidKey` (`src/lib/providerConfig.ts`) e il flusso keychain
      (`src-tauri/src/secrets.rs`) perché un provider locale possa non avere chiave senza rompere la validazione.
- [ ] UI: come estendere `ProviderConfig.svelte` / il pannello impostazioni (§3.5) con selettore provider e
      campi base-URL/modello; default sensati per il provider locale (es. `http://localhost:PORT/v1`).
- [ ] Conferma che la **ladder di degradazione** (`provider`→`response_format`→`temperature`) e l'estrazione
      JSON di fallback restano valide e utili per gli endpoint locali.
- [ ] Impatto sulle chiamate esistenti (`translate.rs`) e sui test.

## Blocked By

- Ticket 01 (servono endpoint/porta/auth reali per definire i campi e i default del provider locale).

## Frontier

È il ponte tra "so come serve il modello" (01) e "l'app può parlarci". Definisce la superficie di
configurazione che le build verticali implementeranno.

## Work Plan

1. Rivedere `llm.rs` (costruzione richiesta, `OPENROUTER_URL`, ladder), `providerConfig.ts`,
   `ProviderConfig.svelte`, `settings.rs`/`settings.ts`, `secrets.rs`.
2. Progettare il modello dati provider e la persistenza (riuso `settings` se possibile).
3. Progettare il flusso key-opzionale e i default del provider locale.
4. Abbozzare le modifiche UI del pannello impostazioni.
5. Scrivere il design nel parent spec, pronto per `to-tickets`.

## Evidence to Capture

- Bozza del modello dati provider e SQL/persistenza.
- Elenco puntuale dei punti di modifica (file:funzione) in core e frontend.
- Nota su compatibilità con la ladder e i test esistenti.

## Out of Scope

- Implementazione (build verticali).
- Validazione qualità/contratto del modello locale (Ticket 03).
