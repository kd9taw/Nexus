// The hosted entry has no native bridge, fixture provider or operating cockpit.
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { readFileSync } from 'node:fs'
export default defineConfig({
  root: 'remote', plugins: [react(), {
    name: 'remote-license-texts',
    generateBundle() {
      const files = ['COPYING', 'NOTICE', 'licenses/remote/THIRD-PARTY.txt']
      this.emitFile({ type: 'asset', fileName: 'remote-licenses.txt', source:
        'Nexus Remote source: https://github.com/kd9taw/Nexus\n\n' +
        files.map(file => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8')).join('\n\n') })
    },
  }], publicDir: false,
  build: { outDir: '../dist-remote', emptyOutDir: true },
})
