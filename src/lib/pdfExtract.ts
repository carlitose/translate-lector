// Coordinate-based text reconstruction for pdf.js `getTextContent()` output.
//
// pdf.js delivers glyph runs in drawing order, not reading order. This module
// rebuilds a natural reading order: it groups runs into lines by baseline y,
// detects a two-column split by a vertical x-gap, emits the left column fully
// before the right, and joins words hyphenated across a line break.
//
// Ported from the validated prototype `prototypes/pdfjs/extract.mjs` (ticket 06).

/** A text run as produced by pdf.js `getTextContent().items`. */
export interface TextItem {
  str: string;
  /** [a, b, c, d, e, f]; e = x (left), f = y (baseline, origin bottom-left). */
  transform: number[];
  width: number;
  height: number;
  hasEOL?: boolean;
}

/** A reconstructed line, tagged with its original page y (origin bottom-left). */
export interface Line {
  y: number;
  text: string;
}

interface Glyph {
  str: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

interface RawLine {
  y: number;
  items: Glyph[];
}

/** Reconstruct the page text in reading order (columns, de-hyphenated). */
export function reconstruct(items: TextItem[], pageWidth: number): string {
  return linesToText(reconstructLines(items, pageWidth));
}

/**
 * Reconstruct ordered lines in reading order (before de-hyphenation), so
 * callers can do document-level analysis such as header/footer stripping.
 * `y` is the original page y (origin bottom-left) of the line.
 */
export function reconstructLines(items: TextItem[], pageWidth: number): Line[] {
  const glyphs: Glyph[] = items
    .filter((it) => it.str.trim().length > 0)
    .map((it) => ({
      str: it.str,
      x: it.transform[4],
      y: it.transform[5],
      w: it.width,
      h: Math.abs(it.transform[3]) || it.height || 10
    }));
  if (glyphs.length === 0) return [];

  // Group into lines: same baseline y within a tolerance (~half glyph height).
  glyphs.sort((a, b) => b.y - a.y || a.x - b.x);
  const lines: RawLine[] = [];
  const tol = 4;
  for (const g of glyphs) {
    let line = lines.find((l) => Math.abs(l.y - g.y) <= tol);
    if (!line) {
      line = { y: g.y, items: [] };
      lines.push(line);
    }
    line.items.push(g);
  }
  for (const l of lines) l.items.sort((a, b) => a.x - b.x);
  lines.sort((a, b) => b.y - a.y); // top to bottom

  // Detect a column split: a vertical band with no glyph separating left from
  // right glyphs across many lines.
  const splitX = detectColumnSplit(lines, pageWidth);

  let lineTexts: Line[];
  if (splitX != null) {
    // Full left column, then full right column. A contiguous full-width line
    // (a title spanning the split with no gutter) is kept whole with the left.
    const left: Line[] = [];
    const right: Line[] = [];
    for (const l of lines) {
      if (isContiguousFullWidth(l, pageWidth)) {
        left.push({ y: l.y, text: joinItems(l.items) });
        continue;
      }
      const li = l.items.filter((g) => g.x < splitX);
      const ri = l.items.filter((g) => g.x >= splitX);
      if (li.length) left.push({ y: l.y, text: joinItems(li) });
      if (ri.length) right.push({ y: l.y, text: joinItems(ri) });
    }
    lineTexts = [...left, ...right];
  } else {
    lineTexts = lines.map((l) => ({ y: l.y, text: joinItems(l.items) }));
  }
  return lineTexts;
}

/**
 * Join ordered lines into text, applying de-hyphenation at line ends and
 * inserting a blank line (a paragraph separator, i.e. a double '\n') where the
 * vertical gap between two consecutive lines is clearly larger than the typical
 * line spacing. Downstream paragraph chunking relies on these separators.
 *
 * Paragraph detection works on each line's page y (origin bottom-left): within a
 * column y decreases top-to-bottom, so the gap to the next line is
 * `prev.y - cur.y` (positive, ~one line height). A gap well beyond the typical
 * spacing marks a new paragraph. The typical spacing is the median of those
 * positive gaps, robust to the occasional large paragraph gap.
 *
 * Column boundaries are handled by construction: reading order lists the whole
 * left column before the right, so the jump from the bottom of the left column
 * to the top of the right column makes y increase — a negative gap, which never
 * clears the threshold and so is never mistaken for a paragraph break.
 */
export function linesToText(lineTexts: Line[]): string {
  const typical = typicalLineSpacing(lineTexts);
  // Gap beyond this * typical spacing => new paragraph. Within a paragraph the
  // line-to-line gaps cluster tightly around the median (wrapped lines share the
  // same leading), so a real paragraph break only needs a modest increment above
  // it. 1.5 was too conservative: paragraphs with slightly looser leading (~1.3x
  // the typical spacing) were fused into oversized units. 1.25 gives a 25% margin
  // over the typical line gap — wide enough to ignore wrapped-line jitter, tight
  // enough to catch those modestly-spaced paragraph breaks. (See ticket 12.)
  const paragraphFactor = 1.25;
  let text = '';
  for (let i = 0; i < lineTexts.length; i++) {
    const cur = lineTexts[i].text;
    const dehyph = /([A-Za-z])[-‐]$/.test(cur);
    if (dehyph && i + 1 < lineTexts.length) {
      text += cur.replace(/[-‐]$/, '');
    } else if (i + 1 < lineTexts.length) {
      const gap = lineTexts[i].y - lineTexts[i + 1].y;
      const paragraphBreak = typical > 0 && gap > typical * paragraphFactor;
      text += cur + (paragraphBreak ? '\n\n' : '\n');
    } else {
      text += cur + '\n';
    }
  }
  return text.trim();
}

/**
 * Median of the positive line-to-line y-gaps, i.e. the typical within-column
 * line spacing. Negative gaps (column boundary jumps back up the page) are
 * ignored so a column switch never skews the estimate. Returns 0 when there is
 * no gap to measure (fewer than two lines, or a single column boundary).
 */
function typicalLineSpacing(lineTexts: Line[]): number {
  const gaps: number[] = [];
  for (let i = 1; i < lineTexts.length; i++) {
    const gap = lineTexts[i - 1].y - lineTexts[i].y;
    if (gap > 0) gaps.push(gap);
  }
  if (gaps.length === 0) return 0;
  gaps.sort((a, b) => a - b);
  const mid = Math.floor(gaps.length / 2);
  return gaps.length % 2 === 0 ? (gaps[mid - 1] + gaps[mid]) / 2 : gaps[mid];
}

/** Insert spaces between items when there is a horizontal gap. */
function joinItems(items: Glyph[]): string {
  let out = '';
  for (let i = 0; i < items.length; i++) {
    const g = items[i];
    if (i > 0) {
      const prev = items[i - 1];
      const gap = g.x - (prev.x + prev.w);
      const space = g.h * 0.25;
      if (gap > space && !/\s$/.test(out)) out += ' ';
    }
    out += g.str;
  }
  return out.replace(/[ \t]+/g, ' ').trim();
}

/**
 * A line is a contiguous full-width heading if it spans most of the page AND
 * has no wide internal gap (a column gutter). Multi-column body lines also span
 * the page width, but they contain a wide gutter, so they are NOT full-width.
 */
function isContiguousFullWidth(l: RawLine, pageWidth: number): boolean {
  const sorted = [...l.items].sort((a, b) => a.x - b.x);
  const minX = sorted[0].x;
  const maxX = Math.max(...sorted.map((g) => g.x + g.w));
  if (maxX - minX <= pageWidth * 0.7) return false;
  let maxGap = 0;
  for (let i = 1; i < sorted.length; i++) {
    maxGap = Math.max(maxGap, sorted[i].x - (sorted[i - 1].x + sorted[i - 1].w));
  }
  return maxGap < pageWidth * 0.05; // no column gutter
}

/**
 * Scan candidate split x positions; a real column boundary is an x where glyphs
 * on many lines fall clearly on one side and few straddle it. Returns the split
 * x, or null for single-column pages.
 */
function detectColumnSplit(lines: RawLine[], pageWidth: number): number | null {
  const bodyLines = lines.filter((l) => l.items.length > 0);
  if (bodyLines.length < 3) return null;
  const isFullWidth = (l: RawLine) => isContiguousFullWidth(l, pageWidth);
  const candidates: { x: number; score: number }[] = [];
  for (let frac = 0.35; frac <= 0.65; frac += 0.05) {
    const x = pageWidth * frac;
    let leftLines = 0;
    let rightLines = 0;
    let straddle = 0;
    for (const l of bodyLines) {
      if (isFullWidth(l)) continue; // headings/titles ignored for split test
      const hasLeft = l.items.some((g) => g.x + g.w < x);
      const hasRight = l.items.some((g) => g.x > x);
      const crosses = l.items.some((g) => g.x < x && g.x + g.w > x);
      if (crosses) straddle++;
      if (hasLeft) leftLines++;
      if (hasRight) rightLines++;
    }
    const maxStraddle = Math.max(1, Math.floor(Math.min(leftLines, rightLines) * 0.25));
    if (leftLines >= 3 && rightLines >= 3 && straddle <= maxStraddle) {
      candidates.push({ x, score: Math.min(leftLines, rightLines) - straddle });
    }
  }
  if (candidates.length === 0) return null;
  candidates.sort((a, b) => b.score - a.score);
  return candidates[0].x;
}
