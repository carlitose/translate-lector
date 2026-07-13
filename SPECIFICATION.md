# translate-lector — Documento di Specifica

> Lettore di PDF desktop con traduzione IA affiancata in tempo reale, percettore di contesto (rolling summary + glossario dinamico) e ripristino completo della sessione.

- **Versione documento**: 1.0
- **Data**: 2026-07-13
- **Autore**: carlo.giuseppe@peppergroup.es
- **Tipo progetto**: App desktop, uso personale (MVP)

---

## 1. Panoramica

**translate-lector** è un'applicazione desktop che apre un PDF e mostra, in un pannello affiancato, la traduzione in tempo reale generata da un LLM nella lingua scelta dall'utente. L'app mantiene la coerenza della traduzione lungo tutto il documento grazie a un **percettore di contesto** (riassunto progressivo + glossario dinamico) e ripristina automaticamente lo stato di lettura alla riapertura.

### Idea centrale
Aprire un PDF → leggere l'originale a sinistra e la traduzione IA a destra → l'IA mantiene il contesto (summary) e la terminologia (glossario) coerenti pagina dopo pagina → alla chiusura/riapertura si riprende esattamente da dove si era rimasti.

### Obiettivi
- Leggere documenti in lingua straniera con traduzione di qualità e contestuale
- Evitare incoerenze di traduzione su documenti lunghi (nomi propri, termini tecnici, tono)
- Ripristino completo del lavoro tra sessioni
- Controllo dei costi API tramite cache

### Fuori ambito (MVP)
- OCR di PDF scansionati
- Text-to-speech (lettura ad alta voce)
- Account, multi-utente, sincronizzazione cloud
- Modifica manuale del testo tradotto

---

## 2. Fase 1 — Requisiti

### 2.1 Requisiti Funzionali (FR)
| ID | Requisito |
|----|-----------|
| FR01 | Aprire e visualizzare un PDF con testo estraibile |
| FR02 | Vista affiancata: PDF originale a sinistra, traduzione a destra |
| FR03 | Traduzione automatica pagina-per-pagina tramite LLM cloud |
| FR04 | Rilevamento automatico lingua origine + selezione lingua destinazione |
| FR05 | **Percettore di contesto**: rolling summary aggiornato a ogni pagina e passato all'IA |
| FR06 | **Glossario dinamico** automatico, con termini modificabili/bloccabili dall'utente |
| FR07 | Cache delle traduzioni per pagina (evita rigenerazione e costi) |
| FR08 | Persistenza sessione: PDF, posizione, lingua, cache traduzioni, glossario, summary |
| FR09 | Cronologia PDF recenti con riapertura in un clic |
| FR10 | Ripristino automatico all'avvio dell'ultima posizione di lettura |
| FR11 | Configurazione della API key del provider LLM (OpenRouter) |
| FR12 | Accesso a qualsiasi modello tramite gateway unico (OpenRouter) |

### 2.2 Requisiti Non Funzionali (NFR)
| ID | Requisito |
|----|-----------|
| NFR01 | App desktop (Windows; possibile cross-platform via Tauri) |
| NFR02 | Uso personale, single-user, nessun login/server |
| NFR03 | Tutti i dati salvati localmente |
| NFR04 | Controllo dei costi API tramite cache |
| NFR05 | UI non bloccante; traduzione pagina in tempo ragionevole |
| NFR06 | Gestione errori di rete/API (retry con backoff, messaggi chiari) |
| NFR07 | Storage sicuro della API key in locale (keychain di sistema) |

### 2.3 User Stories (US)
| ID | Storia |
|----|--------|
| US01 | Come lettore, apro un PDF e vedo la traduzione a lato per leggere nella mia lingua |
| US02 | Come lettore, riprendo da dove avevo lasciato |
| US03 | Come lettore, ottengo traduzioni coerenti in tutto il documento (contesto + glossario) |
| US04 | Come lettore, correggo la traduzione di un termine e viene rispettata |
| US05 | Come lettore, riapro rapidamente i PDF recenti |

### 2.4 Use Cases (UC)
| ID | Caso d'uso |
|----|-----------|
| UC01 | Apri nuovo PDF → estrai testo → traduci pagina corrente |
| UC02 | Scorri a nuova pagina → percettore aggiorna summary+glossario → traduci con contesto |
| UC03 | Modifica termine nel glossario → traduzioni future rispettano il blocco |
| UC04 | Chiudi app → salva sessione; Riapri → ripristina stato |
| UC05 | Configura API key del provider |

### 2.5 Edge Cases (EC)
| ID | Caso limite | Gestione |
|----|-------------|----------|
| EC01 | PDF scansionato/senza testo | Messaggio "non supportato" (no OCR nell'MVP) |
| EC02 | Nessuna connessione | Errore + uso della cache disponibile |
| EC03 | API key mancante/invalida | Richiesta di configurazione |
| EC04 | Pagina/testo molto grande | Chunking del testo e ricomposizione |
| EC05 | Rolling summary troppo lungo (limite token) | Compressione/troncamento automatico |
| EC06 | PDF in cronologia spostato/cancellato dal disco | Riconoscimento via hash, gestione file mancante |
| EC07 | Rate limit / costi API | Backoff e gestione |

### 2.6 Quality Requirements (QR)
| ID | Requisito di qualità |
|----|----------------------|
| QR01 | Coerenza terminologica della traduzione (glossario) |
| QR02 | Affidabilità del ripristino di sessione |
| QR03 | UI fluida e leggibile |

---

## 3. Fase 2 — Specifiche

### 3.1 Interfaccia utente

```
┌─────────────────────────────────────────────────────────────┐
│  [Apri PDF]  [Lingua: Italiano ▼]   translate-lector   [⚙️]  │
├───────────────────────────────┬─────────────────────────────┤
│                               │                             │
│      PDF ORIGINALE            │     TRADUZIONE (IA)         │
│      (pagina renderizzata)    │     (testo tradotto)        │
│                               │                             │
├───────────────────────────────┴─────────────────────────────┤
│  ◀ Pag. 12 / 340 ▶      [Glossario]   ● Tradotto (cache)    │
└─────────────────────────────────────────────────────────────┘
```

- **Sinistra**: PDF originale renderizzato (pdf.js)
- **Destra**: traduzione della pagina corrente, **in sola lettura**
- **Barra superiore**: Apri PDF · selettore lingua (elenco curato + campo libero) · Impostazioni
- **Barra inferiore**: navigazione pagine · accesso Glossario · stato traduzione (spinner / cache / errore)

### 3.2 Motore di traduzione
- **Trigger**: on-demand all'arrivo sulla pagina + **prefetch** in background della pagina successiva
- **Cache** per pagina: le pagine già tradotte non vengono rigenerate
- **Chiamata IA unica strutturata** per pagina, che restituisce in un colpo solo:
  `{ traduzione, summary_aggiornato, nuovi_termini }`

### 3.3 Percettore di contesto
- **Rolling summary** a **limite fisso con auto-compressione**: quando supera il tetto configurato, l'IA lo ricomprime mantenendo solo i punti chiave → dimensione e costo prevedibili, contesto lontano preservato in forma compressa e dettagli recenti freschi.
- **Glossario dinamico** popolato automaticamente dall'IA, con termini modificabili e bloccabili dall'utente.
- I **termini bloccati** (`locked = true`) vengono passati nel prompt come **vincolo assoluto** per tutte le traduzioni successive.
- Summary e glossario sono **persistiti nella sessione** e ricaricati alla riapertura.

#### Struttura voce del glossario
| Campo | Esempio | Descrizione |
|-------|---------|-------------|
| Termine originale | "board" | parola/espressione nella lingua di origine |
| Traduzione | "consiglio" | traduzione scelta |
| Tipo | tecnico | nome proprio / tecnico / comune |
| Bloccato | sì/no | se "sì", l'IA usa sempre questa traduzione |
| Nota | "contesto aziendale" | annotazione opzionale dell'utente |
| Pagina | 12 | dove è comparso la prima volta |

### 3.4 Lingue
- Lingua di **origine**: rilevata automaticamente
- Lingua di **destinazione**: **elenco curato** (~10-15 lingue comuni) **+ campo libero** per qualsiasi altra

### 3.5 Impostazioni (⚙️)
| Impostazione | Descrizione |
|--------------|-------------|
| API key | chiave OpenRouter (salvata nel keychain di sistema) |
| Modello | ID modello OpenRouter (dropdown dei più usati + campo libero) |
| Lingua predefinita | lingua di destinazione di default all'apertura |
| Prefetch | attiva/disattiva la pre-traduzione della pagina successiva |
| Limite summary | dimensione max del rolling summary |
| Cartella dati | dove salvare sessioni/cache/glossario in locale |
| Svuota cache | pulsante per cancellare le traduzioni salvate |

---

## 4. Fase 3 — Design del sistema

### 4.1 Stack tecnologico
| Livello | Tecnologia |
|---------|-----------|
| App shell | **Tauri** (binari leggeri, sicuro) |
| Frontend | **Svelte + TypeScript** nella webview |
| Rendering PDF | **pdf.js** |
| Backend/core | **Rust** (comandi Tauri) |
| Storage | **SQLite** (`rusqlite`/`sqlx`), file `.db` unico |
| Segreti | Keychain di sistema (plugin Tauri) |
| LLM gateway | **OpenRouter** (protocollo OpenAI-compatible) |

### 4.2 Divisione delle responsabilità

**Frontend (Svelte)**
- Rendering PDF con pdf.js ed estrazione del testo di pagina
- UI: layout affiancato, navigazione, pannello glossario, impostazioni
- Gestione dello stato dell'interfaccia
- Invocazione dei comandi Tauri verso il core

**Core (Rust, via comandi Tauri)**
- Chiamate all'LLM via OpenRouter (la API key non è mai esposta nella webview)
- Storage sicuro della API key (keychain di sistema)
- Persistenza di sessione, cache traduzioni, glossario, rolling summary
- Logica del percettore: costruzione prompt, parsing JSON della risposta, compressione del summary, chunking

```
┌──────────────────────────────────────────────┐
│                  Tauri App                     │
│                                                │
│  ┌───────────────┐   comandi   ┌────────────┐ │
│  │ Frontend       │  ────────▶  │ Core Rust  │ │
│  │ (Svelte + TS)  │  ◀────────  │            │ │
│  │  - pdf.js      │   risposte  │  - LLM     │─┼──▶ OpenRouter
│  │  - UI          │             │  - percettore  │
│  │  - stato       │             │  - storage │─┼──▶ SQLite (.db)
│  └───────────────┘             │  - keychain│─┼──▶ OS Keychain
│                                 └────────────┘ │
└──────────────────────────────────────────────┘
```

### 4.3 Modello dati (SQLite)

```sql
-- PDF conosciuti (cronologia)
documents (
  id            INTEGER PRIMARY KEY,
  file_path     TEXT,
  file_hash     TEXT,        -- riconosce il file anche se spostato
  title         TEXT,
  total_pages   INTEGER,
  last_opened_at TEXT
)

-- Stato di lettura per documento
sessions (
  id              INTEGER PRIMARY KEY,
  document_id     INTEGER REFERENCES documents(id),
  target_language TEXT,
  current_page    INTEGER,   -- posizione di ripristino
  scroll_position REAL,
  rolling_summary TEXT,      -- summary progressivo compresso
  updated_at      TEXT
)

-- Traduzioni per pagina (cache)
translations_cache (
  id              INTEGER PRIMARY KEY,
  document_id     INTEGER REFERENCES documents(id),
  page_number     INTEGER,
  target_language TEXT,
  source_text     TEXT,
  translated_text TEXT,
  created_at      TEXT,
  UNIQUE(document_id, page_number, target_language)
)

-- Termini per documento
glossary (
  id              INTEGER PRIMARY KEY,
  document_id     INTEGER REFERENCES documents(id),
  source_term     TEXT,
  translation     TEXT,
  type            TEXT,      -- nome proprio / tecnico / comune
  locked          INTEGER,   -- bool: traduzione imposta dall'utente
  note            TEXT,
  first_seen_page INTEGER
)

-- Configurazione globale (key-value)
settings (
  key   TEXT PRIMARY KEY,
  value TEXT
)
```

### 4.4 Integrazione LLM (OpenRouter)
- Il core Rust implementa il protocollo **OpenAI chat-completions** esposto da OpenRouter
  (`POST https://openrouter.ai/api/v1/chat/completions`).
- **Un solo client** copre tutti i modelli; l'utente sceglie l'ID modello nelle impostazioni
  (es. `anthropic/claude-sonnet-5`, `openai/gpt-...`, `google/gemini-...`).
- Header OpenRouter opzionali (`HTTP-Referer`, `X-Title`) impostati con nome/URL app.
- Uso di `response_format` JSON dove il modello lo supporta; **parsing robusto di fallback**
  (estrazione del blocco JSON) per i modelli che non lo garantiscono.

#### Contratto del percettore (per pagina)

**Input inviato al modello:**
- Lingua di destinazione
- Testo della pagina corrente (estratto da pdf.js)
- Rolling summary attuale (contesto delle pagine precedenti)
- Glossario attuale, con i termini **bloccati** marcati come vincolo assoluto
- Istruzione: traduci in modo coerente con summary e glossario; aggiorna il summary; proponi nuovi termini rilevanti

**Output atteso (JSON):**
```json
{
  "translated_text": "…traduzione della pagina…",
  "updated_summary": "…summary aggiornato/compresso…",
  "new_glossary_terms": [
    { "source_term": "board", "translation": "consiglio", "type": "tecnico", "note": "" }
  ]
}
```

**Regole di gestione:**
- Termini con `locked = true` → vincolo assoluto, l'IA non li cambia
- `new_glossary_terms` non bloccati → salvati; l'utente può poi bloccarli/correggerli
- `updated_summary` oltre il limite → richiesta di compressione (EC05)
- Testo pagina troppo grande → chunking e ricomposizione (EC04)
- Risposta JSON malformata → retry con prompt di correzione, poi fallback
- Errori API/rete → retry con backoff, poi messaggio all'utente (NFR06, EC07)

---

## 5. Flussi principali

### UC01 — Apertura di un nuovo PDF
1. Utente clicca **Apri PDF** e seleziona un file.
2. Frontend renderizza la pagina con pdf.js ed estrae il testo.
3. Se il PDF non ha testo estraibile → messaggio "non supportato" (EC01).
4. Core registra/aggiorna il documento (hash, titolo, pagine) e crea/carica la sessione.
5. Traduzione della pagina corrente (vedi UC02).

### UC02 — Traduzione con contesto
1. All'arrivo su una pagina, il core controlla la **cache** (`translations_cache`).
2. Se presente → mostra subito la traduzione salvata.
3. Se assente → chiamata LLM con testo pagina + rolling summary + glossario.
4. Salva `translated_text`, aggiorna `rolling_summary` e inserisce i nuovi termini nel `glossary`.
5. **Prefetch**: se attivo, pre-traduce in background la pagina successiva.

### UC03 — Modifica di un termine del glossario
1. Utente apre il **Glossario**, modifica una traduzione e la imposta come **bloccata**.
2. Da quel momento il termine viene passato come vincolo assoluto in tutte le traduzioni.

### UC04 — Chiusura e ripristino
1. Alla chiusura, la sessione (pagina, scroll, lingua, summary) è già persistita.
2. All'avvio, l'app ricarica l'ultima sessione e apre il PDF alla posizione salvata (FR10).
3. Se il file è stato spostato/cancellato → gestione file mancante via hash (EC06).

### UC05 — Configurazione
1. Utente apre **Impostazioni**, inserisce la API key OpenRouter e sceglie il modello.
2. La chiave è salvata nel keychain di sistema (NFR07).

---

## 6. Roadmap / Estensioni future
- **OCR** per PDF scansionati (EC01)
- **Text-to-speech** della traduzione (e dell'originale) per apprendimento
- **Modifica manuale** del testo tradotto con salvataggio in cache
- **Modalità apprendimento** (flashcard / spaced repetition sul glossario)
- **Sincronizzazione cloud** delle sessioni tra dispositivi
- **Multi-provider** oltre OpenRouter (se necessario)

---

## 7. Riepilogo decisioni chiave
| Area | Decisione |
|------|-----------|
| Piattaforma | App desktop (Tauri), uso personale/MVP |
| Frontend | Svelte + TypeScript, pdf.js |
| Backend | Rust (core Tauri) |
| Storage | SQLite locale + keychain per la API key |
| LLM | OpenRouter (qualsiasi modello, protocollo OpenAI-compatible) |
| Unità di traduzione | Pagina intera, on-demand + prefetch, con cache |
| Coerenza | Percettore: rolling summary (limite fisso + compressione) + glossario dinamico bloccabile |
| Chiamata IA | Unica per pagina, output JSON `{ traduzione, summary, nuovi_termini }` |
| Sessione | Ripristino completo: PDF, posizione, lingua, cache, glossario, summary |
| PDF | Solo testo estraibile (no OCR nell'MVP) |
| Traduzione UI | Sola lettura |
```
