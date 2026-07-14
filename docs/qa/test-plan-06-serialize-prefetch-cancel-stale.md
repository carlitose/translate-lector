# Test Plan — Ticket 06: serializzazione provider locale + cancellazione job stantii

Branch: `autopilot/06-serialize-prefetch-cancel-stale`
Diff analizzato: `git diff HEAD` (`src-tauri/src/lib.rs`, `src-tauri/src/llm.rs`, `src-tauri/src/translate.rs`)

## Scope

Il provider **locale** (llama-server dietro Unsloth Studio) ora accetta **una sola traduzione alla volta**
(`LocalProviderSlot`, mutex a slot singolo) e ogni job in corso — on-demand o prefetch — controlla a ogni
confine di unità/finestra se la pagina che sta traducendo è ancora quella "corrente" per il documento
(`CurrentPage`, cursore `document_id -> page_number` scritto solo dalle richieste on-demand); se non lo è
più, il job si interrompe restituendo `LlmError::Cancelled` (messaggio a basso profilo, non un errore reale)
senza sprecare altre chiamate al modello. Il provider **cloud** non è toccato: nessuno slot, nessun
controllo di staleness, comportamento pre-ticket invariato.

Si verifica: (a) al massimo una richiesta HTTP in volo verso il server locale in ogni momento, anche con
navigazione rapida e prefetch attivo; (b) priorità sempre all'on-demand (il prefetch cede, non viceversa);
(c) nessun errore visibile all'utente per una cancellazione di routine; (d) le finestre già tradotte in un
job cancellato restano valide in cache; (e) il provider cloud continua a girare concorrente e a completare
sempre, anche se l'utente naviga via nel frattempo.

## Prerequisiti

- App avviata in dev (`npm run dev` + Tauri, oppure build locale) con un documento PDF multi-pagina (≥4
  pagine, preferibilmente con paragrafi lunghi per allungare la finestra di osservazione) già aperto o
  apribile dall'app.
- **Server LLM locale reale**: Unsloth Studio in ascolto su `http://localhost:8888`, header
  `Authorization: Bearer <la propria chiave Unsloth Studio>` (generata in Studio → Settings → API).
  Verificare che risponda prima di iniziare:
  ```
  curl http://localhost:8888/v1/models -H "Authorization: Bearer <la propria chiave Unsloth Studio>"
  ```
  Expected: HTTP 200 con almeno un modello elencato (es. `gemma-4-E2B-it-qat`, cfr. spec latenza — CoT
  lento, ~20-30s/chiamata: utile per avere una finestra di osservazione ampia durante i test di rapidità).
- Avere sott'occhio i log del server Unsloth Studio (console/terminale dove gira) durante tutta la sessione:
  è la fonte primaria per contare le richieste in volo.
- In ⚙️ (ProviderConfig), preparare due configurazioni pronte da alternare:
  - **Provider locale**: preset `unsloth` con base URL `http://localhost:8888/v1/chat/completions`
    (default), impostato come provider attivo.
  - **Provider cloud**: preset `openrouter` con una API key valida configurata, per i test di
    regressione A2/A5.
- Prefetch abilitato (`get_prefetch_enabled`, default `true` — D5): verificare in ⚙️ che non sia stato
  disattivato manualmente per i casi che dipendono dal prefetch (B1-B4).
- Documento con almeno 2-3 pagine NON ancora in cache di traduzione, per garantire che le chiamate reali
  al modello avvengano (una pagina già in cache salta la chiamata HTTP e non è utile per osservare la
  serializzazione).

## Happy Path — Provider locale, navigazione sequenziale con prefetch

| step | action | expected_result |
|---|---|---|
| 1 | Impostare il provider attivo su `unsloth` (locale, `localhost:8888`) in ⚙️ e salvare. | Il banner ambra "provider non raggiungibile" NON appare (il server locale risponde); nessun errore in UI. |
| 2 | Aprire il documento di test sulla pagina 1 (non in cache) e attendere il completamento della traduzione on-demand. | Bottom bar mostra "⏳ Traduzione in corso…" poi "● Tradotto"; nei log del server locale si osserva **esattamente una** richiesta HTTP in arrivo durante l'attesa. |
| 3 | Subito dopo il completamento dello step 2, osservare i log del server per i successivi ~5-10s senza interagire con l'app. | Il prefetch di pagina 2 (`update_context=false`) parte automaticamente e produce **una nuova** richiesta HTTP nei log del server — ma mai in sovrapposizione con un'altra richiesta locale attiva contemporaneamente (slot singolo). |
| 4 | Mentre il prefetch di pagina 2 è ancora in corso (osservabile dal log, prima che risponda), navigare subito su pagina 2 con il pulsante "Next". | La UI passa a pagina 2 e mostra "⏳ Traduzione in corso…"; nei log del server si osserva che il prefetch in corso viene interrotto/non prosegue oltre l'unità in corso, senza generare due richieste HTTP contemporanee verso il server locale. |
| 5 | Attendere il completamento della traduzione di pagina 2. | Pagina 2 mostra "● Tradotto" (non "cache", perché il prefetch cancellato non ha popolato la cache per intero) oppure "● Tradotto (cache)" se il prefetch aveva già completato abbastanza unità prima della cancellazione — in ogni caso nessun errore visibile, nessun testo mancante o corrotto. |
| 6 | Tornare su pagina 1 con "Prev". | Pagina 1 mostra istantaneamente "● Tradotto (cache)" (hit di cache, zero chiamate HTTP nei log) — le unità tradotte prima della eventuale interruzione restano valide. |

## Happy Path — Verifica diretta della serializzazione reale (server locale)

| step | action | expected_result |
|---|---|---|
| 7 | Con provider locale attivo, aprire 3 pagine mai tradotte in sequenza molto rapida (click "Next" 3 volte a distanza di <1s l'uno dall'altro, senza attendere il completamento di nessuna). | Nei log del server Unsloth Studio si osserva che le richieste arrivano e vengono processate **una alla volta, in sequenza** — mai due richieste `/v1/chat/completions` verso `localhost:8888` risultano "in elaborazione" nello stesso istante (nessuna contesa GPU/decode intercalato nei log). |
| 8 | Contare nei log quante richieste HTTP sono state effettivamente completate fino in fondo vs. quante unità sono state processate parzialmente prima di un cambio pagina. | Il numero di richieste "andate a buon fine" corrisponde alle unità effettivamente tradotte e cacheate; non ci sono richieste HTTP duplicate per la stessa pagina/unità dovute a race condition. |
| 9 | Verificare via `curl` diretto a `localhost:8888` (in un terminale separato, mentre l'app sta traducendo) che il server accetti comunque la richiesta — cioè che la serializzazione sia lato app (Mutex Rust) e non un blocco del server stesso. | La chiamata `curl` con lo stesso modello va in coda naturale del server (se il server stesso è mono-richiesta) o viene comunque servita; non è previsto un secondo Mutex lato server — questo passo conferma che la garanzia è nel codice Tauri, non un side-effect del server. |

## Edge Cases

| step | action | expected_result |
|---|---|---|
| 10 | Navigare avanti e indietro molto rapidamente tra due pagine mai tradotte (Next, Prev, Next, Prev in <2s totali), con provider locale. | Nessun crash, nessuna doppia richiesta in volo (log del server); al termine, la pagina su cui l'utente si è fermato mostra il testo corretto (o è ancora in caricamento se il job più recente non è ancora tornato) — mai testo della pagina sbagliata. |
| 11 | Navigare su una pagina il cui prefetch era già stato avviato E completato interamente prima del cambio pagina (aspettare abbastanza perché il prefetch finisca prima di premere "Next"). | La pagina mostra subito "● Tradotto (cache)" senza nuova chiamata HTTP: il prefetch completato con successo (non cancellato) ha popolato la cache normalmente — comportamento pre-ticket preservato quando non c'è competizione. |
| 12 | Con `prefetchEnabled` disattivato in ⚙️, ripetere lo Happy Path 1 (naviga su pagina mai tradotta con provider locale). | Nessuna richiesta di prefetch parte dopo il completamento on-demand (solo 1 richiesta HTTP per pagina nei log); il job on-demand comunque acquisisce/rilascia lo slot locale correttamente (nessuna regressione quando il prefetch è disattivo). |
| 13 | Aprire un documento con una pagina che produce una finestra di traduzione molto lunga (molti paragrafi/token) sul provider locale, poi navigare via a metà della traduzione. | Il job in corso si interrompe al **confine di unità/finestra successivo**, non a metà di una chiamata HTTP già inviata (coerente con "il client è bloccante, niente cancellazione HTTP a metà" — verificabile perché il log del server mostra la richiesta in corso completarsi comunque prima che l'app smetta di aspettarne il risultato). |
| 14 | Riprovare a tradurre (tasto "↻ Riprova traduzione") una pagina il cui job precedente era stato cancellato (`LlmError::Cancelled`). | La traduzione riparte da zero unità mancanti (le unità già cacheate nel run cancellato vengono riusate, solo quelle mancanti richiamano il modello) e completa con successo, senza errori residui in UI. |

## Percorsi Negativi

| step | action | expected_result |
|---|---|---|
| 15 | Spegnere/disconnettere il server Unsloth Studio locale, poi navigare su una pagina non in cache con provider locale attivo. | Errore "server locale non raggiungibile" (EC02 case locale, banner ambra "provider non raggiungibile" già esistente) — **non** deve apparire il messaggio di `LlmError::Cancelled` né un codice tipo "EC0x" allarmante per la cancellazione: sono due path di errore distinti e non vanno confusi. |
| 16 | Con server locale raggiungibile ma provocando una cancellazione (navigazione rapida via da un prefetch in corso, come step 4), controllare il testo esatto mostrato in UI se per qualche motivo un errore di cancellazione dovesse comparire (es. ispezionando la console/log frontend, dato che normalmente il frontend scarta silenziosamente i risultati stantii). | Se visibile, il messaggio è "Traduzione annullata: la pagina non è più quella corrente." — nessuna parola "Errore", nessun codice "EC0x"; in condizioni normali questo testo non deve mai arrivare a schermo perché `translation.ts`/`+page.svelte` scarta già i risultati non più correnti tramite `isCurrentRequest`/token di richiesta. |
| 17 | Provocare più cancellazioni consecutive di seguito (navigazione rapidissima su 4-5 pagine mai tradotte) senza mai attendere un completamento. | Nessun retry automatico osservabile nei log del server per le pagine abbandonate (L4: 0 retry sulla cancellazione — non deve esserci un burst di chiamate ripetute per la stessa pagina abbandonata); l'app resta reattiva, nessun crash/freeze della UI. |

## Rischi di Regressione

| step | action | expected_result |
|---|---|---|
| 18 | Cambiare provider attivo a `openrouter` (cloud) in ⚙️ con API key valida. Ripetere l'Happy Path 1 (naviga rapidamente su più pagine mai tradotte, con prefetch attivo). | Il comportamento cloud è **identico al pre-ticket**: più richieste possono essere concorrenti (nessuno slot acquisito), nessun controllo di staleness — una traduzione cloud avviata e poi abbandonata dall'utente **completa comunque** e popola la cache (nessuna cancellazione anticipata), a differenza del locale. |
| 19 | Con provider cloud attivo, navigare via da una pagina mentre la sua traduzione cloud è ancora in corso, poi tornare sulla stessa pagina dopo che la richiesta cloud abbia avuto tempo di completare in background. | La pagina risulta "● Tradotto (cache)" al ritorno (la richiesta cloud abbandonata ha comunque scritto la cache, comportamento pre-ticket) — conferma che `should_check_is_current` esclude correttamente il cloud. |
| 20 | Con provider locale, tradurre una pagina già in cache (hit) più volte consecutive (Prev/Next ripetuto su pagine già tradotte). | Nessuna chiamata HTTP nei log del server (puro cache hit); lo slot locale non viene mai acquisito per un cache hit (nessun overhead di lock non necessario), risposta istantanea in UI. |
| 21 | Eseguire la suite di test automatici Rust del backend. | `cd src-tauri && cargo test` termina con tutti i test verdi, incluso il conteggio riportato nel ticket (219 test passati, baseline pre-ticket 210); in particolare passano `cursor_tests::*`, `llm::tests::cancelled_is_neither_transient_nor_param_degradable_and_has_a_low_key_message`, `translate::tests::is_current_false_before_second_unit_cancels_without_extra_calls_and_keeps_prior_cache`. |
| 22 | Verificare che il flusso di traduzione con un solo documento aperto e un solo provider (nessuna navigazione rapida, uso "normale" e lento) non sia percettibilmente più lento rispetto a prima del ticket. | Tempo di traduzione per pagina paragonabile al baseline noto (~20-30s/paragrafo col CoT di gemma-4, da spec latenza) — lo slot locale non introduce overhead percepibile quando non c'è contesa (un solo job alla volta comunque, come già avveniva senza contesa). |

## Fuori Scope (non ri-testare in questo giro)

- Timeout applicativo per-richiesta verso il provider locale (competenza del ticket 13,
  `local-llm-provider`) — qui si assume che una singola chiamata HTTP non vada in timeout durante i test.
- Cancellazione HTTP "a metà" della richiesta già inviata al modello: il client è bloccante e la
  cancellazione avviene solo al confine di unità/finestra, mai interrompendo una chiamata già partita —
  già coperto/spiegato nello step 13, non serve un test dedicato più aggressivo.
- Policy di prefetch per provider cloud (invariate, nessuna modifica in questo diff).
- Packing a finestra fissa (`PACK_TARGET_TOKENS`, ticket 04) e cambio modello (L6, GemmaX2) — non toccati
  da questo diff, ticket separati nella stessa wayfinding spec.
- Cambiamenti al contratto frontend/`translation.ts` — il diff conferma "frontend non toccato"; non serve
  ri-testare `isCurrentRequest`/`isLatestNav` in profondità oltre a quanto già coperto per la conferma che
  continuano a scartare risultati stantii lato client (step 16).
