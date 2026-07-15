# 01 — Esentare il prefetch dalla cancellazione is_current

## Parent Spec

[local-prefetch-cancellation-diagnosis.md](../../specs/local-prefetch-cancellation-diagnosis.md)

## What to Build

Il prefetch della pagina N+1 deve tornare a funzionare col provider locale `llamaserver`.
Attualmente ogni prefetch viene cancellato immediatamente perché il predicato `is_current`
(cancellazione job stantii, ticket 06) è cablato anche per le richieste di prefetch: la richiesta è
per la pagina N+1 ma il cursore `CurrentPage` punta a N, quindi `is_current()` è falso già alla prima
unità e la pipeline ritorna `Err(LlmError::Cancelled)` prima di tradurre. Vedi la diagnosi per la
catena completa.

La correzione: **non attaccare `is_current` alle richieste di prefetch** (`update_context == false`),
rispecchiando il comportamento del cloud (`None`). Le richieste on-demand (`update_context == true`)
continuano a usare `is_current` come oggi.

## Acceptance Criteria

- [ ] Con provider `llamaserver`, dopo la traduzione on-demand di una pagina N, il prefetch di N+1
      **completa** e scrive la cache per-unità (verificabile: compare una riga `[usage] ... page=N+1
      ... prefetch=true ...` nel log del backend; arrivando a N+1 la pagina è già tradotta).
- [ ] Le richieste on-demand locali mantengono `is_current` e la cancellazione dei job stantii
      (nessuna regressione del ticket 06): navigando via da una pagina in traduzione, il job viene
      ancora cancellato al confine di unità.
- [ ] Il provider cloud resta invariato (già `is_current: None`).
- [ ] La serializzazione L3 (`LocalProviderSlot`) resta invariata.
- [ ] Nuovo test che copre il caso prefetch: `update_context == false` ⇒ `is_current` non attaccato
      (o, a livello pipeline, un job con `is_current()` falso a idx 0 e `update_context=false` deve
      completare, non cancellare). Il test dimostra il bug prima del fix (RED) e passa dopo (GREEN).
- [ ] Suite completa verde.

## Blocked By

- None - can start immediately.

## Frontier

Ready now. Bug attivo che l'utente sta colpendo dal vivo; fix a una riga + test. È l'unica cosa che
impedisce al prefetch (già implementato, ticket 12) di funzionare col provider locale di default.

## Step-by-Step Implementation Plan

1. **RED**: aggiungere un test che riproduce il bug. Preferire il livello più basso testabile:
   - a livello `translate_page` (translate.rs): un job con `is_current` che ritorna `false` dal
     primo boundary e `update_context=false` — oggi ritorna `Cancelled`; il test asserisce che deve
     completare. Oppure, più semplice e mirato alla causa, a livello del wiring in lib.rs: una
     funzione pura che decide se attaccare `is_current` dato `(should_check_is_current, update_context)`.
   Perché prima: fissa il comportamento atteso e previene la regressione (il gap di test che ha fatto
   passare il bug — vedi diagnosi §Testing Decisions).
2. **GREEN**: in `src-tauri/src/lib.rs` (~riga 567) cambiare il wiring in
   `is_current: if should_check_is_current(&cfg.base_url) && update_context { Some(&is_current) } else { None }`.
   Se aiuta la testabilità, estrarre la decisione in una piccola funzione pura (es.
   `fn should_attach_is_current(base_url, update_context) -> bool`) accanto a `should_check_is_current`
   e testarla. Verificare: il test del passo 1 passa.
3. **Regressione**: eseguire i test esistenti di cancellazione/serializzazione (cerca `is_current`,
   `Cancelled`, `LocalProviderSlot` nei moduli test) — devono restare verdi. Verificare che il caso
   on-demand mantenga la cancellazione.
4. Verifica finale: `cargo build` + `cargo test` in `src-tauri`; suite verde.
5. Verifica manuale (non AFK): con l'app dev, provider `llamaserver`, tradurre una pagina e
   controllare che nel log compaia `prefetch=true` per N+1 e che navigando avanti la pagina sia
   già in cache.

## Testing Plan

- Nuovo unit test del caso prefetch (idx 0 non-current + `update_context=false` → completa) e/o della
  decisione di wiring pura.
- Regressione: test esistenti di is_current/Cancelled/LocalProviderSlot verdi; suite completa
  invariata o superiore.
- Manuale: log `prefetch=true` presente col provider locale; pagina N+1 cache-warm all'arrivo.

## Out of Scope

- Opzione B della diagnosi (staleness del prefetch come "cursore avanzato") — non necessaria ora.
- Qualsiasi modifica al provider cloud o alla serializzazione L3.
- Modifiche al frontend (il prefetch parte già correttamente; il `catch {}` che inghiotte l'errore
  può restare — dopo il fix non arriva più `Cancelled`).
