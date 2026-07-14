# 05 — Cap del summary nelle chiamate translate-only

## Status

**Chiuso senza implementazione** (decisione L2, [decision-brief-latency-03.md](../../specs/decision-brief-latency-03.md), 2026-07-14).

## Parent Spec

[local-translation-latency-wayfinder.md](../../specs/local-translation-latency-wayfinder.md)

## Type

task

## Motivo di chiusura

Le misure del ticket 01 hanno mostrato che il server locale fa **prefix caching**:
`usage.prompt_tokens_details.cached_tokens ≈ 1078/1133` dalla seconda chiamata della stessa pagina in
poi. Il prefisso fisso (system + summary + glossario) costa quindi **quasi zero in latenza** una volta
impacchettata la pagina in poche finestre (ticket 02/04) — l'assunzione originaria "il prefill fisso
domina" (C3 della diagnosi) è smentita. Non implementare: nessun guadagno misurabile a fronte del rischio
di troncare il summary in un punto che degrada la coerenza terminologica del perceptor.

## Outcome (originario, non perseguito)

Le chiamate translate-only inviano una versione **cappata** del rolling summary (valore deciso in L2,
es. ~300 token) invece dell'intero summary fino a 1000 token: il prefill fisso per chiamata cala di
~700 token e `budget_unit_text` cresce di altrettanto (finestre più capienti = ancora meno chiamate).

## Acceptance Criteria

- [ ] ~~Il summary passato alle chiamate translate-only (`translate.rs:640, 706`) è troncato/compresso al
      cap deciso in L2~~ — non perseguito, vedi Motivo di chiusura.
- [ ] ~~Il calcolo del budget usa la stima del summary cappato~~ — non perseguito.
- [ ] ~~Test Rust: budget con summary cappato~~ — non perseguito.

## Blocked By

- N/A — chiuso.

## Frontier

Secondo moltiplicatore di C3 dopo il packing: il summary è la voce più grossa del prefill fisso
(fino a 1000 dei ~1300 token). Indipendente dal Ticket 04, componibile con esso.

## Work Plan

1. Decidere il punto di taglio: al momento della costruzione del prompt per unità
   (`build_translate_only_*` in `translate.rs`).
2. Implementare il cap (troncamento a confine di frase o compressione), aggiornare la stima nel calcolo
   del budget.
3. Test + prova manuale su pagine consecutive.

## Evidence to Capture

- Diff, output test, token del prompt per unità prima/dopo (via `est_tokens`).

## Out of Scope

- Cambiare il limite del rolling summary del perceptor (`DEFAULT_SUMMARY_TOKEN_LIMIT`) o la compressione
  EC05.
- Packing (Ticket 04).
