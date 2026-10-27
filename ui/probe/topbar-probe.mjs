// The top bar in real Chrome, for what jsdom cannot see: `scripts/browser-probe --desktop topbar`.
//
// A status-lane message must never change the top bar's height or move a control to another line.
// The bar is one flex-wrap row, and a message sized by its text used to push FT's last group (Auto /
// Tx 1st / Tx 2nd) onto a second line, so the whole screen moved down a row while the message was up.
// The probe serves src/probe/topbar.tsx with the app's own Vite config, on the FT screen and on CW, at
// the common full-screen sizes (each at the zoom the app's auto-fit gives it, plus 125 % and 150 % at
// 1920×1080), once without a message and once with the slot-unkey-failed lane, and asserts: the message
// is on screen with at least its flag; wherever the lane's line has room for that flag, the bar is the
// same height and every group of the bar starts on the same line (with less room than the flag, the bar
// may take a line rather than hide a critical alarm, and the probe names those sizes); nothing in the
// bar runs past the window's edge.
//
// THE CONTROL, run every time: the same page with the lane's old sizing put back (its own text width,
// no growing). There the bar must come back taller at 1920×1080 on the FT screen, the operator's report;
// if it does not, the check cannot fail and the probe says so instead of passing.
//
// exit: 0 every case holds · 1 a red, or a control that could not fail · 2 the page never drew the bar
import { createServer } from 'vite'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
// Window (CSS px) and the zoom the app's auto-fit picks there (useScale: min(w/1200, h/900) snapped down
// to a step, capped at 100 %), plus two pinned scales on a 1080p screen.
const SIZES = [
  { w: 1920, h: 1080, z: 1 }, { w: 1920, h: 1080, z: 1.25 }, { w: 1920, h: 1080, z: 1.5 },
  { w: 1536, h: 864, z: 0.9 }, { w: 1280, h: 720, z: 0.8 }, { w: 2560, h: 1440, z: 1 },
  { w: 1366, h: 768, z: 0.85 }, { w: 1440, h: 900, z: 1 }, { w: 1512, h: 982, z: 1 }, { w: 1024, h: 768, z: 0.85 },
]
const VIEWS = ['operate', 'cw']
const OLD_LANE = '.status-lane{width:auto!important;flex:0 0 auto!important;margin-inline-start:0!important}.txrx{flex-grow:0!important}'
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const MEASURE = `(() => {
  const bar = document.querySelector('.topbar')
  if (!bar || !document.documentElement.getAttribute('data-viewport')) return null
  const b = bar.getBoundingClientRect()
  const lane = document.querySelector('.status-lane')
  const chip = lane?.querySelector('.status-chip')
  const flag = chip?.querySelector('svg')
  const shown = (e) => { if (!e) return false; const r = e.getBoundingClientRect(), l = lane.getBoundingClientRect(); return r.width > 0 && r.left >= l.left - 0.5 && r.right <= l.right + 0.5 && r.right <= innerWidth }
  return {
    vp: document.documentElement.getAttribute('data-viewport'),
    h: Math.round(b.height * 10) / 10,
    groups: [...bar.children].filter((e) => e.getClientRects().length).map((e) => e.className.split(' ').slice(0, 2).join('.') + '@' + Math.round(e.getBoundingClientRect().top)),
    lane: lane ? { w: Math.round(lane.getBoundingClientRect().width), flag: shown(flag), text: chip?.textContent ?? '' } : null,
    // The room left on the line the TX/RX group (the lane's home) sits on: the bar's content width
    // less the items on that line and the gaps between them.
    free: (() => {
      const z = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1
      const cs = getComputedStyle(bar)
      const g = bar.querySelector(':scope > .txrx')?.getBoundingClientRect()
      if (!g) return null
      const line = [...bar.children].filter((e) => e.getClientRects().length).map((e) => e.getBoundingClientRect()).filter((r) => r.top < g.bottom && r.bottom > g.top)
      const inner = (bar.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight)) * z
      return Math.round(inner - line.reduce((a, r) => a + r.width, 0) - (line.length - 1) * parseFloat(cs.columnGap) * z)
    })(),
    zoom: parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--ui-zoom')) || 1,
    overflowX: Math.max(0, Math.round(bar.scrollWidth - bar.clientWidth), ...[...bar.querySelectorAll('*')].map((e) => Math.round(e.getBoundingClientRect().right - innerWidth))),
  }
})()`

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-topbar-probe-'))
const server = await createServer({
  root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn',
  server: { host: '127.0.0.1', port: 0, strictPort: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function measure({ w, h, z }, view, lane, css) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true })
    return r.exceptionDetails ? null : r.result.value
  }
  await call('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false })
  await call('Page.navigate', { url: `${origin}/probe/topbar.html?view=${view}&zoom=${z}${lane ? '&lane=on' : ''}` })
  let last = null
  for (let i = 0; i < 80; i++) {
    await sleep(150)
    if (css && i === 10) await evaluate(`document.head.insertAdjacentHTML('beforeend', '<style>${css}</style>')`)
    const now = await evaluate(MEASURE)
    if (i > 12 && now && last && JSON.stringify(now) === JSON.stringify(last)) break
    last = now
  }
  await browser.call('Target.closeTarget', { targetId })
  return last
}

let code = 0
const tookLine = []
try {
  for (const s of SIZES) {
    for (const view of VIEWS) {
      const at = `${view} at ${s.w}×${s.h} ${Math.round(s.z * 100)}%`
      const off = await measure(s, view, false)
      const on = await measure(s, view, true)
      console.log(`topbar-probe: ${at}: without ${JSON.stringify(off)}`)
      console.log(`topbar-probe: ${at}: with    ${JSON.stringify(on)}`)
      if (!off || !on || !on.lane) {
        console.error(`topbar-probe: the bar or the lane was never drawn for ${at}`)
        code = 2
        continue
      }
      const reds = []
      // The flag's floor (styles.css `.status-lane` min-width: the 13 px icon, the chip's 2 × 8 px
      // padding and 2 px border, and the 10 px gap), in screen px at this zoom.
      const flag = (10 + 13 + 16 + 2) * off.zoom
      if (off.free >= flag) {
        if (on.h !== off.h) reds.push(`the bar is ${on.h}px tall with the message, ${off.h}px without (${off.free}px free on its line)`)
        if (JSON.stringify(on.groups) !== JSON.stringify(off.groups)) reds.push(`a group moved line: ${JSON.stringify(off.groups)} → ${JSON.stringify(on.groups)}`)
      } else {
        console.log(`topbar-probe: ${at}: only ${off.free}px free on the lane's line, less than its flag (${Math.round(flag)}px): the bar may take a line here rather than hide the alarm`)
        if (on.h !== off.h) tookLine.push(at)
      }
      if (!on.lane.flag) reds.push('the message is not on screen (its flag is clipped)')
      if (on.overflowX > 0 || off.overflowX > 0) reds.push(`the bar runs ${Math.max(on.overflowX, off.overflowX)}px past the window`)
      for (const r of reds) console.error(`topbar-probe: RED ${at} — ${r}`)
      if (reds.length) code = code || 1
    }
  }
  const s = SIZES[0]
  const off = await measure(s, 'operate', false, OLD_LANE)
  const on = await measure(s, 'operate', true, OLD_LANE)
  console.log(`topbar-probe: CONTROL operate at ${s.w}×${s.h}, the lane's old sizing: without ${off?.h}px, with ${on?.h}px ${JSON.stringify(on?.groups)}`)
  if (!off || !on || on.h === off.h) {
    console.error('topbar-probe: CONTROL FAILED — with the old sizing the message did not make the bar taller; the check cannot fail')
    code = code || 1
  }
  if (code === 0) console.log(`topbar-probe: PASS — a status message shows its flag and leaves the bar's height and every group's line alone wherever its line has room for the flag, on the FT screen and on CW, at ${SIZES.map((x) => `${x.w}×${x.h}@${Math.round(x.z * 100)}%`).join(', ')}; the old sizing wraps the bar (control). Took a line for the flag: ${tookLine.length ? tookLine.join('; ') : 'none'}`)
  process.exitCode = code
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
