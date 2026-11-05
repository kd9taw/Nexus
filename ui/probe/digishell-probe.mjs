// The SSTV, RTTY and PSK shells — and CW's and Phone's at the small sizes — in real Chrome, for what jsdom cannot see:
// `scripts/browser-probe --desktop digishell`.
//
// The three screens pin their cockpit header (the mode, the dial, the band, ⊞ Panels, power, CAT) so it stays in the
// window when the screen scrolls; the TX strip parks under it and the dock at the bottom. On a small window at a large
// zoom the three parked surfaces are taller than the window, and a header pinned there sat over Stop TX, Send and
// Esc/Stop at every scroll position (900×600 and 700×800 at 175 %, measured), so the strip unpins the header where
// the three do not fit. This probe serves the REAL app (index.html, main.tsx, App) with the app's own Vite config and
// src/api.ts swapped for probe/waterfalls-api.ts, opens each screen with fresh storage at the sizes below, and at the
// TOP of the screen's scroll and at its END asserts:
//   1. NOTHING IN THE HEADER COVERS A TRANSMIT CONTROL: every control of the TX strip and of the dock that is on
//      screen but not the topmost thing where it stands is covered by something other than the header.
//   2. A STOP CONTROL IS REACHABLE: at least one of Stop TX (strip), Esc/Stop (RTTY/PSK dock), Stop (SSTV's bar) is
//      on screen and topmost. (Which ones, and any transmit control covered by something else, is printed: on a
//      small window at 175 % RTTY and PSK already lost the strip's Stop TX under the dock before the header was
//      pinned, which is that shell's own matter, not this probe's.)
//   3. WHERE THE HEADER IS PINNED, ITS CONTROLS ARE ON SCREEN AT THE END of the scroll (#457), and the page itself
//      never scrolls.
//   Checks 1, 2 and 5 hold at seven scroll positions: the top, every sixth of the way down, and the end.
//   4. THE ⊞ PANELS MENU, OPENED FROM THE HEADER at the top and at the end of the scroll (and, where the header
//      scrolls, at the position where its ⊞ button is reachable, check 6): every item of it that is on screen is
//      the topmost thing where it stands (a pinned header is a stacking context at the strip's and the dock's level,
//      so its menu hung under them until the header lifts while it is open), EVERY item is inside the window from
//      left to right (at 700×800 at 175 % the menu hung off the window's left edge, measured), and a stop control
//      is still reachable with the menu open. (Items covered are a red on SSTV, RTTY and PSK only: see `reds`.) Items below the window are printed, not failed (the menu is that
//      tall at 175 % on a small window everywhere). CLOSED again, the header carries no lift and check 1 holds.
//   5. EVERY STOP CONTROL AND SEND ARE ON SCREEN AND TOPMOST, the menu closed: each of Stop TX (strip), Esc/Stop
//      (RTTY/PSK), Stop (SSTV's bar) and the dock's Send (CW/RTTY/PSK) or PTT (Phone). On a small window at a large
//      zoom the dock was taller than the scrollport: it painted over the strip's TX On/Off, Tune and Stop TX at every
//      position and pushed Send below the window at the top (900×600 and 700×800 at 175 %, measured), until the dock
//      learnt to fold (cockpit-panes.css `.cockpit-txdock[data-fold='on']`).
//   6. THE ⊞ PANELS BUTTON CAN BE CLICKED: pinned, it is topmost at every position; where the header scrolls, there is
//      a scroll position (searched every 6 px) where it is on screen and topmost. Under a dock taller than the
//      scrollport it never was, on RTTY and PSK at 900×600 and 700×800 at 175 % (measured).
//   7. EVERY CONTROL OF THE STRIP AND THE DOCK CAN BE CLICKED at some scroll position (the same 6 px search): a folded
//      dock's macro keys scroll with the panes, and each must come out between the parked surfaces.
// THE CONTROLS, run every time: the header forced to stick on top (z 9) at 900×600 / 175 % must make check 1 fail;
// the lift taken away (the header held at 4 with its menu open) at 1366×768 / 150 % on RTTY must make check 4 fail;
// the dock's fold taken away at 900×600 / 175 % on RTTY must make check 5 fail; and the menu's clamp into the window
// taken away at 700×800 / 175 % on RTTY must make check 4 fail.
//
// exit: 0 every check held · 1 a red, or a control that could not fail · 2 the page never drew a screen
import { createServer } from 'vite'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chrome } from '../../remote/test/browser-runtime.mjs'

const UI = join(dirname(fileURLToPath(import.meta.url)), '..')
const REAL_API = join(UI, 'src', 'api.ts')
const STUB_SRC = join(UI, 'probe', 'waterfalls-api.ts')
const STUB_API = join(UI, 'probe', '__digishell-api.ts')
/** [window W, H, pinned UI scale or null for the app's own fit]. */
const SIZES = [
  [900, 600, '175'], [700, 800, '175'], [1024, 768, '175'], [1280, 720, '175'], [1366, 768, '175'], [1366, 768, '150'],
  [1920, 1080, '150'], [1200, 750, null],
]
/** CW and Phone at the sizes where their docks were taller than the room left (and two that fit, and the fit). */
const SMALL = [[900, 600, '175'], [700, 800, '175'], [1024, 768, '175'], [1280, 720, '175'], [1200, 750, null]]
const VIEWS = [['SSTV', 'sstv', 'main.sstv-view', SIZES, true], ['RTTY', 'rtty', 'main.rtty-cockpit', SIZES, true],
  ['PSK', 'psk', 'main.psk-cockpit', SIZES, true], ['CW', 'cw', 'main.cw-cockpit', SMALL, false], ['Phone', 'phone', 'main.phone-cockpit', SMALL, false]]
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { rtty: true, psk: true, sstv: true } })
const PLANT = ".cockpit-header-pinned, .cockpit-header-pinned[data-pin='off'] { position: sticky !important; top: 0 !important; z-index: 9 !important }"
const NO_LIFT = ".cockpit-header-pinned[data-menu='open'] { z-index: 4 !important }"
const NO_FOLD = ".cockpit-txdock[data-fold='on'] { display: flex !important } .cockpit-txdock-keep[data-fold='on'] { position: static !important; padding: 0 !important; border: 0 !important }"
const NO_CLAMP = '.panels-menu-pop { translate: none !important }'
/** The dock's send control: Send on the keyboard docks, PTT on Phone's. */
const SEND = '.cw-send-btn, .ph-ptt'
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

const measure = (sel) => `(() => {
  const shell = document.querySelector(${JSON.stringify(sel)})
  if (!shell || shell.getBoundingClientRect().height < 1 || !document.documentElement.getAttribute('data-viewport')) return null
  const head = shell.querySelector(':scope > .cockpit-header')
  const strip = shell.querySelector(':scope > .cockpit-txstrip')
  const dock = shell.querySelector(':scope > .cockpit-txdock, :scope > .sstv-tx-bar')
  if (!head || !strip || !dock) return null
  const vis = (el) => el.closest('[hidden]') == null && el.getBoundingClientRect().width > 0 && el.getBoundingClientRect().height > 0
  const label = (b) => (b.textContent || b.getAttribute('aria-label') || b.type || '').replace(/\\s+/g, ' ').trim().slice(0, 20)
  const at = (b) => {
    const q = b.getBoundingClientRect()
    if (q.bottom > innerHeight + 0.5 || q.top < -0.5 || q.right > innerWidth + 0.5 || q.left < -0.5) return { on: false }
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + Math.min(q.height / 2, 6))
    return { on: true, top: t != null && (t === b || b.contains(t)), byHeader: t != null && head.contains(t) }
  }
  const ctls = [...strip.querySelectorAll('button, select, input'), ...dock.querySelectorAll('button, select, input')].filter(vis)
  const btn = head.querySelector('.panels-menu-btn')
  const pos = () => {
    const rows = ctls.map((b) => ({ b, ...at(b) }))
    const stops = rows.filter((r) => /stop/i.test(label(r.b)))
    const sends = rows.filter((r) => r.b.matches(${JSON.stringify(SEND)}))
    const hc = [...head.querySelectorAll('button, select, input')].filter(vis)
    const pb = btn && vis(btn) ? at(btn) : null
    return {
      coveredByHeader: rows.filter((r) => r.on && !r.top && r.byHeader).map((r) => label(r.b)),
      coveredOther: rows.filter((r) => r.on && !r.top && !r.byHeader).map((r) => label(r.b)),
      stopsReachable: stops.filter((r) => r.on && r.top).map((r) => label(r.b)),
      stops: stops.map((r) => label(r.b)),
      // Check 5: every stop control and the send control, each on screen and topmost.
      unreachable: [...stops, ...sends].filter((r) => !(r.on && r.top)).map((r) => label(r.b) + (r.on ? ' (covered)' : ' (off screen)')),
      panels: pb ? pb.on && pb.top : null,
      headReach: hc.filter((b) => { const a = at(b); return a.on && a.top }).length + '/' + hc.length,
    }
  }
  const max = Math.max(0, shell.scrollHeight - shell.clientHeight)
  const sweep = [1, 2, 3, 4, 5].map((k) => { shell.scrollTop = Math.round((max * k) / 6); return { at: Math.round((k * 100) / 6) + ' %', ...pos() } })
  shell.scrollTop = 0
  const top = pos()
  shell.scrollTop = shell.scrollHeight
  const end = pos()
  // Check 6: the first scroll position (every 6 px) where ⊞ Panels is on screen and topmost; -1 if none.
  // Check 7, in the same pass: every control of the strip and the dock that is never on screen and topmost.
  let reach = btn && vis(btn) ? -1 : null
  const never = new Set(ctls)
  for (let s = 0; s <= max + 5; s += 6) {
    shell.scrollTop = Math.min(s, max)
    if (reach === -1) { const a = at(btn); if (a.on && a.top) reach = shell.scrollTop }
    for (const b of [...never]) { const a = at(b); if (a.on && a.top) never.delete(b) }
    if (reach !== -1 && never.size === 0) break
  }
  const neverReached = [...never].map(label)
  shell.scrollTop = 0
  const se = document.scrollingElement
  return {
    vp: document.documentElement.getAttribute('data-viewport'),
    pin: head.getAttribute('data-pin'),
    fold: dock.getAttribute('data-fold'),
    valve: Math.max(0, Math.round(shell.scrollHeight - shell.clientHeight)),
    page: Math.max(0, se.scrollHeight - se.clientHeight),
    top, end, sweep, reach, neverReached,
  }
})()`

/** Open ⊞ Panels from the header with the shell at `where` ('top' | 'end'), read every item, close it again. */
const menuCheck = (sel, where, at = 0) => `(async () => {
  const shell = document.querySelector(${JSON.stringify(sel)})
  const head = shell.querySelector(':scope > .cockpit-header')
  const btn = head.querySelector('.panels-menu-btn')
  if (!btn) return { none: true }
  shell.scrollTop = ${JSON.stringify(where)} === 'end' ? shell.scrollHeight : ${JSON.stringify(where)} === 'reach' ? ${Number(at)} : 0
  await new Promise((r) => setTimeout(r, 150))
  btn.click()
  await new Promise((r) => setTimeout(r, 300))
  const pop = head.querySelector('.panels-menu-pop')
  if (!pop) return { opened: false }
  const items = [...pop.querySelectorAll('button, input, label')].filter((e) => e.getBoundingClientRect().height > 0)
  // Inside the window from left to right, every item, wherever the shell is scrolled.
  const offx = items.filter((b) => { const q = b.getBoundingClientRect(); return q.left < -0.5 || q.right > innerWidth + 0.5 }).length
  const ctl = [...shell.querySelectorAll(':scope > .cockpit-txstrip button, :scope > .cockpit-txdock button, :scope > .sstv-tx-bar button')]
    .filter((b) => /stop/i.test(b.textContent || '') && b.getBoundingClientRect().height > 0)
  const stopsOpen = ctl.filter((b) => {
    const q = b.getBoundingClientRect()
    if (q.bottom > innerHeight + 0.5 || q.top < -0.5 || q.right > innerWidth + 0.5 || q.left < -0.5) return false
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + Math.min(q.height / 2, 6))
    return t != null && (t === b || b.contains(t))
  }).length
  const rows = items.map((b) => {
    const q = b.getBoundingClientRect()
    if (q.bottom > innerHeight + 0.5 || q.top < -0.5 || q.left + q.width / 2 < 0 || q.left + q.width / 2 > innerWidth) return 'off'
    // Outside the shell's own scrollport the menu is CLIPPED by it (a header that has scrolled away
    // opens its menu there), which is the shell's scroll, not a cover.
    const sb = shell.getBoundingClientRect()
    if (q.top + q.height / 2 < sb.top || q.top + q.height / 2 > sb.bottom) return 'clipped'
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
    if (t && (t === b || b.contains(t) || pop.contains(t))) return 'ok'
    return t && t.closest('.cockpit-txstrip') ? 'strip' : t && t.closest('.cockpit-txdock, .sstv-tx-bar') ? 'dock' : 'other:' + (t ? String(t.className).split(' ')[0] || t.tagName : 'none')
  })
  const lifted = head.getAttribute('data-menu')
  btn.click()
  await new Promise((r) => setTimeout(r, 300))
  const closedMenu = head.getAttribute('data-menu')
  shell.scrollTop = 0
  return { opened: true, offx, stopsOpen, n: rows.length, ok: rows.filter((r) => r === 'ok').length, off: rows.filter((r) => r === 'off').length,
    clipped: rows.filter((r) => r === 'clipped').length, covered: rows.filter((r) => r !== 'ok' && r !== 'off' && r !== 'clipped'), lifted, closedMenu, pin: head.getAttribute('data-pin') }
})()`

// THE API SWAP — the waterfalls probe's, verbatim in shape.
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
  name: 'digishell-probe-api',
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

const cacheDir = await mkdtemp(join(tmpdir(), 'nexus-digishell-probe-'))
const server = await createServer({
  root: UI, configFile: join(UI, 'vite.config.ts'), cacheDir, logLevel: 'warn', plugins: [swap],
  server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false },
})
await server.listen()
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
const browser = await chrome()

async function measureAt(route, sel, [W, H, scale], css = null, menus = true) {
  const { targetId } = await browser.call('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await browser.call('Target.attachToTarget', { targetId, flatten: true })
  const call = (method, params) => browser.call(method, params, sessionId)
  const evaluate = async (expression) => {
    const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  try {
    await call('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false })
    await call('Page.navigate', { url: `${origin}/probe-blank` })
    await sleep(300)
    const ls = { 'nexus.workspace': 'dx', 'nexus.features.v1': FEATURES, 'tempo-theme': 'dark', ...(scale ? { 'nexus-ui-scale-mode': scale } : {}) }
    await evaluate(`localStorage.clear(); ${Object.entries(ls).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)})`).join(';')}; 1`)
    await call('Page.navigate', { url: `${origin}/#${route}` })
    let m = null
    for (let i = 0; i < 240 && !m; i++) {
      await sleep(250)
      m = await evaluate(measure(sel)).catch(() => null)
    }
    if (!m) return null
    if (css) await evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    let last = null
    for (let i = 0; i < 40; i++) {
      await sleep(250)
      m = await evaluate(measure(sel))
      if (last && JSON.stringify(m) === JSON.stringify(last)) break
      last = m
    }
    if (menus) {
      m.menuTop = await evaluate(menuCheck(sel, 'top'))
      m.menuEnd = await evaluate(menuCheck(sel, 'end'))
      // Where the header scrolls, the operator opens the menu from where the ⊞ button can be clicked.
      if (m.reach > 0) m.menuReach = await evaluate(menuCheck(sel, 'reach', m.reach))
      // CLOSED AGAIN: the header must be back under the strip and the dock.
      m.after = await evaluate(measure(sel))
    }
    return m
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

const tagOf = ([W, H, s]) => `${W}×${H} ${s ? `${s} %` : 'fit'}`
/** `cover`: whether an open menu's items covered by the strip or the dock are a red. They are on the three screens
 *  whose header pins (SSTV, RTTY, PSK: the lift is theirs). On CW and Phone the menu's lower items stand under the
 *  dock at every size, the 1200×750 fit included, on the base tree as well: printed, a matter of its own. */
function reds(name, m, cover = true) {
  const out = []
  if (m.page > 0) out.push(`${name}: the page scrolls ${m.page} px`)
  const positions = [['the top', m.top], ...m.sweep.map((p) => [p.at, p]), ['the end', m.end],
    ...(m.after ? [['the top, the menu closed again', m.after.top], ['the end, the menu closed again', m.after.end]] : [])]
  for (const [where, p] of positions) {
    if (p.coveredByHeader.length) out.push(`${name}, scrolled to ${where}: the header covers ${p.coveredByHeader.join(', ')}`)
    if (p.stopsReachable.length === 0) out.push(`${name}, scrolled to ${where}: no stop control reachable (${p.stops.join(', ') || 'none found'})`)
    if (p.unreachable.length) out.push(`${name}, scrolled to ${where}: stop or send control not reachable: ${p.unreachable.join(', ')}`)
    if (m.pin === 'on' && p.panels === false) out.push(`${name}, scrolled to ${where}: the pinned header's ⊞ Panels is covered or off screen`)
  }
  if (m.reach === -1) out.push(`${name}: ⊞ Panels is covered or off screen at every scroll position`)
  if (m.neverReached?.length) out.push(`${name}: never on screen and topmost at any scroll position: ${m.neverReached.join(', ')}`)
  for (const [where, mc] of [['the top', m.menuTop], ['the end', m.menuEnd], ['its button', m.menuReach]]) {
    if (!mc || mc.none) continue
    if (!mc.opened) out.push(`${name}: ⊞ Panels did not open with the shell at ${where}`)
    else {
      if (cover && mc.covered.length) out.push(`${name}, ⊞ Panels open at ${where}: ${mc.covered.length} of its items covered (${mc.covered.join(', ')})`)
      if (mc.offx) out.push(`${name}, ⊞ Panels open at ${where}: ${mc.offx} of its items off the window's left or right edge`)
      if (mc.stopsOpen === 0) out.push(`${name}, ⊞ Panels open at ${where}: no stop control reachable`)
      if (mc.closedMenu != null) out.push(`${name}: ⊞ Panels closed, but the header still carries its lift`)
    }
  }
  if (m.pin === 'on') {
    const [r, n] = m.end.headReach.split('/').map(Number)
    if (r < n) out.push(`${name}: pinned, but only ${m.end.headReach} header controls on screen at the end of the scroll`)
  }
  return out
}

let code = 0
const red = (msg) => {
  console.error(`digishell-probe: RED — ${msg}`)
  code = 1
}
try {
  for (const [view, route, sel, sizes, cover] of VIEWS) {
    for (const size of sizes) {
      const m = await measureAt(route, sel, size)
      const name = `${view} at ${tagOf(size)}`
      if (!m) {
        console.error(`digishell-probe: ${name} never drew`)
        process.exitCode = 2
        throw new Error('no screen')
      }
      const info = (p) => `stops ${p.stopsReachable.join('+') || '—'}${p.coveredOther.length ? `, covered by other than the header: ${p.coveredOther.join(', ')}` : ''}`
      const menu = (mc) => (mc?.opened ? `${mc.ok}/${mc.n} on screen and topmost${mc.off ? `, ${mc.off} outside the window` : ''}${mc.clipped ? `, ${mc.clipped} clipped by the scroll` : ''}${mc.covered.length ? `, ${mc.covered.length} covered` : ''}` : 'not opened')
      console.log(`digishell-probe: ${name.padEnd(22)} header ${m.pin === 'on' ? 'pinned' : 'unpinned'} (${m.vp}), dock ${m.fold === 'on' ? 'folded' : 'whole'}, scrolls ${m.valve} px · top: ${info(m.top)} · end: ${info(m.end)}, header ${m.end.headReach} · ⊞ button reachable ${m.reach == null ? 'n/a' : m.reach < 0 ? 'NEVER' : `from scroll ${m.reach}`} · ⊞ Panels at top ${menu(m.menuTop)}, at end ${menu(m.menuEnd)}${m.menuReach ? `, at its button ${menu(m.menuReach)}` : ''}`)
      for (const r of reds(name, m, cover)) red(r)
    }
  }
  // THE CONTROL.
  const size = [900, 600, '175']
  const m = await measureAt('sstv', 'main.sstv-view', size, PLANT, false)
  const caught = m ? reds(`SSTV at ${tagOf(size)} (planted)`, m).filter((r) => r.includes('the header covers')) : []
  if (caught.length === 0) red('CONTROL FAILED — a header forced to stick on top covered nothing the probe could see; check 1 cannot fail')
  else console.log(`digishell-probe: CONTROL — ${caught[0]} (caught)`)
  const size2 = [1366, 768, '150']
  const m2 = await measureAt('rtty', 'main.rtty-cockpit', size2, NO_LIFT)
  const caught2 = m2 ? reds(`RTTY at ${tagOf(size2)} (lift removed)`, m2).filter((r) => r.includes('items covered')) : []
  if (caught2.length === 0) red('CONTROL FAILED — a header held under the strip with its menu open covered no item; check 4 cannot fail')
  else console.log(`digishell-probe: CONTROL — ${caught2[0]} (caught)`)
  const size3 = [900, 600, '175']
  const m3 = await measureAt('rtty', 'main.rtty-cockpit', size3, NO_FOLD, false)
  const caught3 = m3 ? reds(`RTTY at ${tagOf(size3)} (fold removed)`, m3).filter((r) => r.includes('stop or send control not reachable')) : []
  if (caught3.length === 0) red('CONTROL FAILED — a dock kept whole where it does not fit covered no stop or send control; check 5 cannot fail')
  else console.log(`digishell-probe: CONTROL — ${caught3[0]} (caught)`)
  const size4 = [700, 800, '175']
  const m4 = await measureAt('rtty', 'main.rtty-cockpit', size4, NO_CLAMP)
  const caught4 = m4 ? reds(`RTTY at ${tagOf(size4)} (clamp removed)`, m4).filter((r) => r.includes('off the window')) : []
  if (caught4.length === 0) red('CONTROL FAILED — an unclamped ⊞ Panels menu stayed inside the window; check 4 cannot see an overhang')
  else console.log(`digishell-probe: CONTROL — ${caught4[0]} (caught)`)
  if (code === 0) console.log('digishell-probe: PASS — no header covers a transmit control, every stop control and Send/PTT is on screen and topmost at every scroll position of every screen at every size, ⊞ Panels can be clicked, a pinned header stays on screen, every on-screen item of an open ⊞ Panels menu is reachable and inside the window, and all four controls were caught')
  process.exitCode = process.exitCode || code
} catch (e) {
  if (!process.exitCode) {
    console.error(`digishell-probe: ${e?.stack ?? e}`)
    process.exitCode = 2
  }
} finally {
  await browser.stop()
  await server.close()
  await rm(cacheDir, { recursive: true, force: true })
}
