# 02 — Research: contratto sidecar Tauri 2

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Type

research

## Outcome

Il contratto tecnico per gestire llama-server dall'app Tauri 2: come si impacchetta
(`externalBin`/risorse), come si fa spawn/kill dal backend Rust, come si passano env e argomenti,
come si gestiscono porta occupata, crash e chiusura dell'app. Con citazioni della documentazione
ufficiale corrente (via ctx7).

## Acceptance Criteria

- [ ] Documentate le opzioni Tauri 2 per binari esterni: `bundle.externalBin` (sidecar) vs
      `bundle.resources` vs binario in app-data scaricato al primo avvio — con vincoli di ognuna
      (naming per target-triple, firma, dimensioni installer).
- [ ] Documentato il lifecycle: spawn dal backend Rust (plugin shell o `std::process` diretto),
      kill garantito alla chiusura dell'app (anche crash), riavvio su exit inatteso.
- [ ] Documentata la strategia porta: porta fissa 8080 (= preset `llamaserver`) vs porta dinamica +
      override `provider.llamaserver.base_url`; interazione col probe di reachability esistente
      (1.5 s) e col caso "porta già occupata da altro processo".
- [ ] Nota su come le DLL accanto all'exe vengono risolte su Windows (nessun PATH necessario se
      sono nella stessa dir del sidecar?).
- [ ] Esito ripiegato nella mappa.

## Blocked By

- None — can start immediately (parallelo al ticket 01).

## Frontier

Il grilling 03 deve scegliere la distribuzione (bundled vs esterno gestito vs script): senza sapere
cosa Tauri 2 rende facile/difficile, la decisione sarebbe al buio.

## Work Plan

1. `npx ctx7@latest library "Tauri" "bundle external sidecar binary spawn kill lifecycle"` e
   fetch docs mirate (sidecar/shell plugin, bundle config).
2. Verificare nel repo come l'app fa oggi spawn/gestione processi (se mai) e dove vive la config
   bundle (`src-tauri/tauri.conf.json`).
3. Rispondere ai punti dei criteri di accettazione con riferimenti alle docs.
4. Stimare l'impatto sull'installer (dimensioni attuali vs + sidecar CUDA).

## Evidence to Capture

- Estratti/URL delle docs Tauri 2 per ogni claim.
- Config `tauri.conf.json` attuale rilevante.

## Out of Scope

- L'implementazione (ticket 04).
- Il download del modello GGUF (ticket 05).
