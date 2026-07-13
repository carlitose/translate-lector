// Prototype: extract text from PDFs with pdf.js (Node legacy build) and compare
// RAW concatenation vs a coordinate-based reconstruction.
//
// pdf.js API used (pdfjs-dist v6.1.200, legacy/build/pdf.mjs):
//   getDocument({ data }).promise           -> PDFDocumentProxy
//   doc.getPage(n)                           -> PDFPageProxy
//   page.getTextContent()                    -> { items: [{ str, transform:[a,b,c,d,e,f], width, height, hasEOL }] }
//   transform[4] = x (left), transform[5] = y (baseline, origin bottom-left)
//
// Run: node prototypes/pdfjs/extract.mjs
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import * as pdfjs from 'pdfjs-dist/legacy/build/pdf.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIX = join(__dirname, 'fixtures');

// ---- RAW extraction: concatenate item.str in pdf.js delivery order. ----
function rawText(items) {
  let out = '';
  for (const it of items) {
    out += it.str;
    if (it.hasEOL) out += '\n';
    else out += ' ';
  }
  return out.replace(/[ \t]+/g, ' ').trim();
}

// ---- Reconstruction: group into lines by y, detect column split by x-gap,
//      order columns, join hyphenated line-ends. ----
// Returns the final joined text.
function reconstruct(items, pageWidth) {
  return linesToText(reconstructLines(items, pageWidth));
}

// Returns ordered line objects { y, text } in reading order (before de-hyphen
// join), so callers can do document-level analysis such as header/footer
// stripping. `y` is the ORIGINAL page y (origin bottom-left) of the line.
function reconstructLines(items, pageWidth) {
  const glyphs = items
    .filter((it) => it.str.trim().length > 0)
    .map((it) => ({
      str: it.str,
      x: it.transform[4],
      y: it.transform[5],
      w: it.width,
      h: Math.abs(it.transform[3]) || it.height || 10
    }));
  if (glyphs.length === 0) return '';

  // Group into lines: same baseline y within a tolerance (~half glyph height).
  glyphs.sort((a, b) => b.y - a.y || a.x - b.x);
  const lines = [];
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

  // Detect a column split: is there a vertical band (x range) with no glyph
  // that separates left glyphs from right glyphs across many lines?
  const splitX = detectColumnSplit(lines, pageWidth);

  let lineTexts;
  if (splitX != null) {
    // Full left column, then full right column. A contiguous full-width line
    // (title spanning the split, with no column gutter) is kept whole and
    // emitted with the left column.
    const left = [];
    const right = [];
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

// Join ordered lines into text, applying de-hyphenation at line ends.
function linesToText(lineTexts) {
  let text = '';
  for (let i = 0; i < lineTexts.length; i++) {
    const cur = lineTexts[i].text;
    const dehyph = /([A-Za-z])[-‐]$/.test(cur);
    if (dehyph && i + 1 < lineTexts.length) {
      text += cur.replace(/[-‐]$/, '');
    } else {
      text += cur + '\n';
    }
  }
  return text.trim();
}

// Insert spaces between items when there is a horizontal gap.
function joinItems(items) {
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

// A line is a contiguous full-width heading if it spans most of the page AND
// has no wide internal gap (a column gutter). Multi-column body lines also span
// the page width, but they contain a wide gutter, so they are NOT full-width.
function isContiguousFullWidth(l, pageWidth) {
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

// Heuristic: scan candidate split x positions; a real column boundary is an
// x where glyphs on many lines fall clearly on one side or the other and few
// straddle it. Returns split x or null for single-column pages.
function detectColumnSplit(lines, pageWidth) {
  const bodyLines = lines.filter((l) => l.items.length > 0);
  if (bodyLines.length < 3) return null;
  // A contiguous full-width line (title/heading) legitimately straddles any
  // split-x and must not veto column detection.
  const isFullWidth = (l) => isContiguousFullWidth(l, pageWidth);
  const candidates = [];
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
    // Good split: both sides populated on many lines, negligible straddling.
    // A few straddling lines are tolerated (e.g. a centered sub-page-width
    // heading) as long as the two columns dominate.
    const maxStraddle = Math.max(1, Math.floor(Math.min(leftLines, rightLines) * 0.25));
    if (leftLines >= 3 && rightLines >= 3 && straddle <= maxStraddle) {
      candidates.push({ x, score: Math.min(leftLines, rightLines) - straddle });
    }
  }
  if (candidates.length === 0) return null;
  candidates.sort((a, b) => b.score - a.score);
  return candidates[0].x;
}

// Document-level: a line whose text repeats near the SAME top/bottom margin
// band on 2+ pages is a running header/footer and is stripped.
function stripHeaderFooter(pages) {
  if (pages.length < 2) return pages.map((pg) => pg.lines);
  const band = pg => pg.height * 0.12; // top/bottom 12% of page
  const key = (pg, l) => {
    const fromTop = pg.height - l.y;
    const fromBottom = l.y;
    if (fromTop <= band(pg)) return 'H:' + l.text.replace(/\d+/g, '#');
    if (fromBottom <= band(pg)) return 'F:' + l.text.replace(/\d+/g, '#');
    return null;
  };
  const counts = new Map();
  for (const pg of pages)
    for (const l of pg.lines) {
      const k = key(pg, l);
      if (k) counts.set(k, (counts.get(k) || 0) + 1);
    }
  const repeated = new Set([...counts].filter(([, c]) => c >= 2).map(([k]) => k));
  return pages.map((pg) => pg.lines.filter((l) => !repeated.has(key(pg, l))));
}

async function processFile(file) {
  const data = new Uint8Array(readFileSync(join(FIX, file)));
  const doc = await pdfjs.getDocument({ data }).promise;
  console.log('\n' + '='.repeat(70));
  console.log('FILE:', file, '| pages:', doc.numPages);
  console.log('='.repeat(70));
  const pages = [];
  for (let p = 1; p <= doc.numPages; p++) {
    const page = await doc.getPage(p);
    const viewport = page.getViewport({ scale: 1 });
    const { items } = await page.getTextContent();
    console.log(`\n----- PAGE ${p} : RAW -----`);
    console.log(rawText(items));
    const lines = reconstructLines(items, viewport.width);
    console.log(`\n----- PAGE ${p} : RECONSTRUCTED -----`);
    console.log(linesToText(lines));
    pages.push({ page: p, height: viewport.height, lines });
  }
  if (doc.numPages >= 2) {
    const stripped = stripHeaderFooter(pages);
    console.log(`\n----- DOC : RECONSTRUCTED + HEADER/FOOTER STRIPPED -----`);
    console.log(stripped.map(linesToText).join('\n'));
  }
}

const files = readdirSync(FIX).filter((f) => f.endsWith('.pdf')).sort();
for (const f of files) await processFile(f);
