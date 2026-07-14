# 04 — Decisioni: modello/quant, hardware, default vs opt-in, offline

## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)

## Type

grilling

## Outcome

Le decisioni umane che il design del provider locale non può indovinare, registrate come risposte o
assunzioni esplicite nel parent spec.

## Acceptance Criteria

- [ ] **Modello/quantizzazione target**: quale modello locale (famiglia/taglia) e quale quantizzazione si
      vuole usare per la traduzione? (informato dal Ticket 03).
- [ ] **Hardware**: GPU/RAM disponibili? Vincola la taglia del modello e la latenza accettabile.
- [ ] **Default vs opt-in**: il provider locale è il default all'avvio o si sceglie esplicitamente? Cosa
      succede se il server locale non è raggiungibile (fallback a OpenRouter? errore?).
- [ ] **Offline**: quali funzioni devono restare 100% offline (traduzione? tutto?) e quali possono restare cloud.
- [ ] **Ciclo di vita del server**: l'app assume che l'utente avvii Unsloth/endpoint a mano (scelta MVP), o
      si desidera avvio/health-check dall'app? (conferma dello scope "Out of Scope").
- [ ] Ogni punto ha una decisione o un'assunzione esplicita registrata nel parent spec.

## Blocked By

- None per raccogliere le domande; le risposte su modello/hardware sono più informate **dopo** il Ticket 03.

## Frontier

Gate umano prima delle build verticali: default vs opt-in, fallback e offline cambiano il comportamento
runtime e la UI del selettore provider.

## Work Plan

1. Preparare un decision brief con le domande sopra (stile `decision-brief-grilling-03.md`).
2. Portare come evidenza il verdetto qualità/latenza del Ticket 03.
3. Condurre il grilling; registrare risposte o assunzioni.
4. Ripiegare le decisioni in "Decisions So Far" del parent spec.

## Evidence to Capture

- Decision brief con risposte D-numerate.
- Assunzioni prese in assenza di risposta, marcate come tali.

## Out of Scope

- Implementazione. Solo decisioni.
