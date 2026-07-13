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
