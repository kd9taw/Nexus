// The pop-up corner in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop toastcorner`.
//
// #445. A toast's own buttons (Work →, ✕) take pointer events, so a toast standing over a transmit control
// takes that control's clicks: with the column pinned 16 px from its corner, ONE toast in the bottom-left
// corner put its ✕ on Phone's PTT, a burst grew up over Stop TX, and the bottom-right corner (as on main
// before the setting) covered CW's and PSK's Send. The column now takes the room of its corner that touches
// no transmit surface (features/toastRoom.ts, measured by Toasts.tsx).
//
// The probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and src/api.ts
// swapped for probe/toastcorner-api.ts. For each screen (FT, Phone, CW, PSK: the docks and strips the corners meet), at 1024×768 (the supported
// floor) and 1920×1080, and Phone and CW again at the pinned 65 % and 175 % zooms (175 % at 1920×1080 too), in each of the four
// corners, with a burst of nine (its newest toast stands where a lone
// toast would, so the burst covers the one-toast case too; PROBE_BURSTS=1,9 runs both), it pushes the
// toasts through the real toast bus and asserts:
//   1. NO TOAST COVERS A TRANSMIT CONTROL: no toast's box (inside the column's box) meets any button of the
//      window's transmit surfaces (`TX_AVOID`: the TX strip, the TX dock, FT's QSO strip, the top bar's
//      cluster, any Stop button);
//   2. the column is inside the window, stands in the corner chosen, and is never shorter than one toast.
// Then the first visits: FT with a burst of twenty up, switched to PSK and to JTTY, in each corner at both sizes.
// THE CONTROL, at one page: Phone, 1024×768, bottom left, one toast, with the column forced back to the bare
// corner (16 px, no height cap). The probe must see the toast over PTT; if it does not, the check cannot
// fail and the probe says so instead of passing.
//
// exit: 0 every page clear · 1 a red, or a control that could not fail · 2 a page never drew
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'toastcorner-api.ts')
// Served under a name with no file behind it, beside the source, as the other desktop probes do.
const STUB_API = join(UI, 'probe', '__toastcorner-api.ts')
const SIZES = [[1024, 768], [1920, 1080]]
const SCREENS = ['operate', 'phone', 'cw', 'psk']
const CORNERS = ['bottom-right', 'bottom-left', 'top-right', 'top-left']
const BURSTS = (process.env.PROBE_BURSTS ?? '9').split(',').map(Number)
// The pinned zooms the UI offers at its ends (65 % and 175 %), on the two screens whose dock a bottom
// corner meets (Phone's PTT, CW's Send): the column's room is window px divided by the zoom, and a wrong
// division is exactly a toast on PTT at 65 %. PROBE_SET=base, =zoom or =switch runs one part (a loaded
// box's foreground budget); unset, all three.
const ZOOMED = [[[1024, 768], '65'], [[1920, 1080], '65'], [[1024, 768], '175'], [[1366, 768], '175'], [[1920, 1080], '175']]
const SET = process.env.PROBE_SET ?? 'all'
const PAGES = [
  ...(SET === 'all' || SET === 'base' ? SIZES.map((size) => [size, 'auto', SCREENS]) : []),
  ...(SET === 'all' || SET === 'zoom' ? ZOOMED.map(([size, scale]) => [size, scale, ['phone', 'cw']]) : []),
]
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

// The transmit surfaces, as features/txSurfaces.ts names them (read from the source, so the two cannot drift).
const surfacesSrc = await readFile(join(UI, 'src', 'features', 'txSurfaces.ts'), 'utf8')
const TX_AVOID = /export const TX_AVOID = '([^']+)'/.exec(surfacesSrc)?.[1]
if (!TX_AVOID) {
  console.error('toastcorner-probe: could not read TX_AVOID from src/features/txSurfaces.ts')
  process.exit(1)
}

// \`then\`: a mode button to press once the toasts are up (a screen's first visit, its TX strip mounted
// after the column was placed), or null.
const MEASURE = (n, control, then = null) => `(async () => {
  const T = await import('/src/toast.ts')
  ${control ? `const s = document.createElement('style'); s.textContent = '.ui-toast-viewport { bottom: 16px !important; top: auto !important; max-height: none !important; }'; document.head.appendChild(s);` : ''}
  for (let i = 0; i < ${n}; i++) T.pushToast('Toast ' + i + ': W1AW is calling you on 20m at -05 dB', 'info', 0, { action: () => {}, actionLabel: 'Work' })
  await new Promise((r) => setTimeout(r, 900))
  ${then ? `const go = [...document.querySelectorAll('.mode-btn')].find((b) => b.textContent.trim() === ${JSON.stringify(then)})
  if (!go) return { error: 'no ${then} mode button' }
  go.click()
  await new Promise((r) => setTimeout(r, 1500))` : ''}
  const box = (el) => { const r = el.getBoundingClientRect(); return [r.left, r.top, r.right, r.bottom] }
  const meet = (a, b) => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
  const vp = document.querySelector('.ui-toast-viewport')
  if (!vp) return { error: 'no toast column' }
  const v = box(vp)
  // The tallest toast against the column: a column shorter than one toast clips it (CW at 1920×1080,
  // 175 %, once got a 72 px sliver at the bottom for a 74 px toast).
  const tallest = Math.max(0, ...[...vp.querySelectorAll('.ui-toast')].map((t) => t.getBoundingClientRect().height))
  const toasts = [...vp.querySelectorAll('.ui-toast')].map(box)
    .map((t) => [t[0], Math.max(t[1], v[1]), t[2], Math.min(t[3], v[3])]).filter((t) => t[3] > t[1])
  const controls = [...document.querySelectorAll(${JSON.stringify(TX_AVOID)})]
    .flatMap((s) => [s, ...s.querySelectorAll('button, [role="button"], [role="switch"]')])
    .filter((e) => e.matches('button, [role="button"], [role="switch"]'))
    .filter((e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 })
  const covered = [...new Set(controls)].filter((e) => toasts.some((t) => meet(t, box(e))))
    .map((e) => (e.textContent || e.getAttribute('aria-label') || e.className).trim().slice(0, 30) + ' ' + JSON.stringify(box(e).map(Math.round)))
  return {
    corner: vp.dataset.corner,
    column: v.map(Math.round),
    inside: v[0] >= 0 && v[1] >= 0 && v[2] <= innerWidth + 0.5 && v[3] <= innerHeight + 0.5,
    shown: toasts.length,
    clipped: tallest > v[3] - v[1] + 0.5 ? [Math.round(tallest), Math.round(v[3] - v[1])] : null,
    controls: controls.length,
    covered,
  }
})()`

const realSrc = await readFile(REAL_API, 'utf8')
const stubSrc = await readFile(STUB_SRC, 'utf8')
const names = [...realSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1])
const own = new Set([...stubSrc.matchAll(/^export (?:async )?function (\w+)/gm)].map((m) => m[1]))
const stubCode =
  stubSrc +
  `\nexport * from ${JSON.stringify(REAL_API)}\n` +
  names
    .filter((n) => !own.has(n))
    .map((n) => `export async function ${n}(..._a: unknown[]): Promise<any> { return (globalThis as any).__DEFAULT_ARR ? [] : {} }`)
    .join('\n')
const swap = {
  name: 'toastcorner-probe-api',
  enforce: 'pre',
  async resolveId(source, importer, opts) {
    if (opts?.scan) return null
    if (source === STUB_API) return STUB_API
    if (!importer || importer === STUB_API || !/api(\.ts)?$/.test(source)) return null
    const r = await this.resolve(source, importer, { ...opts, skipSelf: true })
    return r && r.id.split('?')[0] === REAL_API ? STUB_API : null
  },
  load(id) {
    return id === STUB_API ? stubCode : null
  },
}

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-toastcorner-probe-'))
const server = await createServer({
  root: UI,
  configFile: join(UI, 'vite.config.ts'),
  cacheDir,
  logLevel: 'warn',
  plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { js8: true, jtty: true, rtty: true, psk: true, sstv: true } })

async function run(screen, [W, H], corner, n, control, scale = 'auto', then = null) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  const until = async (expression, tries = 100) => {
    for (let i = 0; i < tries; i++) {
      await sleep(200)
      const v = await evaluate(expression).catch(() => null)
      if (v) return v
    }
    return null
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    const store = {
      'nexus.workspace': 'dx',
      'nexus.features.v1': FEATURES,
      'tempo-theme': 'dark',
      'nexus.features.wizardSeen': '1',
      'probe.settings': JSON.stringify({ popupCorner: corner }),
      'nexus-ui-scale-mode': scale,
    }
    await evaluate(`localStorage.clear(); ${Object.entries(store).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${origin}/#${screen}` })
    const drew = await until(`document.querySelector('.app') != null && document.querySelector('.cockpit-txstrip, .cockpit-qso') != null`)
    if (!drew) return { error: 'the app never drew' }
    await sleep(1200)
    return await evaluate(MEASURE(n, control, then))
  } finally {
    await browser.call('Target.closeTarget', { targetId }).catch(() => {})
  }
}

let red = 0
let undrawn = 0
try {
  for (const [size, scale, screens] of PAGES)
    for (const screen of screens)
      for (const corner of CORNERS)
        for (const n of BURSTS) {
          const at = `${screen} ${size.join('×')}${scale === 'auto' ? '' : ` at ${scale}%`} ${corner} ×${n}`
          const m = await run(screen, size, corner, n, false, scale)
          if (m.error) {
            undrawn++
            console.log(`NOT DRAWN ${at}: ${m.error}`)
            continue
          }
          const faults = []
          if (m.corner !== corner) faults.push(`the column stands in ${m.corner}, not ${corner}`)
          if (!m.inside) faults.push(`the column is not inside the window (${m.column})`)
          if (m.clipped) faults.push(`a toast ${m.clipped[0]} px tall is clipped by a ${m.clipped[1]} px column`)
          if (m.controls === 0) faults.push('no transmit control found on the page (the probe would see nothing)')
          for (const c of m.covered) faults.push(`a toast covers ${c}`)
          if (faults.length) red++
          console.log(`${faults.length ? 'RED ' : 'ok  '} ${at}: column ${m.column}, ${m.shown} toast(s) shown, ${m.controls} TX controls${faults.length ? '\n     ' + faults.join('\n     ') : ''}`)
        }
  // A FIRST VISIT WITH A BURST UP: FT, then PSK or JTTY, whose TX strip mounts after the column was
  // placed. The column must re-measure on that (it once kept FT's room and lay over PSK's Tune and Stop TX).
  if (SET === 'all' || SET === 'switch')
    for (const size of SIZES)
      for (const then of ['PSK', 'JTTY'])
        for (const corner of CORNERS) {
          const at = `operate → ${then} ${size.join('×')} ${corner} ×20`
          const m = await run('operate', size, corner, 20, false, 'auto', then)
          if (m.error) {
            undrawn++
            console.log(`NOT DRAWN ${at}: ${m.error}`)
            continue
          }
          const faults = []
          if (!m.inside) faults.push(`the column is not inside the window (${m.column})`)
          if (m.clipped) faults.push(`a toast ${m.clipped[0]} px tall is clipped by a ${m.clipped[1]} px column`)
          if (m.controls === 0) faults.push('no transmit control found on the page (the probe would see nothing)')
          for (const c of m.covered) faults.push(`a toast covers ${c}`)
          if (faults.length) red++
          console.log(`${faults.length ? 'RED ' : 'ok  '} ${at}: column ${m.column}, ${m.shown} toast(s) shown, ${m.controls} TX controls${faults.length ? '\n     ' + faults.join('\n     ') : ''}`)
        }
  const c = await run('phone', [1024, 768], 'bottom-left', 1, true)
  if (c.error || c.covered.length === 0) {
    red++
    console.log(`RED  CONTROL: the column forced to the bare corner covered nothing it could see (${c.error ?? 'no cover found'}), so the check cannot fail`)
  } else console.log(`ok   CONTROL: the bare corner is caught: a toast covers ${c.covered[0]}`)
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
console.log(red || undrawn ? `toastcorner-probe: ${red} red, ${undrawn} not drawn` : 'toastcorner-probe: every page clear')
process.exit(red ? 1 : undrawn ? 2 : 0)
