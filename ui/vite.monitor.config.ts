// An explicit preview build, with its own output. Fixtures never enter the desktop build.
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  base: './',
  server: { host: '127.0.0.1', port: 5189, strictPort: true },
  build: { outDir: 'dist-monitor', rollupOptions: { input: 'monitor.html' } },
})
