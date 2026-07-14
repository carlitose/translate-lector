## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)
(design di dettaglio: [design-provider-abstraction.md](../../specs/design-provider-abstraction.md) slice 5;
verdetto contratto: [ticket 03 done](./done/03-prototype-perceptor-contract-local.md))

## What to Build

Validare l'intero percorso applicativo con un **server locale reale** e affinare i default dei preset in
base ai run veri: selezione del provider locale in ⚙️ → apertura di un PDF con testo → traduzione di una
pagina via `localhost` → traduzione mostrata affiancata → cache. È la verifica end-to-end che chiude
l'epica del provider locale.

## Acceptance Criteria

- [ ] Con Unsloth Studio (o altro server OpenAI-compatible) in esecuzione, si seleziona il provider locale
      nelle impostazioni, si imposta base-URL/chiave, e si traduce almeno una pagina reale end-to-end.
- [ ] La traduzione appare nel pannello destro; la seconda visita della stessa pagina usa la **cache**
      (nessuna nuova chiamata).
- [ ] La ladder `json_schema`→fallback si comporta come atteso sul modello reale (D6); si annota se il
      modello popola summary/glossario (best-effort su modelli piccoli, cfr. Ticket 03).
- [ ] Con server spento → messaggio d'errore chiaro, nessun fallback cloud (verifica del Ticket 09).
- [ ] I **default dei preset** (base-URL, placeholder modello, chiave dummy) sono affinati sui run reali e
      aggiornati in `providerConfig.ts`/`settings.rs` se necessario.
- [ ] Note di verifica (latenza osservata, qualità, eventuali aggiustamenti) registrate nel parent spec
      (§Next Review) e/o in un breve report.

## Blocked By

- [08-settings-ui-provider-selector.md](./08-settings-ui-provider-selector.md)
- [09-reachability-healthcheck-and-onboarding.md](./09-reachability-healthcheck-and-onboarding.md)

## Frontier

**HITL** — richiede un server locale in esecuzione (l'utente ha Unsloth Studio su `localhost:8888`). È la
verifica finale integrata; dipende da UI (08) e health-check (09) per esercitare il flusso completo.
L'harness `prototypes/local-llm/validate-perceptor-contract.mjs` ha già validato il *contratto* (Ticket 03);
qui si valida l'*app reale*.

## Step-by-Step Implementation Plan

1. **Prepara l'ambiente**: avvia Unsloth Studio con un modello Q4 (l'utente); annota base-URL/porta e chiave.
   Verifica: `/v1/models` risponde.
2. **Configura in-app**: apri ⚙️, seleziona il provider locale, imposta base-URL/chiave/modello, salva.
   Verifica: i valori persistono (Ticket 08).
3. **Traduci una pagina reale**: apri un PDF con testo, vai su una pagina, attendi la traduzione. Perché ora:
   esercita client→settings→keychain→endpoint locale→percettore→UI. Verifica: traduzione mostrata a destra.
4. **Verifica cache**: torna sulla pagina → nessuna nuova chiamata, traduzione istantanea.
5. **Verifica errore**: spegni il server, prova a tradurre una pagina non in cache → messaggio chiaro, nessun
   costo cloud (Ticket 09).
6. **Tuning default**: se base-URL/modello/chiave-dummy dei preset non combaciano con la realtà, aggiornali.
   Verifica: un nuovo profilo/utente parte con default sensati.
7. **Registra le note** (latenza, qualità, aggiustamenti) nel parent spec §Next Review.

Pitfall: la latenza locale può essere alta (Ticket 03: ~40s/pagina sul Q4) — usare prefetch e non
interpretare la lentezza come blocco. Non reintrodurre fallback cloud.

## Testing Plan

- Manuale/e2e (HITL): i passi sopra contro un server reale.
- Automazione dove possibile: eventuale test d'integrazione che, dato un endpoint locale disponibile, esegue
  una traduzione; altrimenti documentare i passi manuali.
- Regressione: la traduzione via OpenRouter continua a funzionare selezionando quel provider.

## Out of Scope

- Nuove funzionalità di traduzione o percettore (si riusa l'esistente).
- Epica OCR (mappa separata).
- Avvio/gestione del server locale dall'app (D7, post-MVP).
