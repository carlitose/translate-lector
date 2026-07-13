# Decision Brief — Ticket 03 (Grilling / Human Gate)

## Type

Decision brief (AFK deliverable for a human-gated ticket)

## Status

**DECISO (2026-07-13).** L'utente ha confermato tutti i default consigliati.

### Decisioni prese
- **D1 = Pagina discreta.** ⇒ `sessions.scroll_position` è ridondante nell'MVP (mantenuto nello schema ma **non usato**, disponibile per un futuro passaggio all'ibrido).
- **D2 = Hash parziale + dimensione** (SHA-256 primi+ultimi KB + size).
- **D3 = Tauri v2 + Svelte 5 + rusqlite** (confermato; è ciò che è già scaffoldato).
- **D4 = lista di 15 lingue** proposta sotto, adottata come default (campo libero per le altre; modificabile).
- **D5 = tutti i default consigliati** (modello `anthropic/claude-sonnet-5`, lingua Italiano, prefetch ON, limite summary ~800-1000 token, cartella `%APPDATA%/translate-lector`).

Ripiegato in [translate-lector-wayfinder.md](./translate-lector-wayfinder.md) → "Decisions So Far".

Related: [wayfinder spec](./translate-lector-wayfinder.md) · [SPECIFICATION.md](../../SPECIFICATION.md) (§3.1 UI, §4.3 schema)

---

## D1 — Modello di lettura: pagina vs scroll continuo (nodo principale)

**Tensione**: la UI (§3.1) e il motore di traduzione (§3.2) sono **per-pagina**; la cache e il percettore lavorano per pagina; ma `sessions.scroll_position REAL` (§4.3) implica **scroll continuo**.

| Opzione | Pro | Contro |
|---------|-----|--------|
| **A. Pagina discreta** (◀ Pag. N ▶, una pagina per volta) | Allineata a cache/percettore/prefetch così come specificati; "pagina corrente" è banale; ripristino semplice (solo `current_page`) | Lettura meno fluida di un lettore moderno |
| B. Scroll continuo (tutte le pagine in colonna) | UX da lettore moderno | "Pagina corrente" ambigua (quale pagina traduco?); cache/percettore per-pagina diventano complessi; prefetch poco chiaro; ripristino richiede `scroll_position` preciso |
| C. Ibrido (scroll dentro la pagina, navigazione discreta tra pagine) | Fluido ma con unità-pagina chiara; `scroll_position` = offset dentro la pagina | Un po' più di stato da gestire |

**Raccomandazione: A (pagina discreta) per l'MVP.** È l'unica coerente al 100% con cache/percettore/prefetch già specificati e semplifica il ripristino. In tal caso `sessions.scroll_position` diventa ridondante nell'MVP → si può **rimuovere** dallo schema, oppure conservarlo per un futuro passaggio all'ibrido (C). Se preferisci la sensazione di scroll, C è il compromesso migliore e mantiene `scroll_position` come offset intra-pagina.

**Impatto schema**: se A → `scroll_position` opzionale/rimosso. Se C → resta, semantica = offset nella pagina corrente. Se B → serve rivedere l'unità di cache/percettore (sconsigliato per MVP).

---

## D2 — Strategia di hashing del file (EC06)

Serve a riconoscere un PDF anche se spostato/rinominato.

| Opzione | Pro | Contro |
|---------|-----|--------|
| **A. Hash parziale** (SHA-256 dei primi+ultimi N KB + dimensione file) | Velocissimo anche su PDF di centinaia di MB; sufficiente per uso personale | Collisione teorica trascurabile in pratica |
| B. Hash completo del file | Robusto | Lento all'apertura di file grandi (blocca o allunga l'avvio) |
| C. size + mtime | Istantaneo | Fragile: mtime cambia con copie/sync, non riconosce il file spostato |

**Raccomandazione: A (hash parziale + size).** Ottimo compromesso per uso personale; l'apertura resta istantanea. Documentare che è euristico.

---

## D3 — Versioni dello stack

| Componente | Opzioni | Raccomandazione |
|-----------|---------|-----------------|
| Tauri | v1 / **v2** | **v2** — attuale, plugin keychain/stronghold moderni, mobile-ready se mai servisse |
| Svelte | 4 / **5 (runes)** | **5** — stabile, runes semplificano lo stato reattivo del pannello traduzione/glossario |
| SQLite in Rust | **`rusqlite` (bundled)** / `sqlx` | **`rusqlite`** con feature `bundled` — sincrono, semplice, nessuna dipendenza SQLite di sistema su Windows; `sqlx` (async) è sovradimensionato per un DB locale single-user |

**Raccomandazione: Tauri v2 + Svelte 5 + rusqlite(bundled).** (È l'assunzione con cui il ticket 05 sta già scaffoldando; conferma o correggi.)

---

## D4 — Lista curata delle lingue di destinazione (~10-15)

Proposta (elenco curato + campo libero per qualsiasi altra, come da §3.4):

Italiano, Inglese, Spagnolo, Francese, Tedesco, Portoghese, Olandese, Polacco, Russo, Turco, Arabo, Cinese (semplificato), Giapponese, Coreano, Hindi.

**Raccomandazione**: usare questa lista (15). Rimuovi/aggiungi a piacere. La lingua di **origine** resta auto-rilevata.

---

## D5 — Default delle impostazioni (§3.5)

| Impostazione | Raccomandazione default | Note |
|--------------|-------------------------|------|
| Modello OpenRouter di default | `anthropic/claude-sonnet-5` | Ottima qualità di traduzione + supporto structured output; l'utente può cambiarlo. (Confermare col ticket 02) |
| Lingua di destinazione predefinita | Italiano | La tua lingua |
| Prefetch pagina successiva | **On** | Migliora la fluidità; disattivabile per risparmio costi |
| Limite rolling summary | ~800–1000 token (~4–6 KB) | Sopra il limite → compressione (EC05). Valore da confermare col tokenizer scelto in ticket 02 |
| Cartella dati | App-data OS di default (`%APPDATA%/translate-lector`) | Con opzione per cambiarla |

---

## Domande nette da confermare (checklist per il ritorno)

1. **D1**: pagina discreta (A), ibrido (C), o scroll continuo (B)? → decide se `scroll_position` resta nello schema.
2. **D2**: hash parziale (A) ok?
3. **D3**: confermi Tauri v2 + Svelte 5 + rusqlite?
4. **D4**: la lista di 15 lingue va bene? Aggiunte/rimozioni?
5. **D5**: confermi i default (modello, lingua, prefetch, limite summary, cartella)?

Finché non rispondi, questo ticket resta **blocked: awaiting human gate decision**; i default sopra permettono comunque a 05 (scaffold) di procedere senza rilavorazioni sostanziali.
