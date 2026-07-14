# 04 — Cablare il packing nella pipeline (split → pack → translate → reassemble)

## Parent Spec

[local-translation-latency-wayfinder.md](../../specs/local-translation-latency-wayfinder.md)

## Type

task

## Outcome

La pipeline di `translate_page` traduce **finestre impacchettate a taglia fissa** (`PACK_TARGET_TOKENS =
512`, decisione L1) invece di un paragrafo per chiamata: su una pagina densa le chiamate scendono da
~35-70 a ~1-2, con round-trip del testo esatto e cache coerente (chiave `unit_index`+`source_hash`
invariata, stabile per costruzione — vedi ticket 02).

## Decisioni vincolanti (grilling 03, [decision-brief-latency-03.md](../../specs/decision-brief-latency-03.md))

- **L1**: `PACK_TARGET_TOKENS = 512` costante, clampata al `budget_unit_text` corrente solo se questo è
  più stretto di 512. Usare `pack_units` già prototipato in `translate.rs` (non toccare la firma).
- **L2**: nessun cap al summary (ticket 05 chiuso) — non aggiungere logiche di troncamento.
- **L5**: target di accettazione = pagina densa fredda ≤2 minuti, zero timeout.
- **Prerequisito**: il ticket 13 (timeout esplicito per-provider) deve essere risolto **prima** o
  **insieme** a questo — una finestra da 512 token dura ~45s (misurato: E_packed_window 42-50s con
  finestra ~900 tok), oltre il default di 30s del client blocking (`llm.rs:541`). Verificare che il
  timeout locale sia già configurato prima di cablare, altrimenti il packing peggiora il sintomo.

## Acceptance Criteria

- [ ] `split_into_units` seguito da `pack_units(units, PACK_TARGET_TOKENS, ratio)` cablato nel flusso di
      `translate_page` (`translate.rs:654`), clampando `PACK_TARGET_TOKENS` al budget se più stretto.
- [ ] Round-trip esatto invariato (concat finestre == testo pagina); fallback a frasi per paragrafi oltre
      budget invariato (dentro `split_into_units`, a monte del packing).
- [ ] Cache per-unità: nessuna modifica di schema/chiave (L1 garantisce stabilità); verificare con test
      che un repack a budget diverso non produca cache-miss di massa su pagine reali.
- [ ] Retry-troncamento e `working_shape` continuano a funzionare a livello di finestra; ridimensionare
      `max_tokens`/headroom per finestra tenendo conto del CoT (~500 token aggiuntivi misurati) oltre alla
      traduzione vera e propria.
- [ ] Test Rust esistenti verdi + nuovi test su packing cablato e cache; `cargo test` completo verde.
- [ ] Misura di conferma sul server locale: chiamate/pagina e secondi/pagina prima vs dopo (stesso
      protocollo del Ticket 01); verificare il target L5 (≤2 min, zero timeout) su una pagina densa reale.

## Blocked By

- [13-local-inference-timeout.md](../../local-llm-provider/13-local-inference-timeout.md) — timeout
  esplicito per-provider, prerequisito tecnico (vedi sopra).
- Grilling 03 (`done/03-grilling-latency-decisions.md`) — **risolto**, decisioni L1/L2/L5 sopra.

## Frontier

È il fix a maggior impatto residuo dopo L6 (niente cambio modello): meno round-trip, il CoT si paga
1-2 volte per pagina invece di 18. Bloccato solo dal ticket 13.

## Work Plan

1. Verificare che il ticket 13 sia risolto (timeout locale configurabile e sufficiente, es. ~180s).
2. Aggiungere `const PACK_TARGET_TOKENS: u32 = 512;` e cablare
   `pack_units(split_into_units(p.page_text, budget_unit_text, ratio), PACK_TARGET_TOKENS.min(budget_unit_text), ratio)`
   in `translate_page` (`translate.rs:654`).
3. Verificare il calcolo dell'output per finestra (`OUTPUT_TOKENS_PER_INPUT`, headroom, `OUT_UNIT_TOKENS`)
   con finestre più grandi dei vecchi paragrafi — includere margine per il CoT osservato.
4. Test + misura di conferma su pagina densa reale (stesso protocollo di `scratchpad/measure_latency.py`
   del ticket 01, sostituibile con log strumentati se lo script non è più disponibile).

## Evidence to Capture

- Diff, output test, tabella prima/dopo (chiamate e secondi per pagina).

## Out of Scope

- Cap del summary (Ticket 05) e prefetch/cancellazione (Ticket 06).
- Parallelismo tra finestre.
