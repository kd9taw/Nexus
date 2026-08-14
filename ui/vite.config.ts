// `vitest/config` re-exports Vite's defineConfig with the `test` key added to the type.
// Importing it from 'vite' typechecks everything EXCEPT `test`, so `tsc -b` fails with
// TS2769 — and `npm run build` is `tsc -b && vite build`, which four CI jobs run.
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { execSync } from 'node:child_process'

// A build stamp (commit hash + build time) baked in at build time, so the app can SHOW which
// build is running — the product version string is always "0.2.0", which made it impossible
// to tell whether a fresh install actually took. Displayed in Settings.
function buildId(): string {
  let hash = 'local'
  try {
    hash = execSync('git rev-parse --short HEAD', { stdio: ['ignore', 'pipe', 'ignore'] })
      .toString()
      .trim()
  } catch {
    /* not a git checkout — leave "local" */
  }
  const now = new Date().toISOString().slice(0, 16).replace('T', ' ')
  return `${now}Z · ${hash}`
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  define: {
    __BUILD_ID__: JSON.stringify(buildId()),
  },
  // Relative base so the built bundle works when served from inside Tauri.
  base: './',
  server: {
    port: 5173,
    host: true,
  },
  // Vitest: see src/test-setup.ts — Node 25's built-in `localStorage` shadows jsdom's
  // and breaks every suite that clears storage between cases.
  test: {
    setupFiles: ['./src/test-setup.ts'],
  },
  build: {
    outDir: 'dist',
    sourcemap: false,
  },
})
