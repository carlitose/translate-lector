import { reconstruct, type TextItem } from './pdfExtract';
import { collectTextContentItems, type TextContentPage } from './pdfTextStream';

interface MarkedContentItem {
  type: string;
}

/** The PDF.js page surface needed by the production page-text extraction path. */
export interface PdfTextPage extends TextContentPage<TextItem | MarkedContentItem> {
  getViewport(parameters: { scale: number }): { width: number };
}

/** Extract and reconstruct a captured page without reading mutable document state. */
export async function extractPageText(page: PdfTextPage): Promise<string> {
  const viewport = page.getViewport({ scale: 1 });
  const contentItems = await collectTextContentItems(page);
  const items: TextItem[] = contentItems
    .filter((item): item is TextItem => 'str' in item && typeof item.str === 'string')
    .map((item) => ({
      str: item.str,
      transform: item.transform,
      width: item.width,
      height: item.height,
      hasEOL: item.hasEOL
    }));

  return reconstruct(items, viewport.width);
}
