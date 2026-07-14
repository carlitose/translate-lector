## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)
(design: sezione "Design multi-chiamata"; decisioni D1-D6 nel
[decision-brief-stc-05](../../specs/decision-brief-stc-05.md))

## What to Build

Il cuore dell'epica: sostituire la traduzione "una chiamata per pagina con tutto il glossario" con una
**pipeline budget-aware multi-chiamata**, cablando i prototipi già pronti. Per pagina:
1. calcola `budget_input`/`budget_unit_text` dal `n_ctx` del provider (Ticket 07) e da `out_unit` (STC-01);
2. `split_into_units` (STC-02) → unità paragrafo (fallback frase);
3. per ogni unità una chiamata **translate-only** (prompt: system minimale + summary **read-only compatto** +
   `select_glossary(unit, entries, cap)` con locked-first, STC-03) → solo testo tradotto, `max_tokens = out_unit`;
4. riassembla le unità in ordine → traduzione di pagina;
5. **una** chiamata **perceptor-update per pagina** (solo su `update_context=true`) → `updated_summary` +
   `new_glossary_terms` (compressione EC05), come oggi ma separata dalla traduzione (D5/D6).

Con budget ampio (cloud) `split_into_units` restituisce **una sola unità = pagina intera** → degradazione
automatica al comportamento attuale (D2, nessuna regressione cloud).

## Acceptance Criteria

- [ ] `translate_page` usa il percorso budget-aware: budget calcolato da `n_ctx`/`out_unit`; unità da
      `split_into_units`; glossario per-unità da `select_glossary` (non più l'intero glossario nel prompt).
- [ ] **Contratto split** (D5): chiamata di traduzione per unità ritorna solo il testo (JSON minimo o testo
      + fallback extraction esistente); `perceptor-update` per pagina ritorna summary+glossario. `EC08`/ladder
      restano validi.
- [ ] **Update percettore una volta per pagina** (D6); su prefetch (`update_context=false`) nessun update,
      come oggi.
- [ ] **Degradazione cloud**: con `n_ctx` grande, 1 unità = pagina → risultato equivalente all'attuale; i
      test esistenti di `translate.rs` restano verdi (adattati al nuovo flusso dove serve).
- [ ] Coerenza: stessa versione di summary passata a tutte le unità della pagina; ordine preservato nel
      riassemblaggio.
- [ ] `cargo test` verde (nuovi test: split→translate→reassemble; select_glossary nel prompt; perceptor una
      volta/pagina; budget piccolo produce >1 unità, budget grande 1 unità); `cargo build`/`clippy` ok.

## Blocked By

- [06-paragraph-aware-reconstruction.md](./06-paragraph-aware-reconstruction.md)
- [07-nctx-per-provider-config.md](./07-nctx-per-provider-config.md)

## Frontier

Blocked da 06 (paragrafi veri) e 07 (`n_ctx`). È il ticket più grande: sostituisce il loop `split_into_chunks`
(`translate.rs`, soglia char) con il flusso a budget/unità e separa il contratto.

## Step-by-Step Implementation Plan

1. Calcolo budget in `translate.rs` prima del loop: `budget_unit_text` da `n_ctx` (Ticket 07), `out_unit`,
   e `est_tokens` di system/summary/glossario. Perché prima: dimensiona lo split. Verifica: unit test del
   calcolo su n_ctx piccolo/grande.
2. Sostituire `split_into_chunks(CHUNK_CHAR_THRESHOLD)` con `split_into_units(page_text, budget_unit_text, ratio)`
   (STC-02). Verifica: budget grande → 1 unità; piccolo → N unità; round-trip.
3. Per ogni unità: costruire il prompt translate-only con `select_glossary(unit, entries, cap)` (STC-03,
   locked-first) + summary read-only; inviare con `max_tokens = out_unit`. Verifica: il prompt non contiene
   più l'intero glossario; contratto minimo.
4. Riassemblare le unità tradotte in ordine → testo pagina. Verifica: ordine e completezza.
5. Dopo le unità (se `update_context`), una chiamata perceptor-update → summary+glossario (riuso della logica
   esistente di compressione/insert). Verifica: percettore invocato una volta per pagina; prefetch non
   aggiorna.
6. Aggiornare i test di `translate.rs` (MockClient) al nuovo flusso; mantenere verdi quelli non impattati.

Pitfall: non rompere il caso cloud (deve restare equivalente); non passare summary "in avanzamento" tra
unità (D6 = una volta per pagina); mantenere valida la ladder/fallback e la gestione EC08 per-unità.

## Testing Plan

- Unit (Rust, MockClient): registra le richieste → verifica N unità, glossario selezionato nel prompt,
  perceptor una volta/pagina, riassemblaggio ordinato, degradazione cloud (1 unità).
- Regressione: i comportamenti di cache/percettore per pagina esistenti restano coerenti (cache per-unità è
  il Ticket 09; qui la cache può restare per-pagina sul risultato riassemblato).
- Manuale (con server locale): una pagina reale traduce senza EC08, con prompt piccoli.

## Out of Scope

- Cache per-unità (Ticket 09).
- Indice lemma/alias del glossario (follow-up).
- Streaming.
