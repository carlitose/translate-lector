# 01 — Research: sourcing del binario llama-server

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Type

research

## Outcome

Una raccomandazione motivata sulla fonte del binario llama-server per il provider diretto:
riusare il build Unsloth già sul disco, adottare la release ufficiale llama.cpp Windows CUDA, o
compilare in proprio. Con evidenze su: funzionamento GPU standalone, dipendenze DLL, dimensioni,
licenza, story di aggiornamento.

## Acceptance Criteria

- [ ] La release ufficiale llama.cpp Windows CUDA è stata scaricata e provata: `--list-devices`
      vede la GPU **senza** DLL esterne nel PATH, e una chiamata `/v1/chat/completions` con
      `--reasoning off` sul GGUF gemma-4 produce traduzione senza CoT.
- [ ] Tabella comparativa (Unsloth build vs release ufficiale vs build proprio): dimensione totale
      da distribuire, DLL richieste, licenza, come si aggiorna.
- [ ] Raccomandazione esplicita registrata nel ticket e ripiegata in "Decisions So Far" della mappa.

## Blocked By

- None — can start immediately.

## Frontier

Il build Unsloth funziona ma dipende dalle DLL CUDA del venv di Studio
(`~/.unsloth/studio/unsloth_studio/Lib/site-packages/torch/lib`): rimuovere Studio lo romperebbe.
Senza una fonte indipendente, la destinazione ("togliere Unsloth") non è raggiungibile.

## Work Plan

1. Scaricare l'ultima release Windows CUDA da github.com/ggml-org/llama.cpp/releases (zip binari +
   zip `cudart-llama-bin-win-cuda...` se separato).
2. Scompattare in una dir di prova, `llama-server --list-devices`: verificare che CUDA0 appaia
   senza PATH aggiuntivi.
3. Avviare col GGUF di gemma-4 (`~/.cache/huggingface/hub/models--unsloth--gemma-4-E2B-it-qat-GGUF/
   snapshots/2ea63.../gemma-4-E2B-it-qat-UD-Q4_K_XL.gguf`), `-ngl 99 -c 4096 --reasoning off`,
   porta 8080; ripetere una chiamata del protocollo `measure_direct_llama.py` e confrontare i tempi
   col build Unsloth (3.4 s/paragrafo, ~21 s/pagina densa).
4. Misurare la dimensione della dir minima necessaria (exe + DLL) per ciascuna opzione.
5. Verificare la licenza (MIT per llama.cpp; note per il build Unsloth) e la cadenza release.
6. Scrivere la tabella e la raccomandazione; aggiornare la mappa.

## Evidence to Capture

- Output di `--list-devices` e della chiamata di prova (tempi, `completion_tokens`,
  `reasoning_content`).
- Dimensioni in MB delle dir minime.
- URL della release usata.

## Out of Scope

- Il bundling nell'installer Tauri (ticket 02/04).
- La scelta finale di distribuzione (grilling 03).
