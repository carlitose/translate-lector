# Research — Contratto OpenRouter, structured output e conteggio token

- **Ticket**: [02-research-openrouter-contract](../tickets/translate-lector/done/02-research-openrouter-contract.md)
- **Parent spec**: [translate-lector-wayfinder](./translate-lector-wayfinder.md)
- **Design source of truth**: [SPECIFICATION.md](../../SPECIFICATION.md) §3.3, §4.4
- **Data**: 2026-07-13
- **Rete**: `https://openrouter.ai` raggiungibile; doc consultate via WebFetch.

**Legenda evidenza**: ogni affermazione fattuale è marcata `[docs: <url>]` (verificata sulla doc OpenRouter) oppure `[from-knowledge, verify before relying]` (dalla conoscenza del protocollo OpenAI-compatible, da confermare prima di dipenderci).

---

## 1. Come chiamare `POST /api/v1/chat/completions`

Endpoint: `POST https://openrouter.ai/api/v1/chat/completions` — protocollo OpenAI chat-completions. `[docs: https://openrouter.ai/docs/api-reference/overview]`

### Header

| Header | Valore | Obbligatorio | Evidenza |
|--------|--------|--------------|----------|
| `Authorization` | `Bearer <OPENROUTER_API_KEY>` | Sì | `[docs: https://openrouter.ai/docs/api-reference/overview]` |
| `Content-Type` | `application/json` | Sì | `[docs: .../overview]` |
| `HTTP-Referer` | URL/identità dell'app (per i ranking) | No | `[docs: https://openrouter.ai/docs/quickstart]` |
| `X-Title` | Nome dell'app (per i ranking) | No | `[docs: quickstart — vedi nota]` |

> **Nota sul nome header di attribuzione**: il riassuntore WebFetch ha reso il secondo header come `X-OpenRouter-Title`. Il nome canonico documentato e usato dalla community/SDK è **`X-Title`**. `[from-knowledge, verify before relying]` — controllare l'esempio curl live sulla quickstart prima di fissarlo nel client. Entrambi gli header sono **opzionali** e servono solo per comparire nelle leaderboard OpenRouter; l'assenza non impatta la funzionalità.

Per translate-lector: impostare `HTTP-Referer: https://github.com/<owner>/translate-lector` (o URL app) e `X-Title: translate-lector`.

### Request body (campi rilevanti per l'MVP)

```jsonc
{
  "model": "anthropic/claude-sonnet-4.5",   // ID scelto dall'utente nelle impostazioni (§3.5)
  "messages": [
    { "role": "system", "content": "…istruzioni percettore…" },
    { "role": "user",   "content": "…lingua dest + testo pagina + summary + glossario…" }
  ],
  "temperature": 0.2,                        // basso: traduzione deterministica
  "max_tokens": 4096,                        // budget output (translated_text + summary + termini)
  "response_format": { /* vedi §2 */ },
  "stream": false,
  "provider": { "require_parameters": true } // opz.: instrada solo su provider che supportano i parametri richiesti (vedi §2)
}
```

Campi confermati nel tipo `Request`: `messages`/`prompt` (uno obbligatorio), `model` (opzionale, usa default se omesso), `max_tokens` [1, context_length), `temperature` [0,2], `stream`, `stop`, `tools`/`tool_choice`, `response_format`, `top_p`/`top_k`/`frequency_penalty`/`presence_penalty`, `seed`, `logit_bias`, e gli OpenRouter-specific `models`, `route`, `provider`, `user`. `[docs: https://openrouter.ai/docs/api-reference/overview]`

### Response body (campi rilevanti)

```jsonc
{
  "id": "gen-…",
  "model": "anthropic/claude-sonnet-4.5",     // modello effettivamente usato
  "created": 1752400000,
  "choices": [
    {
      "message": { "role": "assistant", "content": "…stringa JSON del percettore…" },
      "finish_reason": "stop"
    }
  ],
  "usage": { "prompt_tokens": 1234, "completion_tokens": 567, "total_tokens": 1801 }
}
```

Confermati: `id`, `choices[]` (con `message` o `delta` se streaming), `usage` (`prompt_tokens`/`completion_tokens`/`total_tokens`), `model`, `created`. `[docs: https://openrouter.ai/docs/api-reference/overview]`

**Implicazione per il core Rust**: la traduzione vive in `choices[0].message.content` come **stringa** che deve contenere il JSON del §4.4. Il parsing è a due livelli: (1) estrai `content`, (2) deserializza `content` come JSON del contratto percettore. Il campo `usage.total_tokens` va persistito/loggato per il controllo costi (NFR04) e per calibrare l'euristica dei token (§3).

### Gestione errori HTTP

- `401` API key mancante/invalida → EC03. `429` rate limit / crediti → EC07 (backoff esponenziale, NFR06). `[from-knowledge, verify before relying]`
- Se si richiede `response_format` a un modello che non lo supporta, la richiesta **fallisce con errore che indica il mancato supporto**. `[docs: https://openrouter.ai/docs/features/structured-outputs]` → vedi §2 per la mitigazione.

---

## 2. Structured output: `response_format` e fallback

### Cosa è documentato

- La doc structured-outputs mostra **solo** `type: "json_schema"`; `type: "json_object"` **non è documentato** su questa pagina. `[docs: https://openrouter.ai/docs/features/structured-outputs]`
- Forma esatta:

```json
{
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "percettore_output",
      "strict": true,
      "schema": { /* JSON Schema */ }
    }
  }
}
```
`[docs: .../structured-outputs]`

- Raccomandazione doc: **`strict: true` sempre**, così il modello segue lo schema esattamente senza campi extra. `[docs: .../structured-outputs]`
- **Modelli/provider supportati** (dalla doc): OpenAI (GPT-4o e successivi), Google Gemini, Anthropic (Sonnet 4.5, Opus 4.1+), la maggior parte dei modelli open-source, tutti i modelli serviti da Fireworks. `[docs: .../structured-outputs]`
- **Modelli non supportati**: la richiesta **fallisce con errore**. Mitigazione documentata: impostare `require_parameters: true` nelle provider preferences e includere `response_format`/`type: json_schema` tra i parametri richiesti, così l'instradamento sceglie solo provider compatibili. `[docs: .../structured-outputs]`
- Esiste un plugin **"Response Healing"** che riduce il rischio di JSON invalido per richieste **non-streaming** con `response_format: json_schema`. `[docs: .../structured-outputs]`

### Tabella modello → supporto (dalla doc)

| Famiglia | `json_schema` (strict) | Note |
|----------|------------------------|------|
| OpenAI GPT-4o+ | Sì | supporto nativo `[docs]` |
| Google Gemini | Sì | `[docs]` |
| Anthropic Claude (Sonnet 4.5, Opus 4.1+) | Sì | modelli Claude più vecchi: non garantito → serve fallback `[docs + from-knowledge]` |
| Open-source (via Fireworks e altri) | Per lo più sì | dipende dal provider; usare `require_parameters` `[docs]` |
| Modelli minori / provider senza supporto | No | la richiesta fallisce se si forza `response_format` `[docs]` |

### Strategia scelta per translate-lector (robusta, indipendente dal modello)

Poiché l'utente può scegliere **qualsiasi** ID modello (§3.5, §4.4), il core **non può assumere** il supporto di `json_schema`. Approccio a livelli:

1. **Tentativo preferito**: inviare `response_format: { type: "json_schema", json_schema: { name, strict: true, schema } }` con lo schema del §4.4 (vedi sotto). Opzionalmente `provider.require_parameters: true` per evitare provider che ignorano il parametro.
2. **Se OpenRouter risponde con errore "parametro non supportato"** (il modello scelto non supporta structured output): **ritentare senza `response_format`**, affidandosi al prompt che impone "rispondi SOLO con JSON valido" (il prompt del §4 chiede già JSON puro).
3. **Parsing robusto sempre attivo** (indipendentemente dal fatto che si sia usato `response_format`):
   - a. Prova `serde_json::from_str` diretto sul `content`.
   - b. Se fallisce, **estrai il primo blocco `{ … }` bilanciato** dal `content` (rimuove eventuali ```json fences, testo di preambolo/coda) e riprova a deserializzare.
   - c. Se ancora fallisce → **un solo retry** con un **prompt di correzione**: si rinvia la richiesta aggiungendo un messaggio che contiene la risposta precedente e l'istruzione "La tua risposta non era JSON valido conforme allo schema. Rispondi di nuovo con SOLO l'oggetto JSON, senza testo aggiuntivo, senza markdown." (allineato a §4.4 "Risposta JSON malformata → retry con prompt di correzione, poi fallback").
   - d. Se anche il retry fallisce → errore all'utente (fallback finale), traduzione non mostrata per quella pagina.

**Schema JSON da passare in `json_schema.schema`** (corrisponde 1:1 al §4.4):

```json
{
  "type": "object",
  "additionalProperties": false,
  "required": ["translated_text", "updated_summary", "new_glossary_terms"],
  "properties": {
    "translated_text": { "type": "string" },
    "updated_summary": { "type": "string" },
    "new_glossary_terms": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["source_term", "translation", "type", "note"],
        "properties": {
          "source_term": { "type": "string" },
          "translation": { "type": "string" },
          "type": { "type": "string", "enum": ["nome proprio", "tecnico", "comune"] },
          "note": { "type": "string" }
        }
      }
    }
  }
}
```

> Nota: con `strict: true` alcuni provider richiedono `additionalProperties: false` e che tutte le properties siano in `required` — lo schema sopra lo rispetta. `[from-knowledge, verify before relying]`

---

## 3. Conteggio token lato Rust (EC05 + budget prompt)

### Requisito

Serve stimare i token per: (a) decidere quando il `rolling_summary` supera il **limite configurato** e va compresso (EC05, §3.3), (b) fare **budget del prompt** (testo pagina + summary + glossario) rispetto alla context window del modello, e (c) decidere il chunking del testo pagina (EC04).

### Opzioni valutate

| Opzione | Pro | Contro |
|---------|-----|--------|
| **`tiktoken-rs`** (tokenizer BPE OpenAI) | Conteggio esatto per modelli OpenAI; veloce; puro Rust | Esatto **solo** per famiglie OpenAI. Claude/Gemini usano tokenizer diversi → comunque una stima. Aggiunge dipendenza + caricamento vocabolario in memoria. |
| **Euristica `chars/4`** (o `chars/3.5`) | Zero dipendenze, istantanea, indipendente dal modello, deterministica | Approssimata; varia per lingua (CJK, cirillico → più token/char). Rischio di sotto/sovrastima ai bordi. |
| **`usage.total_tokens` dalla risposta** | Dato **reale** del provider, gratis | Disponibile solo **dopo** la chiamata → non utile per il budget *pre*-chiamata, ma ottimo per calibrazione. |

### Decisione MVP: **euristica `chars/4` con margine di sicurezza, calibrata su `usage`**

Motivazione:
1. L'utente può scegliere **qualsiasi** modello (OpenAI, Anthropic, Gemini, open-source). Nessun tokenizer singolo è esatto per tutti → anche `tiktoken-rs` sarebbe una stima per i non-OpenAI. Il valore aggiunto di precisione svanisce nell'uso reale multi-modello.
2. Il limite del summary (§3.5 "Limite summary") non richiede precisione al singolo token: è una soglia di compressione con isteresi, non un vincolo hard del provider. Un margine conservativo (es. comprimere quando la stima raggiunge **~80%** del limite) assorbe l'errore dell'euristica.
3. Zero dipendenze/latenza; deterministico e facile da testare (unit test su stringhe note).
4. **Auto-calibrazione**: dopo ogni chiamata si conosce `usage.prompt_tokens` reale e i caratteri inviati → si può aggiornare un fattore `chars_per_token` osservato per il modello corrente e usarlo al posto del 4 fisso. Migliora la stima senza tokenizer.

Formula MVP:
```
est_tokens(text) = ceil(text.chars().count() / ratio)
ratio = fattore osservato per il modello (default 4.0; aggiornato da usage.prompt_tokens quando disponibile)
```
Regola compressione (EC05): se `est_tokens(rolling_summary) >= summary_limit * 0.8` → alla prossima chiamata istruisci il modello a **ricomprimere** il summary (il prompt lo prevede già, vedi §4). Il modello restituisce `updated_summary` compresso; il core verifica di nuovo la soglia.

**Rivalutare post-MVP**: se emerge che l'utente usa quasi sempre modelli OpenAI, aggiungere `tiktoken-rs` dietro feature-flag per quel caso. Non necessario per l'MVP.

---

## 4. Bozza prompt del percettore

Input: lingua destinazione, testo pagina corrente, rolling summary, glossario (con `locked` come vincolo assoluto). Output: **esattamente** il JSON del §4.4.

### System message

```
Sei il motore di traduzione di translate-lector. Traduci il testo di UNA pagina di un
documento verso la lingua di destinazione indicata, mantenendo la coerenza con il resto
del documento.

Devi:
1. Tradurre l'intero testo della pagina in modo fedele, naturale e coerente col tono del
   documento. Non riassumere, non omettere: traduci tutto il contenuto.
2. Rispettare in modo ASSOLUTO le traduzioni dei termini marcati come BLOCCATI nel glossario:
   usa sempre e solo la traduzione indicata, senza eccezioni.
3. Usare le traduzioni del glossario non bloccato quando appropriato, per coerenza.
4. Aggiornare il riassunto progressivo (summary) integrando i punti chiave di questa pagina.
   Se il summary risultante supererebbe circa {SUMMARY_TOKEN_LIMIT} token, COMPRIMILO
   mantenendo solo trama, entità, terminologia e fatti utili alla coerenza futura.
5. Proporre nuovi termini di glossario rilevanti apparsi in questa pagina (nomi propri,
   termini tecnici, espressioni ricorrenti) che non siano già nel glossario.

REGOLE DI OUTPUT (tassative):
- Rispondi con UN SOLO oggetto JSON valido, senza testo prima o dopo, senza markdown, senza
  code fence.
- Lo schema è ESATTAMENTE:
  {
    "translated_text": string,        // traduzione completa della pagina
    "updated_summary": string,        // summary aggiornato (compresso se necessario)
    "new_glossary_terms": [           // può essere []
      {
        "source_term": string,        // termine nella lingua di ORIGINE
        "translation": string,        // sua traduzione nella lingua di destinazione
        "type": "nome proprio" | "tecnico" | "comune",
        "note": string                // "" se nessuna nota
      }
    ]
  }
- Non aggiungere altre chiavi. Non tradurre le chiavi JSON. "note" vuota = "".
```

### User message (template, riempito dal core Rust)

```
LINGUA DI DESTINAZIONE: {TARGET_LANGUAGE}

RIASSUNTO PROGRESSIVO FINORA (contesto delle pagine precedenti):
{ROLLING_SUMMARY}      // "(nessuno: è la prima pagina)" se vuoto

GLOSSARIO ATTUALE:
Termini BLOCCATI (vincolo assoluto — usa esattamente questa traduzione):
{LOCKED_TERMS}         // righe "source_term => translation  [type] (note)"; "(nessuno)" se vuoto
Termini suggeriti (coerenza consigliata, non vincolante):
{UNLOCKED_TERMS}       // idem; "(nessuno)" se vuoto

TESTO DELLA PAGINA DA TRADURRE:
"""
{PAGE_TEXT}
"""

Produci ora il JSON come da schema.
```

### Esempio di risposta valida

Contesto d'esempio: pagina in inglese di un documento aziendale, target = italiano, glossario con `"board" => "consiglio"` bloccato.

```json
{
  "translated_text": "Il consiglio si è riunito il martedì per approvare il bilancio annuale. Il CEO, Jane Doe, ha presentato la strategia per il prossimo trimestre, sottolineando la crescita nel segmento enterprise.",
  "updated_summary": "Documento: relazione aziendale annuale. Il consiglio approva il bilancio; la CEO Jane Doe illustra la strategia trimestrale con focus sulla crescita enterprise. Terminologia fissata: board=consiglio.",
  "new_glossary_terms": [
    { "source_term": "CEO", "translation": "amministratrice delegata", "type": "tecnico", "note": "carica aziendale" },
    { "source_term": "Jane Doe", "translation": "Jane Doe", "type": "nome proprio", "note": "" },
    { "source_term": "enterprise", "translation": "enterprise", "type": "tecnico", "note": "segmento di mercato, lasciato in inglese" }
  ]
}
```

### Verifica round-trip contro SPECIFICATION.md §4.4

| Campo §4.4 | Campo nell'output prompt | Match |
|------------|--------------------------|-------|
| `translated_text` (string) | `translated_text` (string) | ✅ |
| `updated_summary` (string) | `updated_summary` (string) | ✅ |
| `new_glossary_terms[]` | `new_glossary_terms[]` | ✅ |
| `new_glossary_terms[].source_term` | `source_term` | ✅ |
| `new_glossary_terms[].translation` | `translation` | ✅ |
| `new_glossary_terms[].type` | `type` | ✅ |
| `new_glossary_terms[].note` | `note` | ✅ |

Nessun campo extra, nessun campo mancante. I nomi di campo del glossario coincidono anche con lo schema SQLite `glossary` (§4.3: `source_term`, `translation`, `type`, `note`), quindi il mapping verso il DB è diretto (più `document_id`, `locked=0` di default, `first_seen_page` = pagina corrente aggiunti dal core). ✅

---

## Decisioni da ripiegare nel wayfinder

- **Endpoint & auth**: `POST https://openrouter.ai/api/v1/chat/completions`, header `Authorization: Bearer <key>`, `Content-Type: application/json`; header opzionali `HTTP-Referer` + `X-Title` (verificare il nome esatto `X-Title` vs `X-OpenRouter-Title` sull'esempio curl live). La API key resta nel core Rust, mai nella webview.
- **Structured output**: usare `response_format: { type: "json_schema", json_schema: { name, strict: true, schema } }` con lo schema §4.4; `json_object` non è documentato da OpenRouter → non usarlo.
- **Robustezza indipendente dal modello**: poiché il modello è scelto dall'utente, il core NON assume il supporto di `json_schema`. Fallback obbligatorio: try `response_format` → su errore "non supportato" ritenta senza → parsing a livelli (serde diretto → estrazione primo blocco `{…}` bilanciato → 1 retry con prompt di correzione → errore utente). Opzionale `provider.require_parameters: true`.
- **Tokenizer**: **NIENTE `tiktoken-rs` nell'MVP**. Usare euristica `chars/4` con soglia di compressione all'80% del limite summary, auto-calibrata con `usage.prompt_tokens` reale per modello. Rivalutare `tiktoken-rs` post-MVP solo se prevale l'uso di modelli OpenAI.
- **Prompt percettore**: system+user separati; system fissa schema e regole (JSON puro, no markdown, termini `locked` = vincolo assoluto, compressione summary oltre soglia); user porta lingua/summary/glossario(locked+unlocked)/testo pagina. Output round-trip-verificato campo-per-campo con §4.4 e mappabile 1:1 sulla tabella `glossary`.
- **Costi/telemetria**: persistere `usage.total_tokens` per pagina per NFR04 e per calibrare il ratio token.
```
