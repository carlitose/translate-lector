## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)

## Type

grilling

## Outcome

Le decisioni umane che il design non può indovinare, registrate come risposte/assunzioni esplicite.

## Acceptance Criteria

- [ ] **Granularità unità**: paragrafo (default proposto), frase, o finestra a N-token? (informato dai
      Ticket 02/01).
- [ ] **Default vs condizionale**: la strategia budget-aware si attiva **sempre**, o **solo per provider a
      contesto piccolo** (locale) mentre il cloud tiene il percorso pagina-intera?
- [ ] **Latenza**: accettabile fare N chiamate piccole per pagina su modello locale lento? Soglia oltre la
      quale preferire meno chiamate/unità più grandi.
- [ ] **Match glossario**: severità e cap (Ticket 03) — quanto aggressivo il match, quanti unlocked al
      massimo per unità, priorità locked.
- [ ] **Split contratto**: separare "translate-only" (per unità) da "perceptor-update" (per pagina)? (Ticket 04).
- [ ] **Aggiornamento summary/glossario**: una volta per pagina (proposto) o incrementale per unità?
- [ ] Ogni punto ha una decisione o assunzione esplicita registrata nella mappa.

## Blocked By

- None per raccogliere le domande; risposte più informate **dopo** i prototipi 02/03 e il design 04.

## Frontier

Gate umano prima delle build: default-vs-condizionale, granularità e split contratto cambiano molto lo scope.

## Work Plan

1. Preparare un decision brief con le domande sopra (stile `decision-brief-*`).
2. Portare come evidenza i numeri dei Ticket 01/02/03 e il design 04.
3. Condurre il grilling; registrare risposte/assunzioni nella mappa.

## Evidence to Capture

- Decision brief con risposte D-numerate; assunzioni marcate.

## Out of Scope

- Implementazione. Solo decisioni.
