# Decision Brief — Provider diretto llama.cpp (grilling ticket 03)

## Type

Decision brief

## Status

Decided (2026-07-15)

## Contesto

Grilling a valle dei ticket 01-02 di [llamacpp-direct-wayfinder.md](./llamacpp-direct-wayfinder.md)
(sourcing binario e contratto sidecar Tauri, entrambi chiusi) e della validazione qualità del
ticket 07. Sei decisioni (D0-D5) fissano forma e scope della build che rimpiazza Unsloth Studio con
llama-server gestito dall'app.

## Decisioni

### D0 — Scope: uso personale (a monte di tutto)

Il provider diretto serve **solo la macchina dell'utente** (GPU RTX 500 Ada nota, modello già in
cache HuggingFace). Conseguenze: **niente bundling nell'installer, niente firma, niente
download-manager**. Binario e modello sono già sul disco; l'app deve solo lanciarli. Reversibile ma
esplicito: se in futuro si vorrà distribuire l'app, D1/D2 si riaprono come lavoro dedicato
(bundling ~1.1 GB di runtime CUDA o download-on-first-run, code signing) — fuori scope ora.

### D1 — App-managed: spawn e kill automatici, path non impacchettati

L'app fa **spawn** di llama-server quando serve e **kill deterministico** alla chiusura
(`RunEvent::Exit | ExitRequested` con `CommandChild` in managed state, pattern del ticket 02).
Motivo: il valore di "togliere Studio" è l'esperienza a un-click senza i suoi difetti (auto-unload,
proxy che blocca il CoT); uno script manuale lascerebbe metà del fastidio (terminale da gestire,
processo da chiudere). Binario e modello restano a **path locali configurabili, non impacchettati**
(coerente con D0). Lo script manuale resta il fallback documentato.

### D2 — Path espliciti in ⚙️ con default precompilati

Due nuove impostazioni per il provider `llamaserver`: **path del binario llama-server** e **path del
file GGUF**, mostrate in ⚙️ e precompilate ai valori che funzionano oggi. Motivo: l'auto-detect
sulla cache HF è fragile (l'hash di snapshot nel path cambia se si ri-scarica il modello; in cache
c'è più di un GGUF, es. GemmaX2) e nasconde quale file è in uso, rendendo opaco il debug. Se un path
manca → **errore azionabile** ("imposta il path del modello in ⚙️"), non uno spawn opaco. Nessun
default che dipenda dal venv di Studio.

### D3 — Preset `unsloth` mantenuto come opzione normale (non deprecato)

Studio resta installato e può servire per altro (fine-tuning, altri modelli): il preset `unsloth`
**resta selezionabile di prima classe**, senza etichetta "deprecato" e senza migrazione forzata
degli override `provider.unsloth.*`. llama.cpp diretto diventa il **consigliato/default** (D5), ma
non l'unico. Rimuovere il preset sarebbe irreversibile per risparmiare una riga: non vale.

### D4 — Parametri di default del server

Cablati nello spawn (tutti overridabili in ⚙️):

| Parametro | Valore | Motivo |
|-----------|--------|--------|
| porta | `8080` | combacia col `base_url` del preset `llamaserver` |
| `-ngl` | `99` | full offload; ci sta nei 4 GB con questo modello (~1.5 GB usati, misurato) |
| `-c` | `4096` | combacia con `n_ctx` da cui deriva il budget della pipeline; alzarlo richiederebbe alzare l'override `n_ctx` e su 4 GB il margine è poco |
| `--reasoning` | `off` | sopprime il CoT — il punto di tutta la mappa (validato ticket 07) |
| `--parallel` | `1` | l'app serializza già le richieste locali (L3); un solo slot prende tutto il contesto in modo pulito e risparmia VRAM |

### D5 — Default = llama.cpp diretto, spawn on-demand

Su installazione pulita l'app parte **col provider llama.cpp selezionato** (traduzione locale
out-of-the-box); il cloud (OpenRouter) resta selezionabile in ⚙️. Lo **spawn avviene su richiesta**
(alla prima traduzione, non all'avvio dell'app): evita di occupare la GPU da 4 GB e di caricare il
modello quando si apre l'app solo per rileggere un PDF già in cache. Combacia col
`LocalProviderSlot`/cursore on-demand già in main. La qualità senza reasoning è già validata
(ticket 07, "molto buona").

## Assunzioni residue / prerequisiti d'implementazione

1. **Casa stabile del binario**: la release ufficiale testata è in una dir temporanea. Il ticket
   04/05 deve installare la release ufficiale llama.cpp in una **dir fissa nota** e puntarci il path
   di default; l'unico binario "stabile" oggi (Unsloth) dipende dalle DLL del venv di Studio, la
   dipendenza che stiamo rimuovendo → non usarlo come default.
2. **Reap degli orfani su hard-crash** (rischio F.3 del ticket 02): `RunEvent` non scatta se l'app è
   killata a forza. Safeguard economico da includere nel ticket 04: al lancio, reap di un
   llama-server nostro stantio prima di rilanciare.

## Impatto sui ticket di build

- **Ticket 04** (sidecar lifecycle): spawn on-demand con i parametri D4, kill via `RunEvent`, reap
  orfani stantii (assunzione 2), health via `probe_reachable` esistente. Aggiungere
  `tauri-plugin-shell` e il permesso `shell:allow-execute`. Migrare l'entrypoint a
  `.build()?.run(|_, event| ...)`.
- **Ticket 05** (gestione path): due impostazioni path (binario + GGUF) con default precompilati
  (D2) e installazione della release ufficiale in dir stabile (assunzione 1). Errore azionabile se
  mancano.
- **Ticket 06** (preset + docs): `unsloth` resta opzione normale (D3); `llamaserver` diventa default
  (D5); README con setup del provider diretto; chiudere la nota "riaprire L6" nella mappa latenza.
