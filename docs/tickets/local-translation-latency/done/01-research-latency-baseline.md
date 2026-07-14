# 01 — Baseline misurata: prefill/decode e secondi per chiamata sul server locale

## Parent Spec

[local-translation-latency-wayfinder.md](../../specs/local-translation-latency-wayfinder.md)

## Type

research

## Outcome

Numeri reali che validano (o smentiscono) l'assunzione "il prefill ripetuto domina la latenza" (C2+C3
della diagnosi): per una pagina densa reale, quante chiamate, quanti token prefillati/generati per
chiamata, quanti secondi per chiamata, e la ripartizione prefill vs decode sul server locale.

## Acceptance Criteria

- [x] Tabella misurata: token in/out per chiamata, ms per chiamata (vedi Risultati; protocollo sintetico
      rappresentativo delle forme dell'app, non GUI — motivato sotto).
- [x] Ripartizione prefill/decode stimata.
- [x] Verdetto esplicito con stima del guadagno per ciascun fix.
- [x] Risultati ripiegati nella mappa (Decisions So Far / Not Yet Specified).

## Risultati (2026-07-14, server Unsloth Studio localhost:8888, gemma-4-E2B-it-qat-GGUF, n_ctx server 4096)

Protocollo: chiamate `/v1/chat/completions` con prompt che replicano le forme dell'app (system ~110 tok +
summary ~1000 tok + glossario ~120 tok + unità), `temperature 0`, tempo misurato lato client
(`scratchpad/measure_latency.py`, `measure_cot.py`, `test_nothink.py`). Non è stata usata la GUI (serve un
PDF reale e cache fredda ripetibile); le forme sintetiche riproducono fedelmente i token del percorso
translate-only.

| Chiamata | s | prompt tok | cached tok | out tok | finish |
|---|---|---|---|---|---|
| A. controllo corto (system+paragrafo) | 6.7 | 181 | 176 | 96 | length |
| B. per-paragrafo, overhead pieno, 1ª volta | 5.6 | 1133 | 115 | 96 | length |
| C. idem, prefisso identico, unità diversa | 4.9 | 1138 | **1078** | 96 | length |
| D. decode puro (prompt corto, 512 out) | 19.2-19.6 | 181 | 176 | 512 | length |
| E. finestra impacchettata (~900 tok unità), 1024 out | 42.0-50.1 | 1953 | 1128-1948 | 1024 | **length** |
| **F. unità stile-app (paragrafo 55 tok, max_tokens 1024)** | **29.7** | 1133 | 1128 | **559** | stop |

Soppressione del thinking (paragrafo singolo, max_tokens 1024): `reasoning_effort:none`,
`chat_template_kwargs:{enable_thinking:false}`, `reasoning:{enabled:false}`, `/no_think` nel system,
istruzione esplicita "non ragionare" → **nessuna funziona**: sempre ~480-560 token di output, ~20-26 s
(il kwargs deraglia pure l'output). Il modello QAT pensa sempre; il proxy scarta il CoT dal `content`
ma i token vengono comunque generati e pagati (`completion_tokens=559` per una traduzione da ~60 tok).

Tentativo GemmaX2-28-2B (modello traduzione senza reasoning, presente in libreria ma `loaded:false`):
il proxy **serve silenziosamente il modello caricato** ignorando il `model` richiesto → il test richiede
il caricamento manuale in Unsloth Studio (HITL, rimandato al grilling).

## Verdetto

1. **C3 (prefill ripetuto domina) = SMENTITA.** Il server fa **prefix caching**: dalla seconda unità
   della stessa pagina `cached_tokens≈1078/1133` (B vs C). Prefill non cached ≈ 2000 tok/s (~0.5 s per
   1000 tok). L'overhead fisso ri-inviato costa quasi zero *finché il prefisso è identico* (vero
   entro la pagina: il summary cambia solo tra pagine).
2. **Collo di bottiglia reale = decode del CoT.** Decode ≈ 20-27 tok/s; ogni chiamata per-unità genera
   ~500 token di "Thinking Process" scartati + ~60 di traduzione → **~30 s per paragrafo**. Pagina densa
   da ~18 paragrafi ≈ 8-9 min teorici.
3. **C1 corretta: il timeout è dell'APP, non del proxy.** `reqwest::blocking::Client::new()` ha un
   **timeout di default di 30 s** (docs.rs, `blocking/client.rs`: `Timeout(Some(Duration::from_secs(30)))`;
   il client async invece non ne ha — l'assunzione "nessun timeout" del ticket 13 era basata sul
   comportamento async). La chiamata per-unità misura **29.7 s**: esattamente al limite. Le unità che
   sforano → `is_timeout()` → retry ×3 (altri 30 s l'uno) → errore. Spiega perché il sintomo appare
   solo sulle pagine dense (più CoT → chiamate sopra i 30 s).
4. **Interazione critica**: la finestra impacchettata (E) dura 42-50 s → **col timeout attuale di 30 s
   il packing peggiorerebbe le cose**. Il fix del timeout (ticket 13) è un **prerequisito** del packing.

### Stima dei guadagni (pagina densa ~1000 tok sorgente, ~18 paragrafi)

| Intervento | Stima | Note |
|---|---|---|
| Solo fix timeout (180 s) | pagina completa in ~8-9 min | elimina l'errore, non la lentezza |
| + packing (1-2 finestre) | ~1-2 min | CoT pagato 1-2 volte invece di 18 |
| + modello senza reasoning (GemmaX2) | ~35-70 s | elimina il CoT; **leva più grande**, richiede HITL |
| Cap summary | trascurabile per la latenza | il prefisso è già cached; utile solo per allargare `budget_unit_text` |

Floor fisico con questo hardware: ~token-di-traduzione / 25 tok/s ≈ 40 s per pagina densa. Il target
"<10 s" della mappa non è raggiungibile su pagina densa fredda: ridimensionare in L5 (grilling).

## Blocked By

- None — eseguito.

## Evidence

- Script e output: `scratchpad/measure_latency.py`, `measure_cot.py`, `test_nothink.py` (sessione
  2026-07-14); righe di output riportate nella tabella sopra.
- Docs reqwest: docs.rs `blocking/client.rs` — default `Timeout(Some(30s))` (verificato via ctx7).
- Server: `GET /v1/models` → `context_length: 4096`, `native_context_length: 131072`.

## Out of Scope

- Qualsiasi fix (packing, cap summary, timeout): solo misura.
- Tuning del server (n_gpu_layers, batch size).
- Test di GemmaX2 (richiede load manuale in Unsloth Studio → input al grilling 03).
