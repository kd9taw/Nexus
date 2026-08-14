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
  const git = (args: string): string | null => {
    try {
      const out = execSync(`git ${args}`, { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim()
      return out.length > 0 ? out : null
    } catch {
      return null
    }
  }

  const hash = git('rev-parse --short HEAD') ?? 'local'
  // Branch and fork matter as much as the hash once anyone builds from a fork or a
  // topic branch: two builds of different branches at the same commit-less version
  // are otherwise indistinguishable, and "which branch is this?" is the first
  // question asked of a bug report from a self-built binary.
  // ⚠️ NOT `rev-parse --abbrev-ref HEAD`: on a DETACHED head that prints the literal
  // string "HEAD" and exits 0, so a `?? 'detached'` fallback is unreachable (measured).
  // release.yml triggers on `tags: ['v*']` and actions/checkout leaves HEAD detached, so
  // every published release would have named its branch "HEAD" — the exact field this
  // stamp exists to add. `symbolic-ref --short` exits 128 with empty output when detached,
  // so the helper's null path fires. A tag build names its tag, which is what you want on
  // a release artifact.
  const branch =
    git('describe --tags --exact-match HEAD') ?? git('symbolic-ref --short HEAD') ?? 'detached'
  const repo = (() => {
    const url = git('remote get-url origin')
    if (!url) return null
    const tail = url.replace(/\.git$/, '').replace(/\/$/, '').split(/[/:]/).slice(-2)
    return tail.length === 2 ? tail.join('/') : null
  })()
  // A working tree with uncommitted changes is NOT the commit it names.
  // Tracked source only. `status --porcelain` counts refreshed data resources and any
  // untracked scratch file, and this repo's checkout is shared by several agents — an
  // unconditional probe leaves `-dirty` permanently on, which stops it being a signal.
  const dirty = git('status --porcelain --untracked-files=no') ? '-dirty' : ''

  const now = new Date().toISOString().slice(0, 16).replace('T', ' ')
  const where = repo ? `${repo} ${branch}` : branch
  return `${now}Z · ${where}@${hash}${dirty}`
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
