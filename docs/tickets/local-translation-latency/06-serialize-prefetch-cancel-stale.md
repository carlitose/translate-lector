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

- [ ] Con navigazione rapida, mai più di una richiesta in volo verso il provider locale (verificabile dai
      log del server): un solo slot per provider locale, priorità sempre all'on-demand (L3).
- [ ] Un job di traduzione stantio (pagina non più corrente) o un prefetch ceduto per priorità si
      interrompe al confine della finestra in corso: niente nuove chiamate LLM per pagine abbandonate; le
      finestre già completate restano in cache (comportamento cache parziale invariato).
- [ ] L'interruzione non produce errori visibili all'utente né righe di cache corrotte.
- [ ] Test Rust sulla logica di cancellazione/priorità (flag/token controllabile nei test); frontend
      invariato o con modifiche minime a `translation.ts`/`+page.svelte`.

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

## Out of Scope

- Timeout applicativo (ticket 13 di local-llm-provider).
- Cancellare la singola richiesta HTTP già inviata (il client è bloccante: si interrompe al confine di
  unità, non a metà richiesta).
- Politiche di prefetch per provider cloud (invariate).
