## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)
(design: sezione "Design multi-chiamata" → contratto split D5)

## What to Build

Rendere **budget-safe la chiamata perceptor-update** di fine pagina, che oggi è l'ultima chiamata grande e
causa `EC08` su contesto piccolo. Prova (2026-07-14): le chiamate translate-only per paragrafo funzionano
(`finish_reason=stop`), mentre il percettore riusa il **contratto completo** (`build_messages`/`build_request`
+ `response_format` con `translated_text`), quindi chiede al modello di **ri-tradurre l'intera pagina** oltre
a summary+glossario → maxi-output che sfonda i 4k → EC08. E poiché quella chiamata fallisce con `?`, l'app
**scarta la traduzione già fatta** e mostra l'errore.

Obiettivo: il percettore aggiorna **solo** riassunto e glossario, **senza ri-tradurre**, con input compatto,
e un suo fallimento **non** deve buttare via la traduzione della pagina.

## Acceptance Criteria

- [ ] Nuovo **contratto snello** per il perceptor-update: prompt + `response_format` che chiedono SOLO
      `{ updated_summary, new_glossary_terms }` (niente `translated_text`). Riuso della compressione EC05.
- [ ] Input compatto: passare il **glossario selezionato/compatto** per la pagina (es. `select_glossary` sul
      testo pagina o l'unione delle selezioni per-unità), non l'intero glossario; il testo pagina resta l'input
      per il riassunto (se enorme, va bene ridurlo, ma non è il focus di questo ticket).
- [ ] `translate_page` usa il contratto snello per il percettore al posto di `build_messages`/`build_request`
      completi; le chiamate translate-only per unità restano invariate.
- [ ] **Resilienza**: un fallimento del perceptor-update **non** fa fallire l'intera pagina — la traduzione
      (dalle unità) viene comunque restituita e la cache pagina scritta; il summary/glossario semplicemente
      non avanza (log/segnalazione soft), coerente con la cache per-unità (STC-09). Nessun `?` che scarta la
      traduzione riuscita.
- [ ] Su prefetch (`update_context=false`) il percettore resta saltato del tutto (come oggi).
- [ ] `cargo test` verde (nuovi test: contratto snello senza `translated_text`; glossario compatto nel prompt
      percettore; fallimento percettore → pagina comunque tradotta+cachata, summary non avanzato); `cargo
      build`/`clippy` ok.

## Blocked By

- None - can start immediately (STC-08/09 già in `done/`).

## Frontier

Ready. È l'ultimo edge per far funzionare **davvero** il caso 4k end-to-end con il percettore attivo:
elimina l'ultima chiamata grande e rende la pagina resiliente al fallimento del percettore.

## Step-by-Step Implementation Plan

1. In `src-tauri/src/llm.rs`: aggiungere un contratto/prompt snello per il percettore — `response_format`
   con solo `updated_summary` + `new_glossary_terms` (nuovo schema o variante), builder dedicato
   (`build_perceptor_*`) e parse. Perché prima: è il contratto che il flusso userà. Verifica: test dello
   schema (niente `translated_text` richiesto) + parse.
2. In `src-tauri/src/translate.rs`: nel blocco perceptor-update usare il nuovo contratto con il glossario
   **selezionato/compatto** per la pagina; mantenere EC05. Verifica: il prompt del percettore non contiene
   l'intero glossario né chiede la traduzione.
3. **Resilienza**: avvolgere la chiamata percettore in modo che un errore non propaghi con `?` ma venga
   loggato/segnalato; la funzione prosegue, scrive la cache pagina e restituisce `translated_text`; summary
   non avanzato (nessun `set_rolling_summary`/insert termini). Verifica: test con MockClient che fa fallire
   il percettore → risultato con traduzione presente, `updated_summary=None`, cache pagina scritta.
4. Verifica end-to-end manuale (server locale): la pagina del libro si traduce senza EC08.

Pitfall: non far avanzare il summary quando il percettore fallisce (coerenza); mantenere il comportamento
prefetch; non rompere l'equivalenza cloud (il contratto snello va bene anche per il cloud — meno token).

## Testing Plan

- Unit (Rust, MockClient): contratto snello (no `translated_text`); glossario compatto nel prompt percettore;
  percettore-fail → pagina tradotta+cachata, summary non avanzato; prefetch salta il percettore.
- Regressione: summary/glossario si aggiornano correttamente quando il percettore riesce; test STC-08/09 verdi.
- Manuale: pagina reale su server locale a `n_ctx` piccolo → nessun EC08 dal percettore.

## Out of Scope

- Riduzione/riassunto del testo pagina in input al percettore per pagine gigantesche (possibile follow-up).
- Controllo del reasoning lato modello/server (ticket separato, epica provider locale).
