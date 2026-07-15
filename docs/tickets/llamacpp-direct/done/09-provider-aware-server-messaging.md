# 09 — Messaggistica "server locale" provider-aware

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## What to Build

Il messaggio mostrato quando il server locale non risponde è ormai stale per il provider
app-managed. Attualmente dice, sia nel banner frontend (`LOCAL_UNREACHABLE_HINT` in
`src/lib/providerConfig.ts`) sia nell'errore backend `Unreachable` (`src/llm.rs`):
*"Server locale non raggiungibile. Avvia il server (es. Unsloth Studio) o verifica l'indirizzo in
⚙️."*

Con la build attuale (default `llamaserver`, ticket 04/06) è **l'app** ad avviare llama-server: dire
all'utente di avviarlo a mano — e citare Unsloth Studio — è fuorviante per quel provider. Ma il
messaggio è generico per **tutti** i provider locali: per quelli **user-launched** (`unsloth`,
`lmstudio`, `ollama`) il testo attuale è ancora **corretto**. Quindi la soluzione è rendere la
messaggistica **consapevole del provider**, non un semplice replace:

- Provider **app-managed** (`llamaserver`): messaggio tipo *"Il server locale si sta avviando,
  attendi…"* / *"Il server locale non è ancora pronto"* — nessun invito ad avviarlo a mano, nessun
  riferimento a Unsloth Studio.
- Provider **user-launched** (unsloth/lmstudio/ollama): resta l'invito ad avviare il server a mano
  (già corretto).

## Acceptance Criteria

- [ ] Quando il provider selezionato è quello app-managed (`llamaserver`), il banner/errore di
      "server non raggiungibile" NON dice più "Avvia il server (es. Unsloth Studio)"; comunica invece
      che il server locale si sta avviando / non è ancora pronto (e rimanda a ⚙️ per i path).
- [ ] Quando il provider è user-launched (unsloth/lmstudio/ollama), il messaggio resta l'attuale
      invito ad avviarlo a mano.
- [ ] La distinzione app-managed vs user-launched è coerente con come il resto del codice riconosce
      il provider app-managed (allineata a `is_managed_local_provider`/preset `llamaserver`), senza
      introdurre una terza fonte di verità.
- [ ] Aggiornati entrambi i punti: banner frontend (`providerConfig.ts` + relativo test
      `providerConfig.test.ts`) ed errore backend `Unreachable` (`llm.rs` + relativo test).
- [ ] Suite verde (Rust + vitest + svelte-check).

## Blocked By

- None - can start immediately.

## Frontier

Ready now. È la controparte "messaggistica" del ticket 08: anche col fix di readiness, il banner può
comparire (server davvero giù, o provider user-launched non avviato), quindi il testo deve essere
corretto per il provider in uso. Indipendente dal ticket 08 (nessun edge).

## Step-by-Step Implementation Plan

1. **Individuare come il frontend conosce il provider attivo** al momento del banner (il componente
   legge già `get_active_provider`; `LOCAL_UNREACHABLE_HINT` è una costante statica in
   `providerConfig.ts`). Perché prima: decide se la scelta del messaggio avviene lato frontend
   (in base all'id provider) o lato backend (nel costruire l'errore). Preferire il punto che già
   conosce l'id del provider senza doverlo propagare.
2. **Definire i due testi** (app-managed vs user-launched) come costanti/funzione che prende l'id
   del provider e ritorna il messaggio giusto. Riusare il criterio "app-managed" esistente
   (`llamaserver` / `is_managed_local_provider`) — non reinventare la classificazione. Superficie:
   `providerConfig.ts` lato UI; `llm.rs` lato errore `Unreachable` (che già ha `base_url`, da cui si
   può derivare se è il provider gestito, o ricevere l'id).
3. **Aggiornare il banner frontend** perché scelga il testo in base al provider attivo. Verificare
   con `providerConfig.test.ts` (aggiornare l'asserzione che oggi controlla "Server locale non
   raggiungibile" così da coprire entrambi i rami).
4. **Aggiornare l'errore backend `Unreachable`** (`llm.rs:~398`) allo stesso criterio, con il suo
   test (`llm.rs:~2328`). Verificare che il messaggio per un base_url del provider gestito non citi
   più Unsloth Studio, mentre per gli altri resti invariato.
5. **Coerenza**: assicurarsi che i due testi (UI e backend) siano allineati e non divergano; se
   possibile, un'unica fonte per la parte condivisa.

## Testing Plan

- Unit/vitest: `providerConfig.test.ts` copre entrambi i rami (app-managed vs user-launched).
- Unit Rust: il test dell'errore `Unreachable` verifica il testo giusto per un base_url gestito e per
  uno user-launched.
- svelte-check + suite Rust verdi; nessuna regressione dei test esistenti.
- Manuale (facoltativo): cold start col provider `llamaserver` → durante l'avvio il banner mostra il
  nuovo testo, non l'invito a Unsloth Studio.

## Out of Scope

- Logica di readiness / retry sul 503 "Loading model" (ticket 08).
- Rimozione o deprecazione del provider `unsloth` (D3: resta invariato).
- Modifiche ai messaggi degli errori non-`Unreachable`.
