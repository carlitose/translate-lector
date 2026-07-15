# 06 — Serializzare prefetch vs on-demand + cancellare i job stantii

## Parent Spec

[local-translation-latency-wayfinder.md](../../specs/local-translation-latency-wayfinder.md)

## Type

task

## Outcome

Il server locale mono-modello riceve **una sola traduzione alla volta** e non spreca lavoro su pagine
abbandonate: il prefetch non compete mai con la richiesta on-demand corrente, e la navigazione via da una
pagina interrompe (o al più tardi al confine di unità) il job backend in corso.

## Decisioni vincolanti (grilling 03, [decision-brief-latency-03.md](../../specs/decision-brief-latency-03.md))

- **L3**: prefetch **serializzato** con **priorità on-demand** (non disattivato). Un solo job di
  traduzione in volo verso il provider locale; se un prefetch è in corso quando arriva una richiesta
  on-demand, il prefetch cede il passo al confine della finestra corrente (non a metà chiamata HTTP —
  il client è bloccante).
- **L4**: retry-on-timeout locale = **0 retry**, fail-fast (competenza primaria del ticket 13, ma la
  cancellazione qui non deve introdurre retry impliciti).

## Acceptance Criteria

- [x] Con navigazione rapida, mai più di una richiesta in volo verso il provider locale (verificabile dai
      log del server): un solo slot per provider locale, priorità sempre all'on-demand (L3).
      Implementato con `LocalProviderSlot(Mutex<()>)` (`lib.rs`), acquisito per l'intera durata di
      `translate::translate_page` solo quando `llm::is_local_url(&cfg.base_url)` è vero — garanzia
      strutturale (mutex), non solo probabilistica. Il cloud non acquisisce nulla (invariato). Non è stata
      eseguita una verifica manuale con log di un server locale reale (nessun Unsloth Studio disponibile in
      sandbox); la garanzia è comunque data dal mutex stesso, verificabile per ispezione del codice.
- [x] Un job di traduzione stantio (pagina non più corrente) o un prefetch ceduto per priorità si
      interrompe al confine della finestra in corso: niente nuove chiamate LLM per pagine abbandonate; le
      finestre già completate restano in cache (comportamento cache parziale invariato).
      Test: `translate::tests::is_current_false_before_second_unit_cancels_without_extra_calls_and_keeps_prior_cache`.
- [x] L'interruzione non produce errori visibili all'utente né righe di cache corrotte.
      `LlmError::Cancelled` ha un messaggio a basso profilo (nessun "Errore"/codice EC0x allarmante,
      verificato da `llm::tests::cancelled_is_neither_transient_nor_param_degradable_and_has_a_low_key_message`)
      e il frontend scarta già i risultati di richieste non più correnti (`translation.ts`, invariato).
- [x] Test Rust sulla logica di cancellazione/priorità (flag/token controllabile nei test); frontend
      invariato o con modifiche minime a `translation.ts`/`+page.svelte`.
      Frontend non toccato. Test aggiunti: `llm.rs` (1), `translate.rs` (1), `lib.rs` (7, funzioni pure
      `is_page_current`/`update_current_page` senza Tauri/Mutex).

## Blocked By

- Grilling 03 (`done/03-grilling-latency-decisions.md`) — **risolto**, decisione L3 sopra (serializzato,
  priorità on-demand — non disattivato).

## Frontier

Elimina C5: la contesa di più job pesanti sullo stesso server locale, che allunga ogni richiesta e
aumenta la probabilità del taglio del proxy (C1). Ultimo dei tre fix di build per impatto, ma il più
visibile nella navigazione rapida.

## Work Plan

1. Introdurre un token di cancellazione condiviso (es. generazione/`AtomicU64` per documento) letto dal
   loop unità di `translate_page` (`translate.rs:667`): a inizio iterazione, se il job non è più
   corrente, uscire pulito.
2. Serializzare gli accessi al provider locale: coda a slot singolo (mutex/semaforo) attorno alle
   invocazioni `translate_page` per provider locale, con priorità all'on-demand secondo L3
   (`lib.rs:282-319`, comandi Tauri).
3. Frontend: nessun cambiamento di contratto se possibile; al più segnalare la pagina corrente al
   backend a ogni navigazione.
4. Test della cancellazione e della serializzazione; prova manuale con navigazione rapida osservando i
   log del server locale.

## Evidence to Capture

- Diff, output test, log del server locale che mostrano una sola richiesta in volo durante navigazione
  rapida.

### Evidenza raccolta (implementazione)

- `cargo test` (da `src-tauri`): 219 passed (baseline pre-ticket: 210 su questo branch).
- Design realizzato leggermente diverso dal Work Plan iniziale: invece di un token di cancellazione
  `AtomicU64`/generazione per documento, si usa un cursore `document_id -> page_number` (`CurrentPage`,
  `HashMap<i64,i64>` dietro `Mutex`) scritto solo dalle richieste on-demand e letto da un predicato
  `is_current` passato a `TranslateParams`. Stessa proprietà (un job nota di essere stantio al confine di
  unità), ma più leggibile e testabile in modo puro (`is_page_current`/`update_current_page`, senza
  Tauri/Mutex, in `lib.rs`).
- Nessuna verifica manuale con log di un vero server locale (Unsloth Studio) — non disponibile in questo
  ambiente sandboxed. La proprietà "una sola richiesta in volo verso il provider locale" è comunque una
  garanzia strutturale del `Mutex` (`LocalProviderSlot`), non solo osservata a runtime.

## Out of Scope

- Timeout applicativo (ticket 13 di local-llm-provider).
- Cancellare la singola richiesta HTTP già inviata (il client è bloccante: si interrompe al confine di
  unità, non a metà richiesta).
- Politiche di prefetch per provider cloud (invariate).
