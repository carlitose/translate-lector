## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)

## What to Build

Aggiungere un bottone **"Riprova traduzione"** per la pagina corrente, così che dopo un errore (EC03,
server locale irraggiungibile, rete, ecc.) l'utente possa ri-lanciare la traduzione **senza navigare via e
tornare**. Finding emerso durante il Ticket 10 (HITL): manca un modo esplicito per ritentare.

Riusa la funzione esistente `translateCurrentPage()` in `src/routes/+page.svelte`, che non richiede
argomenti (rilegge lo stato corrente) e supera la guardia `shouldTranslate` per la pagina a schermo. Poiché
dopo un errore nulla è in cache, il retry esegue una traduzione reale (il caso d'uso desiderato).

## Acceptance Criteria

- [ ] Nel footer/bottom bar di `+page.svelte` compare un bottone (es. "↻ Riprova traduzione") quando
      `pageStatus === 'error'` (coerente col pattern esistente della status bar).
- [ ] Il click chiama `translateCurrentPage()` e ri-traduce la pagina corrente; a esito positivo lo stato
      torna `translated`/`cached` e l'errore sparisce.
- [ ] Il bottone NON appare quando non c'è errore (nessun uso improprio; il re-translate forzato di una
      pagina già in cache è fuori scope, richiederebbe un cache-bypass nel core).
- [ ] Stile coerente con gli altri bottoni (styling globale `button`, come `.glossary-btn`).
- [ ] `npm run check` pulito; `npx vitest run` verde.

## Blocked By

- None - can start immediately.

## Frontier

Ready now. Indipendente dal Ticket 11. Solo frontend; riusa `translateCurrentPage` esistente.

## Step-by-Step Implementation Plan

1. In `src/routes/+page.svelte`, nel `<footer class="bottombar">` (vicino allo `status-indicator`),
   aggiungere un blocco condizionale `{#if session && pageStatus === 'error'}` con un `<button>` che chiama
   `() => void translateCurrentPage()`. Perché lì: è dove vive lo stato della pagina (§3.1) ed è coerente
   con l'indicatore di stato.
   Verifica: con un errore simulato/reale il bottone appare; senza errore no.
2. Assicurarsi che `translateCurrentPage()` sia invocabile senza argomenti (lo è) e che la guardia
   `shouldTranslate` passi per la pagina corrente. Verifica: cliccando dopo un errore parte una traduzione
   reale (spinner → risultato).
3. Stile: riusare la classe/bottone globale; opzionale classe `retry-btn`. Verifica: aspetto coerente.
4. `npm run check` + `npx vitest run`. Verifica: typecheck pulito, test verdi.

Pitfall: non introdurre un flag di bypass cache (fuori scope). Non duplicare la logica di traduzione: riusare
`translateCurrentPage`. Attenzione a non rompere il gating della status bar esistente.

## Testing Plan

- `npm run check`: pulito.
- `npx vitest run`: verde (se si estrae un helper puro per la condizione di visibilità, aggiungere un test;
  altrimenti coperto dal typecheck).
- Manuale (parte del Ticket 10): provocare un errore (provider senza chiave o server giù) → appare "Riprova
  traduzione" → risolvere (inserire chiave / avviare server) → click → la pagina viene tradotta.

## Out of Scope

- Correzione del messaggio EC03 (Ticket 11).
- Re-traduzione forzata di una pagina già in cache (richiederebbe cache-bypass nel core).
- Retry automatico (la ladder di backoff del core gestisce già i transitori; qui è un'azione manuale).
