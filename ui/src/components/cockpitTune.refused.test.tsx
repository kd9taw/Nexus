// @vitest-environment jsdom
//
// A REFUSED TUNE IS SHOWN, in every cockpit that draws the TX strip's Tune. Tune keys a carrier, and a
// control that keys the transmitter never fails silently: the station returns its refusal with the
// reason (or, on a computer driving a station, client mode does), and the toast shows it. Phone's and
// the ATU's already did; every other cockpit dropped it, unhandled.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, screen } from '@testing-library/react'

const REFUSED = 'Not connected to the station.'
vi.mock('../api', async (importOriginal) => ({
  ...(await import('./stop-line.api.testkit')).stopLineApi(await importOriginal<Record<string, unknown>>()),
  setTune: vi.fn(() => Promise.reject(REFUSED)),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./panes/BoxBody', () => ({ BoxBody: () => <div data-testid="box-body-stub" /> }))

import { pushToast } from '../toast'
import { cw, rtty, psk, jtty, js8, sstv, phone, panelsWith, settle } from './stop-line.testkit'

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  ;(pushToast as unknown as ReturnType<typeof vi.fn>).mockClear()
})
afterEach(cleanup)

describe('a refused Tune', () => {
  it.each([phone, cw, rtty, psk, jtty, js8, sstv].map((c) => [c.cockpit, c] as const))('%s shows why', async (_name, c) => {
    c.render(panelsWith([]) as never)
    await settle()
    fireEvent.click(screen.getByRole('button', { name: /^tune$/i }))
    await settle()
    expect(pushToast).toHaveBeenCalledWith(REFUSED, 'error')
  })
})
