// Hosted account entry and the existing Nexus workspace. Native APIs stay behind
// the explicitly installed, restricted Remote transport; no global Tauri shim.
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { readFileSync } from 'node:fs'
import { bundledLicenses } from './remote-licenses'
export default defineConfig({
  define: { __BUILD_ID__: JSON.stringify(process.env.GITHUB_SHA?.slice(0, 8) ?? 'remote-pilot') },
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
  }], publicDir: false,
  build: { outDir: '../dist-remote', emptyOutDir: true },
})
