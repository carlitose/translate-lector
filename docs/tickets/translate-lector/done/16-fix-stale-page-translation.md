# 16 — Fix: la traduzione mostrata è quella della pagina precedente (cache avvelenata)

## Parent Spec

[stale-page-translation-diagnosis.md](../../specs/stale-page-translation-diagnosis.md)

## What to Build

Correggere il bug per cui, navigando, il pannello mostra la traduzione della pagina precedente con stato "(cache)". Causa (consenso 3/3): race reattiva in `+page.svelte` → `translate_page` invocato con `page_number` nuovo ma `page_text` della pagina precedente → la cache (chiavata senza `source_text`, `INSERT OR IGNORE`) viene avvelenata e poi servita.

Due parti coordinate: (1) prevenire l'invoke disallineato lato frontend; (2) difesa lato cache che verifica il `source_text` e si auto-ripara (pulisce le righe già avvelenate nel `.db` esistente).

## Acceptance Criteria

- [ ] Navigando avanti/indietro, la traduzione mostrata corrisponde SEMPRE alla pagina renderizzata (nessuna traduzione della pagina precedente).
- [ ] `translate_page` non viene mai invocato con un `page_text` che non appartiene al `page_number` inviato (invariante frontend verificato da test sulla logica pura).
- [ ] Su cache-hit, se il `source_text` memorizzato NON combacia col testo corrente della pagina, la cache tratta il caso come miss e **sovrascrive** la riga (le righe già avvelenate si auto-riparano; una riga corretta non viene mai scartata).
- [ ] Il pannello traduzione non mostra un valore stale della pagina precedente durante il caricamento della nuova pagina (reset su navigazione).
- [ ] Happy path invariato (cache-hit legittimo con testo combaciante resta un hit, nessuna ri-traduzione inutile).
- [ ] 102 test Rust + 47 frontend restano verdi; aggiunte regression mirate.

## Blocked By

- None - can start immediately (diagnosi completata, consenso 3/3).

## Frontier

**Ready.** Diagnosi confermata con repro Rust eseguiti. La verifica GUI live (navigazione reale) resta human-only.

## Step-by-Step Implementation Plan

1. **Rust difesa/auto-riparazione** (`src-tauri/src/translate.rs`): sul cache-hit confrontare `translations_cache.source_text` col `page_text` della richiesta; se diverso → miss. Cambiare la scrittura da `INSERT OR IGNORE` a upsert (`INSERT … ON CONFLICT(document_id,page_number,target_language) DO UPDATE` o `INSERT OR REPLACE`) così una traduzione corretta sovrascrive una riga avvelenata. Test: hit con source_text diverso → miss + overwrite; hit con source_text uguale → servito senza chiamare il modello; happy path e prefetch invariati.
2. **Frontend invariante pagina↔testo** (`src/routes/+page.svelte`): introdurre `reconstructedPage` (`$state`) impostato **atomicamente** con `reconstructedText` dentro `showPage`. Nell'`$effect`/`translateCurrentPage` tradurre solo se `reconstructedPage === currentPage`; inviare `page_number` e `page_text` dalla stessa fonte. *Perché*: elimina il run #1 con testo stale.
3. **Reset stato su navigazione** (`goTo` e `loadDocument`): azzerare `translatedText` e riportare `pageStatus` a idle all'inizio della navigazione, così il pannello non mostra la traduzione della pagina precedente durante il render.
4. **Test frontend (vitest)**: se estrai una funzione pura per "deve tradurre?" (gate su page match), testala; altrimenti testa la logica di request-building che accoppia page+text.
5. Verificare tutta la suite + build.

## Testing Plan

- **Rust**: mismatched-source_text hit → miss+overwrite; matching hit → served; prefetch e happy path invariati; regression del repro (scrittura testo-stale poi lettura corretta → NON serve la vecchia).
- **Frontend (vitest)**: gate `reconstructedPage === currentPage`; reset su navigazione.
- **Manuale / QA live (human-only)**: navigare tra molte pagine e verificare corrispondenza; su un `.db` già avvelenato, riaprire e confermare che la pagina si ri-traduce corretta (auto-riparazione).

## Out of Scope

- Rework del modello di rendering (resta pagina-discreta, D1).
- Hash del source_text nella chiave (cambio schema) — si usa la colonna `source_text` già esistente.

## Completion Note (2026-07-14)

**Stato**: implementato end-to-end, TDD (RED → GREEN), tutte le verifiche verdi. La navigazione GUI live (traduzione sempre allineata alla pagina; auto-riparazione di un `.db` già avvelenato alla rivisita) resta **human-only QA**.

**Cosa è cambiato**:

- **FIX 1 — difesa cache + auto-riparazione** (`src-tauri/src/translate.rs`):
  - `cache_lookup` ora seleziona anche `source_text` (ritorna `Option<(translated_text, source_text)>`).
  - `translate_page`: su cache-hit serve la riga **solo se** `source_text` memorizzato combacia col `page_text` richiesto; su mismatch tratta il caso come **miss** e ri-traduce.
  - `cache_insert`: da `INSERT OR IGNORE` a **UPSERT** (`ON CONFLICT(document_id,page_number,target_language) DO UPDATE SET source_text, translated_text, created_at`) → una traduzione corretta **sovrascrive** la riga avvelenata (self-heal).
- **FIX 2 — invariante pagina↔testo** (`src/routes/+page.svelte`, `src/lib/translation.ts`):
  - Nuovo `reconstructedPage` (`$state`) impostato **atomicamente** con `reconstructedText` in `showPage`.
  - Nuovo helper puro `shouldTranslate(reconstructedPage, currentPage, text)` in `translation.ts` (gate: pagine uguali + testo non vuoto).
  - `$effect` e `translateCurrentPage` traducono solo quando `shouldTranslate(...)` è vero; la richiesta invia `page_number` = `reconstructedPage` e `page_text` = `reconstructedText` dalla **stessa fonte accoppiata** (mai `currentPage` fresco con testo stale).
  - Reset del pannello all'inizio di `goTo` e in `loadDocument` (`translatedText=''`, `pageStatus='idle'`, `reconstructedPage=0`).

**Test aggiunti (regression)**:

- Rust (2): `cache_hit_with_mismatched_source_text_is_a_miss_and_overwrites`; `poisoning_repro_stale_write_then_correct_read_retranslates` (repro esatto: scrive pag.10 con testo pag.9 → NON serve la vecchia, ri-traduce, poi hit legittimo).
- Frontend (3): `shouldTranslate` — traduce solo a pagine allineate, non con testo della pagina precedente, non con testo vuoto.
- Happy-path e prefetch invariati (test esistenti restano verdi senza modifiche: `source_text` combaciante = hit).

**Verifica**: `cargo test` 104 passed (102 + 2 nuovi); `cargo build` ok; `vitest` 50 passed (47 + 3 nuovi); `npm run check` 0 errori; `npm run build` ok. (`tauri dev` NON eseguito.)

**QA live pendente (human-only)**: navigare avanti/indietro tra molte pagine e confermare che la traduzione combaci sempre con la pagina renderizzata (nessuna "(cache)" stale); riaprire un `.db` già avvelenato e confermare che la pagina si ri-traduce corretta (auto-riparazione).
