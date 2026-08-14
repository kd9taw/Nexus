import { defineConfig } from 'vite'
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
  const branch = git('rev-parse --abbrev-ref HEAD') ?? 'detached'
  const repo = (() => {
    const url = git('remote get-url origin')
    if (!url) return null
    const tail = url.replace(/\.git$/, '').replace(/\/$/, '').split(/[/:]/).slice(-2)
    return tail.length === 2 ? tail.join('/') : null
  })()
  // A working tree with uncommitted changes is NOT the commit it names.
  const dirty = git('status --porcelain') ? '-dirty' : ''

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
  build: {
    outDir: 'dist',
    sourcemap: false,
  },
})
