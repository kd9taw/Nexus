// Hosted account entry and the existing Nexus workspace. Native APIs stay behind
// the explicitly installed, restricted Remote transport; no global Tauri shim.
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { readFileSync } from 'node:fs'
import { bundledLicenses } from './remote-licenses'
export default defineConfig({
  // The Remote page never offers the street map (its packs live on the station's disk), so the
  // street renderer and MapLibre stay out of this bundle: MapView's lazy import of it is dead code here.
  define: { __BUILD_ID__: JSON.stringify(process.env.GITHUB_SHA?.slice(0, 8) ?? 'remote-pilot'), __STREET_MAP__: 'false' },
  root: 'remote', plugins: [react(), {
    name: 'remote-license-texts',
    generateBundle(_options, bundle) {
      const files = ['COPYING', 'NOTICE', 'licenses/remote/THIRD-PARTY.txt']
      const modules = new Set(Object.values(bundle).flatMap(chunk => chunk.type === 'chunk' ? Object.keys(chunk.modules) : []))
      this.emitFile({ type: 'asset', fileName: 'remote-licenses.txt', source:
        'Nexus Remote source: https://github.com/kd9taw/Nexus\n\n' +
        files.map(file => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8')).join('\n\n') + '\n\n' +
        readFileSync(new URL('./src/data/cqzones.LICENSE.txt', import.meta.url), 'utf8') + '\n\n' + bundledLicenses(modules) })
    },
  }],
  // remote/public: the web app manifest and its icons, copied as they are, at the names the manifest gives them.
  publicDir: 'public',
  // Never a script inlined as a data: URL, whatever its size: the page's `script-src 'self'`
  // refuses one. The receive-audio worklet is a file of the page's own origin for that reason.
  build: { outDir: '../dist-remote', emptyOutDir: true, assetsInlineLimit: file => file.endsWith('.js') ? false : undefined },
})
