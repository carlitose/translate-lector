# 13 — Schermata Impostazioni completa (⚙️)

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md) · design: [SPECIFICATION.md](../../../SPECIFICATION.md) §3.5, NFR07

## What to Build

Espande la config minima (07) alla **schermata Impostazioni completa**: API key (keychain), modello (dropdown dei più usati + campo libero), **lingua di destinazione predefinita** (lista D4 + campo libero), **prefetch** on/off (D5 ON), **limite rolling summary** (D5 ~800-1000 token), **cartella dati** (dove salvare `.db`/cache/glossario) e **svuota cache** (cancella `translations_cache`). Tutte le preferenze non-segrete vivono in `settings`.

## Acceptance Criteria

- [ ] Tutte le impostazioni di §3.5 sono presenti e persistite: API key, modello, lingua default, prefetch, limite summary, cartella dati, svuota cache.
- [ ] Cambiare la lingua default influenza l'apertura di nuovi documenti; cambiare il limite summary influenza la soglia di compressione (09); il toggle prefetch influenza 12.
- [ ] "Svuota cache" cancella `translations_cache` (con conferma) e la UI lo riflette.
- [ ] Cambiare la cartella dati sposta/usa il nuovo percorso per il DB locale in modo sicuro (o richiede riavvio con messaggio chiaro).
- [ ] La API key resta nel keychain (NFR07), mai in `settings`.

## Blocked By

- [07-minimal-provider-config.md](./07-minimal-provider-config.md)

## Frontier

Bloccato da 07 (ne estende il pannello). AFK: tutto verificabile localmente senza LLM (a parte l'uso a valle delle preferenze, già coperto da 09/12).

## Step-by-Step Implementation Plan

1. **Chiavi `settings`**: definisci le chiavi (`model`, `default_target_language`, `prefetch_enabled`, `summary_token_limit`, `data_dir`) con default D5; riusa `get_setting`/`set_setting` di 07. Unit test dei default.
2. **Frontend — schermata Impostazioni**: form completo che legge/scrive le chiavi e i comandi keychain. *Verifica*: `npm run check` pulito.
3. **Svuota cache**: comando `clear_translations_cache()` con conferma UI. Unit test che svuota solo la cache e non documenti/glossario/sessioni.
4. **Cartella dati**: consenti di scegliere la cartella; per l'MVP, se il cambio richiede riavvio, mostralo chiaramente e riapri il DB al percorso nuovo. *Pitfall*: non perdere i dati esistenti — copia/migra o istruisci l'utente; non cancellare il vecchio `.db` automaticamente.
5. **Propagazione runtime**: assicurati che i consumatori (apertura documento, percettore, prefetch) leggano il valore aggiornato.

## Testing Plan

- **Rust unit**: default di tutte le chiavi; `clear_translations_cache` mirato; set/get lingua/prefetch/limite.
- **Manuale**: cambiare ogni impostazione e verificarne l'effetto; svuota cache; cambio cartella dati con dati esistenti.

## Out of Scope

- Multi-provider oltre OpenRouter (roadmap).
- Import/export delle impostazioni.

---

## Completion note (2026-07-13)

Implementato end-to-end in TDD (RED→GREEN). Ultimo ticket della spec.

**Core (Rust)**
- `settings.rs`: nuove chiavi/default `default_target_language`="it" (D4), `data_dir` (path app-data di default); accessor `get_default_target_language`; helper puro `resolve_data_dir(stored, default_dir)`. Riusate `prefetch_enabled`/`summary_token_limit`/`model` esistenti (stesse chiavi di 09/12).
- `db.rs`: `clear_translations_cache(&Connection) -> usize` cancella SOLO `translations_cache`.
- `documents.rs`: `open_or_create_session` adotta la lingua predefinita configurata per le sessioni nuove (fallback "it") → il cambio lingua influenza l'apertura di nuovi documenti.
- `lib.rs`: comandi `get_default_target_language`, `clear_translations_cache`, `get_data_dir`, `set_data_dir` (+ `DataDirResult`). API key sempre e solo nel keychain (NFR07).
- Test unit: default di ogni chiave, `resolve_data_dir`, clear-cache mirato (cache→0, documents/sessions/glossary/settings intatti), lingua predefinita su nuova sessione.

**Frontend**
- `settings.ts` (nuovo, helper puri: `LANGUAGES` D4 condivisa, `resolveLanguage`, `parseSummaryLimit`, `resolvePrefetch`, default) + `settings.test.ts` (9 test).
- `ProviderConfig.svelte` espanso alla schermata Impostazioni completa: API key (keychain), modello, lingua predefinita, prefetch on/off, limite summary, cartella dati, svuota cache (con conferma a due click). `providerConfig.ts` e i suoi test restano invariati.
- `+page.svelte`: `LANGUAGES` importata da `settings.ts` (deduplicata); `refreshPrefetch()` richiamato on-mount e via callback `onSaved` così il toggle prefetch ha effetto live (12).

**Comportamento cartella dati (MVP, come da AC "…o richiede riavvio con messaggio chiaro")**
- `set_data_dir` crea la cartella se manca, persiste la scelta nella tabella `settings` (chiave `data_dir`) e in un file puntatore di bootstrap (`data_dir.txt` nella app-config dir, letto all'avvio prima di aprire il DB). Ritorna `restart_required: true`.
- Il cambio ha effetto **al prossimo avvio**: `database_path` legge il puntatore e apre il `.db` dalla nuova cartella. Il vecchio `.db` **non** viene spostato né cancellato — la UI istruisce l'utente a copiare manualmente i dati (safe, nessuna perdita).

**Verifica**: `cargo test` 85 (77 + 8 nuovi), `cargo build` ok, `npx vitest run` 46 (37 + 9 nuovi), `npm run check` 0 errori, `npm run build` ok.

**QA umana (solo GUI, non simulabile qui)**: applicare ogni impostazione nell'app in esecuzione e verificarne l'effetto a valle (modello nella traduzione, lingua su nuovo documento, prefetch, limite summary), svuota-cache con conferma, e il cambio cartella dati con riavvio effettivo + copia dei dati esistenti.
