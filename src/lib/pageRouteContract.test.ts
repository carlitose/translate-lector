import { describe, expect, it } from 'vitest';
import routeSource from '../routes/+page.svelte?raw';

describe('+page production PDF extraction wiring', () => {
  it('delegates ordinary page extraction to the WKWebView-compatible adapter', () => {
    const showPageSource = routeSource.match(
      /async function showPage[\s\S]*?(?=\n  \/\*\* Does any sampled page)/
    );

    expect(routeSource).toContain("import { extractPageText } from '$lib/pdfPageText';");
    expect(routeSource).toMatch(
      /return\s+extractPageText\(await\s+document\.getPage\(pageNo\)\)/
    );
    expect(showPageSource?.[0]).toContain('const text = await extractPageText(page);');
    expect(routeSource).not.toMatch(/\.getTextContent\s*\(/);
  });
});

describe('+page PDF lifecycle wiring', () => {
  it('retains the public loading task and connects render-safe component teardown', () => {
    expect(routeSource).toContain('PdfDocumentLoadController');
    expect(routeSource).toMatch(
      /openDocument:\s*\(data\)\s*=>\s*pdfjsLib\.getDocument\(\{ data \}\)/
    );
    expect(routeSource).not.toMatch(
      /openDocument:[^\n]*pdfjsLib\.getDocument\(\{ data \}\)\.promise/
    );
    expect(routeSource).toContain(
      'beforeDestroy: async () => pageRenderer.waitForIdle()'
    );
    expect(routeSource).toMatch(
      /onDestroy\(async \(\) => \{[\s\S]*?pageRenderer\.beginDocument\(\);[\s\S]*?await documentLoader\.dispose\(\);/
    );
  });

  it('uses controller identities for route navigation and prefetch continuations', () => {
    expect(routeSource).toContain('const activeDocument = documentLoader.captureActive();');
    expect(routeSource).toContain('!documentLoader.isActive(activeDocument)');
    expect(routeSource).toContain('void prefetchNextPage(requested)');
  });
});
