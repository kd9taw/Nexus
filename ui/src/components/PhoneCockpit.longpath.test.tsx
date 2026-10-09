// @vitest-environment jsdom
//
// #338's LONG-PATH BEAM, AT THE PHONE HOST, the tab a tester named when asking for it. The strip
// asks for the reciprocal with `onPointAt(call, true)` (RotorStrip.longpath.test.tsx); PhoneCockpit
// wired a one-argument `(call) => pointRotatorAtCall(call)`, so LP turned the beam the short way.
// PhoneCockpit.beam.test.tsx covers the call reaching the strip, with the strip stubbed. Here the
// strip is REAL and the call is typed into the real log strip, so the whole chain runs.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { pointRotatorAtCall } from '../api'
import { t } from '../i18n'
import type { AppSnapshot } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "LP asks the backend for the long path, and the toast…", takes
// 0.40 s and 0.47 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const pushToast = vi.fn()

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (CwCockpit.density's pattern). The lookups answer
  // "nothing found", as PhoneCockpit.beam's do, so the real log strip settles quietly.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => ({ rotatorModel: 603, rotatorHost: '127.0.0.1' })),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    // The logbook never answers, so the strip reads its own "not asked yet" default rather than
    // a stub's empty object.
    askLog: vi.fn(() => new Promise(() => {})),
    readRotator: vi.fn(async () => 212),
    getDeclination: vi.fn(async () => null),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    // What the command answers: the bearing, and what it was taken to (here the entity centre).
    pointRotatorAtCall: vi.fn(async () => ({ bearing: 47, to: 'country', grid: null, country: 'Japan' })),
  }
})
vi.mock('../toast', () => ({
  pushToast: (...a: unknown[]) => pushToast(...(a as [])),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The header renders its CHILDREN, which is where Phone puts the strip.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { children?: React.ReactNode; modeIndicator?: React.ReactNode }) => (
    <header className="cockpit-header">
      {p.modeIndicator}
      {p.children}
    </header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))

const point = vi.mocked(pointRotatorAtCall)

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    hunt: null,
    radio: {
      dialMhz: 14.25, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null,
      rigMode: 'USB', transmitting: false, txEnabled: true, txAllowed: true,
      qsoRecording: false, rfPower: null, micGain: null, nrLevel: 0.3, agc: 'fast',
      nb: true, nr: true, notch: null, comp: null, vox: null, filterWidthHz: null,
      splitTxMhz: null, smeterDb: null, rxLevel: 0, phoneSegLo: null, phoneSegHi: null,
    },
  } as unknown as AppSnapshot
}

async function renderWorking(call: string) {
  render(
    <PhoneCockpit
      snap={makeSnap()} theme="dark" onSnap={() => {}} onConsumeWork={() => {}}
      pendingWork={null} fieldDay={undefined} wheelSensitivity={1} spots={[]}
      onWorkSpot={() => {}}
    />,
  )
  fireEvent.change(document.querySelector('input.le-call') as HTMLInputElement, { target: { value: call } })
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  point.mockClear()
  pushToast.mockClear()
})
afterEach(cleanup)

describe('the Phone cockpit beams the long way round when LP is pressed', () => {
  it('LP asks the backend for the long path, and the toast names it', async () => {
    await renderWorking('ja1abc')
    fireEvent.click(await screen.findByRole('button', { name: 'LP' }))
    await waitFor(() => expect(point).toHaveBeenCalledTimes(1))
    expect(point, 'LP slewed the short path').toHaveBeenCalledWith('JA1ABC', true)
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('shell.rotator.pointedLong', { bearing: 47, call: 'JA1ABC', to: t('rotor.pointed.to.country', { country: 'Japan' }) }), 'info'),
    )
  })

  // THE CONTROL: forwarding the flag must not have turned the ordinary beam into a long-path one.
  it('→ CALL still asks for the short path, with the toast it always had', async () => {
    await renderWorking('ja1abc')
    fireEvent.click(await screen.findByRole('button', { name: '→ JA1ABC' }))
    await waitFor(() => expect(point).toHaveBeenCalledTimes(1))
    expect(point).toHaveBeenCalledWith('JA1ABC', false)
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('cw.rotator.pointed', { call: 'JA1ABC', bearing: 47, to: t('rotor.pointed.to.country', { country: 'Japan' }) }), 'info'),
    )
  })
})
