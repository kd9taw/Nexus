// The SSTV, RTTY and PSK shells — and CW's, Phone's, JS8's and JTTY's at the small sizes — in real Chrome, for what jsdom cannot see:
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
//      is still reachable with the menu open, wherever it was opened from. An item outside the menu's OWN scrollport is scrolled out of view in the
//      menu, not covered. And (4b) the menu's box stays inside the window and the shell's
//      scrollport (it opened downward at 0.7 × the window whatever room was below the button, and ran past the window
//      at 150–175 %: CW at 1280×720, 617–1121 px, measured); (4c) a menu shifted sideways stands exactly
//      4 px × the UI zoom from the scrollport's edge or from the stop control's; (4d) every item, brought to the middle of the menu by its own scroll, is
//      the topmost thing where it stands. Items below the window are printed, not failed (the menu is that
//      tall at 175 % on a small window everywhere). CLOSED again, the header carries no lift and check 1 holds.
//   5. EVERY STOP CONTROL AND SEND ARE ON SCREEN AND TOPMOST, the menu closed: each of Stop TX (strip), Esc/Stop
//      (RTTY/PSK), Stop (SSTV's bar) and the dock's Send (CW/RTTY/PSK) or PTT (Phone). On a small window at a large
//      zoom the dock was taller than the scrollport: it painted over the strip's TX On/Off, Tune and Stop TX at every
//      position and pushed Send below the window at the top (900×600 and 700×800 at 175 %, measured), until the dock
//      learnt to fold (cockpit-panes.css `.cockpit-txdock[data-fold='on']`).
//   6. THE ⊞ PANELS BUTTON CAN BE CLICKED: pinned, it is topmost at every position; where the header scrolls, there is
//      a scroll position (searched every 6 px) where it is on screen and topmost. Under a dock taller than the
//      scrollport it never was, on RTTY and PSK at 900×600 and 700×800 at 175 % (measured).
//   8. THE MENU NEVER STANDS OVER THE SHELL'S STOP BUTTON (`.op-btn.stop`: Stop TX, on every screen, Operate's too), on
//      a 9 × 5 grid across the whole button, a single point being how a menu over half of Stop TX passed: opened with
//      a REAL click from the first, middle and last scroll positions where ⊞ is clickable; opened with REAL Tab + Enter
//      at scroll 0 (a keyboard operator opens it from a button the pointer cannot reach, under a sticky surface); and
//      at every step of a walk of the shell with the menu open. Each of those opens is also at least SHORTEST_MENU_PX
//      tall (or as tall as the whole menu), inside the window, and every item is topmost under the menu's own scroll.
//      Run on Operate as well.
//   9. ON A FOLDED DOCK, A FOCUSED CONTROL IS NEVER HIDDEN: real Tab through the dock's rows, and the centre of each control
//      focused is the topmost thing there. They scroll with the panes, so Tab could leave focus on a macro key behind
//      the parked keep row, and Enter then sends a key the operator cannot see (RTTY at 900×600: 15 of 18, measured).
//   10. JTTY'S REFUSAL IS ON SCREEN, whatever the scroll, covering no stop button, and with it showing no control of the
//      keep row or the strip moves. JS8'S HEARTBEAT MENU OPENS IN SIGHT: its first item is topmost as it opens.
//   7. EVERY CONTROL OF THE STRIP AND THE DOCK CAN BE CLICKED at some scroll position (the same 6 px search): a folded
//      dock's macro keys scroll with the panes, and each must come out between the parked surfaces.
//   11. NO POINTER PRESS MOVES ANY KEY: a real press on a text field or a <select> with its centre hidden leaves the shell
//      where it was, held and released, and a real second click at the same point reaches no sender (a press that
//      re-centred the field put the second click of a double click on F1: a send call on JTTY and PSK, measured).
//   12. A WINDOW REGAIN MOVES NOTHING: focusin fires again on the window's way back, with a Tab-focused key since scrolled
//      behind the keep row (a click just after it sent a different key, measured).
//   And JTTY's longest real refusal, on check 9 and in check 10: a real click on the words stays on the words (they once
//   passed it to the hidden key behind them), the whole text is in its tooltip, and Tab stops behind it nowhere.
// THE CONTROLS, run every time: the header forced to stick on top (z 9) at 900×600 / 175 % must make check 1 fail;
// the lift taken away (the header held at 4 with its menu open) at 1366×768 / 150 % on RTTY must make check 4 fail;
// the dock's fold taken away at 900×600 / 175 % on RTTY must make check 5 fail; and the menu's clamp into the window
// taken away at 700×800 / 175 % on RTTY must make check 4 fail; the menu's sizing taken away (0.7 × the window,
// downward) at 1366×768 / 150 % on CW must make check 4b fail; a menu planted over the stop button at 1024×768 / 175 %
// on Phone, and a menu held to 20 px at 1280×720 / 175 % on CW, must make check 8 fail; and a keep row planted over the
// whole shell at 900×600 / 175 % on RTTY must make check 9 fail; and a press and a regain planted to scroll the shell
// at 900×600 / 175 % on JTTY must make checks 11 and 12 fail.
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
const SMALL = [[900, 600, '175'], [700, 800, '175'], [1024, 768, '175'], [1280, 720, '175'], [1366, 768, '150'], [1200, 750, null]]
const VIEWS = [['SSTV', 'sstv', 'main.sstv-view', SIZES, true], ['RTTY', 'rtty', 'main.rtty-cockpit', SIZES, true],
  ['PSK', 'psk', 'main.psk-cockpit', SIZES, true], ['CW', 'cw', 'main.cw-cockpit', SMALL, true], ['Phone', 'phone', 'main.phone-cockpit', SMALL, true],
  ['JS8', 'js8', 'main.js8-cockpit', SMALL, true], ['JTTY', 'jtty', 'main.jtty-cockpit', SMALL, true]]
const FEATURES = JSON.stringify({ profile: 'custom', enabled: { rtty: true, psk: true, sstv: true, js8: true, jtty: true } })
/** Check 8's other screen, which has none of the rest to run (Operate has no `.cockpit-txstrip`). */
const COVER_ONLY = [['Operate', 'operate', 'main.operate-cockpit']]
const COVER_SIZES = [[900, 600, '175'], [700, 800, '175'], [1024, 768, '175'], [1280, 720, '175'], [1366, 768, '150']]
/** The shortest a menu with more to show may be, in window px: 26 px showed less than one entry (measured). */
const SHORTEST_MENU_PX = 60
const PLANT = ".cockpit-header-pinned, .cockpit-header-pinned[data-pin='off'] { position: sticky !important; top: 0 !important; z-index: 9 !important }"
const NO_LIFT = ".cockpit-header-pinned[data-menu='open'] { z-index: 4 !important }"
const NO_FOLD = ".cockpit-txdock[data-fold='on'] { display: flex !important } .cockpit-txdock-keep[data-fold='on'] { position: static !important; padding: 0 !important; border: 0 !important }"
const NO_CLAMP = '.panels-menu-pop { translate: none !important }'
const OVER_STOP = '.panels-menu-pop { position: fixed !important; top: 5% !important; bottom: 5% !important; left: 5% !important; right: 5% !important; width: auto !important; max-width: none !important; max-height: none !important; translate: none !important }'
const TINY_MENU = '.panels-menu-pop { max-height: 20px !important }'
const KEEP_OVER_ALL = ".cockpit-txdock-keep[data-fold='on']::before { content: ''; position: absolute; left: 0; right: 0; top: -900px; bottom: 0 }"
const NO_FIT = ".panels-menu-pop { max-height: calc(0.7 * var(--vh-eff, 100vh)) !important; top: 100% !important; bottom: auto !important }"
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
  const sb0 = shell.getBoundingClientRect()
  const pq = pop.getBoundingClientRect()
  // 4b. The menu's box stays inside the window and inside the shell's scrollport (which clips it).
  const boxOut = pq.top < Math.max(0, sb0.top) - 0.5 || pq.bottom > Math.min(innerHeight, sb0.bottom) + 0.5
    ? Math.round(pq.top) + '–' + Math.round(pq.bottom) + ' against ' + Math.round(Math.max(0, sb0.top)) + '–' + Math.round(Math.min(innerHeight, sb0.bottom)) : null
  // 4c. Where it was shifted sideways, it stands exactly 4 px (× the UI zoom) from the window's edge or the stop button's.
  let clampEdge = null
  if (parseFloat(pop.style.translate || '0') !== 0) {
    const zoom = pq.width / pop.offsetWidth
    const stops = [...shell.querySelectorAll('.op-btn.stop')].map((b) => b.getBoundingClientRect()).filter((q) => q.width > 0)
    const sb = shell.getBoundingClientRect()
    const clientRight = sb.left + (shell.clientLeft + shell.clientWidth) * (sb.width / shell.offsetWidth)
    const gaps = [pq.left - sb.left, clientRight - pq.right, ...stops.flatMap((q) => [q.left - pq.right, pq.left - q.right])]
    if (!gaps.some((g) => Math.abs(g - 4 * zoom) < 1)) clampEdge = 'gaps ' + gaps.map((g) => g.toFixed(1)).join(', ') + ', expected ' + (4 * zoom).toFixed(1)
  }
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
  // The menu's own scrollport: an item outside it is scrolled out of view INSIDE the menu, never a cover.
  // (client sizes are layout px, rects are window px: the ratio is the UI zoom)
  const pz = pop.offsetHeight > 0 ? pq.height / pop.offsetHeight : 1
  const pc = { top: pq.top + pop.clientTop * pz, bottom: pq.top + (pop.clientTop + pop.clientHeight) * pz }
  const rows = items.map((b) => {
    const q = b.getBoundingClientRect()
    // Not wholly inside it: part-scrolled at its edge (check 4d brings each one to the middle and tests it there).
    if (q.top < pc.top - 0.5 || q.bottom > pc.bottom + 0.5) return 'inmenu'
    if (q.bottom > innerHeight + 0.5 || q.top < -0.5 || q.left + q.width / 2 < 0 || q.left + q.width / 2 > innerWidth) return 'off'
    // Outside the shell's own scrollport the menu is CLIPPED by it (a header that has scrolled away
    // opens its menu there), which is the shell's scroll, not a cover.
    const sb = shell.getBoundingClientRect()
    if (q.top + q.height / 2 < sb.top || q.top + q.height / 2 > sb.bottom) return 'clipped'
    const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
    if (t && (t === b || b.contains(t) || pop.contains(t))) return 'ok'
    return t && t.closest('.cockpit-txstrip') ? 'strip' : t && t.closest('.cockpit-txdock, .sstv-tx-bar') ? 'dock' : 'other:' + (t ? String(t.className).split(' ')[0] || t.tagName : 'none')
  })
  // 4d. EVERY item can be reached by the menu's own scroll: brought to the middle of the menu, it is on screen,
  // inside the shell's scrollport, and the topmost thing where it stands.
  const unreached = []
  for (const b of items) {
    const q0 = b.getBoundingClientRect()
    const p0 = pop.getBoundingClientRect()
    pop.scrollTop = Math.max(0, (q0.top - p0.top + q0.height / 2) / pz - pop.clientTop + pop.scrollTop - pop.clientHeight / 2)
    const q = b.getBoundingClientRect()
    const sb = shell.getBoundingClientRect()
    const cx = q.left + q.width / 2, cy = q.top + q.height / 2
    const inside = cy >= Math.max(0, sb.top) && cy <= Math.min(innerHeight, sb.bottom) && cx >= 0 && cx <= innerWidth
    const t = inside ? document.elementFromPoint(cx, cy) : null
    if (!(t && (t === b || b.contains(t) || pop.contains(t)))) unreached.push((b.textContent || b.tagName).replace(/\s+/g, ' ').trim().slice(0, 18))
  }
  pop.scrollTop = 0
  const lifted = head.getAttribute('data-menu')
  btn.click()
  await new Promise((r) => setTimeout(r, 300))
  const closedMenu = head.getAttribute('data-menu')
  shell.scrollTop = 0
  return { opened: true, boxOut, clampEdge, unreached, offx, stopsOpen, n: rows.length, ok: rows.filter((r) => r === 'ok').length, off: rows.filter((r) => r === 'off').length,
    clipped: rows.filter((r) => r === 'clipped').length, inmenu: rows.filter((r) => r === 'inmenu').length, covered: rows.filter((r) => r !== 'ok' && r !== 'off' && r !== 'clipped' && r !== 'inmenu'), lifted, closedMenu, pin: head.getAttribute('data-pin') }
})()`

/** Check 8's page helpers. Every length is window px except scrollTop, which is the shell's own. */
const coverHelpers = (sel) => `(() => {
  const H = window.__cover = {}
  H.shell = () => document.querySelector(${JSON.stringify(sel)})
  H.btn = () => H.shell().querySelector('.panels-menu-btn')
  H.pop = () => H.shell().querySelector('.panels-menu-pop')
  // Every parked stop: the shell's stop button, and whatever in the dock or SSTV's bar says Stop (Esc/Stop).
  H.stops = () => [...H.shell().querySelectorAll('.op-btn.stop, .cockpit-txdock button, .sstv-tx-bar button')].filter((b) => (b.matches('.op-btn.stop') || /stop/i.test(b.textContent || '')) && b.closest('[hidden]') == null && b.getBoundingClientRect().width > 0)
  // Grid points of the stop button (9 × 5, inset past its round corners) with the MENU topmost, worst button.
  H.under = () => {
    const pop = H.pop()
    let worst = 0
    if (pop) for (const b of H.stops()) {
      const q = b.getBoundingClientRect()
      let n = 0
      for (let i = 0; i < 9; i++) for (let j = 0; j < 5; j++) {
        const t = document.elementFromPoint(q.left + q.width * (0.08 + 0.84 * i / 8), q.top + q.height * (0.14 + 0.72 * j / 4))
        if (t && pop.contains(t)) n++
      }
      worst = Math.max(worst, n)
    }
    return worst
  }
  H.topmost = (b) => { const q = b.getBoundingClientRect(); const x = q.left + q.width / 2, y = q.top + q.height / 2
    if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return false
    const t = document.elementFromPoint(x, y); return t != null && (t === b || b.contains(t)) }
  H.max = () => Math.max(0, H.shell().scrollHeight - H.shell().clientHeight)
  H.clickable = () => { const s = H.shell(); const out = []; for (let p = 0; p <= H.max() + 5; p += 6) { s.scrollTop = Math.min(p, H.max()); if (H.topmost(H.btn())) out.push(s.scrollTop) } s.scrollTop = 0; return [...new Set(out)] }
  H.state = () => { const pop = H.pop(); if (!pop) return null; const q = pop.getBoundingClientRect(); const sb = H.shell().getBoundingClientRect()
    const z = pop.offsetHeight > 0 ? q.height / pop.offsetHeight : 1
    const whole = pop.scrollHeight <= pop.clientHeight + 1
    const out = q.top < Math.max(0, sb.top) - 0.5 || q.bottom > Math.min(innerHeight, sb.bottom) + 0.5 || q.left < -0.5 || q.right > innerWidth + 0.5
    return { under: H.under(), h: Math.round(q.height), short: !whole && q.height < ${SHORTEST_MENU_PX}, out, box: [q.left, q.top, q.right, q.bottom].map(Math.round).join(','), up: pop.hasAttribute('data-up'), z } }
  // Every item, brought to the middle of the menu by its own scroll, is topmost.
  H.items = () => { const pop = H.pop(); const z = pop.getBoundingClientRect().height / pop.offsetHeight; const missed = []
    for (const b of pop.querySelectorAll('button, input, label')) {
      if (b.getBoundingClientRect().height === 0) continue
      const q0 = b.getBoundingClientRect(), p0 = pop.getBoundingClientRect()
      pop.scrollTop = Math.max(0, (q0.top - p0.top + q0.height / 2) / z - pop.clientTop + pop.scrollTop - pop.clientHeight / 2)
      const q = b.getBoundingClientRect(); const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2)
      if (!(t && (t === b || b.contains(t) || pop.contains(t)))) missed.push((b.textContent || b.tagName).trim().slice(0, 16))
    }
    pop.scrollTop = 0; return missed }
  // The shell walked with the menu open: two frames at each step, so the refit has run, and run again after any
  // clamp of the shell's scrollTop that the first refit caused by shrinking the shell's scrollable overflow.
  H.walk = async () => { const s = H.shell(); const from = s.scrollTop; let under = 0, out = 0, flips = 0, n = 0, last = null, minH = 1e9, firstOut = null
    for (let y = 0; y <= H.max() + 23; y += 24) { s.scrollTop = Math.min(y, H.max()); await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(r, 0))))
      const st = H.state(); if (!st) break; n++; under = Math.max(under, st.under); if (st.out) { out++; firstOut ??= 'scroll ' + Math.round(s.scrollTop) + ', menu ' + st.box + ', shell ' + [H.shell().getBoundingClientRect().top, H.shell().getBoundingClientRect().bottom].map(Math.round).join('–') } minH = Math.min(minH, st.h); if (last !== null && last !== st.up) flips++; last = st.up }
    s.scrollTop = from; return { n, under, out, flips, minH, firstOut } }
  return 1 })()`

/** Check 8 on one page. `call` drives REAL input: the clicks and keys go through the browser, not element.click(). */
async function coverRun(evaluate, call, sel) {
  await evaluate(coverHelpers(sel))
  if (!(await evaluate('!!__cover.btn()'))) return { none: true }
  const opens = []
  const closeMenu = () => evaluate('(async () => { if (__cover.pop()) __cover.btn().click(); await new Promise((r) => setTimeout(r, 120)); return !__cover.pop() })()')
  const clickable = await evaluate('__cover.clickable()')
  const picks = [...new Set(clickable.length ? [clickable[0], clickable[clickable.length >> 1], clickable[clickable.length - 1]] : [])]
  for (const [i, sc] of picks.entries()) {
    await evaluate(`__cover.shell().scrollTop = ${sc}; 1`)
    await sleep(100)
    const [x, y] = await evaluate('(() => { const q = __cover.btn().getBoundingClientRect(); return [q.left + q.width / 2, q.top + q.height / 2] })()')
    await call('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y })
    await call('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 })
    await call('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 })
    await sleep(250)
    const st = await evaluate('__cover.state()')
    if (!st) { opens.push({ how: `click at scroll ${Math.round(sc)}`, failed: true }); continue }
    const rec = { how: `click at scroll ${Math.round(sc)}`, ...st, missed: await evaluate('__cover.items()') }
    if (i === 0) rec.walk = await evaluate('__cover.walk()')
    opens.push(rec)
    await closeMenu()
  }
  // REAL Tab + Enter from the shell's first control, at scroll 0.
  await evaluate(`(() => { const s = __cover.shell(); s.scrollTop = 0; const f = s.querySelector('button, input, select, a[href]'); f?.focus({ preventScroll: true }); s.scrollTop = 0; return 1 })()`)
  let reached = await evaluate('document.activeElement === __cover.btn()')
  for (let i = 0; i < 60 && !reached; i++) {
    await call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 })
    await call('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 })
    reached = await evaluate('document.activeElement === __cover.btn()')
  }
  if (!reached) opens.push({ how: 'Tab + Enter', failed: true, why: 'Tab never reached the button' })
  else {
    await call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' })
    await call('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 })
    await sleep(250)
    const st = await evaluate('__cover.state()')
    opens.push(st ? { how: 'Tab + Enter', ...st, missed: await evaluate('__cover.items()') } : { how: 'Tab + Enter', failed: true, why: 'Enter did not open it' })
    await closeMenu()
  }
  await evaluate('__cover.shell().scrollTop = 0; 1')
  return { opens, clickable: clickable.length }
}

/** Check 9: real Tab through a folded dock's rows. `call` drives the keys. */
async function focusRun(evaluate, call, sel) {
  const settled = () => evaluate('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => r(1), 30))))')
  const ok = await evaluate(`(() => { const s = document.querySelector(${JSON.stringify(sel)}); const d = s.querySelector(':scope > .cockpit-txdock'); if (d?.getAttribute('data-fold') !== 'on') return false
    window.__focus = { d, s }
    const all = [...s.querySelectorAll('button:not([disabled]), input:not([disabled]), select:not([disabled]), a[href], [tabindex="0"]')].filter((e) => e.getBoundingClientRect().width > 0)
    const i = all.indexOf(all.find((e) => d.contains(e))); (all[i - 1] ?? all[i]).focus(); return true })()`)
  if (!ok) return null
  await settled()
  const seen = []
  for (let i = 0; i < 45; i++) {
    await call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 })
    await call('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 })
    await settled()
    const r = await evaluate(`(() => { const e = document.activeElement; if (!e || !__focus.d.contains(e)) return null; const q = e.getBoundingClientRect()
      const hit = ([a, b]) => { const t = document.elementFromPoint(q.left + q.width * a, q.top + q.height * b); return t != null && (t === e || e.contains(t)) }
      // Whatever a notice lets clicks through, it still hides what is under it from the eye.
      const lifted = [...document.querySelectorAll('.jtty-dock-reason')]; lifted.forEach((x) => { x.style.pointerEvents = 'auto' })
      const pts = [[.5, .5], [.1, .5], [.9, .5], [.5, .15], [.5, .85]].map(hit)
      lifted.forEach((x) => { x.style.pointerEvents = '' })
      return { n: (e.getAttribute('aria-label') || e.textContent || e.placeholder || e.tagName).replace(/\\s+/g, ' ').trim().slice(0, 16), centre: pts[0], whole: pts.every(Boolean) } })()`)
    if (!r) { if (seen.length) break; continue }
    seen.push(r)
  }
  await evaluate('document.activeElement?.blur(); __focus.s.scrollTop = 0; 1')
  return { n: seen.length, hidden: seen.filter((r) => !r.centre).map((r) => r.n), part: seen.filter((r) => r.centre && !r.whole).length }
}

/** Check 10, JS8: the heartbeat menu, opened with a real click from where its button can be clicked. */
async function hbRun(evaluate, call, sel) {
  const at = await evaluate(`(() => { const s = document.querySelector(${JSON.stringify(sel)}); const b = s.querySelector('.js8-hb-menu-btn'); if (!b || b.disabled) return null; const top = () => { const q = b.getBoundingClientRect(); const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2); return t === b || b.contains(t) }
    const max = s.scrollHeight - s.clientHeight; for (let y = 0; y <= max + 5; y += 6) { s.scrollTop = Math.min(y, max); if (top()) { const q = b.getBoundingClientRect(); return [q.left + q.width / 2, q.top + q.height / 2] } } return null })()`)
  if (!at) return { skipped: true }
  await call('Input.dispatchMouseEvent', { type: 'mousePressed', x: at[0], y: at[1], button: 'left', clickCount: 1 })
  await call('Input.dispatchMouseEvent', { type: 'mouseReleased', x: at[0], y: at[1], button: 'left', clickCount: 1 })
  // The reveal waits out a double click's second press (revealAfterPress, 500 ms).
  await sleep(750)
  const r = await evaluate(`(() => { const s = document.querySelector(${JSON.stringify(sel)}); const i = s.querySelector('.js8-hb-menu-item'); if (!i) return { opened: false }; const q = i.getBoundingClientRect(); const t = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2); const seen = t === i || i.contains(t); s.querySelector('.js8-hb-menu-btn').click(); return { opened: true, seen, by: seen ? null : String(t?.className).slice(0, 24) } })()`)
  await evaluate(`document.querySelector(${JSON.stringify(sel)}).scrollTop = 0; 1`)
  return r
}

// The longest real reason the transmitter gives (crates/tempo-app/src/engine/jtty_tx.rs, KEY_REFUSED).
const LONG_REFUSAL = "JTTY keyer: the rig didn't accept PTT, so nothing was sent and what was queued was dropped. Check your PTT method + that Nexus's audio output is routed to the rig (like FT8)."
const LONG_MATCH = 'keyer: the rig'
/** Check 10, JTTY: a refusal raised, read at every 24 px of the shell's scroll, against where the stops stood without it. */
async function refusalRun(evaluate, sel, realMouse, focus) {
  const stopsAt = `[...document.querySelector(${JSON.stringify(sel)}).querySelectorAll('.op-btn.stop, .cockpit-txdock .cw-send-btn, .cockpit-txdock .jtty-stop')].map((b) => Math.round(b.getBoundingClientRect().top))`
  const walk = `(async () => { const s = document.querySelector(${JSON.stringify(sel)}); const max = s.scrollHeight - s.clientHeight; let n = 0, shown = 0, over = 0, moved = 0; const was = {}
    for (let y = 0; y <= max + 23; y += 24) { s.scrollTop = Math.min(y, max); await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0))); n++
      const e = [...s.querySelectorAll('[role=status]')].find((x) => x.textContent.includes(${JSON.stringify(LONG_MATCH)})); if (!e) continue; const q = e.getBoundingClientRect()
      e.style.pointerEvents = 'auto' // it takes no clicks, on purpose: lifted only to ask what is topmost
      const hit = document.elementFromPoint(q.left + q.width / 2, q.top + q.height / 2); e.style.pointerEvents = ''; if (hit === e || e.contains(hit)) shown++
      for (const b of s.querySelectorAll('.op-btn.stop')) { const k = b.getBoundingClientRect(); if (Math.min(k.bottom, q.bottom) > Math.max(k.top, q.top) && Math.min(k.right, q.right) > Math.max(k.left, q.left)) over++ }
      const now = ${stopsAt}.join(); const was = window.__was?.[Math.round(s.scrollTop)]; if (was !== undefined && was !== now) moved++ }
    s.scrollTop = 0; return { n, shown, over, moved } })()`
  // Where the stops stand at each position with no refusal, for "moved".
  await evaluate(`(async () => { const s = document.querySelector(${JSON.stringify(sel)}); const max = s.scrollHeight - s.clientHeight; window.__was = {}; for (let y = 0; y <= max + 23; y += 24) { s.scrollTop = Math.min(y, max); await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0))); window.__was[Math.round(s.scrollTop)] = ${stopsAt}.join() } s.scrollTop = 0; return 1 })()`)
  await evaluate(`(() => { window.__jttyOver = { tx: { refusal: ${JSON.stringify(LONG_REFUSAL)}, txHz: 1500, serial: 1, sending: false, live: false, left: 0, pending: '', notice: null } }; return 1 })()`)
  for (let i = 0; i < 40; i++) {
    await sleep(100)
    if (await evaluate(`[...document.querySelector(${JSON.stringify(sel)}).querySelectorAll('[role=status]')].some((x) => x.textContent.includes(${JSON.stringify(LONG_MATCH)}))`)) break
  }
  const r = await evaluate(walk)
  // The whole text can still be read, and the notice is no taller than its clamp; a real click on it stays on it, and Tab
  // under it never stops behind it.
  r.text = await evaluate(`(() => { const e = document.querySelector('.jtty-dock-reason'); if (!e) return null; const q = e.getBoundingClientRect(); const lh = parseFloat(getComputedStyle(e).lineHeight) || parseFloat(getComputedStyle(e).fontSize) * 1.4
    return { folded: e.closest('[data-fold=on]') != null, lines: Math.round(q.height / (lh * q.height / e.offsetHeight)), title: e.title === ${JSON.stringify(LONG_REFUSAL)} } })()`)
  const at = await evaluate(`(async () => { const s = document.querySelector(${JSON.stringify(sel)}); const e = document.querySelector('.jtty-dock-reason'); const max = s.scrollHeight - s.clientHeight; let pick = null
    for (let y = 0; y <= max + 5 && e; y += 6) { s.scrollTop = Math.min(y, max); const q = e.getBoundingClientRect(); const x = q.left + q.width / 2, yy = q.top + q.height / 2
      if (document.elementsFromPoint(x, yy).some((t) => t.closest('.cw-macro'))) { pick = [x, yy, s.scrollTop]; break } }
    await new Promise((r) => requestAnimationFrame(() => setTimeout(r, 30)))
    window.__clickTo = null; addEventListener('click', (ev) => { window.__clickTo = ev.target.closest?.('.jtty-dock-reason') ? 'notice' : String(ev.target.closest?.('button')?.textContent ?? ev.target.className).slice(0, 24) }, { capture: true, once: true }); return pick })()`)
  if (at && realMouse) {
    await realMouse(at[0], at[1])
    await sleep(120)
    r.clicked = await evaluate('window.__clickTo')
  }
  if (focus) r.focus = await focus()
  await evaluate('window.__jttyOver = undefined; 1')
  await sleep(300)
  return r
}

const SENDER = `((e) => !!e?.closest?.('.cw-macro, .cw-send-btn, .js8-send, .js8-hb, .ph-ptt') && !/stop/i.test(e.textContent || ''))`
/** Check 11: a REAL press on a text field or a <select> whose centre is hidden, on the part that can be clicked. The shell
 *  must not move while it is held, and a real second click at the same point must not reach a sender. */
async function pressRun(evaluate, mouse, sel) {
  const list = await evaluate(`(() => { const s = document.querySelector(${JSON.stringify(sel)}); const d = s.querySelector(':scope > .cockpit-txdock'); if (d?.getAttribute('data-fold') !== 'on') return null
    const k = d.querySelector(':scope > .cockpit-txdock-keep'), st = s.querySelector(':scope > .cockpit-txstrip')
    // Where this control's centre is hidden and a part of it can be clicked. Asked again at each press: a press can change the layout.
    window.__pick = (e) => { const max = Math.max(0, s.scrollHeight - s.clientHeight); for (let y = 0; y <= max + 3; y += 6) { s.scrollTop = Math.min(y, max); const q = e.getBoundingClientRect(); const cy = q.top + q.height / 2; const K = k.getBoundingClientRect(), T = st?.getBoundingClientRect()
        if (!((cy >= K.top && cy <= K.bottom) || (T && cy >= T.top && cy <= T.bottom))) continue
        for (let j = 0; j < 7; j++) for (let i = 0; i < 5; i++) { const x = q.left + q.width * (.1 + .2 * i), yy = q.top + q.height * (.08 + .14 * j); if (x < 0 || yy < 0 || x >= innerWidth || yy >= innerHeight) continue
          const t = document.elementFromPoint(x, yy); if (t === e || e.contains(t)) return { x, y: yy, sc: s.scrollTop } } } return null }
    const all = [...s.querySelectorAll('input:not([type=checkbox]):not([type=radio]):not([type=range]):not([type=file]):not([disabled]), textarea:not([disabled]), select:not([disabled])')].filter((e) => e.getBoundingClientRect().width > 0 && !k.contains(e) && !st?.contains(e))
    window.__els = []; const out = []
    for (const e of all) if (window.__pick(e)) { window.__els.push(e); out.push({ i: window.__els.length - 1, tag: e.tagName, n: (e.getAttribute('aria-label') || e.placeholder || e.tagName).slice(0, 18) }) }
    s.scrollTop = 0; const sels = out.filter((o) => o.tag === 'SELECT'), rest = out.filter((o) => o.tag !== 'SELECT'); return [...sels, ...rest.filter((_, i) => i % Math.ceil(rest.length / 6) === 0)] })()`)
  if (!list) return null
  await evaluate(`(() => { window.__clicks = []; addEventListener('click', (e) => window.__clicks.push(${SENDER}(e.target)), true); return 1 })()`)
  const res = { n: 0, moved: [], onSender: [] }
  const settle = () => evaluate('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => r(1), 20))))')
  const top = `document.querySelector(${JSON.stringify(sel)}).scrollTop`
  for (const t0 of list) {
    await evaluate('document.activeElement?.blur?.(); 1')
    await settle()
    const at = await evaluate(`window.__pick(window.__els[${t0.i}])`)
    if (!at) continue
    const t = { ...t0, ...at }
    await evaluate(`${top} = ${t.sc}; window.__clicks.length = 0; 1`)
    await settle()
    const sc0 = await evaluate(top)
    await mouse('mouseMoved', t.x, t.y)
    await mouse('mousePressed', t.x, t.y, 1)
    await settle()
    const held = await evaluate(top)
    await mouse('mouseReleased', t.x, t.y, 1)
    await settle()
    const after = await evaluate(top)
    res.n++
    const worst = Math.max(Math.abs(held - sc0), Math.abs(after - sc0))
    if (worst > 0.5) res.moved.push(`${t.n} ${Math.round(worst)} px`)
    if (t.tag !== 'SELECT') {
      await mouse('mousePressed', t.x, t.y, 2)
      await mouse('mouseReleased', t.x, t.y, 2)
      await settle()
      if (await evaluate('window.__clicks.some(Boolean)')) res.onSender.push(t.n)
    } else await evaluate('document.activeElement?.blur?.(); 1')
    await evaluate(`${top} = 0; 1`)
  }
  return res
}

/** Check 12: a REAL window regain with a Tab-focused key scrolled behind the keep row. The shell must not move. */
async function regainRun(evaluate, call, browser, targetId, sel) {
  const ok = await evaluate(`(() => { const s = document.querySelector(${JSON.stringify(sel)}); const d = s.querySelector(':scope > .cockpit-txdock'); if (d?.getAttribute('data-fold') !== 'on') return false
    const k = d.querySelector(':scope > .cockpit-txdock-keep'); const all = [...s.querySelectorAll('button:not([disabled]), input:not([disabled]), select:not([disabled]), a[href], [tabindex="0"]')].filter((e) => e.getBoundingClientRect().width > 0)
    const i = all.indexOf(all.find((e) => d.contains(e) && !k.contains(e))); if (i < 0) return false; (all[i - 1] ?? all[i]).focus(); return true })()`)
  if (!ok) return null
  const settle = () => evaluate('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => r(1), 30))))')
  for (let i = 0; i < 12; i++) {
    await call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 })
    await call('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 })
    await settle()
    if (await evaluate(`(() => { const e = document.activeElement; const d = document.querySelector(${JSON.stringify(sel)} + ' > .cockpit-txdock'); return d.contains(e) && !d.querySelector(':scope > .cockpit-txdock-keep').contains(e) && e.tagName === 'BUTTON' })()`)) break
  }
  const sc0 = await evaluate(`(() => { const s = document.querySelector(${JSON.stringify(sel)}); const e = document.activeElement; const k = s.querySelector('.cockpit-txdock-keep[data-fold]'); const max = s.scrollHeight - s.clientHeight
    if (!k || !e) return s.scrollTop
    for (let y = 0; y <= max + 3; y += 3) { s.scrollTop = Math.min(y, max); const q = e.getBoundingClientRect(), K = k.getBoundingClientRect(); const cy = q.top + q.height / 2; if (cy >= K.top && cy <= K.bottom) return s.scrollTop } return s.scrollTop })()`)
  await settle()
  const held = await evaluate(`document.querySelector(${JSON.stringify(sel)}).scrollTop`)
  const other = await browser.call('Target.createTarget', { url: 'about:blank' })
  await browser.call('Target.activateTarget', { targetId: other.targetId })
  await sleep(120)
  await browser.call('Target.activateTarget', { targetId })
  await browser.call('Target.closeTarget', { targetId: other.targetId })
  await sleep(150)
  await settle()
  const after = await evaluate(`(() => { const s = document.querySelector(${JSON.stringify(sel)}); const a = document.activeElement; a?.blur?.(); const y = s.scrollTop; s.scrollTop = 0; return y })()`)
  return { from: Math.round(sc0), moved: Math.round(Math.abs(after - held)) }
}

function pinReds(name, m) {
  const out = []
  if (m.press) {
    if (m.press.moved.length) out.push(`${name}: a press moved the shell on ${m.press.moved.length} of ${m.press.n} fields (${m.press.moved.join(', ')})`)
    if (m.press.onSender.length) out.push(`${name}: the second click of a double click reached a sender on ${m.press.onSender.length} fields (${m.press.onSender.join(', ')})`)
  }
  if (m.regain?.moved > 0) out.push(`${name}: a window regain moved the shell ${m.regain.moved} px`)
  const r = m.refusal
  if (r?.text?.folded) {
    if (!r.text.title) out.push(`${name}: the whole refusal cannot be read: no tooltip`)
    if (r.text.lines > 3) out.push(`${name}: the folded refusal is ${r.text.lines} lines tall`)
  }
  if (r?.clicked && r.clicked !== 'notice') out.push(`${name}: a click on the refusal reached ${r.clicked}`)
  if (r?.focus?.hidden.length) out.push(`${name}: under the refusal, Tab leaves focus hidden on ${r.focus.hidden.length} of ${r.focus.n} controls (${r.focus.hidden.join(', ')})`)
  return out
}

function focusReds(name, m) {
  const out = []
  if (m.focus?.hidden.length) out.push(`${name}: Tab leaves focus hidden on ${m.focus.hidden.length} of ${m.focus.n} controls (${m.focus.hidden.join(', ')})`)
  if (m.hb && !m.hb.skipped) {
    if (!m.hb.opened) out.push(`${name}: the heartbeat menu did not open`)
    else if (!m.hb.seen) out.push(`${name}: the heartbeat menu opens hidden, under ${m.hb.by}`)
  }
  if (m.refusal) {
    if (m.refusal.shown < m.refusal.n) out.push(`${name}: JTTY's refusal is on screen at only ${m.refusal.shown} of ${m.refusal.n} scroll positions`)
    if (m.refusal.over > 0) out.push(`${name}: JTTY's refusal stands over the stop button at ${m.refusal.over} positions`)
    // Folded, a refusal is an overlay and may move nothing. Unfolded, this 2-line string grows the dock as it does on any tree.
    if (m.fold === 'on' && m.refusal.moved > 0) out.push(`${name}: a refusal showing moved a stop or Send at ${m.refusal.moved} positions`)
  }
  return out
}

function coverReds(name, c) {
  const out = []
  if (!c || c.none) return out
  for (const o of c.opens) {
    const at = `${name}, ⊞ Panels opened by ${o.how}`
    if (o.failed) { out.push(`${at}: did not open${o.why ? ` (${o.why})` : ''}`); continue }
    if (o.under > 0) out.push(`${at}: the stop button is under the menu at ${o.under} of 45 points (menu ${o.box})`)
    if (o.short) out.push(`${at}: the menu is only ${o.h} px tall with more to show`)
    if (o.out) out.push(`${at}: the menu runs out of the window or the scrollport (${o.box})`)
    if (o.missed?.length) out.push(`${at}: ${o.missed.length} items not reachable by its own scroll (${o.missed.join(', ')})`)
    if (o.walk) {
      if (o.walk.under > 0) out.push(`${at}, the shell walked with it open: the stop button is under the menu at up to ${o.walk.under} of 45 points`)
      if (o.walk.out > 0) out.push(`${at}, the shell walked with it open: out of the window at ${o.walk.out} of ${o.walk.n} steps (first: ${o.walk.firstOut})`)
      // The side is latched: it changes only when its side runs out of room, so one walk top to end can see that
      // twice at most (out of room below, later out of room above: the whole span). More is the menu oscillating.
      if (o.walk.flips > 2) out.push(`${at}, the shell walked with it open: it changed side ${o.walk.flips} times`)
    }
  }
  return out
}
const coverLine = (c) => (c?.opens ? c.opens.map((o) => (o.failed ? `${o.how} failed` : `${o.how} ${o.h} px${o.up ? ' up' : ''}, stop ${o.under}/45${o.walk ? `, walk min ${o.walk.minH} px, stop ${o.walk.under}/45, ${o.walk.flips} flips` : ''}`)).join(' · ') : 'no ⊞')

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

async function measureAt(route, sel, [W, H, scale], css = null, menus = true, cover = false, coverOnly = false, plant = null) {
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
      m = await evaluate(coverOnly ? `(() => { const s = document.querySelector(${JSON.stringify(sel)}); return s && s.getBoundingClientRect().height > 1 && document.documentElement.getAttribute('data-viewport') ? {} : null })()` : measure(sel)).catch(() => null)
    }
    if (!m) return null
    if (css) await evaluate(`(() => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.appendChild(s); return 1 })()`)
    if (coverOnly) {
      await sleep(800)
      return { cover: await coverRun(evaluate, call, sel) }
    }
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
    if (cover) m.cover = await coverRun(evaluate, call, sel)
    if (cover) {
      if (plant) await evaluate(`(() => { const shell = document.querySelector(${JSON.stringify(sel)}); ${plant}; return 1 })()`)
      const mouse = (type, x, y, clickCount) => call('Input.dispatchMouseEvent', { type, x, y, ...(clickCount ? { button: 'left', clickCount } : {}) })
      m.focus = await focusRun(evaluate, call, sel)
      m.press = await pressRun(evaluate, mouse, sel)
      m.regain = await regainRun(evaluate, call, browser, targetId, sel)
      if (route === 'js8') m.hb = await hbRun(evaluate, call, sel)
      if (route === 'jtty') m.refusal = await refusalRun(evaluate, sel, async (x, y) => { await mouse('mouseMoved', x, y); await mouse('mousePressed', x, y, 1); await mouse('mouseReleased', x, y, 1) }, () => focusRun(evaluate, call, sel))
    }
    return m
  } finally {
    await browser.call('Target.closeTarget', { targetId })
  }
}

const tagOf = ([W, H, s]) => `${W}×${H} ${s ? `${s} %` : 'fit'}`
/** `cover`: whether an open menu's items covered by the strip or the dock are a red — on every screen now. (CW's and
 *  Phone's "covered" items were items scrolled out of view inside the menu's own scroll, which is `inmenu`.) */
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
      if (mc.boxOut) out.push(`${name}, ⊞ Panels open at ${where}: the menu runs past the window or the scrollport (${mc.boxOut})`)
      if (mc.clampEdge) out.push(`${name}, ⊞ Panels open at ${where}: shifted into the window, but not to its 4 px edge (${mc.clampEdge})`)
      if (mc.unreached?.length) out.push(`${name}, ⊞ Panels open at ${where}: ${mc.unreached.length} items not reachable by the menu's own scroll (${mc.unreached.join(', ')})`)
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
      const m = await measureAt(route, sel, size, null, true, true)
      const name = `${view} at ${tagOf(size)}`
      if (!m) {
        console.error(`digishell-probe: ${name} never drew`)
        process.exitCode = 2
        throw new Error('no screen')
      }
      const info = (p) => `stops ${p.stopsReachable.join('+') || '—'}${p.coveredOther.length ? `, covered by other than the header: ${p.coveredOther.join(', ')}` : ''}`
      const menu = (mc) => (mc?.opened ? `${mc.ok}/${mc.n} on screen and topmost${mc.off ? `, ${mc.off} outside the window` : ''}${mc.clipped ? `, ${mc.clipped} clipped by the scroll` : ''}${mc.inmenu ? `, ${mc.inmenu} in the menu's scroll` : ''}${mc.covered.length ? `, ${mc.covered.length} covered` : ''}` : 'not opened')
      console.log(`digishell-probe: ${name.padEnd(22)} header ${m.pin === 'on' ? 'pinned' : 'unpinned'} (${m.vp}), dock ${m.fold === 'on' ? 'folded' : 'whole'}, scrolls ${m.valve} px · top: ${info(m.top)} · end: ${info(m.end)}, header ${m.end.headReach} · ⊞ button reachable ${m.reach == null ? 'n/a' : m.reach < 0 ? 'NEVER' : `from scroll ${m.reach}`} · ⊞ Panels at top ${menu(m.menuTop)}, at end ${menu(m.menuEnd)}${m.menuReach ? `, at its button ${menu(m.menuReach)}` : ''}`)
      console.log(`digishell-probe:   check 8: ${coverLine(m.cover)}`)
      if (m.focus) console.log(`digishell-probe:   check 9: ${m.focus.n} controls focused, ${m.focus.hidden.length} hidden, ${m.focus.part} part-covered`)
      if (m.hb || m.refusal) console.log(`digishell-probe:   check 10: ${JSON.stringify(m.hb ?? m.refusal)}`)
      if (m.press) console.log(`digishell-probe:   check 11: ${m.press.n} pressed, ${m.press.moved.length} moved the shell, ${m.press.onSender.length} second clicks on a sender · check 12: regain moved ${m.regain?.moved ?? 'n/a'} px`)
      for (const r of [...reds(name, m, cover), ...coverReds(name, m.cover), ...focusReds(name, m), ...pinReds(name, m)]) red(r)
    }
  }
  for (const [view, route, sel] of COVER_ONLY) {
    for (const size of COVER_SIZES) {
      const name = `${view} at ${tagOf(size)}`
      const m = await measureAt(route, sel, size, null, false, true, true)
      if (!m) {
        console.error(`digishell-probe: ${name} never drew`)
        process.exitCode = 2
        throw new Error('no screen')
      }
      console.log(`digishell-probe: ${name.padEnd(22)} check 8: ${coverLine(m.cover)}`)
      for (const r of coverReds(name, m.cover)) red(r)
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
  const size5 = [1366, 768, '150']
  const m5 = await measureAt('cw', 'main.cw-cockpit', size5, NO_FIT)
  const caught5 = m5 ? reds(`CW at ${tagOf(size5)} (menu sizing removed)`, m5).filter((r) => r.includes('runs past')) : []
  if (caught5.length === 0) red('CONTROL FAILED — a ⊞ Panels menu sized at 0.7 × the window, opening downward, stayed inside it; check 4b cannot fail')
  else console.log(`digishell-probe: CONTROL — ${caught5[0]} (caught)`)
  const size6 = [1024, 768, '175']
  const m6 = await measureAt('phone', 'main.phone-cockpit', size6, OVER_STOP, false, true)
  const caught6 = m6 ? coverReds(`Phone at ${tagOf(size6)} (menu planted over the stop)`, m6.cover).filter((r) => r.includes('under the menu')) : []
  if (caught6.length === 0) red('CONTROL FAILED — a menu planted over the whole shell left the stop button clear; check 8 cannot fail')
  else console.log(`digishell-probe: CONTROL — ${caught6[0]} (caught)`)
  const size7 = [1280, 720, '175']
  const m7 = await measureAt('cw', 'main.cw-cockpit', size7, TINY_MENU, false, true)
  const caught7 = m7 ? coverReds(`CW at ${tagOf(size7)} (menu held to 20 px)`, m7.cover).filter((r) => r.includes('px tall')) : []
  if (caught7.length === 0) red('CONTROL FAILED — a menu held to 20 px passed as tall enough; the short-menu check cannot fail')
  else console.log(`digishell-probe: CONTROL — ${caught7[0]} (caught)`)
  const size8 = [900, 600, '175']
  const m8 = await measureAt('rtty', 'main.rtty-cockpit', size8, KEEP_OVER_ALL, false, true)
  const caught8 = m8 ? focusReds(`RTTY at ${tagOf(size8)} (keep row planted over the shell)`, m8).filter((r) => r.includes('hidden on')) : []
  if (caught8.length === 0) red('CONTROL FAILED — a keep row planted over the whole shell hid no focused control; check 9 cannot fail')
  else console.log(`digishell-probe: CONTROL — ${caught8[0].slice(0, 110)} (caught)`)
  const size9 = [900, 600, '175']
  const m9 = await measureAt('jtty', 'main.jtty-cockpit', size9, null, false, true, false, "addEventListener('pointerup', () => setTimeout(() => { shell.scrollTop += 60 }, 0), true); addEventListener('focus', () => { shell.scrollTop += 60 })")
  const caught9 = m9 ? pinReds(`JTTY at ${tagOf(size9)} (a press and a regain planted to scroll)`, m9) : []
  for (const what of ['a press moved', 'a window regain moved']) {
    const c = caught9.find((r) => r.includes(what))
    if (!c) red(`CONTROL FAILED — a planted scroll passed: "${what}" cannot fail`)
    else console.log(`digishell-probe: CONTROL — ${c.slice(0, 110)} (caught)`)
  }
  if (code === 0) console.log('digishell-probe: PASS — no header covers a transmit control, every stop control and Send/PTT is on screen and topmost at every scroll position of every screen at every size, ⊞ Panels can be clicked, a pinned header stays on screen, every on-screen item of an open ⊞ Panels menu is reachable and inside the window, the menu never stands over the stop button whether opened by pointer or keyboard or walked with it open, a focused control on a folded dock is never hidden, the JTTY refusal stays on screen, the JS8 heartbeat menu opens in sight, no press or window regain moves the shell, no second click reaches a sender, a click on the JTTY refusal stays on it, and all ten controls were caught')
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
