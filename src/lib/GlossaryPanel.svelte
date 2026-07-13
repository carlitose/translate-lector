<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { isValidTranslation, toUpdateArgs, typeLabel, type GlossaryTermRecord } from './glossary';

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
                <tr class:locked={term.locked}>
                  <td class="source">{term.source_term}</td>
                  <td>
                    <input
                      class="edit"
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
                  <td class="col-page">{term.first_seen_page}</td>
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
    .msg.info {
      color: #4ac26b;
    }
  }
</style>
