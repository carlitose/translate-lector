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
 * Whether a translation may be saved: non-empty once trimmed. A term (locked or
 * not) with a blank translation carries no useful constraint, so we block it.
 */
export function isValidTranslation(value: string | null | undefined): boolean {
  return typeof value === 'string' && value.trim().length > 0;
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

/** Display label for a term's type, with a dash fallback when unset. */
export function typeLabel(type: string | null | undefined): string {
  return typeof type === 'string' && type.trim().length > 0 ? type.trim() : '—';
}
