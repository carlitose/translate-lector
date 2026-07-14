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
