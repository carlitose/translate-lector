# 01 — Decisioni di prodotto per l'aggiunta manuale di termini

## Parent Spec

[manual-glossary-entry-wayfinder.md](../../specs/manual-glossary-entry-wayfinder.md)

## Type

grilling

## Outcome

Le 5 decisioni di prodotto che determinano la forma del backend e della UI sono fissate
(risposte dell'utente) o confermate con le assunzioni raccomandate del map. Registrate nel
map sotto "Not Yet Specified" → decisioni prese.

## Domande da porre (con assunzione raccomandata)

1. **Lucchetto di default** per un termine aggiunto a mano — locked ON o OFF?
   (Raccomandato: **ON** — vincolo assoluto; la checkbox resta modificabile nel form.)
2. **Duplicato** (`source_term` già presente, case-insensitive) — rifiutare con messaggio,
   oppure aprire in modifica la riga esistente, oppure sovrascrivere?
   (Raccomandato: **rifiutare con messaggio**, niente upsert silenzioso.)
3. **Colonna "Pag."** per un termine senza pagina d'origine — mostrare "manuale", "—" o un
   numero (pagina corrente)? (Raccomandato: **"manuale"**, valore memorizzato
   `first_seen_page = 0`.)
4. **Tipo** — campo di testo libero o menù a valori fissi?
   (Raccomandato: **testo libero** con datalist di suggerimenti: comune / tecnico / nome
   proprio.)
5. **Validazione** — quali campi obbligatori? (Raccomandato: `source_term` **e**
   `translation` non vuoti; `type`/`note` opzionali.)

## Acceptance Criteria

- [x] Le 5 decisioni sono registrate nel map (risposte dell'utente, 2026-07-17 — sezione
      "Decisioni di prodotto (fissate)").
- [x] Ogni decisione che diverge dall'assunzione raccomandata è annotata così i ticket 02/03
      la recepiscono (decisione #2: duplicato ⇒ aprire in modifica, non rifiutare — annotata
      nel map con impatto su 02/03).
- [x] L'incertezza residua è esplicita (nessuna domanda lasciata aperta: tutte e 5 risolte
      con scelta dell'utente).

## Decisioni prese (2026-07-17)

1. Lucchetto default = **ON** (checkbox modificabile).
2. Duplicato ⇒ **aprire in modifica** la riga esistente (⚠️ diverge dall'assunzione
   "rifiutare"; impatta contratto 02/03).
3. Colonna "Pag." = **"manuale"** (`first_seen_page = 0`).
4. Tipo = **testo libero + datalist** (comune / tecnico / nome proprio).
5. Validazione = **`source_term` + `translation`** obbligatori (trim); `type`/`note`
   opzionali.

## Blocked By

- None - can start immediately.

## Frontier

È l'edge bloccante: la logica di duplicato, il default del lucchetto e la semantica di
`first_seen_page` cambiano l'implementazione di 02 e 03. In modalità AFK, adottare le
assunzioni raccomandate e procedere.

## Work Plan

1. Porre le 5 domande all'utente (o adottare le assunzioni in AFK).
2. Scrivere le decisioni nel map.
3. Sbloccare 02 e 03.

## Evidence to Capture

- Risposte dell'utente (o nota "assunzioni raccomandate adottate in AFK").

## Out of Scope

- Qualsiasi implementazione: qui si decide soltanto.
