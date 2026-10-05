// `vitest/config` re-exports Vite's defineConfig with the `test` key added to the type.
// Importing it from 'vite' typechecks everything EXCEPT `test`, so `tsc -b` fails with
// TS2769 — and `npm run build` is `tsc -b && vite build`, which four CI jobs run.
import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { execSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { failuresReporter } from './vitest-failures-reporter'
import { bundledLicenses } from './remote-licenses'

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
  plugins: [react(), {
    // The license texts of the npm packages in the desktop bundle. The hosted browser's generator
    // (remote-licenses.ts) reads them for the modules the chunks actually contain, and a package
    // that ships without its text stops the build. Settings ▸ Licenses shows this file. The
    // installers carry the same text as resources/ui/THIRD-PARTY.txt (licenses/ui/ in the
    // repository), and CI's `ui` job fails when that committed copy differs from what this emits.
    // The Rust crates' texts go beside it as rust/THIRD-PARTY.txt, copied from
    // licenses/rust/THIRD-PARTY.txt, which scripts/gen-rust-licenses.py writes and CI's `deny`
    // job keeps current with src-tauri's Cargo.lock; the dialog shows both.
    name: 'desktop-license-texts',
    generateBundle(_options, bundle) {
      const modules = new Set(Object.values(bundle).flatMap(chunk => chunk.type === 'chunk' ? Object.keys(chunk.modules) : []))
      this.emitFile({ type: 'asset', fileName: 'THIRD-PARTY.txt', source:
        'Nexus — third-party notices for the app\'s interface\n\n' +
        'The interface bundles the npm packages below. Each is followed by the license files its\n' +
        'published package carries or, where a package publishes none, by the reviewed upstream text\n' +
        'that NOTICE names for it. The CQ-zone data the interface also bundles comes first.\n' +
        'These notices supplement Nexus COPYING and NOTICE.\n\n' +
        'CQ zone boundaries (cqzones.geojson), from HB9HIL hamradio-zones-geojson\n' +
        readFileSync(new URL('./src/data/cqzones.LICENSE.txt', import.meta.url), 'utf8') + '\n' + bundledLicenses(modules) })
      this.emitFile({ type: 'asset', fileName: 'rust/THIRD-PARTY.txt',
        source: readFileSync(new URL('../licenses/rust/THIRD-PARTY.txt', import.meta.url), 'utf8') })
    },
  }],
  define: {
    __BUILD_ID__: JSON.stringify(buildId()),
    // The street map's renderer (components/StreetMap, MapLibre) is part of the desktop build; the
    // hosted Remote build leaves it out (vite.remote.config.ts).
    __STREET_MAP__: 'true',
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
    // The default reporter, plus one that names the failing tests LAST and in a
    // file — see vitest-failures-reporter.ts for why a red run needs that.
    reporters: ['default', failuresReporter],
  },
  build: {
    outDir: 'dist',
    sourcemap: false,
    // Never a script inlined as a data: URL, whatever its size: the Stations on this network page's
    // `script-src 'self'` refuses one, and its receive-audio worklet is a file of its own origin
    // for that reason (the hosted page's rule, vite.remote.config.ts).
    assetsInlineLimit: file => file.endsWith('.js') ? false : undefined,
    rollupOptions: {
      // Three entries: the desktop app; the TV page the LAN server hands to a browser
      // (connect_web.rs serves `connect-tv.html` at `/`), same components, same chunks, so a
      // Connect improvement reaches the TV in the same build; and the Stations on this network
      // window's page (`lan.html`), which this computer's own loopback origin serves
      // (src-tauri/src/lan_client/origin.rs).
      input: {
        main: 'index.html',
        tv: 'connect-tv.html',
        lan: 'lan.html',
      },
    },
  },
})
