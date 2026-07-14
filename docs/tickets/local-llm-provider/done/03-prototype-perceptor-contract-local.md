# 03 — Tenuta del contratto percettore su modello locale (json/fallback/qualità)

## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)

## Type

prototype

## Outcome

Un **verdetto**: un LLM locale servito via l'endpoint del Ticket 01 produce il JSON del percettore
(`{ translated_text, updated_summary, new_glossary_terms }`, §4.4) in modo affidabile — tramite
`response_format: json_schema` o tramite l'estrazione di fallback già presente — con **qualità di
traduzione e latenza usabili**.

## Acceptance Criteria

- [ ] Spike che invia al modello locale il **prompt reale del percettore** (da `src-tauri/src/llm.rs` /
      `translate.rs`) su almeno 3 pagine di testo rappresentative.
- [ ] Verificato se il modello locale onora `json_schema`; se no, verificato che la **ladder + estrazione
      JSON di fallback** dell'app produce comunque un oggetto valido e parsabile.
- [ ] Valutazione qualitativa della traduzione e della tenuta di summary/glossario su più pagine
      consecutive (coerenza del percettore), confrontata a spanne con OpenRouter.
- [ ] Misura di **latenza per pagina** sul modello locale e nota sull'usabilità (con hardware usato).
- [ ] **Nota di compatibilità con l'epica OCR**: stima se il modello reggerebbe il contratto strutturato
      per-blocco più complesso (OCR Ticket 05) o se servirà un modello locale più capace per quel caso.
- [ ] Verdetto scritto nel parent spec: locale "pronto per traduzione testo semplice" sì/no + condizioni.

## Blocked By

- Ticket 01 (serve un endpoint locale funzionante a cui parlare).

## Frontier

Risolve l'incognita "un modello locale piccolo basta davvero per il nostro contratto?". Senza questo
verdetto, non sappiamo se il provider locale è utile in pratica o solo teoricamente collegabile.

## Work Plan

1. Dal Ticket 01, avviare l'endpoint locale con un modello candidato.
2. Riprodurre il prompt del percettore e inviarlo con e senza `response_format`.
3. Verificare parsing (json_schema vs fallback) e coerenza summary/glossario su pagine consecutive.
4. Misurare latenza; giudicare qualità; annotare compatibilità col contratto OCR.
5. Scrivere il verdetto e le condizioni nel parent spec.

## Evidence to Capture

- Risposte JSON grezze del modello locale (con e senza json_schema).
- Esempi di traduzione + summary/glossario su 3+ pagine, con giudizio qualitativo.
- Tempi per pagina e hardware usato.
- Nota di compatibilità con il contratto strutturato OCR.

## Out of Scope

- Design dell'astrazione provider (Ticket 02).
- Implementazione nell'app (build verticali).

---

## Progress note (autopilot, 2026-07-14) — ⛔ BLOCKED: richiede endpoint locale in esecuzione

Questo ticket **non è completabile AFK**: il verdetto richiede un LLM locale servito su un endpoint
OpenAI-compatible IN ESECUZIONE (Unsloth Studio / LM Studio / Ollama / llama-server), che non può essere
avviato in ambiente autonomo. Nessun risultato di QA è stato inventato.

**Deliverable AFK prodotto** (la parte fattibile senza server):
- Harness di validazione pronto all'uso: `prototypes/local-llm/validate-perceptor-contract.mjs`
  (syntax-check OK con `node --check`). Replica il contratto del percettore (§4.4), invia 3 pagine
  consecutive, misura: contratto JSON rispettato N/3, latenza media, coerenza summary/glossario, e prova
  sia il percorso **con `json_schema`** (`--schema`) sia il **fallback** di estrazione JSON.

**Per chiudere il ticket** (passi umani, ~10 min):
1. Avvia un server locale (vedi [research-unsloth-serving.md](../../specs/research-unsloth-serving.md) §6).
2. `node prototypes/local-llm/validate-perceptor-contract.mjs --base <URL> --model <ID> [--key <k>] [--schema]`
3. Ripeti con e senza `--schema`; annota N/3, latenza, qualità, e la nota di compatibilità col contratto
   strutturato OCR nel parent spec, poi sposta il ticket in `done/`.

**Blocca:** nulla a valle strettamente, ma il verdetto informa D1/D6 del [decision-brief Ticket 04](../../specs/decision-brief-local-llm-04.md).

---

## VERDETTO (eseguito 2026-07-14) — ✅ DONE

Endpoint fornito dall'utente: **Unsloth Studio** su `http://localhost:8888/v1/chat/completions`, modello
servito con id **`unsloth/gemma-4-E2B-it-qat-GGUF`** (E2B ≈ 2B effettivi, instruction-tuned). Harness
`prototypes/local-llm/validate-perceptor-contract.mjs` eseguito CON e SENZA `--schema`, 3 pagine consecutive
EN→IT, su **due caricamenti**: (A) quant iniziale QAT, (B) dopo passaggio dell'utente a un quant **Q4**
(stesso id servito).

**Risultati:**

| Metrica | (A) QAT — schema | (A) QAT — fallback | (B) Q4 — schema | (B) Q4 — fallback |
|---|---|---|---|---|
| Contratto JSON | **3/3** | **3/3** | **3/3** | **3/3** |
| Latenza media | ~9.8 s/pag | ~7.3 s/pag | ~42 s/pag | ~38 s/pag |
| Qualità traduzione | scarsa ("Il testo è stato convocato", "tempesta im geste") | mediocre ("Il consiglio si è riunito all'alba", ma mix EN/IT) | **buona** ("Il consiglio si riunì all'alba…") | **buona** (minor slip: "board"→"la tavola") |
| Summary/glossario | scarsi/rotti | vuoti | **popolati** (pag 2: summary + termini mainsail/reefed/board/course) | vuoti |

**Conclusioni:**
1. **Integrazione validata**: il provider locale funziona end-to-end col contratto del percettore (§4.4);
   il client esistente + estrazione JSON robusta bastano. **Contratto 3/3 in tutti e 4 i run.** Nessun
   blocco tecnico all'aggiunta del provider locale.
2. **La qualità dipende fortemente dal quant/modello**: lo stesso id servito è passato da output rotto
   (QAT) a traduzione fluente e corretta (Q4). → **D1**: puntare a un quant **Q4** (o modello ~7B-14B Q4_K_M)
   per l'uso reale; evitare quant troppo aggressivi/QAT minuscoli.
3. **`json_schema` non è universalmente peggiore**: sul QAT minuscolo degradava la fluenza (→ meglio il
   fallback); sul **Q4 va bene e anzi aiuta a popolare summary/glossario**. → conferma il valore del design
   (Ticket 02) di poter **attivare/disattivare `response_format` per-provider**; default ragionevole per
   locale: **provare con schema, la ladder lo rimuove se rifiutato**. Aggiorna **D6**.
4. **Percettore (summary/glossario) inaffidabile su ~2B**: popolamento incostante (solo pag 2 in un run,
   vuoto altrove). La *traduzione* regge, la *percezione di contesto* no. → per una coerenza forte su
   documenti lunghi serve un modello più capace; su ~2B considerare summary/glossario "best-effort".
5. **Latenza**: ~38-42 s/pagina sul Q4 in questo setup — **lenta** per l'interattività. Con prefetch + cache
   è tollerabile per una lettura pagina-per-pagina, ma il primo caricamento di ogni pagina non-cache pesa.
   → alimenta **D2** (hardware: tenere il modello interamente in GPU) e conferma il valore di prefetch/cache.
6. **Compatibilità contratto OCR (epica OCR, Ticket 05)**: se un ~2B popola già a fatica il JSON semplice
   del percettore, quasi certamente **non reggerà** il contratto strutturato per-blocco più complesso. Per
   la ricostruzione OCR servirà un modello locale più grande, o restare su cloud per quel caso.

Evidenza: output completi dei 4 run dell'harness (cronologia sessione 2026-07-14).
