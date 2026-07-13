// Generates 3 representative test PDFs for the pdf.js extraction prototype.
// No network required — pdfkit is a pure-JS generator installed from npm.
import PDFDocument from 'pdfkit';
import { createWriteStream } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, 'fixtures');

function done(doc, path) {
  return new Promise((resolve) => {
    const stream = createWriteStream(path);
    doc.pipe(stream);
    stream.on('finish', resolve);
    doc.end();
  });
}

// (a) Single-column prose, with a hyphenated word broken across a line end.
async function singleColumn() {
  const doc = new PDFDocument({ size: 'A4', margin: 72 });
  doc.fontSize(18).text('The History of Translation', { align: 'center' });
  doc.moveDown();
  doc.fontSize(12);
  // Manually break a hyphenated word across lines by forcing width so wrapping
  // splits it, plus one explicit hard hyphen at a line end.
  const para1 =
    'Translation is the communication of the meaning of a source language text by means ' +
    'of an equivalent target language text. The English language draws a terminolo' +
    '-\ngical distinction between translating and interpreting.';
  doc.text(para1, { width: 400, lineGap: 2 });
  doc.moveDown();
  const para2 =
    'Because of the laboriousness of the translation process, since the 1940s efforts ' +
    'have been made to automate translation or to mechanically aid the human transla' +
    '-\ntor. More recently, the rise of the Internet has fostered a world-wide market.';
  doc.text(para2, { width: 400, lineGap: 2 });
  await done(doc, join(OUT, 'a-single-column.pdf'));
}

// (b) True two-column layout using absolute x/y positioning.
async function twoColumn() {
  const doc = new PDFDocument({ size: 'A4', margin: 50 });
  const pageW = doc.page.width;
  const top = 60;
  const colW = 230;
  const leftX = 50;
  const rightX = pageW - 50 - colW; // right column starts after a wide gutter

  doc.fontSize(16).text('Academic Two-Column Paper', 50, 30, { width: pageW - 100, align: 'center' });

  const leftText =
    'Abstract. This paper studies text extraction from multi-column documents. ' +
    'Reading order is the central challenge because glyphs are stored in drawing ' +
    'order, not logical order. A naive concatenation interleaves the two columns ' +
    'and destroys meaning. We evaluate a coordinate-based reconstruction heuristic.';

  const rightText =
    'Method. We group text items by their vertical position into lines, then detect ' +
    'a large horizontal gap that separates the left column from the right column. ' +
    'Each column is emitted in full before moving to the next, restoring a natural ' +
    'top-to-bottom, left-to-right reading order for downstream translation.';

  doc.fontSize(11);
  doc.text(leftText, leftX, top, { width: colW, lineGap: 2 });
  doc.text(rightText, rightX, top, { width: colW, lineGap: 2 });
  await done(doc, join(OUT, 'b-two-column.pdf'));
}

// (c) Repeated running header + footer (page number) plus body text, 2 pages.
async function headerFooter() {
  const doc = new PDFDocument({ size: 'A4', margin: 72, bufferPages: true });
  const bodyParas = [
    'The quick brown fox jumps over the lazy dog. This sentence is used as body content ' +
      'so that the running header and footer can be observed interleaving with real prose.',
    'A second paragraph continues the discussion. Running heads repeat on every page and ' +
      'must be stripped before the body text is handed to a translation engine.',
    'A third paragraph fills the first page so that the document naturally spans two pages ' +
      'and the header and footer repeat identically, which is the signal used to detect them.'
  ];
  doc.fontSize(12);
  // Write enough to spill onto a second page so the header/footer repeat.
  for (let i = 0; i < 30; i++) {
    doc.text(bodyParas[i % bodyParas.length]);
    doc.moveDown();
  }
  // Add repeated header/footer to every buffered page.
  const range = doc.bufferedPageRange();
  for (let i = range.start; i < range.start + range.count; i++) {
    doc.switchToPage(i);
    const oldBottom = doc.page.margins.bottom;
    const oldTop = doc.page.margins.top;
    doc.page.margins.bottom = 0;
    doc.page.margins.top = 0;
    doc.fontSize(9).text('Chapter 3 — The Art of Extraction', 72, 30, {
      width: doc.page.width - 144,
      align: 'left'
    });
    doc.fontSize(9).text(`Page ${i - range.start + 1}`, 72, doc.page.height - 40, {
      width: doc.page.width - 144,
      align: 'center'
    });
    doc.page.margins.bottom = oldBottom;
    doc.page.margins.top = oldTop;
  }
  await done(doc, join(OUT, 'c-header-footer.pdf'));
}

await singleColumn();
await twoColumn();
await headerFooter();
console.log('Generated fixtures in', OUT);
