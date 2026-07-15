# 06 — Task: destino del preset unsloth + documentazione

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Type

task

## Outcome

Il preset `unsloth` segue la decisione D3 (tenuto/deprecato/rimosso, con migrazione di eventuali
override), e la documentazione (README + specs) descrive il provider diretto llama.cpp come si usa
davvero.

## Acceptance Criteria

- [ ] Preset `unsloth` allineato a D3; se rimosso, gli override `provider.unsloth.*` esistenti non
      producono errori (migrazione o ignorati in modo pulito).
- [ ] README/docs aggiornati: setup del provider locale diretto, requisiti (GPU, modello),
      troubleshooting essenziale.
- [ ] La nota "riaprire L6" in `local-translation-latency-wayfinder.md` chiusa con rimando a
      questa mappa.
- [ ] Suite verde (i test sui preset in `settings.rs` aggiornati a D3).

## Blocked By

- [03-grilling-llamacpp-direct-decisions.md](./03-grilling-llamacpp-direct-decisions.md)

## Frontier

Ultimo miglio: senza cleanup e docs, l'app ha due provider locali sovrapposti e il setup resta
tribale.

## Work Plan

1. Da dettagliare dopo il grilling (D3).
2. Aggiornare preset/test, README, e la mappa latenza.

## Evidence to Capture

- Diff dei preset e dei test; sezione README risultante.

## Out of Scope

- Disinstallazione di Unsloth Studio dalla macchina (scelta dell'utente, fuori dall'app).
