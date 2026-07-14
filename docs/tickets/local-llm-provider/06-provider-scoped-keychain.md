## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)
(design di dettaglio: [design-provider-abstraction.md](../../specs/design-provider-abstraction.md) §3b, §8)

## What to Build

Consentire di memorizzare **una chiave API per provider** nel keychain di sistema, invece dell'unica chiave
odierna. Ogni provider (openrouter, unsloth, lmstudio, ollama, llama-server) ha il suo account nel keychain
secondo lo schema `"{provider_id}-api-key"`. Lo schema è scelto in modo che `openrouter` →
`openrouter-api-key`, che è **esattamente** l'account usato oggi: **zero migrazione** per gli utenti esistenti.

Per **decisione D5** ogni provider ha sempre una chiave (anche fittizia per i server locali senza auth),
quindi il keychain è il posto giusto per tutte.

## Acceptance Criteria

- [ ] Nuova funzione `account_for(provider_id) -> String` che ritorna `"{provider_id}-api-key"`.
- [ ] Le quattro funzioni pubbliche/comandi di `secrets.rs` (`store`/`load`/`clear`/`has` api key)
      accettano un `provider_id` e usano `account_for(provider_id)` come account (service invariato:
      `translate-lector`).
- [ ] `account_for("openrouter") == "openrouter-api-key"` (l'account odierno) → una chiave OpenRouter già
      salvata viene ritrovata senza migrazione.
- [ ] Gli internal parametrizzati `*_api_key_for(service, account, …)` restano invariati.
- [ ] `cargo test` verde, inclusa una asserzione di back-compat su `account_for("openrouter")`.

## Blocked By

- None - can start immediately. (Parallelo al Ticket 05; indipendente dal client.)

## Frontier

Ready now. Non dipende dal Ticket 05 né da un server locale. Sblocca il Ticket 07 (che deve recuperare la
chiave del provider attivo) e il Ticket 08 (UI che salva chiavi per-provider).

## Step-by-Step Implementation Plan

1. **Aggiungi `account_for(provider_id)`** in `src-tauri/src/secrets.rs`. Perché prima: è la primitiva che
   tutte le funzioni useranno. Verifica: unit test `account_for("openrouter") == "openrouter-api-key"`.
2. **Aggiungi il parametro `provider_id`** alle quattro funzioni pubbliche (store/load/clear/has), passando
   `account_for(provider_id)` agli internal `*_for`. Perché ora: cambia solo la superficie pubblica, gli
   internal restano testati. Verifica: i test esistenti (che usano gli `*_for` con account throwaway)
   restano verdi.
3. **Aggiorna i comandi Tauri** corrispondenti in `lib.rs` per accettare `provider_id` (verranno cablati
   dai Ticket 07/08). Perché ora: mantiene la firma coerente. Verifica: `cargo build`.
4. **Test di back-compat**: salva con `provider_id="openrouter"`, verifica che l'account sia
   `openrouter-api-key`. Verifica: `cargo test` verde.

Pitfall: non cambiare il `service` name (`translate-lector`) — cambierebbe la posizione di tutte le chiavi
esistenti. Solo l'`account` diventa provider-scoped.

## Testing Plan

- Unit (Rust): `account_for` mapping; round-trip store/load/clear per un provider_id fittizio con account
  throwaway (come i test esistenti); asserzione back-compat openrouter.
- I test esistenti di `secrets.rs` devono restare verdi senza modifiche sostanziali.

## Out of Scope

- Scelta del provider attivo e recupero della chiave giusta in `translate_page` (Ticket 07).
- UI di inserimento chiave per-provider (Ticket 08).
- Migrazione dati (non necessaria per design §8).
