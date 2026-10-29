// @vitest-environment jsdom
//
// The CW audio scope's span chips: Filter (the rig's filter round the pitch, the default and the
// zero-beating view), Std (200–3000 Hz) and Full (200–4000 Hz), the waterfalls' own views. The
// pick is kept per window; a stored value no chip names reads as Filter.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { cwScopeWindow } from '../waterfall'
import type { AppSnapshot } from '../types'

const decodeState = {
  text: '',
  wpm: 22,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

const { setCwKeyer } = vi.hoisted(() => ({ setCwKeyer: vi.fn(async (..._a: unknown[]) => null) }))

vi.mock('../api', async (importOriginal) => {
  // ⭐ DERIVED FROM THE REAL MODULE, not a hand-kept list. A hand-kept mock omits any export
  // added after it was written, and a component that calls one THROWS ON MOUNT — so the suite
  // goes red at a seam nothing in the diff explains, and the tempting fix is to make the test
  // pass rather than ask why. That cost five files one evening when a single API call was added
  // to the CW cockpit, this one among them.
  //
  // Every function the module exports is auto-stubbed here; the entries below override only the
  // ones this file's assertions actually depend on, so their shapes are unchanged.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // Hand-kept mock: an export CwCockpit calls but this list omits makes it THROW ON MOUNT,
    // which reads as a behaviour regression rather than the stale mock it actually is.
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    setSettings: vi.fn(async () => ({})),
    sendCw: vi.fn(async () => {}),
    setCwKeyer,
    setCwWpm: vi.fn(async () => {}),
    stopCw: vi.fn(async () => {}),
    cwDecode: vi.fn(async () => decodeState),
    cwClear: vi.fn(async () => {}),
    setAiCw: vi.fn(async () => {}),
    selectPeer: vi.fn(async () => null),
    previewCw: vi.fn(async (t: string) => t),
    pointRotatorAtCall: vi.fn(async () => 0),
    setRigFunc: vi.fn(async () => ({})),
    setFilterWidth: vi.fn(async () => ({})),
    setNrLevel: vi.fn(async () => {}),
    setScopeSpan: vi.fn(async () => ({})),
    setScopeRef: vi.fn(async () => {}),
    setFlexPanSpan: vi.fn(async () => ({})),
    setFlexPanRef: vi.fn(async () => ({})),
    openPanelWindow: vi.fn(async () => {}),
    setTune: vi.fn(async () => ({})),
    setFrequency: vi.fn(async () => ({})),
    haltTx: vi.fn(async () => ({})),
  }
})

// The REAL CockpitHeader: the pitch box is one of its children. Its band and tuning controls are stubbed.
vi.mock('./BandPicker', () => ({ BandPicker: () => null }))
vi.mock('./TuningStrip', () => ({ TuningStrip: () => null }))
// The scope records the window the cockpit hands it; the scope's own click mapping is
// PhoneScope.cwSpan.test.tsx's.
const { scopeProps } = vi.hoisted(() => ({ scopeProps: [] as Record<string, unknown>[] }))
vi.mock('./PhoneScope', () => ({
  PhoneScope: (p: Record<string, unknown>) => {
    scopeProps.push(p)
    return <div data-testid="scope-stub" />
  },
}))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeEach(() => localStorage.clear())
afterEach(() => {
  cleanup()
  setCwKeyer.mockClear()
  scopeProps.length = 0
})

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode: 'CW',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      cwWpm: 22,
      cwKeyer: 'cat',
      nrLevel: 0.3,
      // The rig is on FAST and says so on every read-back.
      agc: 'fast',
      refusedAgc: null,
      nb: true,
      nr: true,
      notch: null,
      filterWidthHz: 500,
      splitTxMhz: null,
      smeterDb: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

async function renderCw(over: Record<string, unknown> = {}) {
  const r = render(
    <CwCockpit snap={makeSnap(over)} theme="dark" onWorkSpot={() => {}} spots={[]} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

/** The window the scope was last handed. */
const view = () => {
  const p = scopeProps[scopeProps.length - 1]
  return { lo: p.viewLoHz as number, hi: p.viewHiHz as number }
}
const chip = (name: string) => screen.getByRole('button', { name })
const group = () => screen.getByRole('group', { name: 'Audio scope span' })

describe('the CW audio scope span', () => {
  it('opens on Filter: the rig filter round the pitch, as before', async () => {
    await renderCw()
    expect(group()).toBeTruthy()
    expect(chip('Filter').getAttribute('aria-pressed')).toBe('true')
    expect(chip('Std').getAttribute('aria-pressed')).toBe('false')
    // 500 Hz filter ⇒ 625 Hz round the 600 Hz default pitch.
    const w = cwScopeWindow(600, 500)
    expect(view()).toEqual({ lo: w.loHz, hi: w.hiHz })
    expect(view()).toEqual({ lo: 288, hi: 913 })
  })

  it('Std and Full show the waterfalls\' views, and the pick is kept for the next open', async () => {
    await renderCw()
    fireEvent.click(chip('Std'))
    expect(view()).toEqual({ lo: 200, hi: 3000 })
    expect(chip('Std').getAttribute('aria-pressed')).toBe('true')
    expect(localStorage.getItem('nexus.cw.scope.span')).toBe('std')
    fireEvent.click(chip('Full'))
    expect(view()).toEqual({ lo: 200, hi: 4000 })
    cleanup()
    await renderCw()
    expect(chip('Full').getAttribute('aria-pressed')).toBe('true')
    expect(view()).toEqual({ lo: 200, hi: 4000 })
    // And back: Filter follows the pitch and the filter again.
    fireEvent.click(chip('Filter'))
    expect(view()).toEqual({ lo: 288, hi: 913 })
    expect(localStorage.getItem('nexus.cw.scope.span')).toBe('filter')
  })

  it('a stored value no chip names opens on Filter', async () => {
    localStorage.setItem('nexus.cw.scope.span', 'wide')
    await renderCw()
    expect(chip('Filter').getAttribute('aria-pressed')).toBe('true')
    expect(view()).toEqual({ lo: 288, hi: 913 })
  })
})
