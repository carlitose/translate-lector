# Diagnosi — Unità di traduzione troncate (finish_reason=length accettato)

## Type

Diagnostic spec

## Status

Confirmed (2026-07-14, triangulate-diagnosis: 2/3 angoli completati — repro-first agent morto senza output;
data-flow + recent-change/env convergono; + repro empirica su server reale). Confidenza alta.

## Symptom

Traducendo una pagina con il provider locale, l'ultimo paragrafo si **tronca a metà frase** (es.
"…L'addestramento degli LLM con milioni o" invece di "…milioni o miliardi di parametri…"); le unità
successive (numero pagina, footer) risultano tradotte. Sembra che "manchi metà pagina".

## Root Cause (confermata)

Due fattori combinati:
1. **`finish_reason == "length"` con contenuto NON vuoto viene accettato in silenzio.** In
   `ChatResponse::content()` (`src-tauri/src/llm.rs:173-193`) solo il caso *contenuto vuoto* + `length` è
   instradato a `OutputBudgetExhausted` (EC08, STC-03/ticket 03). Con contenuto **parziale** non vuoto,
   `content()` ritorna il testo troncato e `parse_translation` (`llm.rs:1028`) lo accetta → l'unità risulta
   "tradotta" ma è tagliata.
2. **Il modello emette una chain-of-thought DENTRO il contenuto** ("Thinking Process:…") che **consuma il
   budget di output** prima/durante la traduzione finale, e **NON** è conteggiata in `reasoning_tokens`
   (per questo un probe con prompt semplice mostrava `reasoning=0` e completava: il prompt reale
   translate-only innesca invece la CoT). L'agente data-flow ha riprodotto: con `max_tokens=200` la bozza
   arriva **esattamente** a "…con milioni o [miliardi]" → combacia col sintomo. Combinato con un cap
   per-unità risicato (`OUT_UNIT_TOKENS=768`, e la formula multi-unità), la CoT + traduzione superano il
   tetto → `finish_reason:"length"`.
3. **Aggravante — la cache ripropone il troncamento**: l'unità troncata è scritta in cache per-unità
   (`translate.rs:725`) **prima** di ogni controllo di completezza; un retry fa cache-hit e **ripete** il
   testo troncato invece di ritradurre. (Si risolve se il fix #1 fa errore sul troncamento: il `?` blocca
   prima dell'insert.)

## Evidence

- `ChatResponse::content()` `src-tauri/src/llm.rs:173-193`: EC08 solo su contenuto vuoto+length; non vuoto+
  length → Ok(testo parziale).
- `parse_translation` `src-tauri/src/llm.rs:1028`: estrae il testo, nessun controllo di `finish_reason`.
- `translate_page` `src-tauri/src/translate.rs`: il cap per-unità multi-unità è
  `scaled.min(p.max_tokens).min(headroom)` con `out_unit = 768`; `complete_and_parse_translation` non
  ispeziona `finish_reason`.
- Repro server (localhost:8888, gemma-4-E2B): paragrafo singolo → `finish=stop`, completion=637 @ max_tokens
  768 (completo); il modello produce ~4 token/parola.

## Decision / Solution (raccomandata)

1. **Non accettare mai un troncamento**: in `ChatResponse::content()` (e/o nel path unità) trattare
   `finish_reason == "length"` come errore **anche con contenuto non vuoto** (non solo vuoto). Così il
   troncamento diventa un errore azionabile invece di una traduzione parziale silenziosa; e il `?` impedisce
   la scrittura in cache dell'unità troncata (risolve l'aggravante #3).
2. **Ritentare l'unità con budget maggiore** prima di arrendersi: su troncamento, rifare la chiamata con
   `max_tokens` più alto (×2, fino all'headroom `n_ctx − prompt − margine`), 1-2 tentativi; se ancora
   troncata al massimo headroom → EC08 azionabile (o spezzare l'unità in frasi e tradurre i sotto-pezzi).
3. **Ridurre/sopprimere la chain-of-thought**: rinforzare `build_translate_only_system_prompt` per vietare
   esplicitamente "Thinking Process"/ragionamento, e/o **strippare** un blocco CoT iniziale in
   `parse_translation`. Alzare inoltre `OUT_UNIT_TOKENS` (es. 1024-1536) per assorbire la verbosità.
4. (Secondario) **STC-06**: due paragrafi con y-gap piccolo sono finiti in un'unica unità; tarare la soglia
   può ridurre la dimensione delle unità.

## Options Considered (scartate come causa primaria)

- **Reasoning che consuma il budget**: escluso in questo caso (`reasoning_tokens=0`); è troncamento diretto.
- **Bug di parsing/round-trip**: escluso — il testo parziale è ben formato, solo incompleto.

## Open Questions

- Comportamento quando anche il retry al massimo headroom tronca (unità enorme su finestra piccola): accettare
  con avviso, o spezzare ulteriormente l'unità (frase) e tradurre i sotto-pezzi? (valutare nel ticket).

## Follow-up

Ticket: `docs/tickets/small-context-translation/11-detect-and-retry-truncated-units.md`.
