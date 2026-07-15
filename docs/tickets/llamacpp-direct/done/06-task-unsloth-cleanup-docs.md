# 06 — Task: destino del preset unsloth + documentazione

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Type

task

## Decisioni dal grilling 03

Vedi [decision-brief-llamacpp-direct-03.md](../../specs/decision-brief-llamacpp-direct-03.md).
Rilevanti qui: **D3** (`unsloth` resta opzione normale, NON deprecato, niente migrazione forzata),
**D5** (`llamaserver` diventa il default selezionato).

## Outcome

Il preset `unsloth` resta un'opzione normale e di prima classe; `llamaserver` (llama.cpp diretto)
diventa il **default** selezionato su installazione pulita. La documentazione (README + specs)
descrive il setup del provider diretto come si usa davvero.

## Acceptance Criteria

- [ ] Preset `unsloth` **invariato e selezionabile** (D3): nessuna etichetta "deprecato", nessuna
      migrazione/rimozione degli override `provider.unsloth.*`.
- [ ] **Default provider = `llamaserver`** (D5): su prima esecuzione (nessuna scelta persistita)
      l'app parte col provider llama.cpp diretto; il test dei preset in `settings.rs` riflette il
      nuovo default.
- [ ] README/docs aggiornati: setup del provider locale diretto (dir del binario ufficiale, path del
      modello, requisiti GPU), troubleshooting essenziale; nota che Unsloth resta un'alternativa.
- [ ] La nota "riaprire L6" in `local-translation-latency-wayfinder.md` chiusa con rimando a questa
      mappa (già fatto in parte; verificare coerenza finale).
- [ ] Suite verde (test sui preset/default in `settings.rs` aggiornati).

## Blocked By

- [03-grilling-llamacpp-direct-decisions.md](./done/03-grilling-llamacpp-direct-decisions.md) →
  **done**. Sbloccato (indipendente da 04/05; può anche partire da solo per la parte docs/default).

## Frontier

Ultimo miglio: senza il default aggiornato e le docs, l'app ha due provider locali senza una guida e
il setup resta tribale.

## Work Plan

1. Impostare `llamaserver` come default provider (D5) e aggiornare i test dei preset.
2. Verificare che `unsloth` resti intatto (D3) — nessun cambiamento se non l'ordine/il default.
3. README: sezione setup provider diretto + troubleshooting.
4. Chiudere la nota L6 nella mappa latenza.

## Evidence to Capture

- Diff dei preset/test; sezione README risultante.

## Out of Scope

- Disinstallazione di Unsloth Studio dalla macchina (scelta dell'utente, fuori dall'app).
