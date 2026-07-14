# Decision brief — Strategia traduzione contesto piccolo (grilling STC-05)

**Ticket:** `docs/tickets/small-context-translation/05-grilling-strategy-decisions.md`
**Parent:** `docs/specs/small-context-translation-wayfinder.md`
**Data:** 2026-07-14
**Stato:** ✅ RISOLTO — D1-D6 confermate dall'utente il 2026-07-14 (tutte le raccomandazioni accettate):
D1 **paragrafo** (fallback frase) · D2 **condizionale sul budget** (cloud degrada a pagina intera) ·
D3 **latenza accettabile** con cache+prefetch · D4 **match bilanciato** (word-boundary + morfologia ultima
parola, cap unlocked 10-20, locked uncapped) · D5 **split contratto** (translate-only + perceptor per pagina) ·
D6 **update percettore una volta per pagina**.

| D | Domanda | Raccomandazione (da confermare) |
|---|---------|-------------------------------|
| **D1** Granularità unità | paragrafo / frase / finestra N-token | **Paragrafo** con fallback frase (STC-02: paragrafi reali ~40-90 token ≪ budget). |
| **D2** Default vs condizionale | sempre, o solo provider a contesto piccolo | **Condizionale sul budget**: attiva chunked/selettivo quando `n_ctx` è piccolo (locale); cloud può restare pagina-intera. Un solo percorso budget-aware che degrada a "1 unità = 1 pagina" quando il budget è ampio è accettabile. |
| **D3** Latenza (N chiamate) | accettabile su locale lento? soglia | **Accettabile** con cache per-unità + prefetch; il guadagno è robustezza (no EC08/timeout). Soglia da fissare (es. avviso se >N unità/pagina). |
| **D4** Match glossario severità/cap | quanto aggressivo, quanti unlocked | **Word-boundary + morfologia ultima parola**; cap unlocked **10-20**, **locked uncapped** (STC-03). Ridurre falsi negativi con indice lemma/alias in una build successiva. |
| **D5** Split contratto | translate-only + perceptor-update separati? | **Sì** (STC-04): chiamate di traduzione minime + 1 percettore per pagina → prompt piccoli. |
| **D6** Update summary/glossario | per-pagina o per-unità | **Per-pagina** (una chiamata percettore dopo le unità); evita di moltiplicare le chiamate pesanti. |

**Nota di build (non decisione umana):** perché emergano vere unità-paragrafo, `pdfExtract.linesToText`
(`src/lib/pdfExtract.ts`) dovrà emettere separatori di paragrafo (da y-gap tra righe); oggi usa un singolo
`\n`. Da includere nei ticket di build.

## Come procedere
1. L'utente conferma/aggiusta D1-D6.
2. Ripiegare in "Decisions So Far" della mappa.
3. `to-tickets` per le build verticali (budget → chunking+separatori paragrafo → select_glossary →
   translate-only per unità + perceptor-update per pagina → cache per-unità → reassemble).
