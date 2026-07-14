## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)

## What to Build

Far sì che la ricostruzione del testo di pagina esponga i **confini di paragrafo**, così a valle
`split_into_units` (STC-02) possa produrre vere unità-paragrafo. Oggi `linesToText` in
`src/lib/pdfExtract.ts` unisce le righe con un singolo `\n` e non emette righe vuote → una pagina
ricostruita è di fatto un **unico paragrafo** (finding STC-02). Introdurre un separatore di paragrafo
(riga vuota) quando il salto verticale (y-gap) tra righe consecutive supera una soglia.

## Acceptance Criteria

- [ ] La ricostruzione inserisce un separatore di paragrafo (es. doppio `\n`) tra righe con y-gap
      significativo (soglia relativa all'altezza di riga), preservando invece i normali a-capo di wrapping.
- [ ] La de-sillabazione e l'ordine di lettura (colonne) esistenti restano invariati.
- [ ] Concatenando i paragrafi si riottiene il contenuto testuale (nessuna perdita di parole).
- [ ] Test unitari su input con y-gap (nuovo paragrafo) vs righe ravvicinate (stesso paragrafo).
- [ ] `npm run check` pulito; `npx vitest run` verde (inclusi i test esistenti di `pdfExtract`).

## Blocked By

- None - can start immediately.

## Frontier

Ready. Prerequisito perché la pipeline (Ticket 08) riceva paragrafi veri; indipendente da `n_ctx` (07).

## Step-by-Step Implementation Plan

1. Rivedere `reconstructLines`/`linesToText` in `src/lib/pdfExtract.ts`: le righe hanno già la `y` (origine
   bottom-left). Perché prima: la logica di paragrafo si basa sul gap tra `y` consecutive.
2. Calcolare una soglia di y-gap relativa all'altezza di riga (es. gap > 1.5× interlinea tipica) e, quando
   superata tra due righe, emettere un separatore di paragrafo in `linesToText`. Verifica: righe ravvicinate
   restano stesso paragrafo; salto ampio crea nuovo paragrafo.
3. Assicurarsi che la de-sillabazione a fine riga e la logica colonne non regrediscano. Verifica: i test
   esistenti restano verdi.
4. Aggiungere test con fixture di righe (y crescenti/decrescenti) che coprono nuovo-paragrafo e
   stesso-paragrafo. Verifica: round-trip contenuto.

Pitfall: non confondere il gap di paragrafo con l'interlinea normale (tarare la soglia); non rompere il
re-flow di riga esistente.

## Testing Plan

- Unit (TS/vitest): y-gap → separatore; righe fitte → nessun separatore; nessuna perdita di testo; test
  `pdfExtract` esistenti verdi.
- Manuale (opzionale, in Ticket 08): su un PDF reale i paragrafi emergono come unità.

## Out of Scope

- Chunking/traduzione (Ticket 08); `n_ctx`/budget (Ticket 07).
