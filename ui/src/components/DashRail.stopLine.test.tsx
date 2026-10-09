// @vitest-environment jsdom
//
// THE STOP LINE BESIDE THE DASHBOARD RAIL, computed in the real App.
//
//   THE OPERATOR MUST NEVER BE UNABLE TO STOP A TRANSMISSION.
//
// The rail is a sibling of the cockpit in App's shell, never inside a cockpit shell, and it renders no
// transmit control (DashRail.test.tsx sweeps every box in the registry for one). What is left to prove
// is that putting it beside a cockpit costs that cockpit nothing: for every operating cockpit, each
// control on its stop-line list (components/stop-line.test.tsx's lists; Operate's from its own sweep)
// is on screen by accessible name with the rail OFF — the baseline, which must find every one or this
// file reads nothing — and with the rail ON it is still on screen, no more disabled, and not inside the
// rail. APRS renders no stop control at all (its census), so its case proves only that the cockpit and
// the rail both render.
//
// WHAT THIS DOES NOT PROVE: geometry. jsdom does not lay out, so "Stop TX is still the topmost element
// at its own centre, fully in view, beside a rail" is measured in a real browser (the report's census),
// not here. Registered in stop-line.test.tsx ELSEWHERE as the `dashrail` vocabulary's sweep.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('../appCockpits.testkit')
  return { ...auto, ...appApiAnswers() }
})
vi.mock('../toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from '../App'
import { COCKPIT_MAIN } from '../appCockpits.testkit'
import { DASH_RAIL_FOLDS, DASH_RAIL_SECTIONS, type DashRailSection } from '../features/dashRail'

// Each case mounts the real App (this file mounts it more than once per case); under the full suite's
// load that outruns vitest's default 5 s per test, which is a budget, not a claim about the app.
// Raised from 30 s on 2026-10-09: the slowest case (operate) takes 1.99 s and 1.60 s on one core (two runs), all of
// it CPU work (5.63 s at a third of a CPU), and a loaded full suite on this box has run cases up to 20 times slower
// than one core, 32 s for this one. 45 s holds that; a test that hangs still fails, after 45 s.
vi.setConfig({ testTimeout: 45_000 })

const STOP_TX: [string, RegExp] = ['Stop TX', /^stop tx$/i]
const TUNE: [string, RegExp] = ['Tune', /^tune$|^tuning…$/i]
// The TX strip draws the latch in FT's words since 2026-10-01 (the header's read "▼ TX On" / "■ TX Off"), as stop-line.test.tsx has it.
const TX_LATCH: [string, RegExp] = ['TX-enable latch', /^tx on$|^tx off$/i]

/** Each operating cockpit's stop-line list — stop-line.test.tsx's, and Operate's from its own sweep. */
const CASES: Record<DashRailSection, Array<[string, RegExp]>> = {
  operate: [['Stop TX', /stop tx/i], ['Tune', /^tune$/i]],
  phone: [['PTT', /push to talk|on air — release to stop|tx locked|tx off — click to enable/i], STOP_TX, TUNE],
  cw: [STOP_TX, TUNE],
  rtty: [STOP_TX, ['Stop (RTTY abort)', /^esc\s*stop$/i], TUNE, TX_LATCH],
  psk: [STOP_TX, ['Stop (PSK abort)', /^esc\s*stop$/i], TUNE, TX_LATCH],
  sstv: [['Stop', /^stop$/i], TX_LATCH],
  aprs: [],
  js8: [STOP_TX, TUNE],
}

const railEl = () => document.querySelector<HTMLElement>('.dash-rail')

async function mountOn(view: string, viewport = 'lg'): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(document.documentElement.getAttribute('data-viewport')).toBe(viewport))
  await act(async () => {})
}

/** Each listed control: on screen at all, operable, and whether any copy of it is inside the rail. */
async function stops(list: Array<[string, RegExp]>) {
  const out = new Map<string, { present: boolean; enabled: boolean; inRail: boolean; inBox: boolean }>()
  for (const [label, name] of list) {
    // Findable once the cockpit's own state has arrived (a latch draws after the cockpit reads it).
    await waitFor(() => expect(screen.queryAllByRole('button', { name }).length, `${label} never drew`).toBeGreaterThan(0), {
      timeout: 5000,
    }).catch(() => {})
    const found = screen.queryAllByRole('button', { name }) as HTMLButtonElement[]
    out.set(label, {
      present: found.length > 0,
      enabled: found.some((b) => !b.disabled),
      inRail: found.some((b) => railEl()?.contains(b) ?? false),
      // In one of the rail's boxes standing in the cockpit's columns (below lg).
      inBox: found.some((b) => b.closest('.pane-frame[data-pane^="rail"]') != null),
    })
  }
  return out
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, js8: true } }),
  )
  document.documentElement.removeAttribute('data-viewport')
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
})

describe('the stop line beside the dashboard rail, in every operating cockpit', () => {
  it('covers every section the rail stands beside', () => {
    expect(Object.keys(CASES).sort()).toEqual([...DASH_RAIL_SECTIONS].sort())
  })

  it.each(DASH_RAIL_SECTIONS.map((s) => [s]))('%s', async (section) => {
    const list = CASES[section]
    await mountOn(section)
    expect(railEl()).toBeNull()
    const base = await stops(list)
    for (const [label, s] of base) expect(s.present, `${label} is not on screen with the rail OFF — nothing to compare`).toBe(true)
    // The cockpit drew (APRS has no stop control to find it by).
    expect(document.querySelector(COCKPIT_MAIN[section]), 'the cockpit did not render').not.toBeNull()
    expect(document.querySelector('.view-crash'), 'something crashed — the comparison would read a crash panel').toBeNull()
    cleanup()

    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ [section]: true }))
    await mountOn(section)
    expect(railEl(), 'the rail did not render beside the cockpit').not.toBeNull()
    const beside = await stops(list)
    for (const [label, s] of beside) {
      const was = base.get(label)!
      expect(s.present, `${label} is gone beside the rail`).toBe(true)
      if (was.enabled) expect(s.enabled, `${label} is disabled beside the rail`).toBe(true)
      expect(s.inRail, `${label} was found inside the rail`).toBe(false)
    }
  })
})

describe('the stop line with the rail’s boxes in the cockpit’s columns, on a window too small for the rail', () => {
  // Beside FT, Phone, CW and JS8, below lg the rail's boxes stand at the foot of the cockpit's column (the
  // operator's "They move into the columns"); FT draws its arranged columns for them. Every control on the
  // cockpit's list is still on screen and no more disabled than with the rail off at the same size, and none is
  // in one of those boxes, which are Conditions boxes and hold no transmit control.
  it.each(DASH_RAIL_FOLDS.map((s) => [s]))('%s', async (section) => {
    Object.defineProperty(window, 'innerWidth', { value: 1280, configurable: true })
    Object.defineProperty(window, 'innerHeight', { value: 800, configurable: true })
    const list = CASES[section]
    await mountOn(section, 'md')
    const base = await stops(list)
    for (const [label, st] of base) expect(st.present, `${label} is not on screen with the rail OFF — nothing to compare`).toBe(true)
    cleanup()

    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ [section]: true }))
    await mountOn(section, 'md')
    expect(railEl(), 'the rail rendered below lg').toBeNull()
    await waitFor(() => expect(document.querySelectorAll('.pane-frame[data-pane^="rail"]').length, 'the rail’s boxes are not in the columns').toBe(4))
    const folded = await stops(list)
    for (const [label, st] of folded) {
      const was = base.get(label)!
      expect(st.present, `${label} is gone with the rail’s boxes in the columns`).toBe(true)
      if (was.enabled) expect(st.enabled, `${label} is disabled with the rail’s boxes in the columns`).toBe(true)
      expect(st.inBox, `${label} was found inside one of the rail’s boxes`).toBe(false)
    }
  })
})
