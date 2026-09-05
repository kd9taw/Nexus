// @vitest-environment jsdom
//
// Task 3 of the POTA map plan: the `operatemap` pop-out mounts a bare MapView with POTA
// hunting on (intent='pota' turns the Parks layer on — see MapView's INTENT_PRESETS). It
// gates on the first snapshot the same way the 'pota' board arm does, because MapView needs
// snap.mygrid/snap.stations to place anything.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, act } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { subscribeSnapshot } from './api'
import { t } from './i18n'
import type { AppSnapshot } from './types'

// Stub the heavy map view — the real component pulls in canvas/globe rendering and a
// self-fetch of the POTA feed, none of which is what this router test is about. Expose the
// one prop under test (intent) on a testid so the assertion is a rendered fact, not a
// snapshot of props.
vi.mock('./components/MapView', () => ({
  MapView: (props: { intent?: string }) => (
    <div data-testid="operatemap-map" data-intent={props.intent ?? ''} />
  ),
}))

vi.mock('./api', () => ({
  subscribeSnapshot: vi.fn(() => () => {}),
  selectPeer: vi.fn(() => Promise.resolve(null)),
  getBandPlan: vi.fn(() => Promise.resolve([])),
  getPropagation: vi.fn(() => Promise.resolve(null)),
  getNeedAlerts: vi.fn(() => Promise.resolve([])),
  getSettings: vi.fn(() => Promise.resolve(null)),
}))

const mockedSubscribe = vi.mocked(subscribeSnapshot)
const SNAP = {
  mygrid: 'EM12',
  stations: [],
  link: { tier: 'FT8' },
  radio: {},
} as unknown as AppSnapshot

afterEach(() => {
  cleanup()
  // Restore the inert default so other it()s in this file stay order-independent.
  mockedSubscribe.mockImplementation(() => () => {})
})

describe('DetachedPanel operatemap panel', () => {
  it('mounts the map with POTA intent once a snapshot arrives, not the unavailable fallback', async () => {
    mockedSubscribe.mockImplementation((cb: (s: AppSnapshot) => void) => {
      cb(SNAP)
      return () => {}
    })
    render(<DetachedPanel panel="operatemap" />)
    await act(async () => {
      await Promise.resolve()
    })
    const map = screen.getByTestId('operatemap-map')
    expect(map.dataset.intent).toBe('pota')
    expect(screen.queryByText(t('detached.unavailable', { panel: 'operatemap' }))).toBeNull()
  })

  // POSITIVE CONTROL — proves the `!snap` gate actually gates: without it, a version of the
  // branch that renders MapView unconditionally would also pass the assertion above.
  it('shows the connecting state before the first snapshot arrives', () => {
    render(<DetachedPanel panel="operatemap" />)
    expect(screen.queryByTestId('operatemap-map')).toBeNull()
    expect(screen.getByText(t('detached.connecting'))).toBeTruthy()
  })
})
