import { defineConfig } from 'vitest/config';

// Standalone Vitest config (no SvelteKit plugin) for pure TS unit tests such as
// the pdf.js text-reconstruction module. Keeps `npm run test` independent of the
// dev server / adapter-static pipeline used by `npm run build`.
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node'
  }
});
