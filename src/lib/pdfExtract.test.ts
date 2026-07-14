import { describe, it, expect } from 'vitest';
import { reconstruct, type TextItem } from './pdfExtract';

// Helper: build a pdf.js-shaped text item. `h` sets transform[3] (glyph height).
function item(str: string, x: number, y: number, w: number, h = 12): TextItem {
  return { str, transform: [h, 0, 0, h, x, y], width: w, height: h };
}

describe('reconstruct (ported from prototypes/pdfjs/extract.mjs)', () => {
  it('joins a word hyphenated across a line break in a single column', () => {
    // Line 1 ends with a soft hyphen; line 2 continues the word.
    const items: TextItem[] = [
      item('The English draws a terminolo-', 72, 700, 300),
      item('gical distinction between them.', 72, 680, 300)
    ];
    const out = reconstruct(items, 600);
    expect(out).toBe('The English draws a terminological distinction between them.');
  });

  it('preserves reading order across a two-column layout', () => {
    // Three lines, each with a left-column item and a right-column item split by
    // a wide gutter. Expected: whole left column first, then whole right column.
    const items: TextItem[] = [
      item('Left one', 50, 700, 150),
      item('Right one', 350, 700, 150),
      item('Left two', 50, 680, 150),
      item('Right two', 350, 680, 150),
      item('Left three', 50, 660, 150),
      item('Right three', 350, 660, 150)
    ];
    const out = reconstruct(items, 600);
    expect(out).toBe(
      ['Left one', 'Left two', 'Left three', 'Right one', 'Right two', 'Right three'].join('\n')
    );
  });

  it('orders a single column top-to-bottom regardless of item delivery order', () => {
    const items: TextItem[] = [
      item('second line', 72, 680, 200),
      item('first line', 72, 700, 200),
      item('third line', 72, 660, 200)
    ];
    const out = reconstruct(items, 600);
    expect(out).toBe('first line\nsecond line\nthird line');
  });
});

describe('reconstruct — paragraph-aware separation', () => {
  it('inserts a paragraph separator when the vertical gap exceeds typical line spacing', () => {
    // Three closely-spaced lines (spacing 20), then a ~3x gap to a new paragraph.
    const items: TextItem[] = [
      item('First paragraph line one', 72, 700, 200),
      item('first paragraph line two', 72, 680, 200),
      item('first paragraph line three', 72, 660, 200),
      item('Second paragraph begins here', 72, 600, 200)
    ];
    const out = reconstruct(items, 600);
    expect(out).toBe(
      'First paragraph line one\n' +
        'first paragraph line two\n' +
        'first paragraph line three\n\n' +
        'Second paragraph begins here'
    );
  });

  it('keeps closely-spaced lines in the same paragraph (no separator)', () => {
    const items: TextItem[] = [
      item('Line one of one paragraph', 72, 700, 200),
      item('line two of one paragraph', 72, 680, 200),
      item('line three of one paragraph', 72, 660, 200),
      item('line four of one paragraph', 72, 640, 200)
    ];
    const out = reconstruct(items, 600);
    expect(out).not.toContain('\n\n');
    expect(out).toBe(
      [
        'Line one of one paragraph',
        'line two of one paragraph',
        'line three of one paragraph',
        'line four of one paragraph'
      ].join('\n')
    );
  });

  it('still de-hyphenates across a line break with paragraph detection active', () => {
    const items: TextItem[] = [
      item('The English draws a terminolo-', 72, 700, 300),
      item('gical distinction between them.', 72, 680, 300),
      item('another close line here', 72, 660, 200),
      item('A separate paragraph follows.', 72, 600, 200)
    ];
    const out = reconstruct(items, 600);
    expect(out).toBe(
      'The English draws a terminological distinction between them.\n' +
        'another close line here\n\n' +
        'A separate paragraph follows.'
    );
  });

  it('does not insert a paragraph break at a two-column boundary', () => {
    // Reading order jumps from the bottom of the left column back up to the top
    // of the right column: y increases (negative gap), which must not be read as
    // a paragraph break.
    const items: TextItem[] = [
      item('Left one', 50, 700, 150),
      item('Right one', 350, 700, 150),
      item('Left two', 50, 680, 150),
      item('Right two', 350, 680, 150),
      item('Left three', 50, 660, 150),
      item('Right three', 350, 660, 150)
    ];
    const out = reconstruct(items, 600);
    expect(out).not.toContain('\n\n');
    expect(out).toBe(
      ['Left one', 'Left two', 'Left three', 'Right one', 'Right two', 'Right three'].join('\n')
    );
  });

  it('loses no words when paragraphs are separated', () => {
    const items: TextItem[] = [
      item('alpha beta gamma', 72, 700, 200),
      item('delta epsilon zeta', 72, 680, 200),
      item('eta theta iota', 72, 580, 200)
    ];
    const out = reconstruct(items, 600);
    expect(out).toContain('\n\n'); // a paragraph boundary exists
    const words = out.split(/\s+/).filter(Boolean);
    expect(words).toEqual([
      'alpha',
      'beta',
      'gamma',
      'delta',
      'epsilon',
      'zeta',
      'eta',
      'theta',
      'iota'
    ]);
  });
});
