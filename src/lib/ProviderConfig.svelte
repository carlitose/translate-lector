<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import {
    COMMON_MODELS,
    DEFAULT_MODEL,
    isCommonModel,
    isValidKey,
    resolveModel
  } from './providerConfig';
  import {
    LANGUAGES,
    DEFAULT_SUMMARY_TOKEN_LIMIT,
    isCommonLanguage,
    resolveLanguage,
    parseSummaryLimit,
    resolvePrefetch
  } from './settings';

  // Controlled open state (bindable so the parent's ⚙️ button toggles it).
  // `onSaved` lets the parent re-read settings that affect the live view
  // (e.g. the prefetch toggle, ticket 12).
  let { open = $bindable(false), onSaved }: { open?: boolean; onSaved?: () => void } =
    $props();

  const CUSTOM = '__custom__';

  // --- Provider ---
  let hasKey = $state(false);
  let keyInput = $state(''); // plaintext, only while typing — never persisted in the webview
  let modelSelect = $state(DEFAULT_MODEL); // dropdown value ('__custom__' for free text)
  let modelFree = $state(''); // free-text model id

  // --- Reading preferences ---
  let langSelect = $state('it'); // default target language dropdown ('__custom__' for free)
  let langFree = $state('');
  let prefetchOn = $state(true);
  let summaryLimit = $state(String(DEFAULT_SUMMARY_TOKEN_LIMIT));

  // --- Data folder ---
  let dataDir = $state('');
  let dataDirMsg = $state('');

  // --- Clear cache ---
  let clearConfirm = $state(false);
  let clearMsg = $state('');

  let busy = $state(false);
  let error = $state('');
  let info = $state('');

  // Effective values from the current form state.
  const chosenModel = $derived(resolveModel(modelSelect === CUSTOM ? modelFree : modelSelect));
  const chosenLanguage = $derived(
    resolveLanguage(langSelect === CUSTOM ? langFree : langSelect)
  );

  /** Load the current state from the core whenever the panel opens. */
  $effect(() => {
    if (open) void load();
  });

  async function load(): Promise<void> {
    error = '';
    info = '';
    clearMsg = '';
    dataDirMsg = '';
    clearConfirm = false;
    keyInput = '';
    try {
      hasKey = await invoke<boolean>('has_api_key');

      const storedModel = resolveModel(await invoke<string>('get_model'));
      if (isCommonModel(storedModel)) {
        modelSelect = storedModel;
        modelFree = '';
      } else {
        modelSelect = CUSTOM;
        modelFree = storedModel;
      }

      const storedLang = resolveLanguage(
        await invoke<string | null>('get_setting', { key: 'default_target_language' })
      );
      if (isCommonLanguage(storedLang)) {
        langSelect = storedLang;
        langFree = '';
      } else {
        langSelect = CUSTOM;
        langFree = storedLang;
      }

      prefetchOn = resolvePrefetch(
        await invoke<string | null>('get_setting', { key: 'prefetch_enabled' })
      );

      const storedLimit = await invoke<string | null>('get_setting', {
        key: 'summary_token_limit'
      });
      summaryLimit = String(
        storedLimit != null && storedLimit.trim() ? parseSummaryLimit(storedLimit) : DEFAULT_SUMMARY_TOKEN_LIMIT
      );

      dataDir = await invoke<string>('get_data_dir');
    } catch (e) {
      error = `Errore nel caricamento delle impostazioni: ${e}`;
    }
  }

  async function save(): Promise<void> {
    error = '';
    info = '';
    busy = true;
    try {
      // A key is optional: store it only when the user typed one; otherwise keep
      // the existing key (the API key never leaves the keychain — NFR07).
      if (isValidKey(keyInput)) {
        await invoke('store_api_key', { key: keyInput.trim() });
      }
      await invoke('set_setting', { key: 'model', value: chosenModel });
      await invoke('set_setting', { key: 'default_target_language', value: chosenLanguage });
      await invoke('set_setting', {
        key: 'prefetch_enabled',
        value: prefetchOn ? 'true' : 'false'
      });
      await invoke('set_setting', {
        key: 'summary_token_limit',
        value: String(parseSummaryLimit(summaryLimit))
      });
      // Reflect the normalised value back in the field.
      summaryLimit = String(parseSummaryLimit(summaryLimit));
      keyInput = '';
      hasKey = await invoke<boolean>('has_api_key');
      info = 'Impostazioni salvate.';
      onSaved?.();
    } catch (e) {
      error = `Errore nel salvataggio: ${e}`;
    } finally {
      busy = false;
    }
  }

  async function deleteKey(): Promise<void> {
    error = '';
    info = '';
    busy = true;
    try {
      await invoke('clear_api_key');
      keyInput = '';
      hasKey = await invoke<boolean>('has_api_key');
      info = 'API key eliminata.';
    } catch (e) {
      error = `Errore nell'eliminazione: ${e}`;
    } finally {
      busy = false;
    }
  }

  /** Pick a new data folder (§3.5). Takes effect at the next launch. */
  async function changeDataDir(): Promise<void> {
    error = '';
    dataDirMsg = '';
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected !== 'string') return; // cancelled
    busy = true;
    try {
      const res = await invoke<{ path: string; restart_required: boolean }>('set_data_dir', {
        path: selected
      });
      dataDir = res.path;
      dataDirMsg = res.restart_required
        ? 'Cartella impostata. Riavvia l’app per usarla; copia manualmente i dati esistenti nella nuova cartella (il vecchio file non viene spostato né cancellato).'
        : 'Cartella dati aggiornata.';
    } catch (e) {
      error = `Errore nel cambio cartella: ${e}`;
    } finally {
      busy = false;
    }
  }

  /** Empty the translation cache after an explicit confirmation (§3.5). */
  async function clearCache(): Promise<void> {
    error = '';
    clearMsg = '';
    if (!clearConfirm) {
      clearConfirm = true; // first click arms the confirmation
      return;
    }
    busy = true;
    try {
      const removed = await invoke<number>('clear_translations_cache');
      clearMsg = `Cache svuotata (${removed} traduzioni rimosse).`;
    } catch (e) {
      error = `Errore nello svuotamento cache: ${e}`;
    } finally {
      clearConfirm = false;
      busy = false;
    }
  }

  function close(): void {
    open = false;
  }
</script>

{#if open}
  <div
    class="overlay"
    role="button"
    tabindex="-1"
    onclick={close}
    onkeydown={(e) => e.key === 'Escape' && close()}
  >
    <!-- Stop clicks inside the dialog from closing it. -->
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-label="Impostazioni"
      tabindex="0"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <header class="dialog-head">
        <h2>Impostazioni</h2>
        <button class="icon" aria-label="Chiudi" onclick={close}>✕</button>
      </header>

      <div class="scroll">
        <div class="field">
          <span class="label">API key OpenRouter</span>
          <span class="status" class:present={hasKey}>
            {hasKey ? '● key presente' : '○ nessuna key'}
          </span>
          <input
            type="password"
            placeholder={hasKey ? 'inserisci per sostituire…' : 'sk-or-…'}
            bind:value={keyInput}
            autocomplete="off"
          />
        </div>

        <div class="field">
          <span class="label">Modello</span>
          <select bind:value={modelSelect}>
            {#each COMMON_MODELS as m (m.id)}
              <option value={m.id}>{m.label}</option>
            {/each}
            <option value={CUSTOM}>Altro (ID personalizzato)…</option>
          </select>
          {#if modelSelect === CUSTOM}
            <input type="text" placeholder="es. mistralai/mistral-large" bind:value={modelFree} />
          {/if}
          <span class="hint">Verrà usato: <code>{chosenModel}</code></span>
        </div>

        <div class="field">
          <span class="label">Lingua di destinazione predefinita</span>
          <select bind:value={langSelect}>
            {#each LANGUAGES as l (l.code)}
              <option value={l.code}>{l.label}</option>
            {/each}
            <option value={CUSTOM}>Altra (personalizzata)…</option>
          </select>
          {#if langSelect === CUSTOM}
            <input type="text" placeholder="es. catalano" bind:value={langFree} />
          {/if}
          <span class="hint">Nuovi documenti si apriranno in: <code>{chosenLanguage}</code></span>
        </div>

        <div class="field field-row">
          <label class="toggle">
            <input type="checkbox" bind:checked={prefetchOn} />
            <span class="label">Prefetch della pagina successiva</span>
          </label>
        </div>

        <div class="field">
          <span class="label">Limite rolling summary (token)</span>
          <input type="number" min="1" step="50" bind:value={summaryLimit} />
          <span class="hint">Oltre questa soglia il summary viene compresso.</span>
        </div>

        <div class="field">
          <span class="label">Cartella dati</span>
          <span class="path" title={dataDir}>{dataDir || '—'}</span>
          <div>
            <button onclick={changeDataDir} disabled={busy}>Cambia cartella…</button>
          </div>
          {#if dataDirMsg}
            <span class="hint">{dataDirMsg}</span>
          {/if}
        </div>

        <div class="field">
          <span class="label">Cache traduzioni</span>
          <div>
            <button class="danger" onclick={clearCache} disabled={busy}>
              {clearConfirm ? 'Conferma svuotamento' : 'Svuota cache'}
            </button>
            {#if clearConfirm}
              <button onclick={() => (clearConfirm = false)} disabled={busy}>Annulla</button>
            {/if}
          </div>
          {#if clearConfirm}
            <span class="hint">Verranno cancellate tutte le traduzioni salvate. Confermi?</span>
          {/if}
          {#if clearMsg}
            <span class="hint">{clearMsg}</span>
          {/if}
        </div>
      </div>

      {#if error}
        <p class="msg error">{error}</p>
      {:else if info}
        <p class="msg info">{info}</p>
      {/if}

      <footer class="dialog-actions">
        <button class="danger" onclick={deleteKey} disabled={busy || !hasKey}>Elimina key</button>
        <button class="primary" onclick={save} disabled={busy}>Salva</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .dialog {
    width: min(30rem, 92vw);
    max-height: 90vh;
    background: #ffffff;
    color: #0f0f0f;
    border-radius: 10px;
    padding: 1.1rem 1.25rem 1.25rem;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3);
    display: flex;
    flex-direction: column;
    gap: 0.9rem;
  }

  .scroll {
    display: flex;
    flex-direction: column;
    gap: 0.9rem;
    overflow-y: auto;
    min-height: 0;
  }

  .dialog-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .dialog-head h2 {
    margin: 0;
    font-size: 1.1rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .field-row .toggle {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 0.5rem;
    cursor: pointer;
  }

  .field-row .toggle input {
    width: auto;
  }

  .label {
    font-weight: 600;
    font-size: 0.9rem;
  }

  .status {
    font-size: 0.8rem;
    opacity: 0.75;
  }

  .status.present {
    color: #1a7f37;
    opacity: 1;
  }

  .hint {
    font-size: 0.78rem;
    opacity: 0.7;
  }

  .path {
    font-size: 0.8rem;
    opacity: 0.75;
    word-break: break-all;
    font-family: monospace;
  }

  .field input,
  .field select {
    padding: 0.45rem 0.55rem;
    border: 1px solid #cfcfcf;
    border-radius: 6px;
    font: inherit;
    background: #ffffff;
    color: inherit;
  }

  .msg {
    margin: 0;
    font-size: 0.85rem;
  }

  .msg.error {
    color: #b3261e;
  }

  .msg.info {
    color: #1a7f37;
  }

  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.6rem;
  }

  button {
    border-radius: 8px;
    border: 1px solid transparent;
    padding: 0.4em 0.9em;
    font: inherit;
    font-weight: 500;
    cursor: pointer;
    background: #f0f0f0;
    color: #0f0f0f;
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  button.primary {
    background: #396cd8;
    color: #ffffff;
  }

  button.danger {
    background: #ffffff;
    border-color: #d3b3b3;
    color: #b3261e;
  }

  button.icon {
    background: transparent;
    padding: 0.2em 0.5em;
  }

  @media (prefers-color-scheme: dark) {
    .dialog {
      background: #1f1f1f;
      color: #f6f6f6;
    }
    .field input,
    .field select {
      background: #0f0f0f;
      color: #f6f6f6;
      border-color: #3a3a3a;
    }
    button {
      background: #333;
      color: #f6f6f6;
    }
    button.danger {
      background: #1f1f1f;
    }
    .status.present {
      color: #4ac26b;
    }
    .msg.info {
      color: #4ac26b;
    }
  }
</style>
