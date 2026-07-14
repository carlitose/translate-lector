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
