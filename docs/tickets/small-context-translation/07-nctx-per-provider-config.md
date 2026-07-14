## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)

## What to Build

Rendere il **context window (`n_ctx`) configurabile per-provider**, così la formula di budget (STC-01) sa
quanto spazio ha il modello attivo. Riusa il pattern di `ProviderConfig`/settings (Ticket 07 dell'epica
provider) già usato per `base_url`/`model`/`max_tokens`.

## Acceptance Criteria

- [ ] `ProviderConfig` espone `n_ctx` (o `context_window`), risolto da `get_provider_config` con override
      da settings (`provider.<id>.n_ctx`) e default sensati (locale 4096; cloud grande, es. molto alto così
      il budget non vincola).
- [ ] Il valore è editabile dal pannello impostazioni (campo per il provider attivo), come base-URL/modello.
- [ ] (Opzionale) probe del server per leggere il context reale (`/props`/`/v1/models`) con fallback al
      valore configurato; se complesso, rimandare e lasciare solo il setting manuale.
- [ ] `cargo test` verde (default/override di `n_ctx`); `npm run check` + `vitest` verdi se si tocca la UI.

## Blocked By

- None - can start immediately (parallelo al Ticket 06).

## Frontier

Ready. Fornisce l'input mancante alla formula di budget del Ticket 08; indipendente dal paragrafo (06).

## Step-by-Step Implementation Plan

1. Aggiungere il campo `n_ctx` a `ProviderConfig` (`src-tauri/src/settings.rs`) e ai preset (locale 4096,
   cloud alto); helper `provider_nctx_key(id)`; lettura override in `get_provider_config`. Perché prima: è il
   dato che il budget consuma. Verifica: test default+override.
2. Esporre `n_ctx` nel pannello impostazioni (`src/lib/ProviderConfig.svelte` + `providerConfig.ts` preset),
   provider-scoped come gli altri campi. Verifica: salva/ricarica per-provider.
3. (Opzionale) probe server per il context reale con fallback. Verifica: se il server risponde, il campo si
   pre-popola; altrimenti resta il default/override.

Pitfall: non far dipendere il budget da un `n_ctx` assente → default robusto. Coerenza con `max_tokens`
per-provider già presente.

## Testing Plan

- Unit (Rust): `get_provider_config` con/ senza override `n_ctx`.
- `npm run check` + `vitest` per la UI.
- Manuale: impostare `n_ctx` per il provider locale e verificarne la persistenza.

## Out of Scope

- Uso del budget nel loop di traduzione (Ticket 08).
