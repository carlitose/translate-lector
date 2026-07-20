# 02 — Validare il flusso PDF combinato in Tauri su macOS

## Parent Spec

[pdfjs-wkwebview-text-extraction-diagnosis.md](../../specs/pdfjs-wkwebview-text-extraction-diagnosis.md)

## Type

HITL

## What to Build

Produrre evidenza QA sul runtime reale macOS WKWebView dopo il ticket 01. La verifica deve
dimostrare end-to-end che un PDF testuale si apre senza l'errore `readableStream` e che la
compatibilità ripristinata convive con selettore diretto, rendering, traduzione e guardia
EC01. Salvare i risultati in un breve report sotto `docs/qa/`.

Questa slice copre il passo 5 di **Implementation Plan** e il gate manuale di **Testing
Decisions** della parent spec.

## Acceptance Criteria

- [ ] Una build Tauri del branch contenente il ticket 01 viene avviata su macOS/WKWebView.
- [ ] Un fixture PDF testuale e un PDF reale si aprono senza `undefined is not a function`
      e mostrano canvas e traduzione coerenti con la pagina corrente.
- [ ] Il salto diretto copre prima pagina, ultima pagina e un salto lungo; frecce e input
      restano sincronizzati e la navigazione rapida non lascia canvas o testo stale.
- [ ] Dopo il riavvio, la sessione ripristina la pagina raggiunta direttamente.
- [ ] Un PDF senza testo estraibile mostra `formato non supportato (no OCR)` invece di un
      errore generico.
- [ ] Il report QA registra versione/commit provato, fixture, passi, risultati osservati ed
      eventuali screenshot o log; nessun passo non eseguito è dichiarato superato.

## Blocked By

- [01-restore-wkwebview-text-extraction.md](./01-restore-wkwebview-text-extraction.md)

## Frontier

Blocked by ticket 01 and by human access to a macOS GUI session. Questa verifica non può
essere sostituita da Node/Vitest perché il difetto dipende dalle capability del WKWebView
incorporato.

## Step-by-Step Implementation Plan

1. Confermare che il branch provato include il ticket 01 e annotare commit e versione
   macOS/Tauri nel report QA.
2. Avviare l'app con il normale comando Tauri e aprire prima il fixture testuale usato
   dalla diagnosi; verificare assenza dell'errore e contenuto visibile.
3. Ripetere con un PDF reale multipagina. Eseguire salti diretti, frecce, navigazione
   rapida e riavvio, confrontando sempre numero, canvas e traduzione.
4. Aprire un PDF immagine/senza testo e verificare il messaggio EC01 previsto.
5. Registrare ogni passo come pass/fail/skipped con evidenza osservabile. In caso di fail,
   lasciare questo ticket aperto e creare una diagnosi o un follow-up mirato; non correggere
   il codice dentro il ticket QA.

## Testing Plan

- Test manuale end-to-end su macOS WKWebView con almeno due PDF testuali e un PDF senza
  testo.
- Conservare nel report i risultati automatici del ticket 01 come prerequisito, senza
  presentarli come prova del runtime WebKit.
- Verifica Windows/WebView2 consigliata se disponibile, ma non bloccante per la conferma
  del fix macOS.

## Out of Scope

- Implementazione di fix, modifica dei test automatici, OCR e decisioni sul guardrail di
  processo.
