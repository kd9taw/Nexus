// @vitest-environment jsdom
//
// #338's LONG-PATH BEAM, AT THE CW HOST. The strip asks for the reciprocal with
// `onPointAt(call, true)` (RotorStrip.longpath.test.tsx); CwCockpit wired a one-argument
// `(call) => pointRotatorAtCall(call)`, so LP turned the beam the short way. The strip is REAL
// here, and the worked call comes from the decoder guide, exactly as it reaches the strip on air.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { pointRotatorAtCall } from '../api'
import { t } from '../i18n'
import type { AppSnapshot } from '../types'

const pushToast = vi.fn()

/** What `cw_decode` returns: the guide has settled on the station being worked. */
const decodeState = {
  text: '',
  wpm: 0,
  sent: [],
  keyerError: null,
  candidates: [],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null,
  workedCall: 'JA1ABC',
  rst: null,
  name: null,
}

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (CwCockpit.density's pattern); the overrides are
  // the ones the cockpit reads on mount and the four the strip needs to draw its beam buttons.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => ({
      macros: { cwProfiles: [], activeCwProfile: 0 },
      rotatorModel: 603,
      rotatorHost: '127.0.0.1',
    })),
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    cwDecode: vi.fn(async () => decodeState),
    previewCw: vi.fn(async (text: string) => text),
    readRotator: vi.fn(async () => 212),
    getDeclination: vi.fn(async () => null),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    pointRotatorAtCall: vi.fn(async () => 47),
  }
})
vi.mock('../toast', () => ({
  pushToast: (...a: unknown[]) => pushToast(...(a as [])),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The header renders its CHILDREN, which is where CW puts the strip; its own chrome is not
// what this file is about.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ children }: { children?: unknown }) => (
    <header className="cockpit-header">{children as never}</header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const point = vi.mocked(pointRotatorAtCall)

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.025, band: '20m', catOk: true, sideband: 'CW', transmitting: false, txEnabled: true,
      txAllowed: true, tuning: false, splitTxMhz: null, filterWidthHz: null, rigMode: 'CW',
    },
  } as unknown as AppSnapshot
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

describe('the CW cockpit beams the long way round when LP is pressed', () => {
  it('LP asks the backend for the long path, and the toast names it', async () => {
    render(<CwCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    fireEvent.click(await screen.findByRole('button', { name: 'LP' }))
    await waitFor(() => expect(point).toHaveBeenCalledTimes(1))
    expect(point, 'LP slewed the short path').toHaveBeenCalledWith('JA1ABC', true)
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('shell.rotator.pointedLong', { bearing: 47, call: 'JA1ABC' }), 'info'),
    )
  })

  // THE CONTROL: forwarding the flag must not have turned the ordinary beam into a long-path one.
  it('→ CALL still asks for the short path, with the toast it always had', async () => {
    render(<CwCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    fireEvent.click(await screen.findByRole('button', { name: '→ JA1ABC' }))
    await waitFor(() => expect(point).toHaveBeenCalledTimes(1))
    expect(point).toHaveBeenCalledWith('JA1ABC', false)
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(t('cw.rotator.pointed', { call: 'JA1ABC', bearing: 47 }), 'info'),
    )
  })
})
