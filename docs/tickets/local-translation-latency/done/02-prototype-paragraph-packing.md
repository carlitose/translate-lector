# 02 — Prototipo: packing di paragrafi per finestra entro budget + semantica cache

## Parent Spec

[local-translation-latency-wayfinder.md](../../specs/local-translation-latency-wayfinder.md)

## Type

prototype

## Outcome

Prova che impacchettare paragrafi adiacenti fino a `budget_unit_text` (~900 token in locale) riduce le
chiamate per pagina di ~8-10× mantenendo il round-trip esatto del testo, e una risposta chiara
all'incognita di design: **che cosa succede alla cache per-unità quando il packing cambia gli indici**.

## Acceptance Criteria

- [x] Funzione pura prototipale `pack_units(units, budget_tokens, ratio) -> Vec<String>` con unit test:
      round-trip esatto, nessuna finestra oltre budget, atomi oltre budget restano finestre a sé.
- [x] Conteggio misurato: pagina densa da 18 paragrafi → 1-2 finestre (≥9× di riduzione, sopra il
      target di 5×).
- [x] Analisi della semantica cache con numeri (vedi Risultati).
- [x] Evidenze e raccomandazione ripiegate nella mappa come input al grilling (Ticket 03).

## Risultati (2026-07-14)

**Prototipo**: `pack_units(units, budget, ratio)` in `src-tauri/src/translate.rs` (dopo
`pack_sentences`, **non cablato**). Greedy e deterministico: accumula unità adiacenti finché
`est_tokens ≤ budget`. Composizione col flusso esistente: `pack_units(split_into_units(text, b, r), b, r)`
— il fallback a frasi per paragrafi oltre budget resta dentro `split_into_units`, invariato.

**Test** (6 nuovi, suite completa 210/210 verde): round-trip esatto su prosa/tecnico/pagina densa a 4
budget; finestre entro budget; riduzione ≥4× asserita (misurata 18→1 a budget 900); atomo oltre budget
resta finestra a sé; input degeneri preservati; analisi repack.

**Analisi cache** (test `measure_pack_repack_cache_stability`, repack 900→700 token, come quando
summary/glossario cambiano la quota fissa del budget):

| Semantica | Finestre | Stabili dopo repack | Note |
|---|---|---|---|
| Per-paragrafo (oggi) | 18 | **18/18** | massima stabilità, ma 18 chiamate = 18 CoT |
| Finestre a budget dinamico | 1 → 2 | **0** (per indice E per hash) | ogni repack azzera la cache della pagina |
| **Finestre a taglia FISSA (512)** | 2 → 2 | **2/2** | stabile per costruzione, ~9× meno chiamate |

**Raccomandazione (input a L1 del grilling)**: packing a **taglia fissa** — una costante
`PACK_TARGET_TOKENS ≈ 512`, indipendente dal `budget_unit_text` dinamico, clampata al budget solo quando
il budget è più stretto della costante (edge case: glossario/summary enormi). Così il packing dipende
solo dal testo della pagina → chiave di cache `(unit_index, source_hash)` attuale resta valida senza
migrazioni, e i repack tra pagine/sessioni non buttano via nulla. La cache granulare per-paragrafo
sarebbe ancora più stabile ma vanificherebbe il taglio delle chiamate (il CoT si paga per chiamata).

**Nota per la build (ticket 04)**: con finestre da ~512 token di input la traduzione può superare
`OUT_UNIT_TOKENS=1024` (CoT ~500 + traduzione ~512-1024): dimensionare `max_tokens` per finestra con
`OUTPUT_TOKENS_PER_INPUT` + margine CoT, e ricordare che il fix timeout (ticket 13) è prerequisito
(finestra impacchettata ≈ 42-50 s > 30 s default).

## Blocked By

- None — eseguito.

## Frontier

Il packing è il fix a maggior impatto stimato (C2+C3), ma tocca la decisione D1 di STC (unità = paragrafo)
e la chiave della cache per-unità. Senza un prototipo che quantifichi riduzione delle chiamate e costo in
cache-miss, il grilling deciderebbe alla cieca.

## Work Plan

1. Leggere `split_into_units`/`split_paragraphs` (`src-tauri/src/translate.rs:198-260`) e la cache
   per-unità (`translate.rs:389-487`).
2. Scrivere il prototipo di packing greedy (accumula paragrafi finché `est_tokens ≤ budget`), riusando
   `est_tokens` e la logica separatori esistente. Non cablarlo nella pipeline.
3. Unit test di round-trip e budget; misurare N unità prima/dopo su testi di pagina reali (riusare le
   fixture dei test STC se presenti).
4. Simulare un repack con budget diverso (±200 token) e contare i cache-miss risultanti con la chiave
   attuale.
5. Scrivere la raccomandazione sulla chiave di cache e ripiegare nella mappa.

## Evidence to Capture

- Path e firma del prototipo, output dei test, tabella N-unità prima/dopo, conteggio miss su repack.

## Out of Scope

- Cablare il packing nella pipeline (`translate_page`) — è il Ticket 04.
- Migrazioni DB reali: solo analisi/proposta di chiave.
- Cap del summary (Ticket 05) — qui il budget si calcola con i parametri attuali.
