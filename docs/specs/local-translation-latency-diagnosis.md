# Diagnosi — Traduzione locale lenta e timeout su pagine dense (localhost:8888)

## Type

Diagnostic spec

## Status

Confirmed, **rivista dopo misure reali** (2026-07-14, ticket 01 di local-translation-latency: C1
corretta, C3 smentita, aggiunta C6 — vedi le note ⚠ nelle sezioni; nessun fix applicato)

## Symptom

Con provider locale (Unsloth Studio → llama-server su `http://localhost:8888`):

- `Errore di rete/servizio LLM (timeout): error sending request for url (http://localhost:8888/v1/chat/completions)` su pagine con molti caratteri.
- Latenza percepita molto alta anche senza timeout (~38-42 s/pagina misurati sul modello Q4 `gemma-4-E2B-it-qat-GGUF`).

Il sintomo si presenta **nonostante** la pipeline small-context (STC) sia già attiva: perceptor separato, unità sotto budget, finestre < 4k token.

## Root Cause

Non c'è una singola causa: è l'effetto composto di cinque fattori. Le pagine dense sono il caso peggiore perché moltiplicano il numero di chiamate.

### C1 — Timeout implicito di 30 s del client blocking (⚠ RIVISTA: è dell'app, non del proxy)

- `ChatCompletionsClient::new` costruisce il client con `reqwest::blocking::Client::new()` senza `.timeout()` esplicito (`src-tauri/src/llm.rs:541`), identico per provider locale e cloud (`src-tauri/src/lib.rs:294-298`).
- ⚠ **Correzione (misure ticket 01, 2026-07-14)**: a differenza del client async, il **client blocking di reqwest ha un timeout di DEFAULT di 30 secondi** (docs.rs, `blocking/client.rs`: `Timeout(Some(Duration::from_secs(30)))`). Il taglio è quindi **dell'app stessa**, non del proxy di Unsloth Studio come ipotizzato in prima stesura (e nell'ipotesi del ticket 13). La chiamata per-unità reale misura **29.7 s** — esattamente al limite: le unità che sforano producono `e.is_timeout()` → `LlmError::Timeout` (`classify_send_error`, `llm.rs:565-573`), retry ×3 da altri ~30 s l'uno, poi errore. Spiega perché il sintomo appare solo su pagine dense (più CoT, C6 → più chiamate sopra i 30 s).
- Il fix è certo e locale: `Client::builder().timeout(...)` per-provider — [13-local-inference-timeout.md](../tickets/local-llm-provider/13-local-inference-timeout.md) (aperto; la sua fase di conferma va aggiornata con questo dato).

### C2 — Una chiamata per paragrafo, senza packing: decine di round-trip sequenziali per pagina

- `split_into_units` (`src-tauri/src/translate.rs:198-222`) emette **una unità per paragrafo** quando il paragrafo sta nel budget; **non impacchetta** paragrafi adiacenti fino a riempire `budget_unit_text`. Il packing (`pack_sentences`) scatta solo come fallback per paragrafi *oltre* budget.
- Paragrafi reali misurati a ~40-90 token (STC-02), contro un budget utile di ~900 token per finestra (vedi §Modello di costo): il budget è usato al 5-10%.
- Le unità sono tradotte in un loop **strettamente sequenziale** (`for (idx, unit) in units.iter().enumerate()`, `translate.rs:667`), senza alcuna concorrenza; + 1 chiamata perceptor-update a fine pagina quando `update_context=true` (`translate.rs:818-865`). Una pagina densa da ~30-70 paragrafi ⇒ ~30-70 round-trip HTTP + 1.

### C3 — Overhead fisso di prompt ri-inviato a ogni unità (⚠ SMENTITA dalle misure: il prefill NON domina)

Ogni chiamata translate-only ri-manda per intero system (~150 tok), rolling summary (fino a 1000 tok, `translate.rs:640, 706`) e glossario selezionato (`translate.rs:73, 698-707`): ~1300 token fissi per ~40-90 di payload.

⚠ **Smentita (misure ticket 01, 2026-07-14)**: il server fa **prefix caching** — dalla seconda unità della stessa pagina `usage.prompt_tokens_details.cached_tokens ≈ 1078/1133`, e il prefill non cached viaggia a ~2000 tok/s (~0.5 s per 1000 token). L'overhead fisso costa quindi **quasi zero** finché il prefisso resta identico (vero entro la pagina: il summary cambia solo tra pagine). Il cap del summary resta utile solo per **allargare `budget_unit_text`** (finestre più capienti), non per la latenza.

Il vero collo di bottiglia è il **decode**, in particolare il CoT del modello → vedi C6.

### C4 — Retry annidati moltiplicano il caso lento

- Il timeout è classificato **transient** (`is_transient`, `llm.rs:335-343`) → `RetryingChatClient` con `max_attempts: 3` e backoff esponenziale da 500 ms (`llm.rs:419-437, 455-472`). Ogni unità che va in timeout ri-esegue la stessa generazione lenta fino a 3 volte.
- Retry su troncamento: `finish_reason == "length"` con contenuto non vuoto → retry con `max_tokens` raddoppiato, fino a `TRUNCATION_MAX_RETRIES = 2` (`translate.rs:65, 750-776`), cioè fino a 3 iterazioni per unità.
- I due livelli si **annidano**: caso peggiore ~9 richieste HTTP per una singola unità, tutte sequenziali. Per una richiesta sistematicamente lenta il retry-on-timeout non recupera nulla: raddoppia/triplica solo l'attesa (osservazione già presente nel ticket 13).

### C5 — Nessuna cancellazione backend + prefetch concorrente sullo stesso server locale

- `translate_page` gira su `spawn_blocking` **senza cancellation token** (`src-tauri/src/lib.rs:282-319`): se l'utente naviga via, il frontend scarta solo il risultato (`isCurrentRequest`, `src/lib/translation.ts:129-136`; `+page.svelte:429, 435`) ma il backend continua a macinare tutte le N unità + perceptor della pagina abbandonata.
- `prefetchNextPage()` parte subito dopo ogni traduzione riuscita (`+page.svelte:432, 450-467`; default on, `DEFAULT_PREFETCH_ENABLED = true` in `settings.rs`). Con navigazione rapida, prefetch di N+1 e traduzione on-demand di N+1/N+2 girano **in parallelo** come task `spawn_blocking` distinti contro lo stesso server locale mono-modello: le richieste si contendono la GPU, ognuna rallenta, e la probabilità di sforare il timeout di 30 s (C1) sale.

### C6 — Il modello genera ~500 token di CoT per OGNI chiamata (aggiunta dalle misure, causa dominante)

- Misura reale (ticket 01): una chiamata per-unità con paragrafo da 55 token e `max_tokens=1024` produce `completion_tokens=559` in **29.7 s** — ma il `content` restituito è solo la traduzione (~60 token). Il modello (`gemma-4-E2B-it-qat-GGUF`) genera **~500 token di "Thinking Process"** che il proxy scarta ma che vengono decodificati e pagati (decode ~20-27 tok/s ⇒ ~20-25 s solo di CoT).
- Il costo del CoT è **per chiamata**, non per token tradotto: con una chiamata per paragrafo (C2) si paga ~30 s a paragrafo → pagina densa ~8-9 min teorici, e ogni singola chiamata flirta col timeout di 30 s (C1).
- La soppressione via API **non funziona** su questo modello: `reasoning_effort:none`, `chat_template_kwargs:{enable_thinking:false}`, `reasoning:{enabled:false}`, `/no_think`, istruzioni esplicite nel system → sempre ~480-560 token, ~20-26 s (testate tutte, `scratchpad/test_nothink.py`).
- Coerente con la unit-truncation-diagnosis (CoT in-content che mangia `out_unit`): è lo stesso fenomeno, visto qui dal lato latenza.
- Mitigazione più efficace: **modello di traduzione senza reasoning** (GemmaX2-28-2B è già nella libreria del server, `loaded:false`; il proxy però serve silenziosamente il modello caricato ignorando il `model` richiesto → serve load manuale in Unsloth Studio per validarlo).

## Modello di costo (numeri, provider locale n_ctx=4096)

Formula in `compute_budget_unit_text` (`translate.rs:101-116`):

```
out_unit      = OUT_UNIT_TOKENS = 1024        (translate.rs:56)
margine       = BUDGET_MARGIN   = 0.15        (translate.rs:69)
budget_input  = floor((4096 − 1024) × 0.85) ≈ 2611
budget_unit_text ≈ 2611 − system(~150) − summary(fino a ~1000) − glossario(locked + 256 riserva)
                 ≈ ~900 token di testo pagina per finestra
```

Con paragrafi da 40-90 token: una pagina da ~3000 token produce **~35-70 chiamate** invece delle **~4** teoricamente possibili impacchettando a pieno budget. ⚠ Il costo per chiamata NON è il prefill (prefix-cached, C3 rivista) ma il **CoT: ~500 token × ~35-70 chiamate ≈ 17k-35k token decodificati a vuoto** a 20-27 tok/s (C6).

## Mitigazioni già esistenti (per completezza)

- **Cache per-unità persistente** `unit_translations`, chiave `(document_id, page_number, unit_index, target_language, source_hash FNV-1a)` (`translate.rs:389-487`), scritta subito dopo ogni unità riuscita (`translate.rs:784`): un retry dopo timeout ri-traduce solo le unità mancanti.
- **Cache pagina** `translations_cache` con verifica del `source_text`: hit istantaneo, zero chiamate.
- **Perceptor saltato in prefetch** (`update_context=false`) e il suo fallimento non scarta la traduzione completata.
- **`working_shape`** riusato tra unità: il fallback param-degrade si paga una volta sola per pagina (`translate.rs:665, 757-761`).

Queste mitigazioni riducono il costo dei *retry di pagina*, non la latenza della prima passata.

## Direzioni di fix (solo elencate — fuori scope di questo documento; riordinate dopo le misure del ticket 01)

Ordinate per impatto sulla latenza della prima passata:

1. **Timeout esplicito per-provider (~180 s) + niente retry-on-timeout in locale** = ticket 13 esistente. È il fix diretto del sintomo (C1) e **prerequisito** del packing: una finestra impacchettata dura 42-50 s > del default di 30 s.
2. **Modello di traduzione senza reasoning** (es. GemmaX2-28-2B, già in libreria): elimina C6, la causa dominante (~10× su unità piccole). Richiede validazione HITL di qualità/velocità.
3. **Packing di paragrafi adiacenti** fino a `budget_unit_text`: il CoT si paga 1-2 volte per pagina invece di 35-70 (~5-7× anche col modello attuale). Attenzione alla semantica della cache per-unità (`unit_index` cambia al variare del packing; il `source_hash` protegge comunque la correttezza).
4. **Serializzare prefetch vs on-demand e cancellare i job stantii** (C5): evita la contesa sul server mono-modello.
5. **Cap del summary inviato alle chiamate translate-only** — declassato: il prefisso è prefix-cached (C3 rivista); serve solo ad allargare `budget_unit_text`.

## Open Questions

- ~~Chi taglia la connessione e con quale limite~~ → **RISPOSTO** (ticket 01): il default di 30 s del client blocking di reqwest (C1 rivista).
- ~~Ripartizione prefill/decode reale~~ → **MISURATA** (ticket 01): prefill ~2000 tok/s + prefix caching; decode 20-27 tok/s; domina il CoT (C6).
- Qualità e velocità reali di GemmaX2-28-2B come modello di traduzione (richiede load manuale in Unsloth Studio) → grilling 03 della mappa latenza (L6).
