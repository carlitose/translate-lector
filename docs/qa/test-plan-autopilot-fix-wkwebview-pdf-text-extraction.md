# Test Plan — Estrazione testo PDF su macOS WKWebView

Branch: `autopilot/fix-wkwebview-pdf-text-extraction`
Diff analizzato: working tree corrente rispetto a `HEAD`; escluso il diff preesistente e non correlato di `package-lock.json`.

## Scope

- La modifica sostituisce, per le pagine PDF ordinarie, `getTextContent()` con il consumo di `streamTextContent()` tramite reader esplicito; il ramo `isPureXfa` continua invece a usare `getTextContent()`.
- La verifica conferma su macOS/WKWebView che apertura, guardia EC01, rendering, navigazione e prefetch lavorino senza il precedente `TypeError`, mantenendo XFA e WebView2 funzionanti.
- L'app non mostra il testo sorgente ricostruito direttamente: il suo arrivo corretto è verificato congiuntamente da superamento della guardia EC01, canvas renderizzato e traduzione coerente con la pagina.

## Prerequisites

- macOS con toolchain del progetto installata; avviare l'app desktop dalla root con `npm run tauri dev` e tenere visibili terminale e Console del Web Inspector, se disponibile.
- Provider di traduzione raggiungibile e configurato in **⚙️**, con log delle richieste osservabili; usare dati/cache QA isolati oppure una combinazione documento-lingua mai tradotta.
- Prefetch abilitato in **⚙️**.
- Fixture testuali versionate:
  - `prototypes/pdfjs/fixtures/a-single-column.pdf` per l'apertura iniziale;
  - `prototypes/pdfjs/fixtures/c-header-footer.pdf` per navigazione multipagina.
- Un PDF reale testuale di almeno 3 pagine, con contenuto visivamente distinguibile in ogni pagina.
- Un PDF image-only/scansione di almeno 1 pagina, verificato senza livello di testo/OCR nascosto.
- Un fixture **pure XFA** noto, con almeno una stringa univoca leggibile; registrarne il comportamento baseline atteso e assicurarsi che PDF.js lo identifichi con `page.isPureXfa === true`.
- Per il follow-up, una macchina Windows con WebView2 e gli stessi quattro tipi di documento disponibili.

## Happy Path

1. [ ] **Azione:** Avviare `npm run tauri dev` su macOS e attendere la finestra Tauri.
   **Risultato atteso:** l'app si apre in WKWebView, mostra **Apri PDF** e non registra errori PDF.js in terminale o Console.

2. [ ] **Azione:** Fare clic su **Apri PDF** e selezionare `prototypes/pdfjs/fixtures/a-single-column.pdf`.
   **Risultato atteso:** scompare **Caricamento…**, compare il canvas della pagina e il footer indica `Pag. 1 / 1`; non compaiono né `formato non supportato (no OCR)` né `Errore nell'apertura del PDF: TypeError: undefined is not a function`.

3. [ ] **Azione:** Attendere la traduzione della fixture e confrontarla con i contenuti visibili nel canvas (`The History of Translation`, paragrafi sulla traduzione).
   **Risultato atteso:** il lato destro passa da **Traduzione in corso…** a testo non vuoto semanticamente coerente e il footer mostra `● Tradotto` o `● Tradotto (cache)`; ciò conferma che un testo ricostruito non vuoto è arrivato al flusso di traduzione.

4. [ ] **Azione:** Aprire `prototypes/pdfjs/fixtures/c-header-footer.pdf`.
   **Risultato atteso:** il footer indica più pagine, il canvas mostra la prima pagina e la traduzione contiene contenuto della pagina 1; nessun `TypeError` appare in UI o Console.

5. [ ] **Azione:** Fare clic su **▶** per passare alla pagina 2, quindi su **◀** per tornare alla pagina 1.
   **Risultato atteso:** indicatore, canvas e traduzione seguono sempre la pagina corrente; i pulsanti sono disabilitati solo ai limiti e nessuna pagina mostra testo appartenente all'altra.

6. [ ] **Azione:** Navigare rapidamente `1 → 2 → 1 → 2`, senza attendere il completamento intermedio.
   **Risultato atteso:** l'app resta reattiva e si stabilizza su `Pag. 2 / N` con canvas e traduzione della pagina 2; nessun errore di stream, testo stantio o duplicato.

7. [ ] **Azione:** Con cache QA fredda e prefetch abilitato, tornare a pagina 1 e attendere `● Tradotto`; osservare poi nei log del provider il completamento della richiesta automatica per pagina 2 prima di premere **▶**.
   **Risultato atteso:** il prefetch di pagina 2 viene eseguito senza `TypeError` e senza errore visibile nell'app.

8. [ ] **Azione:** Dopo il completamento del prefetch, premere **▶** una volta.
   **Risultato atteso:** pagina 2 mostra `● Tradotto (cache)` senza una nuova richiesta di traduzione equivalente nei log; canvas e testo restano corretti, confermando l'estrazione nel percorso di prefetch.

9. [ ] **Azione:** Selezionare con **Apri PDF** il PDF reale testuale di almeno 3 pagine.
   **Risultato atteso:** titolo, numero totale di pagine e canvas corrispondono al documento; la prima pagina viene tradotta senza il precedente errore WKWebView.

10. [ ] **Azione:** Visitare in sequenza le prime tre pagine del PDF reale e confrontare, per ciascuna, una frase/argomento visibile nel canvas con il testo tradotto.
    **Risultato atteso:** ogni pagina produce una traduzione non vuota e coerente, l'indicatore avanza correttamente e Console/UI restano prive del `TypeError` relativo a `readableStream`.

## Edge Cases

11. [ ] **Azione:** Aprire il fixture pure XFA e attendere il completamento del caricamento.
    **Risultato atteso:** il documento non viene respinto da EC01 e non mostra un errore generico; pagina/form resta visualizzabile secondo il baseline registrato e il testo XFA univoco raggiunge la traduzione.

12. [ ] **Azione:** Navigare avanti e indietro nel fixture pure XFA, se multipagina, oppure riaprirlo da **Recenti** se è a pagina singola.
    **Risultato atteso:** il comportamento resta identico al baseline; nessun tentativo visibile di passare il ramo XFA attraverso lo stream ordinario e nessuna regressione di apertura/estrazione.

13. [ ] **Azione:** Riaprire il PDF reale tramite la voce **Recenti**, poi chiudere e riavviare l'app per attivare il ripristino dell'ultima sessione.
    **Risultato atteso:** entrambi i percorsi ricaricano il documento e la pagina salvata con canvas e traduzione corretti, senza `TypeError` o falso EC01.

## Negative / Error Paths

14. [ ] **Azione:** Fare clic su **Apri PDF** e annullare il selettore senza scegliere un file.
    **Risultato atteso:** il documento corrente e il suo stato restano invariati; non compare alcun errore.

15. [ ] **Azione:** Aprire il PDF image-only privo di qualunque text layer.
    **Risultato atteso:** dopo il campionamento EC01 compare esattamente `formato non supportato (no OCR)`; non compaiono canvas/controlli attivi per quel documento e soprattutto non compare il vecchio `TypeError` né un errore generico.

16. [ ] **Azione:** Dopo il rifiuto EC01, aprire nuovamente `a-single-column.pdf`.
    **Risultato atteso:** l'errore EC01 viene cancellato, il canvas riappare e la traduzione riparte; il rifiuto precedente non lascia lo stato PDF bloccato.

17. [ ] **Azione:** Tentare di aprire un file `.pdf` troncato/corrotto preparato per QA, quindi aprire una fixture valida.
    **Risultato atteso:** il file corrotto produce `Errore nell'apertura del PDF: …` senza crash; la fixture valida successiva si apre normalmente. La propagazione specifica di un errore di `reader.read()` resta coperta dal test unitario, perché non è inducibile in modo affidabile senza strumentare l'app.

## Regression Risks

18. [ ] **Azione:** Su Windows/WebView2, aprire `a-single-column.pdf` e il PDF reale, poi navigare almeno `1 → 2 → 3 → 2`.
    **Risultato atteso:** canvas, conteggio pagine e traduzioni corrispondono alle pagine correnti; nessun errore di estrazione o regressione rispetto al comportamento Windows precedente.

19. [ ] **Azione:** Sempre su Windows, ripetere il controllo di prefetch degli step 7–8.
    **Risultato atteso:** la pagina prefetched viene servita dalla cache e l'uso del reader esplicito non altera WebView2.

20. [ ] **Azione:** Sempre su Windows, aprire il PDF image-only e il fixture pure XFA.
    **Risultato atteso:** l'image-only mostra esattamente `formato non supportato (no OCR)`; XFA conserva apertura, visualizzazione ed estrazione baseline.

21. [ ] **Azione:** Registrare ambiente/versione WebView2, documenti usati ed esito degli step 18–20. Se non è disponibile una macchina Windows, marcare questi tre step `PENDING — follow-up manuale Windows/WebView2` con responsabile/data prevista.
    **Risultato atteso:** la mancanza temporanea di Windows è documentata come follow-up e non viene confusa con un fallimento del fix macOS.

## Out of Scope

- OCR o supporto ai PDF image-only oltre alla conservazione dell'errore EC01.
- Upgrade/downgrade di `pdfjs-dist`, polyfill globali di `ReadableStream` o spostamento dell'estrazione nel core Rust.
- Qualità linguistica del provider, layout reconstruction improvements, cache/persistenza e configurazione provider oltre a quanto necessario per osservare l'estrazione.
- La modifica preesistente di `package-lock.json`, esplicitamente esclusa dall'analisi e da qualsiasi intervento.
- La correzione solo-test in `src-tauri/src/sidecar.rs`, che rende portabile un fixture di path e non modifica il comportamento runtime.
