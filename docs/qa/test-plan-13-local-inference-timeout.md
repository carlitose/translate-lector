# Test Plan — Ticket 13: timeout HTTP esplicito/configurabile per-provider

## Scope

Il chat client (`ChatCompletionsClient` in `src-tauri/src/llm.rs`) ora costruisce il `reqwest::blocking::Client`
con un timeout esplicito e per-provider (`ProviderConfig.timeout_secs`, default cloud=30s / locale=180s,
override in `provider.<id>.timeout_secs`), il messaggio di errore per un timeout su un **provider locale** è
riscritto per essere azionabile (indica ⚙️ Impostazioni, modello più veloce, n_ctx più piccolo), e la
`RetryPolicy` non ritenta più su timeout per i provider locali (`RetryPolicy::for_base_url`), mentre tutto il
resto (retry su `ServerError`/`RateLimited`/`Offline`, timeout+retry per OpenRouter/cloud) resta invariato.

Verifica: eseguire `cargo test` nel core (automatico, già presente nel diff), poi eseguire i passi manuali
sotto contro l'app reale, usando sia il server locale reale (`http://localhost:8888`, Unsloth Studio) sia
scenari sintetici (server "muto"/lento) per osservare il timeout, il messaggio e l'assenza di retry.

**Nota di scope**: questo ticket **non** tocca la UI — non esiste un campo nelle Impostazioni per editare
`timeout_secs` (confermato in `docs/tickets/local-llm-provider/done/13-local-inference-timeout.md`, criterio
"Non toccata la UI/mapping errori frontend"). Per impostare un override di `timeout_secs` durante il test
manuale, usare i comandi Tauri generici già esistenti `get_setting`/`set_setting` dalla console DevTools
dell'app (vedi Prerequisiti).

## Prerequisiti

- Ambiente dev: `npm run tauri dev` (o `npm run dev` + shell Tauri) dalla root del repo, con DevTools
  abilitati sulla finestra webview (tasto destro → Ispeziona, o `Ctrl+Shift+I`) per poter invocare comandi
  Tauri dalla console ed osservare eventuali marker di errore.
- Server LLM locale reale raggiungibile su `http://localhost:8888` (Unsloth Studio), header
  `Authorization: Bearer <la propria chiave Unsloth Studio>` (generata in Studio → Settings → API).
  Verificare che risponda prima di iniziare:
  `curl http://localhost:8888/v1/models -H "Authorization: Bearer <la propria chiave Unsloth Studio>"`.
- Un PDF di prova già caricabile nell'app (una pagina qualsiasi, non serve che sia lunga per i passi felici).
- Provider attivo impostato su un provider **locale** (default D3 = `unsloth`) per i passi che riguardano il
  comportamento locale; un passo dedicato userà `openrouter` per verificare che il comportamento cloud resti
  invariato (richiede una API key OpenRouter valida already-stored, se si vuole testare il passo cloud reale;
  altrimenti quel passo può restare solo a livello di unit test già coperto dal diff).
- Comando helper da eseguire in DevTools console per leggere/scrivere il timeout per-provider:
  ```js
  // legge il timeout risolto (preset o override) per un provider
  await window.__TAURI__.core.invoke('get_provider_config', { providerId: 'unsloth' })
  // imposta un override esplicito (secondi) — es. 5s per forzare un timeout veloce nei test
  await window.__TAURI__.core.invoke('set_setting', { key: 'provider.unsloth.timeout_secs', value: '5' })
  // rimuove l'override tornando al default del preset (180s)
  await window.__TAURI__.core.invoke('set_setting', { key: 'provider.unsloth.timeout_secs', value: '' })
  ```

## Happy Path

1. Con provider attivo = `unsloth`, nessun override di `timeout_secs` impostato, eseguire
   `await window.__TAURI__.core.invoke('get_provider_config', { providerId: 'unsloth' })` da DevTools →
   il campo `timeout_secs` nella risposta vale `180`.
2. Con lo stesso setup, sull'app tradurre una pagina reale usando il server locale reale su
   `http://localhost:8888` con una richiesta che completa in tempi normali (pochi secondi/decine di secondi)
   → la traduzione va a buon fine, nessun errore, nessun timeout percepito (ben sotto i 180s di default).
3. Ripetere il passo 1 ma con provider attivo = `openrouter` → il campo `timeout_secs` vale `30` (default
   cloud, invariato rispetto al comportamento implicito precedente di reqwest).
4. Impostare un override esplicito valido: `set_setting({ key: 'provider.unsloth.timeout_secs', value: '60' })`,
   poi rileggere con `get_provider_config` → `timeout_secs` ora vale `60` (l'override per-provider è onorato).

## Edge Cases

1. Impostare un override non numerico: `set_setting({ key: 'provider.unsloth.timeout_secs', value: 'abc' })`,
   poi `get_provider_config` → `timeout_secs` torna al default del preset (`180`), il valore invalido è
   ignorato silenziosamente (nessun crash/errore visibile).
2. Impostare un override a `0`: `set_setting({ key: 'provider.unsloth.timeout_secs', value: '0' })`, poi
   `get_provider_config` → `timeout_secs` torna a `180` (0 non è un intero positivo valido, viene scartato).
3. Impostare un override molto basso e reale (es. `2` secondi) contro il server locale reale su
   `http://localhost:8888`, poi avviare una traduzione che nella pratica richiede più di 2s (es. una pagina
   con parecchio testo) → la richiesta fallisce con `LlmError::Timeout` in ~2s (non attende 180s né si blocca),
   e l'errore mostrato all'utente è il messaggio azionabile locale (vedi Percorsi Negativi #1). Ripristinare
   poi l'override a un valore ragionevole (o rimuoverlo) per non lasciare l'ambiente di test rotto.
4. Provider sconosciuto/non-preset (se raggiungibile via `get_provider_config` con un `id` inventato) →
   `timeout_secs` ricade sul default locale generoso (`180`), non su quello cloud breve (coerente con la
   scelta "un id sconosciuto è trattato come locale" nel diff di `settings.rs`).

## Percorsi Negativi / Errore

1. **Messaggio di timeout azionabile su server locale**: con provider attivo `unsloth` puntato al server reale
   `http://localhost:8888` e un override di `timeout_secs` molto basso (es. `1`-`2` secondi, impostato come
   sopra) o, in alternativa, arrestando temporaneamente il server locale mentre il client tenta la richiesta,
   avviare la traduzione di una pagina → l'errore mostrato nell'app contiene il testo azionabile
   ("Il server locale è troppo lento o ha chiuso la connessione. Aumenta il timeout in ⚙️ ... usa un modello
   più veloce, o riduci n_ctx del server."), **non** il messaggio generico con prefisso
   "Errore di rete/servizio LLM (timeout): ..." e **non** un doppio prefisso (verificare che la stringa non
   contenga "Errore di rete/servizio LLM (timeout): Il server locale").
2. **Nessun retry su timeout in locale**: con l'override di `timeout_secs` basso del passo precedente,
   osservare (log del server locale, se disponibile, o tempo totale trascorso lato app) che dopo il primo
   fallimento per timeout la richiesta **non** viene ritentata automaticamente (nessun secondo/terzo tentativo
   con backoff) — il tempo totale osservato è ~1x il timeout configurato, non ~3x (che sarebbe il comportamento
   pre-fix con retry ×3). Questo è il punto centrale della decisione L4.
3. **Messaggio generico invariato su cloud**: con provider attivo `openrouter` (richiede API key valida o
   simulazione di un endpoint lento se non si vuole spendere una chiamata reale a pagamento), forzare un
   timeout → l'errore mostrato ha il prefisso generico "Errore di rete/servizio LLM (timeout): ..." (comportamento
   pre-esistente, non il testo azionabile locale). Se non è pratico riprodurlo end-to-end con una vera chiamata
   OpenRouter, questo passo può essere considerato coperto dal test automatico
   `timeout_on_a_remote_url_keeps_the_generic_message` in `llm.rs` — annotarlo come "verificato via unit test"
   nel report.
4. **Retry ancora attivo per altri errori transient in locale**: forzare un errore non-timeout ma transient
   contro il provider locale (es. arrestare del tutto il server prima della richiesta → `Offline`/connessione
   rifiutata, oppure — se il server locale espone un modo di restituire 429/5xx — usarlo) → l'app ritenta
   comunque fino a `max_attempts` con backoff (comportamento invariato), a differenza del caso puro Timeout.

## Rischi di Regressione

1. **Traduzioni cloud (OpenRouter) via percorso felice**: tradurre una pagina reale con provider `openrouter`
   e una richiesta che completa normalmente entro 30s → funziona come prima (il timeout esplicito di 30s non
   introduce regressioni per il caso normale, dato che coincide col default implicito di reqwest).
2. **`probe_reachable` / test di connessione al provider** (non toccato dal diff, ma condivide `is_local_url`):
   verificare che il test di raggiungibilità del server locale (usato es. all'avvio o al cambio provider)
   continui a funzionare e a distinguere correttamente locale vs remoto.
3. **Altri override numerici esistenti** (`max_tokens`, `n_ctx`) tramite l'UI in `ProviderConfig.svelte`:
   verificare che impostare/rimuovere `n_ctx` e `max_tokens` dalle Impostazioni continui a funzionare come
   prima — il refactor di `resolve_u32_override` in `settings.rs` ora è condiviso da tutti e tre i campi
   (`max_tokens`, `n_ctx`, `timeout_secs`), quindi un bug introdotto lì impatterebbe anche i due preesistenti.
   Passi concreti: aprire Impostazioni provider, impostare un `n_ctx` custom valido, salvare, ricaricare la
   pagina/riavviare l'app, verificare che il valore custom sia ancora quello impostato (non tornato al
   default); ripetere con un valore non numerico e verificare il fallback al default senza errori.
4. **Cambio provider attivo**: passare da `unsloth` a `openrouter` e viceversa dalle Impostazioni, verificare
   che `get_provider_config` riporti il `timeout_secs` corretto per ciascuno (180 vs 30, salvo override) e che
   la traduzione usi il client con il timeout del provider effettivamente attivo (non quello del provider
   precedente).
5. **Messaggi di errore per altri `LlmError`** (`Unreachable`, `ServerError`, `RateLimited`, `MissingApiKey`,
   ecc.): verificare a campione che continuino a mostrare il loro testo invariato — il diff tocca solo il ramo
   `Timeout` di `user_message()`, ma vale la pena una rapida controprova che gli altrimenti non siano stati
   toccati per errore.

## Out of Scope

- Non esiste (e non va aggiunto in questo test) un campo UI dedicato in Impostazioni per editare
  `timeout_secs`: è esplicitamente fuori scope per il ticket 13. I passi sopra usano `set_setting`/
  `get_setting` via DevTools come proxy accettato per il test manuale.
- Fix di empty-content / gestione di `max_tokens` insufficiente (già coperti da un ticket precedente,
  `docs/tickets/local-llm-empty-content/done/`) — non ri-testare qui.
- Configurazione lato server/proxy del timeout di Unsloth Studio (azione dell'utente sul server locale, non
  dell'app) — fuori scope, solo da documentare.
- Streaming delle risposte per ridurre la percezione di latenza — miglioramento futuro non incluso in questo
  ticket.
