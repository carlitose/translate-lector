## Parent Spec

[unit-truncation-diagnosis.md](../../specs/unit-truncation-diagnosis.md)
(strategia: [small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md), STC-06)

## What to Build

Tarare la soglia di separazione dei paragrafi in `pdfExtract.linesToText` così che due paragrafi
distinti con un y-gap **piccolo** (poco sopra la spaziatura di riga tipica) non finiscano fusi in
un'unica unità troppo grande. È la voce **secondaria #4** della diagnosi del troncamento: unità più
piccole = meno pressione sul budget di output del modello locale, quindi meno probabilità che una
singola chiamata venga troncata. È indipendente dal ticket 11 (che rileva/recupera i troncamenti);
questo li **previene alla fonte** riducendo la dimensione media delle unità.

Il separatore oggi è: `paragraphBreak = typical > 0 && gap > typical * paragraphFactor` con
`paragraphFactor = 1.5`. Il problema osservato è che paragrafi con interlinea leggermente maggiore
(ma < 1.5× la mediana) non vengono spezzati. Va reso più sensibile senza introdurre falsi positivi
(righe wrappate dentro lo stesso paragrafo NON devono diventare paragrafi separati).

## Acceptance Criteria

- [ ] Un y-gap moderatamente più grande della spaziatura tipica (es. ~1.2–1.3×) viene riconosciuto
      come separatore di paragrafo, mentre il normale gap riga-a-riga (~1.0×) resta un singolo `\n`.
- [ ] Nessun falso positivo su righe wrappate dello stesso paragrafo (gap ≈ mediana → nessun `\n\n`).
- [ ] Il confine di colonna (gap negativo, salto in cima alla colonna destra) continua a NON essere
      trattato come paragrafo (comportamento invariato).
- [ ] La scelta della soglia è motivata nel codice (commento) e nei test, non un numero magico nudo.
- [ ] Regressione verde: i test esistenti di `pdfExtract` restano verdi (o aggiornati con motivazione
      esplicita se la nuova soglia cambia un output atteso legittimamente).
- [ ] `npm run check` e `npx vitest run` verdi.

## Blocked By

- None - può iniziare subito (indipendente dal ticket 11; STC-06/reconstruction già in `done/`).

## Frontier

Ready now. Miglioramento di qualità secondario: riduce le unità sovradimensionate che alimentano il
troncamento. Non blocca né è bloccato dal ticket 11.

## Step-by-Step Implementation Plan

1. `src/lib/pdfExtract.test.ts`: RED — aggiungere un caso con tre righe dove il gap fra la 2ª e la 3ª
   è ~1.25× la mediana (paragrafo nuovo che oggi verrebbe fuso) e verificare che l'output contenga il
   separatore `\n\n` nel punto giusto; aggiungere/mantenere un caso di controllo dove righe con gap ≈
   mediana restano un unico paragrafo (nessun `\n\n`). Riutilizzare i fixture `Line[]` esistenti.
2. `src/lib/pdfExtract.ts` (`linesToText`): GREEN — abbassare/regolare `paragraphFactor` (es. 1.3) o,
   se serve robustezza, usare una soglia derivata (es. mediana + una frazione della dispersione dei
   gap) purché resti semplice e commentata. Verificare che i nuovi test passino e i vecchi restino
   verdi. Non toccare `computeTypicalSpacing`/gestione gap negativi.
3. Rifinitura: aggiornare il commento sopra `paragraphFactor` per spiegare la nuova soglia e perché.
   Ri-eseguire `npx vitest run` e `npm run check`.
4. Verifica end-to-end manuale (server locale, opzionale ma consigliata): la pagina 39 del libro
   produce più unità-paragrafo più piccole; combinata col ticket 11, nessun troncamento.

Pitfall: non rendere la soglia così bassa da spezzare le righe wrappate dello stesso paragrafo (falsi
positivi che frammentano il testo e moltiplicano le chiamate). Mantenere invariata l'esclusione dei
gap negativi (confine di colonna). La `typical` è una mediana robusta: non sostituirla con una media.

## Testing Plan

- Unit (vitest, `pdfExtract.test.ts`): paragrafo con gap ~1.25× mediana → `\n\n`; righe con gap ≈
  mediana → nessun `\n\n`; confine di colonna (gap negativo) → nessun `\n\n`. Regressione sui test
  esistenti di estrazione/ricostruzione.
- Manuale (opzionale): confronto del numero di unità prodotte su una pagina reale prima/dopo.

## Out of Scope

- Il rilevamento/retry dei troncamenti (ticket 11).
- Filtro di footer/numeri di pagina come "non traducibili" (possibile follow-up separato).
- Rilevamento paragrafi basato su indentazione/rientro prima riga o font-size (miglioria futura).
