<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import {
    isValidTranslation,
    isValidNewTerm,
    toUpdateArgs,
    toAddArgs,
    typeLabel,
    pageLabel,
    type GlossaryTermRecord,
    type AddTermFormValues,
    type AddTermResult
  } from './glossary';

  // Free-text `type` with a small set of suggestions (product decision #4).
  const TYPE_SUGGESTIONS = ['comune', 'tecnico', 'nome proprio'];

  /** Blank form values; `locked` defaults to true (product decision #1). */
  function emptyForm(): AddTermFormValues {
    return { sourceTerm: '', translation: '', termType: '', note: '', locked: true };
  }

  // Controlled open state (bindable so the bottom-bar [Glossario] toggles it)
  // plus the current document whose terms are shown.
  let {
    open = $bindable(false),
    documentId = null
  }: { open?: boolean; documentId?: number | null } = $props();

  let terms = $state<GlossaryTermRecord[]>([]);
  let loading = $state(false);
  let error = $state('');
  let info = $state('');
  // id of the row currently being saved (for a per-row busy state).
  let savingId = $state<number | null>(null);
  // "Aggiungi termine" form state + its busy flag.
  let form = $state<AddTermFormValues>(emptyForm());
  let adding = $state(false);
  // Row transiently highlighted after a duplicate is opened in edit.
  let highlightId = $state<number | null>(null);

  /** Reload the document's terms whenever the panel opens. */
  $effect(() => {
    if (open && documentId != null) void load(documentId);
  });

  async function load(docId: number): Promise<void> {
    error = '';
    info = '';
    loading = true;
    try {
      terms = await invoke<GlossaryTermRecord[]>('list_glossary', { documentId: docId });
    } catch (e) {
      error = `Errore nel caricamento del glossario: ${e}`;
      terms = [];
    } finally {
      loading = false;
    }
  }

  async function saveTerm(term: GlossaryTermRecord): Promise<void> {
    if (!isValidTranslation(term.translation)) {
      error = 'La traduzione non può essere vuota.';
      return;
    }
    error = '';
    info = '';
    savingId = term.id;
    try {
      await invoke('update_glossary_term', { ...toUpdateArgs(term) });
      // Reflect the trimmed, persisted values locally.
      term.translation = term.translation.trim();
      term.note = term.note.trim();
      info = `«${term.source_term}» salvato.`;
    } catch (e) {
      error = `Errore nel salvataggio: ${e}`;
    } finally {
      savingId = null;
    }
  }

  /**
   * Add a brand-new term. On `inserted` the list reloads, a confirmation shows
   * and the form resets. On `duplicate` (product decision #2) no row is added —
   * the existing row is scrolled to, highlighted and focused for inline edit.
   * Validation failures come back as `Err(String)` and are shown non-intrusively.
   */
  async function addTerm(): Promise<void> {
    if (documentId == null) return;
    if (!isValidNewTerm(form.sourceTerm, form.translation)) return;
    const label = form.sourceTerm.trim();
    error = '';
    info = '';
    adding = true;
    try {
      const result = await invoke<AddTermResult>('add_glossary_term', {
        ...toAddArgs(documentId, form)
      });
      // `load` clears info/error, so reload first, then set the message.
      await load(documentId);
      if (result.status === 'duplicate') {
        info = `«${label}» esiste già — aperto in modifica.`;
        await focusExistingRow(result.id);
      } else {
        info = `«${label}» aggiunto.`;
        form = emptyForm();
      }
    } catch (e) {
      error = `Errore nell'aggiunta: ${e}`;
    } finally {
      adding = false;
    }
  }

  /** Scroll to, highlight and focus the existing row's translation input. */
  async function focusExistingRow(id: number): Promise<void> {
    // Restart the flash even when the same duplicate is re-submitted: drop the
    // class, let Svelte flush, then re-add it so the keyframes replay.
    highlightId = null;
    await tick();
    highlightId = id;
    await tick();
    const row = document.getElementById(`gloss-row-${id}`);
    if (!row) return;
    row.scrollIntoView({ block: 'center', behavior: 'smooth' });
    // Clear the highlight once the flash finishes so the class doesn't linger.
    row.addEventListener(
      'animationend',
      () => {
        if (highlightId === id) highlightId = null;
      },
      { once: true }
    );
    const input = row.querySelector<HTMLInputElement>('[data-role="translation"]');
    // preventScroll so the default focus jump doesn't fight the smooth scroll.
    input?.focus({ preventScroll: true });
    input?.select();
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
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-label="Glossario"
      tabindex="0"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <header class="dialog-head">
        <h2>Glossario</h2>
        <button class="icon" aria-label="Chiudi" onclick={close}>✕</button>
      </header>

      {#if error}
        <p class="msg error">{error}</p>
      {:else if info}
        <p class="msg info">{info}</p>
      {/if}

      {#if documentId != null}
        <form
          class="add-form"
          aria-label="Aggiungi termine"
          onsubmit={(e) => {
            e.preventDefault();
            void addTerm();
          }}
        >
          <input
            class="edit"
            aria-label="Termine"
            placeholder="Termine"
            bind:value={form.sourceTerm}
          />
          <input
            class="edit"
            aria-label="Traduzione"
            placeholder="Traduzione"
            bind:value={form.translation}
          />
          <input
            class="edit"
            list="glossary-type-suggestions"
            aria-label="Tipo"
            placeholder="Tipo"
            bind:value={form.termType}
          />
          <datalist id="glossary-type-suggestions">
            {#each TYPE_SUGGESTIONS as suggestion}
              <option value={suggestion}></option>
            {/each}
          </datalist>
          <input
            class="edit"
            aria-label="Nota"
            placeholder="Nota"
            bind:value={form.note}
          />
          <label class="lock-toggle" title="Vincolo assoluto">
            <input type="checkbox" aria-label="Blocca" bind:checked={form.locked} />
            Bloccato
          </label>
          <button
            type="submit"
            class="add"
            disabled={adding || !isValidNewTerm(form.sourceTerm, form.translation)}
          >
            {adding ? '…' : 'Aggiungi'}
          </button>
        </form>
      {/if}

      {#if loading}
        <p class="empty">Caricamento…</p>
      {:else if documentId == null}
        <p class="empty">Apri un PDF per vedere il glossario.</p>
      {:else if terms.length === 0}
        <p class="empty">Nessun termine ancora. Verranno raccolti man mano che traduci.</p>
      {:else}
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Termine</th>
                <th>Traduzione</th>
                <th>Tipo</th>
                <th class="col-lock" title="Vincolo assoluto">Bloccato</th>
                <th>Nota</th>
                <th class="col-page" title="Pagina di prima comparsa">Pag.</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each terms as term (term.id)}
                <tr
                  id={`gloss-row-${term.id}`}
                  class:locked={term.locked}
                  class:highlight={highlightId === term.id}
                >
                  <td class="source">{term.source_term}</td>
                  <td>
                    <input
                      class="edit"
                      data-role="translation"
                      aria-label={`Traduzione di ${term.source_term}`}
                      bind:value={term.translation}
                    />
                  </td>
                  <td class="type">{typeLabel(term.term_type)}</td>
                  <td class="col-lock">
                    <input
                      type="checkbox"
                      aria-label={`Blocca ${term.source_term}`}
                      bind:checked={term.locked}
                    />
                  </td>
                  <td>
                    <input
                      class="edit"
                      aria-label={`Nota per ${term.source_term}`}
                      placeholder="—"
                      bind:value={term.note}
                    />
                  </td>
                  <td class="col-page">{pageLabel(term.first_seen_page)}</td>
                  <td>
                    <button
                      class="save"
                      onclick={() => saveTerm(term)}
                      disabled={savingId === term.id || !isValidTranslation(term.translation)}
                    >
                      {savingId === term.id ? '…' : 'Salva'}
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
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
    width: min(60rem, 94vw);
    max-height: 88vh;
    background: #ffffff;
    color: #0f0f0f;
    border-radius: 10px;
    padding: 1.1rem 1.25rem 1.25rem;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3);
    display: flex;
    flex-direction: column;
    gap: 0.9rem;
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

  .empty {
    opacity: 0.7;
    font-style: italic;
  }

  .table-wrap {
    overflow: auto;
    min-height: 0;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.88rem;
  }

  th,
  td {
    text-align: left;
    padding: 0.35rem 0.5rem;
    border-bottom: 1px solid #ececec;
    vertical-align: middle;
  }

  th {
    position: sticky;
    top: 0;
    background: #ffffff;
    font-size: 0.78rem;
    opacity: 0.7;
  }

  .col-lock,
  .col-page {
    text-align: center;
    width: 1%;
    white-space: nowrap;
  }

  .source {
    font-weight: 600;
  }

  .type {
    opacity: 0.75;
    font-style: italic;
  }

  tr.locked {
    background: rgba(57, 108, 216, 0.08);
  }

  tr.highlight {
    animation: row-flash 1.6s ease-out;
  }

  @keyframes row-flash {
    0% {
      background: rgba(255, 196, 0, 0.55);
    }
    100% {
      background: transparent;
    }
  }

  .add-form {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    padding: 0.6rem;
    border: 1px solid #ececec;
    border-radius: 8px;
    background: rgba(57, 108, 216, 0.04);
  }

  .add-form .edit {
    flex: 1 1 8rem;
    width: auto;
  }

  .lock-toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.85rem;
    white-space: nowrap;
  }

  .edit {
    width: 100%;
    box-sizing: border-box;
    padding: 0.35rem 0.5rem;
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

  button {
    border-radius: 8px;
    border: 1px solid transparent;
    padding: 0.35em 0.8em;
    font: inherit;
    font-weight: 500;
    cursor: pointer;
    background: #396cd8;
    color: #ffffff;
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  button.icon {
    background: transparent;
    color: inherit;
    padding: 0.2em 0.5em;
  }

  @media (prefers-color-scheme: dark) {
    .dialog {
      background: #1f1f1f;
      color: #f6f6f6;
    }
    th {
      background: #1f1f1f;
    }
    th,
    td {
      border-color: #3a3a3a;
    }
    .edit {
      background: #0f0f0f;
      color: #f6f6f6;
      border-color: #3a3a3a;
    }
    tr.locked {
      background: rgba(57, 108, 216, 0.18);
    }
    .add-form {
      border-color: #3a3a3a;
      background: rgba(57, 108, 216, 0.1);
    }
    .msg.info {
      color: #4ac26b;
    }
  }
</style>
