## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)

## Type

prototype

## Outcome

Prova che una pagina reale può essere divisa in **unità piccole a livello di paragrafo** (entro il budget
del Ticket 01) traducibili singolarmente e **riassemblabili** senza perdere testo né ordine.

## Acceptance Criteria

- [ ] Funzione di splitting che produce unità paragrafo (o frase quando un paragrafo eccede il budget),
      riusando/estendendo `split_into_chunks` (`src-tauri/src/translate.rs:72`) e la ricostruzione di
      `src/lib/pdfExtract.ts`; ogni unità sta entro `budget_input` (Ticket 01).
- [ ] Concatenando le unità si riottiene il testo originale (nessuna perdita); l'ordine è preservato.
- [ ] Misurate le dimensioni reali delle unità su ≥2 pagine reali (incl. una del PDF "Build a LLM…") in
      token stimati.
- [ ] Gestione dei casi limite: paragrafo lunghissimo (split a frase), righe/liste, sillabazione già gestita
      da pdfExtract.
- [ ] Prototipo/nota registrata; verdetto sulla granularità (paragrafo vs frase) come input al Ticket 05.

## Blocked By

- None - can start immediately (usa il budget del Ticket 01 come parametro; può prototipare con un budget
  ipotetico e affinare dopo).

## Frontier

Ready. Prova la fattibilità del "traduci un paragrafetto" mantenendo integrità e ordine.

## Work Plan

1. Rivedere la ricostruzione testo (`pdfExtract.ts`) e `split_into_chunks`.
2. Implementare/estendere lo split a paragrafo entro un budget di token; frase come fallback.
3. Verificare round-trip (concat = originale) e misurare le dimensioni su pagine reali.
4. Annotare granularità raccomandata e casi limite.

## Evidence to Capture

- Distribuzione dimensioni unità (token) su pagine reali; esempi di split; prova del round-trip.

## Out of Scope

- Selezione glossario (Ticket 03); orchestrazione chiamate/percettore (Ticket 04).
