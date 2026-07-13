// Pure, framework-free helpers for session restore & the "Recenti" list
// (ticket 11, FR09/FR10/EC06). Kept out of the Svelte component so the boot
// decision and display formatting are unit-testable without a DOM or the
// Tauri bridge.

/** The most-recent session joined to its document (mirrors Rust `LastSession`). */
export interface LastSession {
  session_id: number;
  document_id: number;
  target_language: string;
  current_page: number;
  file_path: string;
  file_hash: string;
  title: string;
  total_pages: number;
}

/** A recently-opened document for the "Recenti" list (mirrors `RecentDocument`). */
export interface RecentDocument {
  document_id: number;
  file_path: string;
  file_hash: string;
  title: string;
  total_pages: number;
  last_opened_at: string;
}

/**
 * What to do at startup given the last session and whether its file still
 * exists on disk (EC06). `none` when there is nothing to restore, `restore`
 * when the file is present, `missing` when it moved or was deleted.
 */
export type RestoreDecision = 'none' | 'restore' | 'missing';

export function restoreDecision(
  session: LastSession | null | undefined,
  fileExists: boolean
): RestoreDecision {
  if (!session) return 'none';
  return fileExists ? 'restore' : 'missing';
}

/** Base file name (with extension) of a path, tolerating both slash styles. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/**
 * Clamp a saved page into the valid 1..total range (D1, page-discrete). Guards
 * against a stored `current_page` that outruns a file whose page count changed.
 */
export function clampPage(page: number, total: number): number {
  if (total < 1) return 1;
  return Math.min(Math.max(page, 1), total);
}
