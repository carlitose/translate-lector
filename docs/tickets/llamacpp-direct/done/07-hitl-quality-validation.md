# 07 — Task (HITL): validazione qualità della traduzione senza reasoning

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Status

**Done** (2026-07-15) — esito: **accettabile, qualità buona**. L'utente ha letto pagine reali col
provider `llamaserver` + `--reasoning off` e ha giudicato la traduzione "molto buona". Nessuna
regressione bloccante rispetto alla resa con CoT via Studio. → sblocca D5 del grilling (il provider
diretto può diventare il default consigliato). Lo scivolone sintetico sugli articoli
("I scienziati") non si è rivelato un problema pratico sulle pagine reali; resta come nota di
sorveglianza leggera.

## Type

task (HITL — richiede lettura umana)

## Outcome

Giudizio umano registrato: la qualità della traduzione di gemma-4 **senza** CoT (`--reasoning off`)
è accettabile per l'uso reale? Decide se il provider diretto può diventare il default consigliato
(input per D5 del grilling).

## Acceptance Criteria

- [ ] Almeno 3-5 pagine reali (PDF già in libreria, inclusa una pagina densa) tradotte con provider
      `llamaserver` + `--reasoning off` e lette dall'utente.
- [ ] Confronto qualitativo con la stessa pagina via Studio (con CoT), se il confronto è pratico.
- [ ] Esito registrato nel ticket: accettabile / accettabile con riserve / non accettabile, con
      esempi concreti (il test sintetico ha mostrato un "I scienziati" → sorvegliare gli articoli).

## Blocked By

- None — can start immediately: il server è avviabile a mano oggi
  (comando registrato nella mappa e in questo ticket, vedi sotto).

## Frontier

Se la qualità senza CoT non regge, la destinazione va ridimensionata (es. `--reasoning-budget N`
invece di `off`): meglio saperlo prima della build 04-06.

## Work Plan

1. Avviare llama-server direttamente (build Unsloth per ora):

   ```powershell
   $env:PATH = "C:\Users\CGS03\.unsloth\studio\unsloth_studio\Lib\site-packages\torch\lib;$env:PATH"
   & "$env:USERPROFILE\.unsloth\llama.cpp\build\bin\Release\llama-server.exe" `
     -m "$env:USERPROFILE\.cache\huggingface\hub\models--unsloth--gemma-4-E2B-it-qat-GGUF\snapshots\2ea637031baa8dc847d64b5dbb7011fd6a445849\gemma-4-E2B-it-qat-UD-Q4_K_XL.gguf" `
     --port 8080 -ngl 99 -c 4096 --reasoning off
   ```

2. Nell'app: ⚙️ → provider `llama.cpp server (locale)`.
3. Tradurre e leggere le pagine campione; annotare errori (articoli, glossario, struttura).
4. Registrare l'esito nel ticket e nella mappa.

## Evidence to Capture

- Pagine usate, tempi percepiti, esempi di errori/qualità.

## Out of Scope

- Automazione della valutazione qualità.
- Tuning di sampling/prompt (eventuale follow-up).
