<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import * as pdfjsLib from 'pdfjs-dist';
  import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
  import {
    PdfDocumentLoadController,
    type ActivePdfDocument,
    type PdfDocumentLoadState
  } from '$lib/pdfDocumentLoad';
  import { extractPageText } from '$lib/pdfPageText';
  import { LatestRenderCoordinator } from '$lib/pdfRender';
  import ProviderConfig from '$lib/ProviderConfig.svelte';
  import GlossaryPanel from '$lib/GlossaryPanel.svelte';
  import { isLocalProvider, shouldShowLocalHint, localUnreachableHint } from '$lib/providerConfig';
  import {
    translationErrorMessage,
    pageStatusLabel,
    resultStatus,
    requestKey,
    isCurrentRequest,
    shouldTranslate,
    contextNote,
    type TranslationResult,
    type AdvanceContextResult,
    type PageStatus,
    type RequestKey
  } from '$lib/translation';
  import {
    restoreDecision,
    fileName,
    clampPage,
    parsePageDraft,
    type LastSession,
    type RecentDocument
  } from '$lib/session';
  import { LANGUAGES } from '$lib/settings';

  pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;

  let configOpen = $state(false);
  let glossaryOpen = $state(false);

  // --- Core-facing types (mirror the Rust `documents` structs). ---
  interface DocumentRecord {
    document_id: number;
    file_path: string;
    file_hash: string;
    title: string;
    total_pages: number;
  }
  interface SessionRecord {
    session_id: number;
    document_id: number;
    target_language: string;
    current_page: number;
  }

  type PdfDocument = Awaited<ReturnType<typeof pdfjsLib.getDocument>['promise']>;
  type ActiveDocument = ActivePdfDocument<PdfDocument, SessionRecord>;

  // pdf.js document handle — kept out of reactive state (not a plain object).
  let pdfDoc: PdfDocument | null = null;
  let documentGeneration = 0;
  let canvasEl: HTMLCanvasElement | undefined = $state();

  let title = $state('');
  let totalPages = $state(0);
  let currentPage = $state(1);
  let pageDraft = $state('');
  let reconstructedText = $state('');
  // The page number `reconstructedText` was extracted from. Set ATOMICALLY with
  // `reconstructedText` in `showPage` so the translation effect can enforce the
  // page↔text invariant and never translate stale text under a new page number
  // (ticket 16). 0 = no page text loaded yet.
  let reconstructedPage = $state(0);
  let targetLanguage = $state('it');
  let session = $state<SessionRecord | null>(null);
  let loading = $state(false);
  let errorMsg = $state('');

  // --- Recents & restore state (ticket 11, FR09/FR10/EC06). ---
  let recents = $state<RecentDocument[]>([]);
  // Set when the last-session or a recent file can't be found on disk (EC06).
  let missingFile = $state<RecentDocument | null>(null);
  let relocateError = $state('');

  // --- Translation state (tickets 08 & 12). ---
  let translatedText = $state('');
  let translating = $state(false);
  let translationError = $state('');
  // Per-page status for the bottom bar (§3.1): idle/loading/cached/translated/error.
  let pageStatus = $state<PageStatus>('idle');
  // Non-intrusive note when the perceptor-update failed to advance the context
  // for this page (ticket 02): the translation is still shown; only the
  // summary/glossary context did not advance. Empty = no note.
  let contextNoteText = $state('');
  // Monotonic token so a slow response for a stale page/language is ignored.
  let translationSeq = 0;
  // PDF.js forbids concurrent RenderTasks on one canvas. This coordinator
  // cancels an obsolete task, waits for its cleanup, and only starts the latest
  // navigation's replacement.
  const pageRenderer = new LatestRenderCoordinator();
  // Prefetch the next page in the background (decision D5, read from settings).
  let prefetchEnabled = $state(true);
  // Non-blocking onboarding hint shown when the active provider is local and its
  // server is not reachable (ticket 09, D3/D7). Empty = no hint. Never blocks the
  // app or switching to OpenRouter.
  let providerHint = $state('');

  /** The identity of the page currently on screen (document, page, language). */
  function currentKey(): RequestKey | null {
    if (!session) return null;
    return {
      documentId: session.document_id,
      pageNumber: currentPage,
      targetLanguage
    };
  }

  const isCurated = $derived(LANGUAGES.some((l) => l.code === targetLanguage));
  const canPrev = $derived(!loading && currentPage > 1);
  const canNext = $derived(!loading && currentPage < totalPages);

  const RENDER_SCALE = 1.4;
  const EC01_MESSAGE = 'formato non supportato (no OCR)';

  async function extractDocumentPageText(
    document: PdfDocument,
    pageNo: number
  ): Promise<string> {
    return extractPageText(await document.getPage(pageNo));
  }

  /** Render `pageNo` onto the canvas and show its reconstructed text. */
  async function showPage(
    pageNo: number,
    activeDocument: ActiveDocument,
    generation = documentGeneration
  ): Promise<void> {
    const myToken = pageRenderer.beginNavigation();
    const canvas = canvasEl;
    if (!canvas) return;

    const document = activeDocument.document;
    const page = await document.getPage(pageNo);
    if (
      !pageRenderer.isLatest(myToken) ||
      !pageRenderer.isCurrentDocument(generation) ||
      !documentLoader.isActive(activeDocument)
    ) {
      return;
    }
    const viewport = page.getViewport({ scale: RENDER_SCALE });
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const rendered = await pageRenderer.render(myToken, () => {
      // Resizing clears the bitmap, so it must happen after the cancelled task
      // has fully settled and immediately before the replacement starts.
      canvas.width = viewport.width;
      canvas.height = viewport.height;
      return page.render({ canvasContext: ctx, viewport, canvas });
    });
    if (!rendered) return;

    const text = await extractPageText(page);
    // Drop a superseded render: committing it would leave `reconstructedPage`
    // != `currentPage`, and since navigation is the only reliable re-trigger a
    // later language change would then skip re-translating the visible page.
    if (
      !pageRenderer.isLatest(myToken) ||
      !pageRenderer.isCurrentDocument(generation) ||
      !documentLoader.isActive(activeDocument)
    ) {
      return;
    }
    // Assign page + text together so a reader of `reconstructedText` always sees
    // the matching `reconstructedPage` (no window where they disagree).
    reconstructedText = text;
    reconstructedPage = pageNo;
  }

  /** Does any sampled page yield extractable text? (EC01 guard.) */
  async function hasExtractableText(
    document: PdfDocument,
    pageCount: number,
    generation: number
  ): Promise<boolean> {
    const sample = Math.min(pageCount, 10);
    for (let p = 1; p <= sample; p++) {
      if (!pageRenderer.isCurrentDocument(generation)) return false;
      const text = await extractDocumentPageText(document, p);
      if (!pageRenderer.isCurrentDocument(generation)) return false;
      if (text.trim().length > 0) return true;
    }
    return false;
  }

  function baseName(path: string): string {
    const file = path.split(/[\\/]/).pop() ?? path;
    return file.replace(/\.pdf$/i, '');
  }

  function clearDocumentBoundState(): void {
    pdfDoc = null;
    session = null;
    title = '';
    totalPages = 0;
    currentPage = 1;
    pageDraft = '';
    reconstructedText = '';
    reconstructedPage = 0;
    translatedText = '';
    translating = false;
    translationError = '';
    pageStatus = 'idle';
    contextNoteText = '';
  }

  function publishPreparedDocument(prepared: ActiveDocument): void {
    // These assignments share one synchronous Svelte update: reactive work can
    // observe either the cleared state or this complete matching bundle.
    pdfDoc = prepared.document;
    session = prepared.session;
    title = prepared.title;
    totalPages = prepared.totalPages;
    currentPage = prepared.currentPage;
    pageDraft = String(prepared.currentPage);
    targetLanguage = prepared.targetLanguage;
  }

  let routePublication: ActiveDocument | null = null;

  function applyDocumentLoadState(
    state: PdfDocumentLoadState<PdfDocument, SessionRecord>
  ): void {
    loading = state.phase === 'loading' || state.phase === 'activating';

    if (state.publication !== routePublication) {
      clearDocumentBoundState();
      routePublication = state.publication;
      if (state.publication) publishPreparedDocument(state.publication);
    }

    if (state.phase === 'unsupported') {
      errorMsg = EC01_MESSAGE;
    } else if (state.phase === 'error') {
      errorMsg = `Errore nell'apertura del PDF: ${state.error}`;
    } else {
      errorMsg = '';
    }
  }

  const documentLoader = new PdfDocumentLoadController<
    Uint8Array,
    PdfDocument,
    SessionRecord
  >({
    readBytes: async (filePath) => {
      const buffer = await invoke<ArrayBuffer>('read_pdf_bytes', { path: filePath });
      return new Uint8Array(buffer);
    },
    openDocument: (data) => pdfjsLib.getDocument({ data }),
    hasExtractableText: (document, pageCount) =>
      hasExtractableText(document, pageCount, documentGeneration),
    registerDocument: ({ path: filePath, totalPages: pageCount, title: docTitle }) =>
      invoke<DocumentRecord>('register_document', {
        path: filePath,
        totalPages: pageCount,
        title: docTitle
      }),
    openSession: (documentId) =>
      invoke<SessionRecord>('open_or_create_session', { documentId }),
    titleFromPath: baseName,
    clampPage,
    beforeDestroy: async () => pageRenderer.waitForIdle(),
    onStateChange: applyDocumentLoadState
  });

  /**
   * Load, register and render a PDF by absolute path. Shared by the file
   * picker, the "Recenti" list and startup restore (ticket 11). Rehydrates the
   * saved page + target language from the session (rolling_summary and glossary
   * live in the DB and are used by the core / glossary panel on demand).
   */
  async function loadDocument(path: string): Promise<void> {
    // Invalidate/cancel the visible document's render before the first load
    // await. Every later commit is tied to this document identity.
    const loadGeneration = pageRenderer.beginDocument();
    documentGeneration = loadGeneration;
    relocateError = '';
    missingFile = null;
    translationSeq++; // invalidate any in-flight translation from a prior doc

    await documentLoader.load(path, {
      hasExtractableText: (document, pageCount) =>
        hasExtractableText(document, pageCount, loadGeneration),
      activate: async (prepared) => {
        if (
          !pageRenderer.isCurrentDocument(loadGeneration) ||
          !documentLoader.isActive(prepared)
        ) {
          return;
        }

        await showPage(prepared.currentPage, prepared, loadGeneration);
        if (
          !pageRenderer.isCurrentDocument(loadGeneration) ||
          !documentLoader.isActive(prepared)
        ) {
          return;
        }
        await refreshRecents();
      }
    });
  }

  async function openPdf(): Promise<void> {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'PDF', extensions: ['pdf'] }]
    });
    if (typeof selected !== 'string') return; // cancelled
    await loadDocument(selected);
  }

  /** Reload the "Recenti" list (FR09) from the document history. */
  async function refreshRecents(): Promise<void> {
    try {
      recents = await invoke<RecentDocument[]>('list_recent_documents', { limit: 10 });
    } catch {
      recents = [];
    }
  }

  /** Open a recent PDF in one click (FR09), guarding a moved/deleted file (EC06). */
  async function openRecent(doc: RecentDocument): Promise<void> {
    const exists = await invoke<boolean>('file_exists', { path: doc.file_path });
    if (!exists) {
      missingFile = doc;
      relocateError = '';
      return;
    }
    await loadDocument(doc.file_path);
  }

  /** EC06: let the user point at the moved file; accepted only if the hash matches. */
  async function locateMissing(): Promise<void> {
    if (!missingFile) return;
    const selected = await open({
      multiple: false,
      filters: [{ name: 'PDF', extensions: ['pdf'] }]
    });
    if (typeof selected !== 'string') return; // cancelled
    try {
      const relocated = await invoke<DocumentRecord | null>('relocate_document', {
        documentId: missingFile.document_id,
        candidatePath: selected
      });
      if (!relocated) {
        relocateError = 'Il file selezionato non corrisponde a questo documento.';
        return;
      }
      await loadDocument(relocated.file_path);
    } catch (e) {
      relocateError = `Errore nel ricollegare il file: ${e}`;
    }
  }

  /** EC06: drop the missing document from "Recenti" without deleting its data. */
  async function removeMissing(): Promise<void> {
    if (!missingFile) return;
    try {
      await invoke('remove_recent', { documentId: missingFile.document_id });
    } catch {
      // Non-fatal: worst case it reappears next launch.
    }
    missingFile = null;
    relocateError = '';
    await refreshRecents();
  }

  /** Re-read the prefetch toggle from settings (ticket 12/13). Called on mount
   *  and after the Settings panel saves so the change takes effect live. */
  async function refreshPrefetch(): Promise<void> {
    try {
      prefetchEnabled = await invoke<boolean>('get_prefetch_enabled');
    } catch {
      prefetchEnabled = true; // D5 default
    }
  }

  /**
   * Onboarding/health check (ticket 09, D3/D7): when the active provider is a
   * local one, probe its reachability and, if the server is down, show a
   * non-blocking hint inviting the user to start it or open ⚙️. Best-effort and
   * silent on any failure — it never blocks the app nor switching to OpenRouter,
   * and never triggers a translation or a cloud call (D4). Called on mount and
   * after the Settings panel saves (the provider may have changed).
   */
  async function refreshProviderHealth(): Promise<void> {
    try {
      const activeId = await invoke<string>('get_active_provider');
      if (!isLocalProvider(activeId)) {
        providerHint = '';
        return;
      }
      const reachable = await invoke<boolean>('check_provider_reachable', {
        providerId: activeId
      });
      providerHint = shouldShowLocalHint(activeId, reachable) ? localUnreachableHint(activeId) : '';
    } catch {
      providerHint = ''; // never block on a health-check failure
    }
  }

  /** Re-read live settings after the Settings panel saves (ticket 12/13 + 09). */
  async function onSettingsSaved(): Promise<void> {
    await refreshPrefetch();
    await refreshProviderHealth();
  }

  // Startup restore (FR10): reopen the last document at its saved page/language,
  // or surface the EC06 missing-file state; always populate "Recenti".
  onMount(async () => {
    await refreshPrefetch();
    void refreshProviderHealth(); // non-blocking onboarding hint (ticket 09)
    await refreshRecents();
    let last: LastSession | null = null;
    try {
      last = await invoke<LastSession | null>('get_last_session');
    } catch {
      return;
    }
    if (!last) return;
    const exists = await invoke<boolean>('file_exists', { path: last.file_path });
    const decision = restoreDecision(last, exists);
    if (decision === 'restore') {
      await loadDocument(last.file_path);
    } else if (decision === 'missing') {
      missingFile = {
        document_id: last.document_id,
        file_path: last.file_path,
        file_hash: last.file_hash,
        title: last.title,
        total_pages: last.total_pages,
        last_opened_at: ''
      };
    }
  });

  onDestroy(async () => {
    pageRenderer.beginDocument();
    await documentLoader.dispose();
  });

  async function persistSession(
    sessionToPersist = session,
    pageToPersist = currentPage,
    languageToPersist = targetLanguage
  ): Promise<void> {
    if (!sessionToPersist) return;
    await invoke('update_session', {
      sessionId: sessionToPersist.session_id,
      currentPage: pageToPersist,
      targetLanguage: languageToPersist
    });
  }

  async function goTo(pageNo: number): Promise<void> {
    const activeDocument = documentLoader.captureActive();
    if (loading || !activeDocument || pageNo < 1 || pageNo > totalPages) return;
    const generation = documentGeneration;
    const activeSession = activeDocument.session;
    // Reset the translation pane immediately so the previous page's translation
    // is not shown while the new page renders (ticket 16). The effect will
    // re-translate once the new page's text is extracted.
    translatedText = '';
    translationError = '';
    pageStatus = 'idle';
    currentPage = pageNo;
    pageDraft = String(pageNo);
    await showPage(pageNo, activeDocument, generation);
    if (
      loading ||
      !pageRenderer.isCurrentDocument(generation) ||
      !documentLoader.isActive(activeDocument) ||
      currentPage !== pageNo
    ) {
      return;
    }
    await persistSession(activeSession, pageNo, targetLanguage);
  }

  function commitPageDraft(): void {
    const destination = parsePageDraft(pageDraft, totalPages);
    if (loading || destination === null || !pdfDoc) {
      pageDraft = pdfDoc ? String(currentPage) : '';
      return;
    }

    pageDraft = String(destination);
    if (destination !== currentPage) void goTo(destination);
  }

  function onPageDraftKeydown(event: KeyboardEvent): void {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    commitPageDraft();
  }

  function setLanguage(value: string): void {
    const activeSession = session;
    targetLanguage = value;
    void persistSession(activeSession, currentPage, value);
  }

  /**
   * Translate the current page (UC02). The core checks its cache first, so a
   * revisit is instant and makes no network call; transient errors are retried
   * with backoff in the core (NFR06). EC03/EC02/EC07 surface as dedicated hints.
   *
   * Results are tagged by (document, page, language): if the user navigated away
   * while it was translating, the stale result is dropped (ticket 12).
   */
  async function translateCurrentPage(): Promise<void> {
    if (loading || !session || !pdfDoc) return;
    // Couple `page_number` with the exact text it was extracted from: send
    // `reconstructedPage`/`reconstructedText` from the same source, never a
    // fresh `currentPage` mixed with stale text (ticket 16 invariant).
    const pageText = reconstructedText;
    const requested: RequestKey = {
      documentId: session.document_id,
      pageNumber: reconstructedPage,
      targetLanguage
    };
    if (!shouldTranslate(reconstructedPage, currentPage, pageText)) return;
    const seq = ++translationSeq;
    translating = true;
    pageStatus = 'loading';
    translationError = '';
    contextNoteText = '';
    try {
      const result = await invoke<TranslationResult>('translate_page', {
        documentId: requested.documentId,
        pageNumber: requested.pageNumber,
        targetLanguage: requested.targetLanguage,
        pageText,
        updateContext: true // real navigation advances the percettore context
      });
      // Drop a result the user has navigated away from (obsolete request).
      // Note the two intentionally different page sources: `requested` is keyed
      // on `reconstructedPage` (the page whose text we actually translated),
      // while `now`/`currentKey()` is keyed on `currentPage` (the page on
      // screen). They are guaranteed equal at send time by the `shouldTranslate`
      // gate above; this check catches navigation that happened AFTER sending.
      const now = currentKey();
      if (seq !== translationSeq || !now || !isCurrentRequest(requested, now)) return;
      // Two-phase arrival (ticket 01): render the translation IMMEDIATELY — an
      // arrival on a fully prefetched page returns with zero blocking LLM calls,
      // so the text is shown at once without waiting for the perceptor. Clearing
      // `translating` here (before phase 2) is what actually reveals the text in
      // the template: the `{:else if translating}` branch hides `translatedText`
      // until this flips. The guarded `finally` below re-clears it harmlessly.
      translatedText = result.translated_text;
      pageStatus = resultStatus(result.from_cache);
      translating = false;
      // Phase 2 — advance the document context (rolling summary + glossary) OFF
      // the response path. The user is already reading; this call runs the
      // once-per-page perceptor-update in the background. It is idempotent (a
      // re-visit is a cheap no-op via the `context_advanced` marker), so it is
      // safe to call after every on-demand translation, cache hit included.
      // Ordering (spec §Decision): await it BEFORE prefetching N+1 so the prefetch
      // uses the advanced summary and context grows in order.
      try {
        const adv = await invoke<AdvanceContextResult>('advance_context', {
          documentId: requested.documentId,
          pageNumber: requested.pageNumber,
          targetLanguage: requested.targetLanguage,
          pageText
        });
        // Drop a stale result if the user navigated away meanwhile (the note is
        // for the page on screen only).
        const stillCurrent = currentKey();
        if (seq === translationSeq && stillCurrent && isCurrentRequest(requested, stillCurrent)) {
          // Non-intrusive note if the perceptor-update did not advance the
          // context for this page (ticket 02); empty on success / no-op.
          contextNoteText = contextNote(adv);
        }
      } catch {
        // Advancing the context is best-effort: a failure never blocks reading
        // (the translation is already shown). A later re-visit retries it.
      }
      // Only the translation continuation that still owns this exact page may
      // warm N+1. A stale continuation must not capture a newly loaded document.
      if (seq === translationSeq) void prefetchNextPage(requested);
    } catch (e) {
      const now = currentKey();
      if (seq !== translationSeq || !now || !isCurrentRequest(requested, now)) return;
      translatedText = '';
      translationError = translationErrorMessage(e);
      pageStatus = 'error';
    } finally {
      if (seq === translationSeq) translating = false;
    }
  }

  /**
   * Prefetch the NEXT page in the background (ticket 12, D5). Warms only the
   * per-unit cache (STC-09) with `updateContext: false`; under option B (ticket
   * 01) the per-page cache row is written on real navigation, so the percettore
   * still runs and the glossary grows on arrival. Never advances the
   * summary/glossary out of order, and never touches the current view or status.
   * Best-effort: any error (offline, rate limit) is swallowed — it just means no
   * warm cache.
   */
  async function prefetchNextPage(sourceRequest: RequestKey): Promise<void> {
    const activeDocument = documentLoader.captureActive();
    if (loading || !prefetchEnabled || !activeDocument) return;
    const current = currentKey();
    if (!current || !isCurrentRequest(sourceRequest, current)) return;
    const document = activeDocument.document;
    const generation = documentGeneration;
    const activeSession = activeDocument.session;
    const next = sourceRequest.pageNumber + 1;
    if (next > totalPages) return;
    try {
      const nextText = await extractDocumentPageText(document, next);
      const now = currentKey();
      if (
        loading ||
        !pageRenderer.isCurrentDocument(generation) ||
        !documentLoader.isActive(activeDocument) ||
        !now ||
        !isCurrentRequest(sourceRequest, now)
      ) {
        return;
      }
      if (!nextText.trim()) return;
      await invoke<TranslationResult>('translate_page', {
        documentId: activeSession.document_id,
        pageNumber: next,
        targetLanguage: sourceRequest.targetLanguage,
        pageText: nextText,
        updateContext: false // prefetch caches only; no context mutation
      });
    } catch {
      // Non-fatal: the page will simply be translated on demand on arrival.
    }
  }

  // Retranslate whenever the page text or target language changes. The core
  // cache keeps this cheap: only genuinely new (page, language) pairs call out.
  $effect(() => {
    // Track the inputs that define a translation.
    void targetLanguage;
    const page = currentPage;
    const textPage = reconstructedPage;
    const text = reconstructedText;
    const ready = !loading && session !== null;
    // Only translate once the extracted text belongs to the page on screen; this
    // suppresses the stale pre-fire (currentPage=N, text of N-1) that poisoned
    // the cache (ticket 16).
    if (ready && shouldTranslate(textPage, page, text)) void translateCurrentPage();
  });
</script>

<main class="app">
  <header class="topbar">
    <button onclick={openPdf} disabled={loading}>Apri PDF</button>
    <label class="lang">
      Lingua:
      <select
        value={isCurated ? targetLanguage : '__custom__'}
        onchange={(e) => {
          const v = e.currentTarget.value;
          if (v !== '__custom__') setLanguage(v);
        }}
      >
        {#each LANGUAGES as l (l.code)}
          <option value={l.code}>{l.label}</option>
        {/each}
        {#if !isCurated}
          <option value="__custom__">{targetLanguage} (personalizzata)</option>
        {/if}
      </select>
      <input
        class="lang-free"
        placeholder="altra lingua…"
        oninput={(e) => {
          const v = e.currentTarget.value.trim();
          if (v) setLanguage(v);
        }}
      />
    </label>
    <span class="brand">translate-lector</span>
    <button
      class="settings-btn"
      aria-label="Impostazioni"
      title="Impostazioni"
      onclick={() => (configOpen = true)}
    >
      ⚙️
    </button>
  </header>

  <ProviderConfig bind:open={configOpen} onSaved={onSettingsSaved} />

  {#if providerHint}
    <div class="provider-hint" role="status">
      <span class="provider-hint-text">{providerHint}</span>
      <span class="provider-hint-actions">
        <button onclick={() => (configOpen = true)}>Apri ⚙️ Impostazioni</button>
        <button onclick={refreshProviderHealth} title="Riprova il controllo">Riprova</button>
        <button
          class="provider-hint-dismiss"
          aria-label="Chiudi avviso"
          onclick={() => (providerHint = '')}
        >
          ✕
        </button>
      </span>
    </div>
  {/if}

  <section class="panes">
    <div class="pane pane-left">
      {#if loading}
        <p class="notice">Caricamento…</p>
      {:else if errorMsg}
        <p class="notice">{errorMsg}</p>
      {:else if missingFile}
        <div class="missing">
          <p class="notice notice-error">File mancante: «{missingFile.title}»</p>
          <p class="missing-path">{missingFile.file_path}</p>
          {#if relocateError}
            <p class="notice notice-error">{relocateError}</p>
          {/if}
          <div class="missing-actions">
            <button onclick={locateMissing}>Individua file…</button>
            <button onclick={removeMissing}>Rimuovi dai recenti</button>
          </div>
        </div>
      {:else if totalPages === 0}
        <p class="notice">Apri un PDF per iniziare.</p>
        {#if recents.length > 0}
          <div class="recents">
            <h3 class="recents-head">Recenti</h3>
            <ul>
              {#each recents as doc (doc.document_id)}
                <li>
                  <button
                    class="recent-item"
                    title={doc.file_path}
                    onclick={() => openRecent(doc)}
                  >
                    <span class="recent-title">{doc.title}</span>
                    <span class="recent-file">{fileName(doc.file_path)}</span>
                  </button>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      {/if}
      <canvas
        bind:this={canvasEl}
        class:hidden={loading || totalPages === 0 || !!errorMsg || !!missingFile}
      ></canvas>
    </div>
    <div class="pane pane-right">
      <h2 class="pane-heading">
        Traduzione{title ? ` — ${title}` : ''}
        {#if translating}<span class="spinner" aria-label="Traduzione in corso">⏳</span>{/if}
      </h2>
      {#if translationError}
        <p class="notice notice-error">{translationError}</p>
      {:else if translating}
        <!-- During a translation, show only the placeholder — never the previous
             page's (or otherwise stale) translation, regardless of what is still
             held in `translatedText`. -->
        <p class="notice">Traduzione in corso…</p>
      {:else}
        <textarea class="reconstructed" readonly value={translatedText}></textarea>
      {/if}
    </div>
  </section>

  <footer class="bottombar">
    <button onclick={() => goTo(currentPage - 1)} disabled={!canPrev}>◀</button>
    <label class="pageinfo">
      <span>Pag.</span>
      <input
        class="page-selector"
        type="number"
        aria-label={totalPages
          ? `Vai alla pagina, da 1 a ${totalPages}`
          : 'Vai alla pagina'}
        value={pageDraft}
        min={1}
        max={totalPages}
        step={1}
        disabled={totalPages === 0 || loading}
        oninput={(event) => (pageDraft = event.currentTarget.value)}
        onkeydown={onPageDraftKeydown}
        onblur={commitPageDraft}
      />
      <span>/ {totalPages}</span>
    </label>
    <button onclick={() => goTo(currentPage + 1)} disabled={!canNext}>▶</button>
    <button
      class="glossary-btn"
      onclick={() => (glossaryOpen = true)}
      disabled={!session}
      title="Glossario del documento"
    >
      [Glossario]
    </button>
    {#if session && pageStatus !== 'idle'}
      <span class="status-indicator" class:status-error={pageStatus === 'error'}>
        {pageStatusLabel(pageStatus)}
      </span>
    {/if}
    {#if session && contextNoteText}
      <!-- Non-intrusive note (ticket 02): the perceptor-update did not advance
           the summary/glossary context for this page. The translation itself is
           shown and cached; this is informational, not an error. -->
      <span class="context-note" role="status">{contextNoteText}</span>
    {/if}
    {#if session && pageStatus === 'error'}
      <!-- Retry the current page after a failed translation (ticket 12). Reuses
           translateCurrentPage(); nothing is cached after an error, so this
           performs a real re-translate once the user has fixed the cause. -->
      <button class="retry-btn" onclick={() => void translateCurrentPage()}>
        ↻ Riprova traduzione
      </button>
    {/if}
  </footer>

  <GlossaryPanel bind:open={glossaryOpen} documentId={session?.document_id ?? null} />
</main>

<style>
  :global(body) {
    margin: 0;
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    color: #0f0f0f;
    background: #f6f6f6;
  }

  .topbar,
  .bottombar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 0.9rem;
    background: #ffffff;
    border-bottom: 1px solid #e2e2e2;
  }

  .bottombar {
    border-top: 1px solid #e2e2e2;
    border-bottom: none;
    justify-content: center;
    flex-wrap: wrap;
  }

  .brand {
    margin-left: auto;
    font-weight: 600;
    opacity: 0.7;
  }

  .lang {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .lang-free {
    width: 8rem;
  }

  .settings-btn {
    padding: 0.3em 0.55em;
    font-size: 1.05rem;
    line-height: 1;
  }

  .provider-hint {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
    padding: 0.5rem 0.9rem;
    background: #fff4e5;
    border-bottom: 1px solid #f0c98a;
    color: #7a4a00;
    font-size: 0.88rem;
  }

  .provider-hint-text {
    flex: 1;
    min-width: 12rem;
  }

  .provider-hint-actions {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .provider-hint-dismiss {
    padding: 0.3em 0.55em;
    line-height: 1;
  }

  .panes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    flex: 1;
    min-height: 0;
  }

  .pane {
    overflow: auto;
    padding: 0.75rem;
  }

  .pane-left {
    border-right: 1px solid #e2e2e2;
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .pane-right {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .pane-heading {
    margin: 0 0 0.5rem;
    font-size: 0.9rem;
    font-weight: 600;
    opacity: 0.65;
  }

  canvas {
    max-width: 100%;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  }

  canvas.hidden {
    display: none;
  }

  .reconstructed {
    flex: 1;
    resize: none;
    border: 1px solid #e2e2e2;
    border-radius: 6px;
    padding: 0.75rem;
    font-family: inherit;
    font-size: 0.95rem;
    line-height: 1.5;
    white-space: pre-wrap;
    background: #ffffff;
    color: inherit;
  }

  .notice {
    opacity: 0.7;
    font-style: italic;
  }

  .notice-error {
    color: #b3261e;
    opacity: 1;
    font-style: normal;
  }

  .missing {
    align-self: stretch;
    max-width: 32rem;
    margin: 0 auto;
  }

  .missing-path {
    font-size: 0.8rem;
    opacity: 0.6;
    word-break: break-all;
  }

  .missing-actions {
    display: flex;
    gap: 0.6rem;
    margin-top: 0.6rem;
  }

  .recents {
    align-self: stretch;
    max-width: 32rem;
    margin: 0.5rem auto 0;
  }

  .recents-head {
    margin: 0 0 0.4rem;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    opacity: 0.55;
  }

  .recents ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .recent-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.1rem;
    width: 100%;
    text-align: left;
  }

  .recent-title {
    font-weight: 600;
  }

  .recent-file {
    font-size: 0.78rem;
    opacity: 0.6;
  }

  .spinner {
    font-style: normal;
    margin-left: 0.35rem;
  }

  button {
    border-radius: 8px;
    border: 1px solid transparent;
    padding: 0.4em 0.9em;
    font: inherit;
    font-weight: 500;
    color: #0f0f0f;
    background: #ffffff;
    cursor: pointer;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
  }

  button:hover:not(:disabled) {
    border-color: #396cd8;
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .pageinfo {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.35rem;
    white-space: nowrap;
  }

  .page-selector {
    box-sizing: border-box;
    width: clamp(3.5rem, 10vw, 7rem);
    min-width: 0;
    padding: 0.3em 0.4em;
    border: 1px solid #b8b8b8;
    border-radius: 5px;
    font: inherit;
    font-variant-numeric: tabular-nums;
    text-align: right;
    color: inherit;
    background: inherit;
  }

  .page-selector:disabled {
    opacity: 0.5;
  }

  .status-indicator {
    font-size: 0.82rem;
    opacity: 0.75;
    white-space: nowrap;
  }

  .status-indicator.status-error {
    color: #b3261e;
    opacity: 1;
  }

  .context-note {
    font-size: 0.82rem;
    opacity: 0.7;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 42ch;
  }

  @media (prefers-color-scheme: dark) {
    .app {
      color: #f6f6f6;
      background: #2f2f2f;
    }
    .topbar,
    .bottombar {
      background: #1f1f1f;
      border-color: #3a3a3a;
    }
    .pane-left {
      border-color: #3a3a3a;
    }
    .reconstructed {
      background: #1f1f1f;
      border-color: #3a3a3a;
    }
    .provider-hint {
      background: #3a2e18;
      border-color: #5a4620;
      color: #ffd9a0;
    }
    button {
      color: #ffffff;
      background: #0f0f0f98;
    }
  }
</style>
