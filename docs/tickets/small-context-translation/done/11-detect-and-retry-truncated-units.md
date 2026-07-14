## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)
(diagnosi: [unit-truncation-diagnosis.md](../../specs/unit-truncation-diagnosis.md))

## What to Build

Impedire che una traduzione di unità **troncata** (`finish_reason: "length"` con testo parziale) venga
accettata come completa, e recuperarla. Diagnosi confermata (triangolazione): oggi `ChatResponse::content()`
ritorna il testo non vuoto **prima** di controllare `finish_reason`, quindi una risposta troncata passa come
valida → paragrafo tagliato a metà (es. "…con milioni o"). Aggrava: il modello locale emette una
chain-of-thought **nel contenuto** ("Thinking Process:…") che consuma il budget, e l'unità troncata viene
messa in cache → il retry ripete il troncamento.

## Acceptance Criteria

- [ ] `finish_reason == "length"` con contenuto **non vuoto** NON viene più accettato come traduzione: sul
      path unità diventa un errore/segnale di troncamento (riordinare `content()` o usare un accessor dedicato
      per il path translate-only). Il caso vuoto+length resta EC08 come oggi.
- [ ] **Retry con budget maggiore**: su troncamento di un'unità, ritentare con `max_tokens` più alto (×2, fino
      all'headroom `n_ctx − prompt − margine`), max 1-2 tentativi. Se ancora troncata al massimo headroom →
      errore EC08 azionabile (oppure spezzare l'unità in frasi e tradurle — scegliere e motivare).
- [ ] L'unità troncata **non** viene scritta in cache per-unità (il fallimento/`?` deve precedere
      `unit_cache_insert`), così un retry ritraduce davvero.
- [ ] **Ridurre la chain-of-thought**: rinforzare `build_translate_only_system_prompt` (vietare
      esplicitamente "Thinking Process"/ragionamento, "rispondi SOLO con la traduzione") e/o strippare un
      blocco CoT iniziale in `parse_translation`. Alzare `OUT_UNIT_TOKENS` (es. 1024-1536) per assorbire la
      verbosità del modello.
- [ ] Equivalenza cloud preservata; prefetch invariato; test STC-08/09/10 verdi.
- [ ] `cargo test` verde (nuovi test: non-empty+length → errore/retry; retry riuscito completa la traduzione;
      unità troncata non cachata; CoT strippata/soppressa); `cargo build`/`clippy` ok.

## Blocked By

- None - can start immediately (STC-06..10 in `done/`).

## Frontier

Ready. È il difetto che rende le pagine "a metà": senza questo, la pipeline a paragrafi produce traduzioni
troncate silenziose sul modello locale verboso.

## Step-by-Step Implementation Plan

1. `src-tauri/src/llm.rs`: far sì che un troncamento (`finish_reason=="length"`) non venga restituito come
   `Ok(testo)` sul path unità — riordinare `content()` o aggiungere un metodo che segnala il troncamento;
   propagare un errore/enum dedicato. Verifica: test `content non vuoto + length → errore`.
2. `src-tauri/src/translate.rs`: nel loop unità, su troncamento ritentare con `max_tokens` cresciuto (fino
   headroom), bounded; solo dopo il successo scrivere `unit_cache_insert`. Verifica: retry completa; unità
   troncata non finisce in cache.
3. Sopprimere/strippare la CoT: aggiornare il system prompt translate-only e/o `parse_translation`; alzare
   `OUT_UNIT_TOKENS`. Verifica: su prompt che prima produceva "Thinking Process:", ora esce solo la
   traduzione (test su parse + prompt).
4. Verifica end-to-end manuale (server locale): la pagina del libro si traduce **completa**.

Pitfall: non rompere il caso vuoto+length (EC08) né l'equivalenza cloud; assicurarsi che il retry rispetti
`prompt + output ≤ n_ctx`; non cachare parziali.

## Testing Plan

- Unit (Rust, MockClient): non-empty+length → errore/retry; retry con budget maggiore → traduzione completa;
  cache non scrive parziali; CoT strippata. Regressione STC-08/09/10.
- Manuale: pagina 39 del libro sul provider locale → nessun troncamento a "…con milioni o".

## Out of Scope

- Controllo del reasoning lato server/modello (epica provider locale).
- Filtro di footer/numeri pagina come "non traducibili" (possibile follow-up separato).
