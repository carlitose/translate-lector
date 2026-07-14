## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)

## What to Build

Rendere **provider-neutrale** il messaggio d'errore EC03 (API key mancante/invalida) mostrato nel
frontend. Oggi la costante `MISSING_KEY_HINT` dice hardcoded *"API key OpenRouter mancante o non valida…"*
e viene mostrata per **qualsiasi** provider: con Unsloth selezionato l'utente vede comunque "OpenRouter",
il che confonde (sembra che l'app ignori il provider locale). Finding emerso durante il Ticket 10 (HITL).

Il core Rust emette già un messaggio neutrale (Ticket 05: `"EC03: API key mancante o non valida…"`); è la
stringa **frontend** rimasta indietro, perché `translationErrorMessage` sostituisce l'errore del core con
`MISSING_KEY_HINT`.

Nota di contesto (non è un bug di logica): l'EC03 è corretto quando manca la chiave del provider attivo —
il keychain è per-provider (Ticket 06), quindi la chiave va inserita col provider giusto selezionato. Qui
si corregge solo il **testo**.

## Acceptance Criteria

- [ ] `MISSING_KEY_HINT` (`src/lib/translation.ts`) non menziona più "OpenRouter"; testo neutrale e
      azionabile, es.: *"API key mancante o non valida per il provider selezionato. Configurala in ⚙️ (in
      alto a destra)."*
- [ ] `translationErrorMessage` continua a restituire questa costante per gli errori EC03 (nessun cambio di
      logica).
- [ ] I test frontend restano verdi: `translation.test.ts` confronta con la costante `MISSING_KEY_HINT`
      (non col testo letterale), quindi passa senza modifiche. (Opzionale: aggiornare la stringa di *esempio*
      alla riga ~20 che contiene "OpenRouter" come input di test, solo per coerenza.)
- [ ] `npm run check` pulito; `npx vitest run` verde.

## Blocked By

- None - can start immediately.

## Frontier

Ready now. Indipendente dal Ticket 12. Nessuna dipendenza dal core (solo frontend).

## Step-by-Step Implementation Plan

1. In `src/lib/translation.ts`, cambiare il testo della costante `MISSING_KEY_HINT` (righe ~41-42) a una
   versione provider-neutrale e azionabile. Perché: è l'unica fonte del messaggio EC03 lato UI.
   Verifica: leggere il valore e confermare che non contenga "OpenRouter".
2. Eseguire `npx vitest run` — il test che confronta `translationErrorMessage('EC03: …')` con
   `MISSING_KEY_HINT` (riga ~47) resta verde perché confronta la costante. Verifica: suite verde.
3. (Opzionale) aggiornare la stringa d'esempio con "OpenRouter" nel test di rilevamento EC03 (riga ~20) per
   coerenza; non cambia il comportamento.
4. `npm run check` per il typecheck. Verifica: 0 errori.

Pitfall: non toccare la logica di `translationErrorMessage`/`isMissingKeyError`; cambia solo la stringa.

## Testing Plan

- `npx vitest run`: i test di `translation.ts` restano verdi.
- `npm run check`: pulito.
- Manuale (parte del Ticket 10): con provider locale senza chiave, un errore di traduzione mostra il nuovo
  testo neutrale (non "OpenRouter").

## Out of Scope

- Bottone di retry (Ticket 12).
- Qualsiasi cambiamento al core Rust o alla risoluzione del provider (già corretti).
- Includere il nome/label del provider attivo nel messaggio (possibile miglioramento futuro; richiederebbe
  di passare il provider a `translation.ts`).
