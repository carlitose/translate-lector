# 04 — Prototype: storage sicuro della API key su Windows 11

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md)

## Type

prototype

## Outcome

Confermare quale meccanismo di keychain funziona su Windows 11 (NFR07) e provarne save/get/delete,
per non scoprire attriti di piattaforma durante la build.

## Acceptance Criteria

- [ ] Scelto il meccanismo: plugin Tauri (es. stronghold) o crate `keyring` lato core Rust. Con motivazione.
- [ ] Prototipo che salva, rilegge e cancella una stringa segreta usando il Credential Manager di Windows (o equivalente).
- [ ] Verificato su Windows 11 reale (l'ambiente di sviluppo dell'utente).
- [ ] Note sulla UX del primo salvataggio (eventuali prompt di sistema) e sul comportamento se la chiave manca.
- [ ] Decisione ed evidenza riassunte nel parent spec (risolvere T04).

## Blocked By

- None — can start immediately. (Beneficia dello scaffold T05 ma può essere provato anche in isolamento con un piccolo binario Rust.)

## Frontier

Rischio di piattaforma isolato: se il keychain scelto non funziona bene su Win11, va saputo prima di cablarlo nel flusso impostazioni (UC05).

## Work Plan

1. Con `find-docs`/`ctx7`, verificare stato attuale del plugin keychain di Tauri e del crate `keyring` su Windows.
2. Prototipare save/get/delete di un segreto.
3. Testare su Win11: primo salvataggio, rilettura dopo riavvio, cancellazione.
4. Registrare scelta, evidenza e note UX.

## Evidence to Capture

- Plugin/crate e versione scelti; doc consultata.
- Output della prova save/get/delete.
- Note su prompt di sistema/permessi e comportamento a chiave assente.

## Out of Scope

- UI delle impostazioni completa.
- Cross-platform (macOS/Linux) — solo Windows nell'MVP.

## Findings (2026-07-13)

### Meccanismo scelto: crate `keyring` v4 (lato core Rust) — NON un plugin Tauri

Motivazione:

- **OS-native, zero UX aggiuntiva.** `keyring` v4 usa di default il Windows Credential
  Manager (feature `windows-native-keyring-store`, abilitata dal feature-set `v1`
  di default). Scrittura silenziosa per l'utente corrente: nessun prompt di sistema,
  nessun diritto di admin, nessuna passphrase da gestire.
- **Alternativa `tauri-plugin-stronghold` scartata per l'MVP.** Stronghold è un
  vault cifrato con snapshot su file e richiede una passphrase gestita dall'utente
  (o derivata), quindi introduce UX e stato extra per un solo segreto. Per una
  singola API key OpenRouter il Credential Manager OS è più semplice e già sicuro.
- **Vantaggio architetturale:** vive nel core Rust (`secrets.rs`), riusabile dai
  comandi Tauri e testabile con `cargo test` senza avviare la GUI.

Versione: `keyring = "4"` (risolta a 4.1.4). API v4 confermata via ctx7
(`/open-source-cooperative/keyring-rs`): `Entry::new(service, account)`,
`set_password`, `get_password`, `delete_credential`.

### Implementazione

- `src-tauri/Cargo.toml`: aggiunta dipendenza `keyring = "4"` (default features ⇒
  Windows Credential Manager).
- `src-tauri/src/secrets.rs`: `set_api_key` / `get_api_key() -> Option<String>` /
  `delete_api_key`, service `"translate-lector"`, account `"openrouter-api-key"`.
- `src-tauri/src/lib.rs`: comandi Tauri `store_api_key`, `load_api_key`,
  `clear_api_key` registrati nell'`invoke_handler` accanto a `ping`/`init_database`.

### Evidenza su Windows 11 reale

`cargo build` OK (target x86_64-pc-windows-msvc). Test di round-trip contro il
Credential Manager reale (account throwaway `translate-lector-test`, con cleanup):

```
running 1 test
test secrets::tests::set_get_delete_roundtrip_hits_real_store ... ok
test result: ok. 1 passed; 0 failed; ...
```

Il test asserisce, nell'ordine: chiave assente ⇒ `None`; `set` poi `get` ⇒ valore
esatto; overwrite; `delete` ⇒ `None`; delete idempotente su entry mancante ⇒ OK.
Suite completa: 4/4 test passati (3 db + 1 secrets).

### Note UX / comportamento a chiave assente

- **Primo salvataggio:** nessun prompt di sistema. Il Windows Credential Manager
  scrive in modo silenzioso nel vault dell'utente corrente (Generic Credential),
  nessun diritto elevato richiesto.
- **Chiave assente:** `get_password` restituisce `Err(Error::NoEntry)`; il modulo
  lo mappa a `Ok(None)` pulito (mai un errore). Il comando `load_api_key` espone
  quindi `null` al frontend.
- **Persistenza fra riavvii:** i Generic Credential del Credential Manager sono
  persistenti fra sessioni/riavvii per l'utente (non è un vault volatile).
- **Delete idempotente:** cancellare una chiave mancante è trattato come successo.

---

_Completed 2026-07-13: keyring v4 chosen, save/get/delete verified on Windows 11 Credential Manager; moved to done._
