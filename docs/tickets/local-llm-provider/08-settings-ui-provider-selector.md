## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)
(design di dettaglio: [design-provider-abstraction.md](../../specs/design-provider-abstraction.md) §5)

## What to Build

Portare la selezione del provider nell'interfaccia impostazioni: un **selettore provider** in cima al
pannello, un campo **base-URL** e i campi **chiave/modello resi provider-scoped**. Dopo questo ticket
l'utente può scegliere OpenRouter o un provider locale (Unsloth/LM Studio/Ollama/llama-server), impostarne
base-URL, modello e chiave, e vederli salvati/ricaricati per-provider.

Per **decisione D5** ogni provider ha sempre una chiave: per i provider locali senza auth la UI **pre-compila
un placeholder di chiave fittizia** (es. `local`) così "funziona" senza costringere l'utente a inventarla,
mantenendo la guardia EC03 soddisfatta.

## Acceptance Criteria

- [ ] In `providerConfig.ts`: array `PROVIDERS` (id/label/base_url/default model + eventuale chiave dummy
      suggerita per i locali) e helper di risoluzione base-URL; `DEFAULT_MODEL`/`COMMON_MODELS`/`resolveModel`
      /`isValidKey`/`isCommonModel` restano.
- [ ] In `ProviderConfig.svelte`: `<select>` provider legato a `active_provider`; al cambio si caricano
      base-URL/modello/stato-chiave di quel provider.
- [ ] Campo **base-URL** (testo libero) mostrato per il provider attivo, precompilato col default/override.
- [ ] Campo **chiave** provider-aware ("API key {label}"); per i provider locali mostra il placeholder
      dummy e nessun warning "nessuna key"; la chiave è comunque salvata (D5).
- [ ] Campo **modello**: widget invariato; salva su `provider.<id>.model`; `COMMON_MODELS` per openrouter,
      testo libero per i locali.
- [ ] `load()`/`save()` usano i comandi provider-scoped (`active_provider`, `provider.<id>.base_url`,
      `provider.<id>.model`, chiave via comando con `provider_id`). I campi di lettura (lingua, prefetch,
      limite summary, cartella dati, svuota cache) **restano invariati e provider-independent**.
- [ ] `npm run check` (TypeScript/svelte-check) verde; test unit `providerConfig` verdi.

## Blocked By

- [07-provider-presets-and-active-provider.md](./07-provider-presets-and-active-provider.md)

## Frontier

Blocked by 07 (servono i comandi `get/set_active_provider`, `get_provider_config`, i preset e la chiave
provider-scoped). È l'ultimo pezzo perché un utente possa selezionare il provider senza toccare i settaggi
a mano.

## Step-by-Step Implementation Plan

1. **`providerConfig.ts`**: aggiungi `PROVIDERS` (specchio di `COMMON_MODELS`) con base-URL e chiave dummy
   suggerita per i locali; aggiungi `keyAcceptable`/resolver. Perché prima: la UI legge da qui. Verifica:
   unit test dei preset/resolver.
2. **`ProviderConfig.svelte` — selettore**: aggiungi il `<select>` provider in cima, legato a
   `active_provider`; on-change ricarica lo stato. Verifica: cambiando provider i campi si aggiornano.
3. **Campo base-URL**: aggiungi input testo precompilato col base-URL risolto del provider. Verifica: il
   valore si salva/ricarica per-provider.
4. **Chiave/modello provider-scoped**: adatta i widget esistenti; per i locali mostra il placeholder dummy;
   passa `provider_id` ai comandi chiave. Verifica: salvataggio/ricarica corretti per due provider diversi.
5. **`load()`/`save()`**: instrada tutto sui comandi provider-scoped del Ticket 07/06; lascia intatti i
   campi di lettura. Verifica: `npm run check` verde; giro manuale di salvataggio/riapertura.

Pitfall: non toccare le preferenze di lettura (sono provider-independent). Attenzione a non mostrare warning
"chiave mancante" per i provider locali (confonderebbe, dato il placeholder dummy).

## Testing Plan

- Unit (TS): preset resolution, `keyAcceptable`, mapping provider→default base-URL/model.
- `npm run check` (svelte-check) verde.
- Manuale: seleziona ciascun provider, imposta base-URL/modello/chiave, chiudi e riapri le impostazioni →
  i valori persistono per-provider; passando da openrouter a un locale e viceversa lo stato è corretto.

## Out of Scope

- Health-check / gestione irraggiungibilità e onboarding (Ticket 09).
- Validazione end-to-end contro server reale e tuning dei default (Ticket 10).
- Popolamento del modello via `/v1/models` (post-MVP).
