# 03 — Rendering della ricostruzione tipografica (re-typeset pagina)

## Parent Spec

[ocr-layout-translation-wayfinder.md](../../specs/ocr-layout-translation-wayfinder.md)

## Type

prototype

## Outcome

Una prova disposable che, **dati box di layout + testo tradotto**, re-impagina una pagina credibile:
blocchi tradotti posizionati come l'originale, figure/immagini conservate, e una strategia funzionante
per l'**overflow** (la traduzione è spesso più lunga dell'originale). Dimostra la fattibilità del
rendering della "copia perfetta tradotta".

## Acceptance Criteria

- [ ] Prototipo (in `prototypes/ocr/` o come pagina Svelte isolata) che prende l'output del Ticket 02 per
      una pagina e produce un re-typeset visivo con testo tradotto finto/hardcoded.
- [ ] Blocchi di testo posizionati per bounding box (HTML/CSS assoluto o canvas/SVG) sopra le regioni-immagine
      conservate della pagina originale.
- [ ] Strategia di **overflow** scelta e dimostrata: shrink-to-fit del font, crescita del box, o clamp con
      indicatore; nota sui trade-off.
- [ ] Strategia di **font**: sostituzione/fallback quando il font originale è ignoto; leggibilità accettabile.
- [ ] Conservazione delle **figure**: le regioni non-testo restano immagine originale, solo i blocchi di
      testo sono sostituiti.
- [ ] Confronto affiancato originale vs ricostruzione per almeno 2 pagine (screenshot nel parent spec/ticket).

## Blocked By

- Ticket 02 (serve sapere quali box/struttura sono realmente disponibili).

## Frontier

È l'ultima grande incognita di fattibilità del traguardo "ricostruzione tipografica". Se il rendering
credibile è troppo costoso, la mappa ripiega su uno stadio intermedio (overlay-su-immagine) e la
destinazione va rinegoziata con l'utente (collegato al Ticket 04).

## Work Plan

1. Definire un modello dati intermedio "pagina ricostruibile" (blocchi con box, testo, font-size stimato,
   regioni-immagine) — bozza che alimenterà il design del Ticket 05.
2. Renderizzare i blocchi con testo tradotto finto, posizionati per box, sopra l'immagine di pagina.
3. Implementare e confrontare almeno una strategia di overflow; documentare il comportamento.
4. Gestire una pagina con figura per provare la conservazione delle regioni-immagine.
5. Catturare confronti affiancati e annotare il "pavimento di fedeltà" raggiungibile (input al Ticket 04).

## Evidence to Capture

- Screenshot originale vs ricostruzione (2+ pagine).
- Nota sulla strategia di overflow e font, con trade-off.
- Bozza del modello dati "pagina ricostruibile" (per il Ticket 05).
- Elenco onesto dei limiti visivi residui.

## Out of Scope

- Chiamata reale di traduzione LLM (testo finto va bene qui).
- Persistenza/cache (Ticket 05 + build).
- Export in PDF (dipende dalla decisione del Ticket 04).
