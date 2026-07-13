# 12 — Prefetch pagina successiva + stati + gestione errori

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md) · design: [SPECIFICATION.md](../../../SPECIFICATION.md) §3.2, §3.1 (barra stato), NFR05/NFR06, EC02/EC07

## What to Build

Rende l'esperienza fluida e robusta: **prefetch** in background della pagina successiva (D5 ON) così che spesso la traduzione sia già pronta; **indicatori di stato** nella barra inferiore (spinner "traduzione in corso", "● Tradotto (cache)", "errore"); **gestione errori** rete/API con **retry + backoff esponenziale** e messaggi chiari; uso della cache disponibile quando offline (EC02); gestione rate limit/costi (EC07). UI non bloccante (NFR05).

## Acceptance Criteria

- [ ] Con prefetch ON, all'arrivo su pagina N si avvia in background la traduzione di N+1; navigando avanti spesso è già pronta (dalla cache).
- [ ] La barra inferiore riflette lo stato: in corso (spinner) / da cache / errore.
- [ ] Un errore di rete/API attiva retry con backoff esponenziale (numero tentativi limitato); esaurito il retry, messaggio chiaro all'utente senza bloccare la UI (NFR06).
- [ ] Offline: le pagine già in cache restano leggibili; le nuove mostrano errore gestito (EC02).
- [ ] Rate limit (429) gestito con backoff e messaggio dedicato (EC07).
- [ ] Navigare via da una pagina mentre traduce non rompe lo stato (richiesta obsoleta ignorata/annullata).

## Blocked By

- [09-percettore-summary-glossary.md](./09-percettore-summary-glossary.md)

## Frontier

Bloccato da 09 (il prefetch deve tradurre con il contesto percettore corrente). AFK per stati/retry/annullamento (mock); comportamento reale su rate limit verificabile con key reale.

## Step-by-Step Implementation Plan

1. **Retry/backoff nel client** (`llm.rs`): wrapper con tentativi limitati e backoff esponenziale su errori transitori (timeout, 5xx, 429). Unit test con client mock che fallisce poi riesce.
2. **Prefetch**: dopo aver mostrato la pagina N, se prefetch ON e N+1 non è in cache, avvia in background `translate_page(N+1)`; scrivi in cache. *Pitfall*: aggiornare summary/glossario da una pagina prefetchata fuori ordine è pericoloso — per l'MVP il prefetch salva solo la traduzione in cache e **non** avanza il rolling_summary finché l'utente non arriva davvero su quella pagina (evita corruzione del contesto).
3. **Annullamento/obsolescenza**: identifica ogni richiesta per (document, page, lingua); ignora i risultati non più pertinenti alla pagina corrente. Test su race navigazione.
4. **Indicatori di stato** (frontend): stato per pagina (idle/loading/cached/error) mostrato in barra. *Verifica*: `npm run check` pulito.
5. **EC02 offline / EC07 rate limit**: distingui i messaggi; consenti lettura da cache offline.

## Testing Plan

- **Rust unit** (mock): retry+backoff su errori transitori; prefetch scrive in cache; prefetch non muta il summary; richieste obsolete scartate.
- **Manuale / QA gated**: con key reale, navigare avanti/indietro velocemente e verificare stati, prefetch e messaggi d'errore staccando la rete.

## Out of Scope

- Coda di prefetch multipagina (solo N+1 nell'MVP).
- Streaming della risposta (non necessario per l'MVP).

---

## Completion Note (2026-07-13)

Implemented end-to-end with strict TDD (MockClient, no network):

- **Retry + backoff** (`llm.rs`): `RetryPolicy` (exponential, default 3 attempts / 500ms base) and `RetryingChatClient` decorator. New typed errors `Timeout`/`ServerError`/`RateLimited`(429, EC07)/`Offline`(EC02) with `is_transient()`; permanent errors (`MissingApiKey`/EC03, 4xx, parse, storage) are not retried. `OpenRouterClient` classifies transport/status failures. Backoff delays are `Duration::ZERO` in tests.
- **Prefetch** (`translate.rs`): `TranslateParams.update_context`. `false` (prefetch) caches only `translated_text`; `true` (real navigation) advances `rolling_summary` + glossary as before. Prefetch never mutates context out of order — covered by a dedicated test asserting summary UNCHANGED and no glossary rows added, plus a cache-hit no-op test.
- **Obsolete-request handling** (`translation.ts`): pure `requestKey`/`isCurrentRequest` keyed by (document, page, language); stale results dropped in `+page.svelte`.
- **Status indicators** (`translation.ts` + `+page.svelte`): `PageStatus` idle/loading/cached/translated/error → bottom bar ("● Tradotto (cache)").
- **Errors**: EC02 (offline) vs EC07 (rate limit) vs EC03 (missing key) distinguished via markers → dedicated hints.
- **Settings**: `prefetch_enabled` (default ON, D5) read by `get_prefetch_enabled`; Tauri command added. `translate_page` command gains `update_context`; wraps client in `RetryingChatClient`.

Verification: `cargo test` 77 passed (67 baseline + 10 new), `cargo build` clean; `vitest` 37 passed (28 + 9 new); `npm run check` 0 errors; `npm run build` ok.

Human-only QA (need real key / network toggling): live 429 backoff + EC07 message; offline (EC02) cache reading + message; visual prefetch smoothness navigating forward; spinner/cache/error indicator behaviour in the running app.
