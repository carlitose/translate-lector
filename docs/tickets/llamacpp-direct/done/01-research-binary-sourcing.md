# 01 — Research: sourcing del binario llama-server

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Status

**Done** (2026-07-15). Raccomandazione: **release ufficiale llama.cpp Windows CUDA** (MIT,
standalone, GPU vista senza DLL di Studio, resa identica al build Unsloth). Costo di
distribuzione: **~1.1 GB** di runtime CUDA — è la vera domanda per il grilling. Vedi §Findings.

## Type

research

## Outcome

Una raccomandazione motivata sulla fonte del binario llama-server per il provider diretto:
riusare il build Unsloth già sul disco, adottare la release ufficiale llama.cpp Windows CUDA, o
compilare in proprio. Con evidenze su: funzionamento GPU standalone, dipendenze DLL, dimensioni,
licenza, story di aggiornamento.

## Acceptance Criteria

- [x] La release ufficiale llama.cpp Windows CUDA è stata scaricata e provata: `--list-devices`
      vede la GPU **senza** DLL esterne nel PATH, e una chiamata `/v1/chat/completions` con
      `--reasoning off` sul GGUF gemma-4 produce traduzione senza CoT. → §Findings 1-2.
- [x] Tabella comparativa (Unsloth build vs release ufficiale vs build proprio): dimensione totale
      da distribuire, DLL richieste, licenza, come si aggiorna. → §Findings 3.
- [x] Raccomandazione esplicita registrata nel ticket e ripiegata in "Decisions So Far" della mappa.
      → §Findings 4 + mappa aggiornata.

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

## Findings

### 1. La release ufficiale vede la GPU standalone

Release `b10016`, asset `llama-b10016-bin-win-cuda-12.4-x64.zip` (237 MB) +
`cudart-llama-bin-win-cuda-12.4-x64.zip` (373 MB), estratti nella **stessa cartella**. Scelto il
build **CUDA 12.4** e non 13.3 perché il driver installato (596.47) arriva a **CUDA 13.2**: 13.3 è
appena sopra il supporto, 12.4 è ampiamente forward-compatible.

`llama-server.exe --list-devices` (senza alcuna DLL di Studio nel PATH):

```
Available devices:
  CUDA0: NVIDIA RTX 500 Ada Generation Laptop GPU (4093 MiB, 3245 MiB free)
```

Il ggml-cuda ufficiale include il runtime; serve solo affiancare le 3 DLL cudart
(`cudart64_12.dll`, `cublas64_12.dll`, `cublasLt64_12.dll`) dallo zip cudart. **Nessuna dipendenza
dal venv di Studio** → obiettivo "togliere Unsloth" raggiungibile.

### 2. Resa di traduzione identica al build Unsloth

Stesso protocollo `measure_direct_llama.py`, GGUF gemma-4-E2B-it-qat-UD-Q4_K_XL, `--reasoning off`,
GPU su porta 8081:

| | Paragrafo singolo | Pagina densa (2 finestre 512) | CoT |
|---|---|---|---|
| Studio (proxy) | 29.7 s / 559 tok | ~99 s | ~500 tok, insopprimibile |
| Build Unsloth diretto | 3.4 s / 46 tok | ~21 s | zero |
| **Release ufficiale** | **3.5 s / 46 tok** | **~20 s** | **zero** |

Prestazioni sovrapponibili. Il modello carica in ~2.3 s. `--reasoning off` sopprime il CoT su
entrambi i binari (era il proxy di Studio a bloccarlo).

### 3. Tabella comparativa sourcing

| Opzione | GPU standalone | Dim. set minimo GPU | `ggml-cuda.dll` | Licenza | Aggiornamento |
|---|---|---|---|---|---|
| **Build Unsloth** (`~/.unsloth/llama.cpp/`) | ❌ dipende dalle DLL CUDA del venv torch di Studio | — (non distribuibile: è la dipendenza da rimuovere) | 169 MB | MIT (upstream) | legato a Studio |
| **Release ufficiale** (b10016, cuda-12.4) | ✅ sì (cudart affiancato) | **~1078 MB** | 509 MB | MIT | release ~quotidiane su GitHub, tag `bNNNNN` |
| **Build proprio** (target solo sm_89 = RTX Ada) | ✅ sì | stimato molto < 1 GB (ggml-cuda per una sola arch) | ridotto | MIT | richiede infra di build CI |

Nota chiave sulla dimensione: la `ggml-cuda.dll` ufficiale (509 MB) è ~3× quella di Unsloth
(169 MB) perché include i kernel per **molte** compute-capability CUDA; il build Unsloth ne targetta
poche. Il set minimo GPU della release ufficiale è **~1.1 GB**, dominato da ggml-cuda (509) +
cublasLt (452) + cublas (95). È il numero che pesa sull'installer (→ grilling D1).

### 4. Raccomandazione

**Adottare la release ufficiale llama.cpp Windows CUDA come fonte del binario.** Motivi: MIT,
GPU standalone (rimuove del tutto la dipendenza da Studio, che è lo scopo della mappa), resa
identica, aggiornamento pulito via tag GitHub. Il build Unsloth è escluso proprio perché legato a
Studio; il build proprio resta un'opzione futura *solo se* la dimensione del bundle diventa il
vincolo dominante (ridurrebbe ggml-cuda targettando la sola sm_89).

**Conseguenza per il grilling (D1)**: bundling nell'installer = **+~1.1 GB**. Se è troppo, le vie
sono (a) download-on-first-run del pacchetto binari da GitHub, oppure (b) build proprio slim.
Decisione umana.

### Evidenza catturata

- Binari in `scratchpad/llamacpp-official/extracted/` (temporaneo, non committato).
- Release: https://github.com/ggml-org/llama.cpp/releases/tag/b10016
- Driver: 596.47 (CUDA 13.2 max) → scelto build CUDA 12.4.
