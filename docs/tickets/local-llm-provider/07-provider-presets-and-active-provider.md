## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)
(design di dettaglio: [design-provider-abstraction.md](../../specs/design-provider-abstraction.md) §1-4, §7, §8)

## What to Build

Introdurre il concetto di **provider selezionabile** nel core: un insieme di provider built-in (preset in
codice), la persistenza del provider attivo e delle sue override (`base_url`, `model`) nella **tabella
`settings` esistente**, e il cablaggio di `translate_page` perché costruisca il client dal provider attivo
(base-URL + modello + chiave provider-scoped dal Ticket 06).

Dopo questo ticket l'app può **effettivamente tradurre tramite un provider locale** se `active_provider`
è impostato di conseguenza (anche se l'unico modo di cambiarlo, per ora, è via `settings`/comando — la UI
arriva nel Ticket 08).

**Decisione D3**: il provider attivo di **default** è **locale (Unsloth)**, non openrouter. Gli utenti
esistenti con una chiave OpenRouter possono comunque selezionare openrouter (la cui `base_url`/`model` di
default resta quella odierna, con fallback al valore legacy `model`).

## Acceptance Criteria

- [ ] Struct `ProviderConfig { id, label, base_url, model }` e una **tabella di preset built-in** in
      `settings.rs`: `openrouter`, `unsloth`, `lmstudio`, `ollama`, `llamaserver` con le base-URL di default
      dalla ricerca (design §2 / research §Q3).
- [ ] Accessori: `get_active_provider()` (default = **unsloth**, D3), `provider_base_url_key(id)`,
      `provider_model_key(id)`, `get_provider_config(id)` che legge le override da `settings` e, per
      openrouter, **fa fallback al valore legacy `model`** quando `provider.openrouter.model` è assente.
- [ ] `active_provider` e `provider.<id>.{base_url,model}` persistono nella tabella `settings` (nessuna
      nuova tabella; `db.rs` invariato).
- [ ] `translate_page` risolve la `ProviderConfig` attiva, recupera la chiave via
      `secrets::get_api_key(active_id)` (Ticket 06) e costruisce `ChatCompletionsClient` con la sua
      `base_url`/chiave/`send_openrouter_headers = (id=="openrouter")` (Ticket 05).
- [ ] Nuovi comandi Tauri: `get_active_provider`, `set_active_provider`, `get_provider_config` /
      `list_providers`, registrati in `generate_handler!`.
- [ ] `cargo test` verde con test su default/override/fallback legacy.

## Blocked By

- [05-core-client-configurable-base-url.md](./05-core-client-configurable-base-url.md)
- [06-provider-scoped-keychain.md](./06-provider-scoped-keychain.md)

## Frontier

Blocked by 05 (il client deve accettare `base_url`) e 06 (chiave per-provider). È il cuore dell'astrazione:
una volta chiuso, l'app è provider-agnostica end-to-end nel core.

## Step-by-Step Implementation Plan

1. **Definisci `ProviderConfig` + tabella preset** in `settings.rs` (id/label/base_url/model di default per
   i 5 provider). Perché prima: è il modello dati su cui poggia tutto il resto. Verifica: unit test che
   elenca i preset attesi.
2. **Aggiungi gli accessori** `get_active_provider` (default unsloth), `get_provider_config(id)` con lettura
   override da `settings` e fallback legacy `model` per openrouter. Verifica: test default + override +
   fallback.
3. **Aggiungi i comandi Tauri** (`get/set_active_provider`, `get_provider_config`/`list_providers`) e
   registrali. Verifica: `cargo build`; chiamata dai devtools/temporanea ritorna i preset.
4. **Cabla `translate_page`**: risolvi config attiva → chiave (Ticket 06) → costruisci il client (Ticket
   05) con la base-URL del provider. Perché ora: chiude il percorso di traduzione provider-agnostico.
   Verifica: impostando manualmente `active_provider=openrouter` la traduzione funziona come prima;
   impostando `unsloth` con base-URL/chiave corrette, la richiesta parte verso il server locale.
5. **Test**: default provider, round-trip override base_url/model, fallback legacy model per openrouter.
   Verifica: `cargo test` verde.

Pitfall: il default D3 (unsloth) implica che, senza server, la traduzione fallirà — è atteso e gestito dal
Ticket 09 (health-check/errore chiaro). Non implementare qui il fallback al cloud (vietato da D4).
Non rompere il fallback legacy `model`: gli utenti openrouter esistenti devono mantenere il modello scelto.

## Testing Plan

- Unit (Rust): preset table; `get_active_provider` default; `get_provider_config` default+override+fallback
  legacy; risoluzione base-URL usata da `translate_page` (via mock client dove possibile).
- I test di `translate.rs` (MockClient) restano verdi; se `translate_page` cambia firma interna, adattare
  il minimo.
- Manuale: impostare a mano `active_provider` e verificare che la richiesta vada all'endpoint giusto.

## Out of Scope

- UI del selettore provider (Ticket 08).
- Health-check / gestione irraggiungibilità (Ticket 09).
- Validazione end-to-end contro server reale (Ticket 10).
