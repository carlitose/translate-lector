# Test Plan — Ticket 04: packing a finestre fisse cablato in `translate_page`

## Scope

- **Cosa cambia**: `translate_page` (`src-tauri/src/translate.rs`) non traduce più un paragrafo per
  chiamata LLM: le unità prodotte da `split_into_units` vengono impacchettate da `pack_units` in
  finestre a taglia FISSA `PACK_TARGET_TOKENS = 512` (clampata a `budget_unit_text` solo se più
  stretto). Il cap di output per finestra è calcolato dalla nuova funzione pura
  `window_output_cap(body_tokens, max_tokens, headroom)` = `corpo×2 + COT_RESERVE_TOKENS(512)`,
  floor `OUT_UNIT_TOKENS`, bounded da `max_tokens` del provider e headroom di contesto.
- **Come si verifica**: su una pagina densa col provider locale le chiamate scendono da ~18 a 1-2,
  la pagina fredda si traduce in ≤2 minuti senza timeout (target L5), il testo ricomposto è
  identico all'originale nella struttura dei paragrafi, la cache per-finestra è stabile, e il
  percorso cloud (n_ctx grande → una unità = pagina intera) resta invariato.

## Comportamento cambiato (riassunto per il tester)

| Prima (per-paragrafo) | Dopo (ticket 04) |
|---|---|
| 1 chiamata LLM per paragrafo (~18+1 su pagina densa) | 1 chiamata per finestra impacchettata (1-2 + perceptor) |
| cap output per unità = `OUT_UNIT_TOKENS` fisso | cap per finestra = `corpo×2 + 512 CoT`, bounded (~1536 coi default locali) |
| ~530 s/pagina densa, timeout col default 30 s | ~99 s/pagina densa, zero timeout (misura ticket) |
| righe cache `unit_translations` per-paragrafo | righe per-finestra (stessa chiave `unit_index`+`source_hash`; le vecchie righe sono MISS e vengono sovrascritte/prunate) |

## Prerequisiti

- Build dell'app: `npm run tauri dev` (o build release) dalla root del repo.
- Provider locale: server OpenAI-compatible su `http://localhost:8888` con modello
  `gemma-4-E2B-it-qat` caricato (n_ctx 4096), auth `Bearer <la propria chiave Unsloth Studio>`.
- Ticket 13 risolto: timeout per-provider locale configurato (~180 s) — prerequisito, una finestra
  da 512 token dura ~45-50 s.
- PDF di test con almeno una pagina densa (≥15 paragrafi, ~900 token) e una pagina corta
  (1 paragrafo).
- Per i passi cloud: credenziali di un provider cloud configurate nell'app.
- DB dell'app raggiungibile (SQLite) per ispezionare `unit_translations` nei passi cache.

## Happy Path

| step | action | expected_result |
|---|---|---|
| HP1 | Avvia il server locale, apri l'app, apri il PDF di test e vai alla pagina densa (~18 paragrafi); lancia la traduzione col provider locale e cronometra | La pagina fredda completa in ≤2 minuti, senza errori di timeout (target L5) |
| HP2 | Durante HP1 osserva i log del server locale (o dell'app) | 2-3 richieste `/chat/completions` totali per la pagina (1-2 finestre + 1 perceptor), NON ~19 |
| HP3 | Confronta il testo tradotto con l'originale | Tutti i paragrafi presenti, nell'ordine originale, separatori di paragrafo preservati; nessun paragrafo mancante o duplicato |
| HP4 | Nei log del server verifica il `max_tokens` delle richieste di finestra | ~1536 per una finestra piena (corpo×2 + 512), comunque ≤ max_tokens del provider (2048) e coerente con `prompt + output ≤ n_ctx` |
| HP5 | Rinaviga alla stessa pagina (o richiedi di nuovo la traduzione) | Servita dalla cache di pagina: 0 chiamate LLM, risultato identico |

## Edge Cases

| step | action | expected_result |
|---|---|---|
| EC1 | Traduci una pagina con un solo paragrafo corto col provider locale | Una sola finestra → 1 chiamata di traduzione + perceptor; per il percorso a unità singola vale `max_tokens` di pagina, non il cap finestra |
| EC2 | Traduci una pagina col provider cloud (n_ctx grande) | Comportamento invariato (degrade D2): una unità = pagina intera, 1 chiamata, `max_tokens` pieno |
| EC3 | Pagina con un paragrafo oltre il budget (testo monolitico >512 token senza righe vuote) | Fallback a frasi dentro `split_into_units` invariato; le frasi vengono poi impacchettate in finestre; round-trip del testo preservato |
| EC4 | Summary/glossario molto grandi che restringono `budget_unit_text` sotto 512 | Il budget di packing è clampato (`PACK_TARGET_TOKENS.min(budget_unit_text)`): finestre più piccole, nessun errore EC08 da prompt oltre contesto |
| EC5 | DB con righe `unit_translations` scritte dalla versione per-paragrafo (pre-ticket), poi traduci la stessa pagina con la nuova build | Le vecchie righe NON vengono servite (hash diverso → MISS), le finestre vengono tradotte e scritte via UPSERT, le righe in coda oltre il nuovo `unit_count` vengono prunate; testo finale corretto |
| EC6 | Ripeti la traduzione dopo un cambio di parametri che altera `budget_unit_text` (es. summary cresciuto) su pagina già in cache per-finestra | Nessun cache-miss di massa: le finestre dipendono solo dal testo e dalla costante 512, quindi restano stabili al repack |

## Percorsi negativi / errori

| step | action | expected_result |
|---|---|---|
| NP1 | Traduzione di una finestra troncata dal modello (`finish_reason=length`) | Il retry-troncamento a livello di finestra scatta con cap maggiorato; se il troncamento persiste, errore EC08 (`OutputBudgetExhausted`) mostrato all'utente con codice EC08 |
| NP2 | Il server locale fallisce (HTTP error) sulla seconda finestra | Errore propagato all'utente; la prima finestra resta in cache; al retry solo la finestra fallita + perceptor vengono richiamate |
| NP3 | Il perceptor fallisce dopo che tutte le finestre sono tradotte | La traduzione è comunque restituita (fail soft), il summary non avanza; al retry la pagina è servita dalla cache di pagina |
| NP4 | Server locale spento | Errore di connessione chiaro, nessun crash dell'app |

## Rischi di regressione

| step | action | expected_result |
|---|---|---|
| RR1 | Selezione glossario per-unità: pagina con termini di glossario in paragrafi diversi che finiscono nella stessa finestra | Il glossario selezionato per la finestra include i termini di tutti i paragrafi contenuti; le traduzioni rispettano i termini bloccati |
| RR2 | `working_shape` / continuità del summary tra pagine | Invariati: il perceptor gira una volta per pagina come prima |
| RR3 | Traduzione cloud end-to-end (pagina qualsiasi) | Nessun cambiamento percepibile: 1 chiamata, stessa qualità, stessi tempi |
| RR4 | Guardia EC08 `prompt + output ≤ n_ctx` sulle richieste di finestra | Mai violata anche col nuovo cap maggiorato (bounded da headroom) |
| RR5 | Suite Rust completa | `cargo test` da `src-tauri` verde (baseline 228/228) |

## Fuori scope

- Cap del summary (ticket 05) e prefetch/cancellazione (ticket 06).
- Parallelismo tra finestre (non implementato, esplicitamente out of scope del ticket).
- Cambio modello locale (decisione L6) e tuning del server (n_ctx, quantizzazione).
- Conferma e2e nella GUI contro il server reale: coperta dal ticket 10 (e2e HITL) dell'epica
  local-llm-provider; qui si cita la misura HTTP già eseguita (sezione "Misura di conferma" del
  ticket 04).
