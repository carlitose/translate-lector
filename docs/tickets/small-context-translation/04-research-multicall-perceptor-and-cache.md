## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)

## Type

research

## Outcome

Design chiuso di come orchestrare **molte chiamate piccole** (una per unità) mantenendo la coerenza del
percettore (summary + glossario) **senza** esplodere numero di chiamate e latenza, con la granularità di
cache e l'eventuale **split del contratto** (traduci-unità vs aggiorna-percettore). Prepara le build.

## Acceptance Criteria

- [ ] Decisione su **quante chiamate**: unità tradotte con contesto minimo (summary compatto + glossario
      selezionato dal Ticket 03), e **aggiornamento summary/glossario una volta per pagina** (step separato
      e compatto) vs incrementale per unità. Motivare rispetto a latenza/coerenza.
- [ ] **Split del contratto** valutato: chiamata "translate-only" che ritorna solo il testo tradotto
      dell'unità (prompt/JSON minimi, meno budget) + una chiamata percettore per pagina che ritorna
      `updated_summary` + `new_glossary_terms`. Confronto col contratto unico attuale (`llm.rs` ~700).
- [ ] **Cache**: granularità per-unità (`document_id, page, unit_index, target_language`) e riassemblaggio;
      estensione schema vs riuso `translations_cache` (§4.3). Comportamento su cache parziale della pagina.
- [ ] Coerenza cross-unità: il summary corrente (read-only) è passato alle unità; ordine e continuità
      garantiti.
- [ ] Stima di costo/latenza: N unità × chiamate piccole vs 1 chiamata grande, su modello locale lento.
- [ ] Design registrato, pronto per `to-tickets`.

## Blocked By

- [01-research-token-budget-model.md](./01-research-token-budget-model.md)
- [02-prototype-paragraph-chunking.md](./02-prototype-paragraph-chunking.md)
- [03-prototype-deterministic-glossary-selection.md](./03-prototype-deterministic-glossary-selection.md)

## Frontier

Blocked dai tre precedenti (budget, chunking, selezione glossario): solo con quei numeri si può decidere
split chiamate, cache e orchestrazione senza indovinare.

## Work Plan

1. Rivedere il flusso attuale (`translate.rs`: chunk loop, summary carry-forward, update per pagina) e il
   contratto (`llm.rs` `response_format`).
2. Progettare l'orchestrazione multi-chiamata (translate-only per unità + perceptor-update per pagina) e la
   cache per-unità (bozza SQL/migrazione o riuso).
3. Stimare costo/latenza vs oggi; definire il comportamento su cache parziale/navigazione.
4. Scrivere il design nella mappa.

## Evidence to Capture

- Contratto/i JSON proposti; bozza schema cache; stima chiamate/latenza; regole di coerenza.

## Out of Scope

- Implementazione (build verticali dopo la mappa).
