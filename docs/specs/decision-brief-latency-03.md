# Decision Brief — Latenza traduzione locale (grilling ticket 03)

## Type

Decision brief

## Status

Decided (2026-07-14)

## Contesto

Grilling a valle di [local-translation-latency-diagnosis.md](./local-translation-latency-diagnosis.md) e
delle misure/prototipo dei ticket 01-02 di
[local-translation-latency-wayfinder.md](./local-translation-latency-wayfinder.md). Sei decisioni
(L1-L6) sbloccano i ticket di build 04-06 e chiudono/ridimensionano il ticket 05.

## Decisioni

### L6 — Modello locale: resta gemma-4-E2B-it-qat (con perceptor completo)

Validato in sessione: GemmaX2-28-2B (modello di traduzione senza reasoning, caricato in Unsloth Studio,
`ctx=8192`) è ~9× più veloce (2.5-4 s vs 29.7 s per chiamata) ma **incompatibile con la pipeline STC**:
- col prompt "app-like" (system + summary + glossario, stesso di `build_translate_only_*`) risponde con
  **output vuoto o l'inglese non tradotto** (`F1_para_app_like`, `F3_packed_app_like`: `content` vuoto o
  = testo sorgente) — il modello non segue istruzioni in stile chat, solo il formato canonico
  `Translate this from English to Italian:\nEnglish: ...\nItalian:` (senza system, senza glossario, senza
  summary) produce traduzioni corrette.
- Su finestre multi-paragrafo col formato canonico (`F5_canonical_packed`, 16 paragrafi impacchettati)
  **fonde/riassume invece di tradurre riga per riga** (56 token di output per ~900 token di input): non
  affidabile per il round-trip che la cache per-unità richiede.
- **Decisione**: nessun beneficio di velocità è utilizzabile senza rompere glossario locked e summary del
  perceptor — requisiti non negoziabili del prodotto. Si resta su gemma-4-E2B-it-qat; il floor di latenza
  con CoT (~500 token/chiamata) è accettato e mitigato da L1 (packing).
- **Non chiuso per sempre**: se in futuro un modello di traduzione locale supporta prompt system + JSON
  contract, va rivalutato (nuova voce in Not Yet Specified della mappa).

### L1 — Packing a taglia fissa: PACK_TARGET_TOKENS = 512

Confermata la raccomandazione del ticket 02: `pack_units` impacchetta le unità-paragrafo di
`split_into_units` in finestre da **512 token fissi** (costante, non derivata dal `budget_unit_text`
dinamico), clampate al budget corrente solo quando questo è più stretto di 512 (caso limite: glossario o
summary molto grandi). Motivazione: la cache per-unità (chiave `unit_index` + `source_hash`) resta
**stabile ai repack** (misurato 2/2 stabili vs 0/2 col budget dinamico), senza migrazioni di schema.
Rivede D1 di STC-05: l'unità di **chiamata** diventa la finestra impacchettata; l'unità di **split**
resta il paragrafo (il fallback a frase per paragrafi oltre budget non cambia).

Effetto atteso: pagina densa da 18 paragrafi (~700 token) → 1-2 finestre invece di 18 chiamate.

### L2 — Cap del summary: ticket 05 chiuso senza implementazione

Le misure del ticket 01 hanno mostrato che il server fa prefix caching (`cached_tokens≈1078/1133` dalla
seconda chiamata della stessa pagina): il summary intero (fino a 1000 token) costa quasi zero in latenza
una volta impacchettata la pagina in 1-2 finestre. Nessun guadagno misurabile → ticket 05 chiuso, nessun
cambiamento al summary inviato alle chiamate translate-only.

### L3 — Prefetch locale: serializzato con priorità on-demand

Il provider locale è mono-modello: prefetch e traduzione on-demand in parallelo si contendono la GPU e
si rallentano a vicenda (C5 della diagnosi). Decisione: un solo job di traduzione alla volta verso il
provider locale; se un prefetch è in corso quando arriva una richiesta on-demand, il prefetch cede il
passo al **confine della finestra corrente** (non a metà chiamata HTTP). Il provider cloud non è
toccato (nessuna contesa: resta concorrente).

### L4 — Retry-on-timeout locale: 0 retry, fail-fast

Col timeout esplicito del ticket 13 (~180 s), un timeout in locale segnala un problema reale (server
bloccato, modello troppo lento per quella finestra), non un blip transitorio. Ritentare
automaticamente triplica l'attesa nel caso peggiore senza recuperare nulla (osservazione già nel ticket
13, confermata dalle misure: il timeout di 30 s di oggi scattava già "al pelo" su chiamate
sistematicamente lente). Decisione: **0 retry** sul timeout per il provider locale, con messaggio
d'errore azionabile (competenza del ticket 13); gli altri errori transient (5xx, connection reset)
mantengono il retry ×3 esistente. Il provider cloud non cambia.

### L5 — Target di latenza: ≤2 minuti a freddo + prefetch fluido

Il target originario "<10 s" della mappa non è raggiungibile con questo hardware/modello (floor
misurato ~40 s/pagina densa solo di decode). Nuovo target accettato:
- **Pagina densa, cache fredda, navigazione diretta**: ≤2 minuti, **zero timeout**.
- **Lettura sequenziale con prefetch**: la latenza percepita deve restare bassa perché il prefetch (ora
  serializzato, L3) ha il tempo di completarsi prima che l'utente arrivi alla pagina successiva nel
  ritmo di lettura normale — hit-rate atteso ≥80% in uso reale, da verificare in QA e non bloccante per
  l'accettazione tecnica dei ticket di build.

## Impatto sui ticket di build

- **Ticket 04** (packing cablato): usa `PACK_TARGET_TOKENS = 512` (L1); **bloccato anche dal ticket 13**
  (timeout esplicito), perché una finestra da 512 token dura più dei 30 s di default attuali.
- **Ticket 05** (cap summary): **chiuso, non implementato** (L2) — nessun task residuo.
- **Ticket 06** (prefetch/cancellazione): implementa la serializzazione a priorità on-demand (L3), non
  la disattivazione; include la logica di cancellazione al confine di finestra già prevista.
- **Ticket 13** (epica local-llm-provider, timeout): da aggiornare con la correzione C1 (default 30 s
  del client blocking, non il proxy) e con la policy retry L4 (0 retry sul timeout locale).

## Assunzione residua

La stima "CoT ~500 token pagato una volta per finestra invece che per paragrafo" è misurata su finestre
sintetiche (ticket 01/02); va confermata su pagine reali durante l'e2e del ticket 04, che è anche il
punto di verifica del target L5.
