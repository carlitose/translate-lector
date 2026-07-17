// Pure, framework-free helpers for the glossary panel (ticket 10, UC03).
// Kept out of the Svelte component so the mapping/validation is unit-testable
// without a DOM or the Tauri bridge.

/** A glossary row as returned by the `list_glossary` core command (§4.3). */
export interface GlossaryTermRecord {
  id: number;
  source_term: string;
  translation: string;
  term_type: string;
  locked: boolean;
  note: string;
  first_seen_page: number;
}

/** Argument shape for the `update_glossary_term` core command. */
export interface UpdateTermArgs {
  id: number;
  translation: string;
  note: string;
  locked: boolean;
}

/**
 * Result returned by the `add_glossary_term` core command: `inserted` when a new
 * row was created, `duplicate` when the term already existed. `id` is the row's
 * id in both cases (the new row, or the pre-existing one).
 */
export interface AddTermResult {
  status: 'inserted' | 'duplicate';
  id: number;
}

/**
 * Editable values collected by the "Aggiungi termine" form. `locked` defaults
 * to `true` (a manually added term is authoritative — product decision #1).
 */
export interface AddTermFormValues {
  sourceTerm: string;
  translation: string;
  termType: string;
  note: string;
  locked: boolean;
}

/**
 * Argument shape for the `add_glossary_term` core command (camelCase, as Tauri
 * expects). `first_seen_page` is not carried — the backend stamps the sentinel
 * `0` ("manuale") itself.
 */
export interface AddTermArgs {
  documentId: number;
  sourceTerm: string;
  translation: string;
  termType: string;
  note: string;
  locked: boolean;
}

/**
 * Whether a translation may be saved: non-empty once trimmed. A term (locked or
 * not) with a blank translation carries no useful constraint, so we block it.
 */
export function isValidTranslation(value: string | null | undefined): boolean {
  return typeof value === 'string' && value.trim().length > 0;
}

/**
 * Whether a brand-new term may be added: both `source_term` and `translation`
 * must be non-empty once trimmed (product decision #5). `type`/`note` are
 * optional. Reuses {@link isValidTranslation} for each field.
 */
export function isValidNewTerm(
  sourceTerm: string | null | undefined,
  translation: string | null | undefined
): boolean {
  return isValidTranslation(sourceTerm) && isValidTranslation(translation);
}

/**
 * Map a (possibly edited) term record to the exact args expected by
 * `update_glossary_term`, trimming the free-text fields. Only the mutable
 * fields travel — `source_term`, `type` and `first_seen_page` are immutable.
 */
export function toUpdateArgs(term: GlossaryTermRecord): UpdateTermArgs {
  return {
    id: term.id,
    translation: term.translation.trim(),
    note: term.note.trim(),
    locked: term.locked
  };
}

/**
 * Map the "Aggiungi termine" form values to the exact args expected by
 * `add_glossary_term`, trimming the free-text fields.
 */
export function toAddArgs(documentId: number, form: AddTermFormValues): AddTermArgs {
  return {
    documentId,
    sourceTerm: form.sourceTerm.trim(),
    translation: form.translation.trim(),
    termType: form.termType.trim(),
    note: form.note.trim(),
    locked: form.locked
  };
}

/** Display label for a term's type, with a dash fallback when unset. */
export function typeLabel(type: string | null | undefined): string {
  return typeof type === 'string' && type.trim().length > 0 ? type.trim() : '—';
}

/**
 * Display label for the "Pag." column. A manually added term carries the
 * sentinel `first_seen_page === 0` and is shown as "manuale" (product decision
 * #3); every other term shows its page number.
 */
export function pageLabel(firstSeenPage: number): string {
  return firstSeenPage === 0 ? 'manuale' : String(firstSeenPage);
}
