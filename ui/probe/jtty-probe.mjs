// The JTTY screen in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop jtty`.
//
// The dock's Messages box (WSJT-X's message style) and Edit keys button sit in the dock row beside the
// Call box, the serial and the transmit controls, and the Log QSO prompt sits at the top of the log
// column above the strip's bottom-sticky commit block. At 1024×768 (the supported floor), 1366×768 and
// 1920×1080 the probe serves src/probe/jtty.tsx (the real JttyCockpit, header, waterfall and log strip
// included) with the app's own Vite config and asserts every part — Messages, Edit keys, the eight
// F-keys, the prompt's words, Log QSO and Not now — is inside the window, held by each box that clips it,
// and the topmost thing where it stands. Then the same with Edit keys pressed: the eight key boxes and
// Defaults in place of the keys.
//
// THE CONTROL, run every time: the same page at 1024×768 with a box laid over Messages and over Log QSO.
// Both must come back NOT on screen, and everything else still on screen; if not, the check cannot fail
// and the probe says so instead of passing.
//
// exit: 0 every size held · 1 a red, or a control that could not fail · 2 the page never drew the prompt
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const SIZES = [
  { w: 1024, h: 768 },
  { w: 1366, h: 768 },
  { w: 1920, h: 1080 },
]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-jtty-probe-'))
const server = await createServer({
  root: UI,
  configFile: join(UI, 'vite.config.ts'),
  cacheDir,
  logLevel: 'warn',
  server: { host: '127.0.0.1', port: 0, strictPort: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

/** One window of `w`×`h`, the screen in it, measured once its parts have stopped moving. */
async function measure(w, h, query) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/jtty.html?${query}` })
  let last = null
  for (let i = 0; i < 150; i++) {
    await sleep(150)
    const r = await call('Runtime.evaluate', { expression: 'typeof __jttyProbe === "function" ? __jttyProbe() : null', returnByValue: true })
    const now = r.exceptionDetails ? null : r.result.value
    if (now && now.keys === 8 && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  await browser.call('Target.closeTarget', { targetId })
  return last
}

let code = 0
try {
  for (const query of ['edit=0', 'edit=1']) {
    for (const { w, h } of SIZES) {
      const m = await measure(w, h, query)
      console.log(`jtty-probe: ${query === 'edit=1' ? 'editing the keys, ' : ''}window ${w}×${h}: ${JSON.stringify(m)}`)
      if (!m) {
        console.error(`jtty-probe: the screen never drew its prompt at ${w}×${h}`)
        code = 2
      } else if (m.keys !== 8 || !Object.values(m.seen).every(Boolean)) {
        console.error(`jtty-probe: RED at ${w}×${h} (${query}) — a part is clipped, covered or missing (see "seen")`)
        code = code || 1
      }
    }
  }
  const c = await measure(1024, 768, 'edit=0&control=1')
  console.log(`jtty-probe: CONTROL window 1024×768, Messages and Log QSO covered: ${JSON.stringify(c)}`)
  const { messages, logQso, ...rest } = c?.seen ?? {}
  if (!c || messages || logQso || !Object.values(rest).every(Boolean)) {
    console.error('jtty-probe: CONTROL FAILED — a covered part came back on screen (or an uncovered one did not); the check cannot be trusted')
    code = code || 1
  }
  if (code === 0) console.log(`jtty-probe: PASS — Messages, Edit keys, the eight F-keys and the Log QSO prompt (its words, the replaced note, Log QSO, Not now) on screen at ${SIZES.map((s) => `${s.w}×${s.h}`).join(', ')}; the eight key boxes and Defaults too while editing; the control's covered parts seen as covered`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
