# Diagnosi — La traduzione mostrata è quella della pagina precedente ("(cache)")

## Type

Diagnostic spec

## Status

Confirmed (consenso 3/3, confidenza alta — triangulate-diagnosis 2026-07-14; due repro Rust eseguiti)

## Symptom

Nell'app in esecuzione, navigando tra le pagine il pannello di traduzione mostra la traduzione della **pagina precedente**, non di quella corrente. Screenshot: "Pag. 10 / 123", stato "● Tradotto (cache)"; a sinistra il testo di pag. 10 ("Introduction", "It is very difficult to find a black cat…"), a destra solo "Ignoranza" (traduzione del titolo/pagina precedente del PDF "ignorance-how-it-drives-science…").

## Root Cause (consenso 3/3)

Race reattiva nel frontend `src/routes/+page.svelte`. L'`$effect` di traduzione (~390-397) dipende da `currentPage` **e** `reconstructedText`. In `goTo()` (~311-316) `currentPage = N` è impostato **sincrono**, mentre `reconstructedText` è aggiornato solo **dopo** gli `await` di `showPage` (~riga 124). Sotto lo scheduling di Svelte 5 l'effect fa un **doppio fire**:
- run #1: `(currentPage=N, reconstructedText=testo(N-1))` → `translate_page(page_number=N, page_text=testo(N-1))`;
- run #2: `(currentPage=N, reconstructedText=testo(N))`.

La cache Rust (`src-tauri/src/translate.rs`, `cache_lookup` ~99-107 / `cache_insert` ~111-130) è chiavata su `(document_id, page_number, target_language)` e **ignora `source_text`**, con `INSERT OR IGNORE` (prima scrittura vince). Il run #1 quindi **avvelena** lo slot della pagina N con la traduzione della pagina N-1. Da lì in poi la pagina N serve la riga avvelenata come `from_cache=true`.

Il guard obsolete-request (`src/lib/translation.ts`, ticket 12) protegge solo il **display**, non la **scrittura**: entrambi i firing hanno lo stesso `pageNumber=N`, quindi `requestKey` identica → non distingue il pre-fire con testo stale.

## Evidence

- Due repro Rust indipendenti (aggiunti e poi rimossi dai subagent): 1ª chiamata `translate_page(page=10, testo=pag.9)` → salva "trad. pag.9" sotto pag.10; 2ª chiamata `translate_page(page=10, testo=pag.10)` → `from_cache=true`, testo = "trad. pag.9", **0 chiamate al modello**. Riproduce esattamente sintomo + stato "(cache)".
- Frontend: `goTo` scrive `currentPage` (riga 313) prima dell'`await showPage`; `reconstructedText` è riassegnato solo a riga ~124.
- Nessun off-by-one: pdf.js `getPage` 1-based, `currentPage` 1-based, chiave read == chiave write.
- Il **prefetch** (ticket 12) è corretto (scrive N+1 sotto N+1) e anzi **maschera** il bug al primo arrivo → emerge su revisit/ripristino.
- Interazione ticket 11 (nav/restore) + ticket 12 (effect+cache); ticket 14 non coinvolto (`git show 939d3da --stat`).

## Decision / Solution

Due modifiche coordinate:

1. **Frontend (primario)** `+page.svelte`: garantire l'invariante "il `page_number` inviato è la pagina da cui è stato estratto `page_text`". Tracciare la pagina del testo estratto (es. `reconstructedPage` impostato **atomicamente** con `reconstructedText` in `showPage`) e nell'effect/`translateCurrentPage` tradurre solo se `reconstructedPage === currentPage`; inviare `page_number` e `page_text` dalla stessa fonte accoppiata. Resettare `translatedText`/stato all'inizio di `goTo` per pulire subito il pannello stale.

2. **Rust (difesa-in-profondità + auto-riparazione)** `translate.rs`: la colonna `translations_cache.source_text` **esiste già** (schema §4.3). Verificare che il `source_text` memorizzato combaci col testo corrente sul cache-hit; su mismatch trattare come **miss** e **sovrascrivere** la riga (upsert / `INSERT OR REPLACE` sulla chiave unica invece di `INSERT OR IGNORE`). Effetto: le righe **già avvelenate** nel `.db` dell'utente si auto-correggono e nessuna scrittura disallineata può più essere servita.

## Options Considered

- **Solo reset di `translatedText` su navigazione**: insufficiente — non spiega/evita un risultato *persistente* sbagliato marcato "(cache)"; la riga avvelenata resterebbe.
- **Solo fix frontend (prevenire l'invoke disallineato)**: corretto per i casi nuovi ma **non pulisce** le righe già avvelenate nel DB esistente → serve anche la difesa lato cache.
- **Hash del source_text nella chiave di cache**: valido ma richiede cambio schema; la verifica del `source_text` già memorizzato ottiene lo stesso risultato senza migrazione.
- **Affidarsi al guard obsolete-request**: non applicabile — stessa `pageNumber`, guard cieco al pre-fire.

## Testing Decisions

- Rust: cache-hit con `source_text` diverso → miss + sovrascrittura (no wrong serve); happy path invariato; un test che riproduce "scrittura con testo stale poi lettura corretta → NON serve la vecchia".
- Frontend (vitest): logica pura che, dato `reconstructedPage !== currentPage`, non emette l'invoke; reset stato su navigazione.
- Mantenere verdi 102 Rust + 47 frontend.
- QA live (human-only): navigare avanti/indietro tra pagine e verificare che la traduzione combaci sempre con la pagina; verificare l'auto-riparazione su un `.db` già avvelenato.

## Open Questions

- Nessuna bloccante. (La verifica GUI live la fa l'utente.)

## Follow-up

Ticket: [16-fix-stale-page-translation.md](../tickets/translate-lector/16-fix-stale-page-translation.md)
