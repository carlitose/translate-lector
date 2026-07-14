## Parent Spec

[local-llm-empty-content-diagnosis.md](../../specs/local-llm-empty-content-diagnosis.md)

## Type

task

## What to Build

Gestire in modo dedicato e **azionabile** il caso "risposta con `finish_reason: length` e `content`
vuoto/null" (tipico di un modello reasoning che esaurisce il budget di token), invece del generico errore
fatale `LlmError::Http("risposta senza contenuto testuale…")` (`src-tauri/src/llm.rs:159-170`).

## Acceptance Criteria

- [ ] Quando la risposta ha `content` vuoto/null **e** `finish_reason == "length"` (o è presente
      `reasoning_content`), l'errore/messaggio è specifico e azionabile, es.: *"Il modello locale ha
      esaurito il budget di token (probabile reasoning entro una finestra piccola). Usa un modello
      non-reasoning, riduci il testo, o aumenta il context del server."*
- [ ] Valutato/implementato un **retry automatico** una tantum con reasoning disattivato e/o `max_tokens`
      ridotto prima di arrendersi (senza loop infiniti; coerente con la ladder esistente).
- [ ] La distinzione non rompe il caso content-null legittimo dei modelli reasoning già gestito in
      deserializzazione (test "bug #2", `llm.rs:1210-1214`).
- [ ] Il messaggio arriva al frontend con un marcatore utile (come EC02/EC03) se serve un hint dedicato;
      `translationErrorMessage` (`src/lib/translation.ts`) aggiornato se necessario.
- [ ] `cargo test` verde (nuovo test: finish_reason=length + content vuoto → errore/retry dedicato);
      `npm run check`/`vitest` verdi se tocca la UI.

## Blocked By

- None (idealmente dopo il Ticket 01 per la forma esatta della risposta).

## Frontier

Ready. Rete di sicurezza: anche col fix di headroom (Ticket 02), un modello reasoning può ancora esaurire il
budget su pagine grandi → serve un messaggio chiaro e/o un retry mirato.

## Work Plan

1. Estendere il parsing della risposta per leggere `finish_reason` (e presenza di reasoning) accanto a
   `content` in `ChatResponse::content()` / chiamante.
2. Se `content` vuoto & `finish_reason==length` → variante d'errore dedicata con messaggio azionabile; opz.
   un retry con reasoning off / max_tokens ridotto.
3. Aggiornare `LlmError`/`user_message` e, se serve, `translation.ts`.
4. Test: risposta length+empty → esito dedicato; content-null reasoning legittimo resta gestito.

## Testing Plan

- Unit (Rust): finish_reason=length + content vuoto → variante/messaggio dedicati; nessuna regressione sul
  caso reasoning legittimo.
- Manuale: pagina grande su modello reasoning → messaggio chiaro (o traduzione dopo retry), non l'errore
  generico.

## Out of Scope

- Ridurre a monte la probabilità del problema (Ticket 02).
- Chunking del testo pagina (possibile follow-up separato).
