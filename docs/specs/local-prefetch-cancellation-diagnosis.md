# Diagnosi — Il prefetch della pagina N+1 non parte mai col provider locale

## Type

Diagnostic spec

## Status

Confirmed (2026-07-15, triangulazione a 3 diagnosi indipendenti, convergenza 3/3, confidenza alta)

## Symptom

Con il provider locale `llamaserver` selezionato, il **prefetch della pagina successiva (N+1) non
funziona**: arrivando alla pagina N+1 viene ritradotta da zero invece di essere già in cache.

Evidenza dal log del backend (app in esecuzione, `document_id=2`): traduzioni consecutive registrate
solo come `[usage] ... page=50..53 ... prefetch=false ...`, **zero righe `prefetch=true`**. Il campo
`prefetch` è vero esattamente quando `update_context == false` (una richiesta di prefetch), quindi
nessuna richiesta di prefetch raggiunge il punto in cui `[usage]` viene loggato. La traduzione
on-demand funziona regolarmente.

## Root Cause

La cancellazione dei job stantii (ticket 06 dell'epica latenza) è incompatibile col prefetch sul
provider locale. Sequenza deterministica:

1. Navigazione a pagina N (on-demand, `update_context=true`): il cursore `CurrentPage` viene scritto
   a **N** *prima* di tradurre (`src-tauri/src/lib.rs:507-510`).
2. Alla fine, il frontend lancia il prefetch di **N+1** (`src/routes/+page.svelte:432`,
   `updateContext:false`).
3. Per il provider locale, `is_current` è cablato per **ogni** richiesta — prefetch inclusa — senza
   esenzione per `update_context==false` (`src-tauri/src/lib.rs:567`; il cloud riceve `None` via
   `should_check_is_current`).
4. La closure `is_current` cattura la `page_number` della richiesta (N+1) e la confronta col cursore
   (N): `is_page_current(cursor=N, N+1) == false` (`lib.rs:511-514`, `lib.rs:82-87`).
5. Il loop delle unità in `translate::translate_page` controlla `is_current()` in cima alla **prima**
   iterazione (idx 0) e ritorna `Err(LlmError::Cancelled)` prima di qualsiasi chiamata al modello e
   prima della riga `[usage]` (`src-tauri/src/translate.rs:770-784`; log a `~1012`).
6. `LlmError::Cancelled` è silenzioso e il `catch {}` del prefetch nel frontend lo inghiotte
   (`+page.svelte:464-466`) → nessun sintomo visibile, solo cache fredda.

**Local-only** perché il cloud ha `is_current: None`: il predicato `current == page_number` è sempre
falso per un prefetch (N ≠ N+1), quindi il prefetch locale è strutturalmente impossibile così com'è
cablato.

## Evidence

- `src-tauri/src/lib.rs:507-514` — cursore scritto a N (solo on-demand); closure `is_current`
  confronta la `page_number` della richiesta col cursore.
- `src-tauri/src/lib.rs:567` — `is_current` cablato per il locale senza esenzione prefetch.
- `src-tauri/src/lib.rs:82-87` — `is_page_current`: `current == page_number`.
- `src-tauri/src/translate.rs:770-784` — check a idx 0 → `Err(LlmError::Cancelled)`.
- `src-tauri/src/translate.rs:~1012` — `[usage]` loggato *dopo* il loop → mai raggiunto dal prefetch.
- `src/routes/+page.svelte:432, 451-463, 464-466` — il prefetch parte (default on), invoca
  `translate_page` con `updateContext:false`, e il `catch {}` inghiotte il `Cancelled`.
- Log runtime: zero `prefetch=true`, on-demand invariato.
- Regressione: `is_current`/`Cancelled` introdotti dal ticket 06 (commit `d75b52c`, merge `6b645e8`),
  successivo alla feature di prefetch (ticket 12).

## Decision / Solution

Non cablare `is_current` per le richieste di prefetch. Fix minimale in `src-tauri/src/lib.rs:567`:

```rust
is_current: if should_check_is_current(&cfg.base_url) && update_context {
    Some(&is_current)
} else {
    None
},
```

Preserva la serializzazione L3 (`LocalProviderSlot`), la cancellazione is_current per l'on-demand, e
il comportamento cloud. Va aggiunto un test che copra esplicitamente il caso prefetch
(`update_context=false` con un `is_current` falso a idx 0 → oggi ritorna `Cancelled`, dopo il fix
deve tradurre).

## Options Considered

- **A — Esentare il prefetch da `is_current`** (scelta): minimale, rispecchia il `None` del cloud,
  mantiene le garanzie del ticket 06 per l'on-demand.
- **B — Ridefinire la staleness del prefetch** come "il cursore è *avanzato* oltre N+1" (snapshot del
  cursore allo spawn, cancella solo se cambiato): più robusta se un domani si vuole cancellare un
  prefetch superato da una navigazione a pagina ≠ N,N+1, ma più complessa. Non necessaria ora — con
  A, un prefetch superato viene comunque scartato lato frontend (`isCurrentRequest`) e non avanza il
  contesto (`update_context=false`).

## Testing Decisions

- Gap che ha fatto passare il bug: i test di cancellazione esistenti (`translate.rs:2270`, ecc.)
  assumono che il job parta *current* e diventi stantio dopo la prima unità; nessuno esercita il
  prefetch con `is_current()` falso a idx 0.
- Aggiungere un test unitario del caso prefetch (idx 0 non-current → deve completare, non cancellare)
  e/o un test a livello di `translate_page` che verifichi il wiring `update_context==false → is_current None`.

## Open Questions

- Nessuna bloccante. Eventuale follow-up (opzione B) solo se emergesse la necessità di cancellare un
  prefetch genuinamente superato durante navigazione rapida.
