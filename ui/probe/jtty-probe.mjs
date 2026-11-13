// The JTTY screen in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop jtty`.
//
// The dock's Messages box (WSJT-X's message style) and Edit keys button sit in the dock row beside the
// Call box, the serial and the transmit controls, and the Log QSO prompt sits at the top of the log
// column above the strip's bottom-sticky commit block. At the representative screens (probe/sizes.mjs CORE:
// 1366×768 at 175 %, the floor, to 4K at 200 %) the probe serves src/probe/jtty.tsx (the real JttyCockpit, header, waterfall and log strip
// included) with the app's own Vite config and asserts every part — Messages, Edit keys, the eight
// F-keys, the prompt's words, Log QSO and Not now — is inside the window, held by each box that clips it,
// and the topmost thing where it stands. Then the same with Edit keys pressed: the eight key boxes and
// Defaults in place of the keys.
//
// THE FLOOR (2026-11-12) is measured as the app draws it there: at its own fit, 65 % (1366×768 at the OS's 175 % is a
// 780×438 window, which the app fits to 65 %), where every part is on screen unscrolled. At a PINNED 100 % the header,
// the waterfall's own chrome and the dock alone (477 px) are taller than the 438 px window, so nothing in the panes can
// be on screen unscrolled and the shell scrolls, as the layout contract has it: there the prompt's parts must be
// reached by that scroll, on screen and topmost, and the dock's (Messages, Edit keys, the keys) must stand where they are.
//
// THE CONTROL, run every time: the same page at a mid laptop (1440×900, where every part is on screen) with a
// box laid over Messages and over Log QSO. Both must come back NOT on screen, and everything else still on
// screen; if not, the check cannot fail and the probe says so instead of passing.
//
// THE REACH CONTROL: the prompt's words left unpainted at the pinned floor must come back unreached at every scroll.
//
// exit: 0 every size held · 1 a red, or a control that could not fail · 2 the page never drew the prompt
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'
import { CORE, FLOOR, metrics } from './sizes.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
/** [size, zoom, reach]: CORE at the app's 100 %, the floor at its own fit, and the floor pinned at 100 % judged by reach. */
const SIZES = [[FLOOR, 0.65, false], [FLOOR, 1, true], ...CORE.filter((s) => s !== FLOOR).map((s) => [s, 1, false])]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
const nameOf = ([s, zoom, reach]) => `${s.name}${zoom === 1 ? '' : ` at the app's ${Math.round(zoom * 100)} % (its fit)`}${reach ? ' pinned at 100 %, the prompt reached by the shell\'s scroll' : ''}`

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

/** One window of `size` (probe/sizes.mjs), the screen in it, measured once its parts have stopped moving. */
async function measure([size, zoom, reach], query) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  await call('Emulation.setDeviceMetricsOverride', metrics(size))
  await call('Page.navigate', { url: `${origin}/probe/jtty.html?${query}&zoom=${zoom}${reach ? '&reach=1' : ''}` })
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
    for (const row of SIZES) {
      const size = { name: nameOf(row) }
      const m = await measure(row, query)
      console.log(`jtty-probe: ${query === 'edit=1' ? 'editing the keys, ' : ''}window ${size.name}: ${JSON.stringify(m)}`)
      if (!m) {
        console.error(`jtty-probe: the screen never drew its prompt at ${size.name}`)
        code = 2
      } else if (m.keys !== 8 || !Object.values(m.seen).every(Boolean)) {
        console.error(`jtty-probe: RED at ${size.name} (${query}) — a part is clipped, covered or missing (see "seen")`)
        code = code || 1
      }
    }
  }
  const ctl = SIZES.find(([s]) => s.name === CORE[1].name)
  const c = await measure(ctl, 'edit=0&control=1')
  console.log(`jtty-probe: CONTROL window ${nameOf(ctl)}, Messages and Log QSO covered: ${JSON.stringify(c)}`)
  const { messages, logQso, ...rest } = c?.seen ?? {}
  if (!c || messages || logQso || !Object.values(rest).every(Boolean)) {
    console.error('jtty-probe: CONTROL FAILED — a covered part came back on screen (or an uncovered one did not); the check cannot be trusted')
    code = code || 1
  }
  // THE REACH CONTROL: at the pinned floor, the prompt's words unpainted must be judged unreached at every scroll.
  const r = await measure([FLOOR, 1, true], 'edit=0&hide=1')
  console.log(`jtty-probe: REACH CONTROL ${nameOf([FLOOR, 1, true])}, the prompt's words unpainted: ${JSON.stringify(r?.seen ?? null)}`)
  if (!r || r.seen.promptText || !r.seen.logQso) {
    console.error('jtty-probe: REACH CONTROL FAILED — unpainted words were reached (or Log QSO was not); the reach check cannot be trusted')
    code = code || 1
  }
  if (code === 0) console.log(`jtty-probe: PASS — Messages, Edit keys, the eight F-keys and the Log QSO prompt (its words, the replaced note, Log QSO, Not now) on screen at ${SIZES.map(nameOf).join(', ')}; the eight key boxes and Defaults too while editing; the control's covered parts seen as covered`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
