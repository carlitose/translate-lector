## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)
(design: sezione "Design multi-chiamata" → Cache granularità unità)

## What to Build

Portare la cache delle traduzioni a **granularità di unità** (paragrafo), così una pagina interrotta a metà
(errore/timeout su un'unità) non ritraduce le unità già fatte, e il riassemblaggio avviene dalle unità
cachate. Estende la pipeline del Ticket 08.

## Acceptance Criteria

- [ ] Cache per-unità con chiave `(document_id, page_number, unit_index, target_language)` + un **hash del
      source** dell'unità (per invalidare se il testo cambia). Estensione di `translations_cache` (§4.3) con
      `unit_index`/hash, o nuova tabella `unit_translations` (scegliere e migrare; motivare).
- [ ] La pipeline (Ticket 08) traduce **solo le unità mancanti** in cache; le presenti sono riusate;
      riassemblaggio per `unit_index`.
- [ ] **Cache parziale** robusta: se un'unità fallisce (EC08/timeout), le unità già tradotte restano cachate;
      un retry ritraduce solo le mancanti.
- [ ] Compatibilità con il percettore per-pagina (D6): l'update summary/glossario resta per pagina, non
      per-unità; la cache non altera la coerenza del summary.
- [ ] Invalidazione corretta quando cambia il testo pagina (hash) o la lingua target.
- [ ] `cargo test` verde (hit/miss per-unità, cache parziale, invalidazione per hash); migrazione schema
      testata (`db.rs`).

## Blocked By

- [08-budget-aware-multicall-pipeline.md](./08-budget-aware-multicall-pipeline.md)

## Frontier

Blocked da 08 (serve il flusso a unità). Ottimizzazione/robustezza che rende la lentezza locale sopportabile
(nessun rework) e resiliente agli errori a metà pagina.

## Step-by-Step Implementation Plan

1. Estendere lo schema cache (`src-tauri/src/db.rs`): `unit_index` + `source_hash` (o nuova tabella
   `unit_translations`), con migrazione. Perché prima: il resto legge/scrive qui. Verifica: init schema +
   test round-trip.
2. Nella pipeline (Ticket 08), prima di tradurre un'unità consultare la cache per-unità (hash del source);
   tradurre solo i miss; scrivere i risultati. Verifica: seconda visita → nessuna chiamata; miss parziale →
   solo le mancanti.
3. Riassemblare dalla cache per `unit_index`. Verifica: ordine e completezza; testo identico alla prima resa.
4. Gestire invalidazione per cambio testo/lingua (hash mismatch → miss). Verifica: modificare il source →
   ritraduzione della sola unità cambiata.

Pitfall: coerenza tra cache per-unità e l'eventuale cache/vista per-pagina (evitare doppie fonti di verità);
non far avanzare il summary sulle sole unità cachate (D6). Attenzione all'ordine stabile di `unit_index`.

## Testing Plan

- Unit (Rust): hit/miss per-unità; cache parziale dopo un errore simulato; invalidazione per hash;
  migrazione schema.
- Regressione: risultato di pagina identico con e senza cache; percettore invariato.
- Manuale: interrompere una pagina a metà (server giù su un'unità), riprovare → solo le mancanti ritradotte.

## Out of Scope

- La pipeline stessa (Ticket 08).
- Indice lemma/alias glossario; streaming.
