# Decision brief — LLM locale (grilling Ticket 04)

**Ticket:** `docs/tickets/local-llm-provider/04-grilling-local-llm-decisions.md`
**Parent spec:** `docs/specs/local-llm-provider-wayfinder.md`
**Data:** 2026-07-14
**Stato:** ⛔ **GATE UMANO** — prodotto AFK dall'autopilot; le decisioni D1-D7 attendono conferma dell'utente.
Nessuna risposta è stata inventata; sotto ci sono **raccomandazioni** da confermare o correggere.

> Evidenza a supporto: [research-unsloth-serving.md](./research-unsloth-serving.md) (serving, hardware),
> [design-provider-abstraction.md](./design-provider-abstraction.md) (astrazione, domande aperte).
> Il verdetto su qualità/latenza reale (Ticket 03) **non è disponibile AFK** perché richiede un endpoint
> locale in esecuzione: D1 e D6 sono più informati dopo il Ticket 03.

---

## D1 — Modello/quantizzazione target

**Domanda:** quale modello locale e quale quantizzazione per la traduzione?

**Contesto:** Unsloth esporta GGUF; il sweet spot è **Q4_K_M**. Per la traduzione servono buone capacità
multilingue e di *instruction following* (deve rispettare il contratto JSON del percettore).

**Raccomandazione (da confermare):** un modello **multilingue ~7B-14B in Q4_K_M** (es. famiglia Qwen/Gemma/
Llama recenti con forte multilingua). Non decidere il modello esatto finché il Ticket 03 non misura la
qualità reale su pagine campione.

- [ ] Conferma "~7B-14B Q4_K_M multilingue"
- [ ] Oppure modello specifico: ____________________

## D2 — Hardware disponibile

**Domanda:** GPU/VRAM e RAM della macchina d'uso?

**Contesto (research §Q4):** ~7B Q4_K_M ≈ 4.7 GB → **8 GB VRAM** comodi; ~13-14B ≈ 8-9 GB → **12 GB VRAM**.
Se il modello sborda dalla GPU alla RAM la velocità crolla. CPU-only è possibile ma lento.

**Raccomandazione:** dimensionare il modello (D1) alla VRAM reale; tenere il modello **interamente in GPU**.

- [ ] GPU/VRAM: ____________  RAM: ____________  (o "solo CPU")

## D3 — Provider locale: default o opt-in?

**Domanda:** all'avvio il provider attivo è OpenRouter (cloud) o il locale?

**Contesto:** il design (Ticket 02) mette **`active_provider` default = `openrouter`** per non rompere gli
utenti esistenti; il locale si seleziona nelle impostazioni.

**Raccomandazione (da confermare):** **opt-in** — default OpenRouter, locale selezionabile. Semplice e
retro-compatibile.

- [ ] Opt-in (default OpenRouter) — *raccomandato*
- [ ] Locale come default

## D4 — Comportamento se il server locale non è raggiungibile

**Domanda:** se il provider attivo è locale ma l'endpoint non risponde, cosa fa l'app?

**Opzioni:** (a) **errore chiaro** "server locale non raggiungibile" (NFR06), nessun fallback automatico;
(b) **fallback automatico** a OpenRouter (richiede key cloud presente); (c) chiedere all'utente.

**Raccomandazione (da confermare):** **(a) errore chiaro senza fallback automatico** — il fallback silenzioso
al cloud contraddice la scelta "offline/privato" e può generare costi API a sorpresa. Eventuale pulsante
manuale "riprova / passa a cloud".

- [ ] (a) Errore chiaro, nessun fallback — *raccomandato*
- [ ] (b) Fallback automatico a OpenRouter
- [ ] (c) Chiedi all'utente

## D5 — Auth per i server locali senza chiave

**Domanda:** LM Studio/Ollama/llama-server spesso non richiedono chiave, ma la guardia EC03 oggi rifiuta
chiavi vuote. Come gestirlo?

**Contesto:** il design (Ticket 02) introduce `requires_key` per-provider: se `false`, nessun header
`Authorization` inviato e la guardia EC03 non scatta. Unsloth Studio invece **richiede** `sk-unsloth-…`.

**Raccomandazione (da confermare):** **`requires_key=false` per i provider locali senza auth** (nessuna
chiave finta da digitare); `requires_key=true` per Unsloth Studio e OpenRouter.

- [ ] `requires_key` per-provider come sopra — *raccomandato*
- [ ] Preferisci sempre una chiave (anche finta) per uniformità

## D6 — La tenuta di `json_schema`/fallback blocca il rilascio del provider locale?

**Domanda:** se un server locale non onora `response_format: json_schema`, è accettabile affidarsi alla
**ladder di degradazione + estrazione JSON di fallback** già presente?

**Contesto:** Ollama ignora `json_schema` sul path `/v1`; LM Studio lo supporta bene; llama-server con
spigoli. L'app degrada già e fa parsing robusto. Il Ticket 03 misurerà l'affidabilità reale.

**Raccomandazione (da confermare):** **sì, il fallback è accettabile** come rete di sicurezza; consigliare
LM Studio a chi vuole `json_schema` nativo. Decisione finale dopo il verdetto del Ticket 03.

- [ ] Fallback accettabile — *raccomandato* (conferma dopo Ticket 03)
- [ ] Richiedi `json_schema` nativo (restringe le opzioni server)

## D7 — Ciclo di vita del server locale

**Domanda:** l'app assume che l'utente avvii Unsloth Studio/endpoint a mano, o vuole avvio/health-check
dall'app?

**Contesto:** la mappa mette l'orchestrazione in-app **fuori scope MVP**.

**Raccomandazione (da confermare):** **l'utente avvia il server a mano** nell'MVP; l'app fa al più un
**health-check** ("server locale non raggiungibile") ma non gestisce il processo.

- [ ] Utente avvia a mano + health-check — *raccomandato*
- [ ] L'app deve avviare/gestire il server (post-MVP)

---

## Come procedere

1. L'utente rivede D1-D7, spunta/annota le scelte.
2. Le risposte vengono ripiegate in "Decisions So Far" del parent spec.
3. Il Ticket 04 passa a `done/`; il Ticket 03 resta in attesa di un endpoint locale in esecuzione.
4. Si derivano i ticket di build verticali con `to-tickets` (slice già elencate nel design del Ticket 02).
