// Which views may command the rig's MODE, and which spot-click may change it.
//
// Two reports, one fault: Nexus deciding the rig's mode from what you CLICKED rather than how
// you are OPERATING.
//
//   #80 (FT-991A) — opening POTA/SOTA flipped the rig from USB to D-U. POTA/SOTA declares
//   `workspace: 'dx'` for LAYOUT reasons, and the view→mode effect treated "has a workspace"
//   as "is an operating mode", then fell through to 'digital' for anything that was not
//   cw/phone/rtty. A hunting board is not a mode: you work a park on whatever the activator
//   is running.
//
//   #81 path B — clicking an FT8 spot from inside the Phone cockpit commanded DATA-USB (mic
//   out of circuit on a 991A) AND disarmed the mic, while deliberately keeping the operator on
//   the Phone screen, so PTT then did nothing with no explanation.
//
// Both were FALLTHROUGHS — `: 'digital'` as the else-branch. This pins the explicit forms, so
// a new workspace view cannot silently acquire the power to retune somebody's radio.
//
// Parses App.tsx rather than rendering it: the whole app is not mountable in jsdom, and what
// is being asserted is a static declaration, not a rendered result. That is a real limit —
// it cannot prove the wiring executes, only that the map says what it should. The wiring is
// covered by the assertion that neither call site passes the raw factory.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const APP = readFileSync(resolve(process.cwd(), 'src/App.tsx'), 'utf8')
/** ⚠️ THE MAP MOVED (#143). `RIG_MODE_BY_VIEW` and the home-on-entry rule now live in
 *  `rigModeForView.ts` — extracted so the SEQUENCE of views could be tested, which is what
 *  #143 turned out to be. This guard still owns the #80/#81 rules and now reads the map where
 *  it lives and the WIRING from App.tsx. If the map moves again, move this line, do not
 *  weaken the assertions: they are why a hunting board cannot acquire the power to retune
 *  somebody's radio. */
const MODEMAP = readFileSync(resolve(process.cwd(), 'src/rigModeForView.ts'), 'utf8')

/** The `RIG_MODE_BY_VIEW` object literal, as view → mode pairs. */
function rigModeMap(): Record<string, string> {
  const start = MODEMAP.indexOf('RIG_MODE_BY_VIEW')
  expect(start, 'RIG_MODE_BY_VIEW must exist — it is the allowlist this whole test is about').toBeGreaterThan(-1)
  const body = MODEMAP.slice(start, MODEMAP.indexOf('}', start))
  const out: Record<string, string> = {}
  for (const m of body.matchAll(/^\s*([a-z]+):\s*'(cw|phone|rtty|keyboard|digital)'/gm)) out[m[1]] = m[2]
  return out
}

describe('which views may command the rig mode', () => {
  it('names every mode-owning view explicitly, and POTA/SOTA is not one', () => {
    const map = rigModeMap()
    expect(map).toEqual({
      cw: 'cw',
      phone: 'phone',
      rtty: 'rtty',
      psk: 'keyboard',
      operate: 'digital',
      chat: 'digital',
    })
    // The report, stated as an assertion: opening the hunting board must not touch the rig.
    expect(map.pota, 'POTA/SOTA is a hunting board, not a mode (#80)').toBeUndefined()
  })

  it('has no else-branch that could hand an unlisted view a mode', () => {
    // The shape that caused #80: `… : 'digital'` as a fallthrough on the view name.
    const start = MODEMAP.indexOf('RIG_MODE_BY_VIEW')
    const region = MODEMAP.slice(start, start + 1400)
    expect(
      /:\s*'digital'\s*$/m.test(region.split('const mode =')[1] ?? ''),
      "the mode must be LOOKED UP, never defaulted — a default is how a hunting board got DATA-USB",
    ).toBe(false)
    // The refusal for an unlisted view now lives in `rigModeTransition`: no mode, no assert,
    // and — added by #143 — the home guard is handed back untouched so a later operating
    // view still homes.
    expect(MODEMAP).toContain('if (!mode) return { followFreq: false, nextHomed: lastHomed }')
  })

  it('does not treat "has a workspace" as "is an operating mode"', () => {
    // `workspace` is a layout concept; three features declare it and only two are modes.
    //
    // ⚠️ STRIP COMMENTS FIRST. This assertion failed on its first run against the FIXED code,
    // because the comment above the fix quotes the old expression verbatim to explain it — the
    // guard was reading prose as if it were code. A check that cannot tell an explanation of a
    // bug from the bug is worse than no check: it would have been "fixed" by deleting the
    // comment. Same trap the wiki generator hit today from the other side.
    const effect = (
      APP.slice(APP.indexOf('opModeSeeded.current = true'), APP.indexOf('void setOperatingMode(mode')) +
      MODEMAP
    ).replace(/^\s*\/\/.*$/gm, '').replace(/^\s*\*.*$/gm, '')
    expect(
      /featureById\([^)]*\)\?\.workspace/.test(effect),
      'the rig-mode decision must not read `workspace` — that conflation is exactly #80',
    ).toBe(false)
    // The stripper must not be what makes this pass: the region still has to contain real code.
    expect(effect).toContain('RIG_MODE_BY_VIEW[view]')
  })
})

describe('an in-cockpit spot click stays in the cockpit’s own mode', () => {
  it('takes the cockpit as an argument instead of reading the spot’s mode', () => {
    const start = APP.indexOf('const handleWorkSpotHere')
    const body = APP.slice(start, APP.indexOf('const handleWorkSpot =', start))
    expect(body).toContain("(cockpit: 'phone' | 'cw')")
    expect(body).toContain('const kind = cockpit')
    // The old shape, verbatim — clicking an FT8 spot from Phone put the rig in DATA.
    expect(
      /s\.mode === 'CW'/.test(body),
      "must not derive the rig mode from the SPOT's mode (#81 path B / #80)",
    ).toBe(false)
  })

  it('wires each cockpit to its own bound handler, never the raw factory', () => {
    // A raw `onWorkSpot={handleWorkSpotHere}` would pass the SpotRow as `cockpit` and
    // typecheck only by accident — pin the bound forms.
    expect(APP).toContain('onWorkSpot={workSpotHereCw}')
    expect(APP).toContain('onWorkSpot={workSpotHerePhone}')
    expect(
      /onWorkSpot=\{handleWorkSpotHere\}/.test(APP),
      'no call site may pass the unbound factory',
    ).toBe(false)
  })
})
