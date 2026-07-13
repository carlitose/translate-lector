# 02 — Research: contratto OpenRouter, structured output e conteggio token

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md)

## Type

research

## Outcome

Definire con evidenza il contratto reale verso OpenRouter per la chiamata unica del percettore:
supporto `response_format` JSON, strategia di fallback di parsing, come contare/stimare i token
per la compressione del summary (EC05) e il budget del prompt.

## Acceptance Criteria

- [ ] Documentato come chiamare `POST /api/v1/chat/completions` su OpenRouter (header, auth, `HTTP-Referer`/`X-Title`).
- [ ] Chiarito quali famiglie di modelli supportano `response_format: json_schema`/`json_object` e cosa fare per quelle che non lo garantiscono (estrazione blocco JSON, retry con prompt di correzione).
- [ ] Decisione sul conteggio token lato Rust: tokenizer reale (quale crate) vs stima euristica (char/4). Con motivazione.
- [ ] Bozza del prompt del percettore (input: lingua dest, testo pagina, rolling summary, glossario con termini bloccati; output: JSON del §4.4).
- [ ] Findings salvati come nota nel parent spec o in `docs/specs/` e linkati.

## Blocked By

- None — can start immediately.

## Frontier

Sblocca il cuore del core Rust (percettore). Senza chiarezza su structured-output e token budget, la logica di prompt/parsing/compressione è solo congettura.

## Work Plan

1. Usare la skill `find-docs`/`ctx7` per la documentazione corrente di OpenRouter (structured outputs, models, headers).
2. Verificare comportamento `response_format` per modelli tipici (`anthropic/claude-*`, `openai/gpt-*`, `google/gemini-*`).
3. Valutare crate di tokenizzazione Rust (es. `tiktoken-rs`) vs stima; decidere in ottica MVP.
4. Redigere la bozza del prompt e il contratto JSON, allineati a SPECIFICATION.md §4.4.

## Evidence to Capture

- Estratti/URL della doc OpenRouter consultata.
- Tabella modello → supporto structured output.
- Crate/tokenizer scelto e motivazione.
- Bozza prompt + esempio di risposta JSON valida.

## Out of Scope

- Implementazione del client Rust (sarà un ticket di build).
- Scelta del modello di default (→ ticket 03 grilling).

---

_Completato 2026-07-13. Findings: [../../specs/research-openrouter-contract.md](../../specs/research-openrouter-contract.md)._
