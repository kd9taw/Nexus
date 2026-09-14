// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { render, waitFor, screen } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'

// Same jsdom shims the sibling Logbook suites need: react-virtual measures the scroll
// element and rows via offsetHeight + a ResizeObserver, neither of which jsdom implements.
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    getLog: vi.fn(),
    deleteQso: noop(), editQso: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSent: vi.fn(() => Promise.resolve({})),
    markQslCard: vi.fn(() => Promise.resolve({})),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

function oneContact() {
  return [
    {
      call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000,
      confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
    },
  ]
}

// #270 (ddobbins1686): the per-row QRZ push existed in 1.10.3, but its only visible mark was a
// bare ↥ arrow, so an operator hunting for "Upload to QRZ" reported the control as gone.
describe('Logbook row — the QRZ push is recognisable as QRZ (#270)', () => {
  it('shows QRZ on the button and keeps its accessible name', async () => {
    ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue(oneContact())
    const { container } = render(
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
    )
    await waitFor(() => expect(container.querySelector('.log-scroll > div')).not.toBeNull())

    // Found by its accessible name — that name is what screen readers and the existing
    // guidance ("Push <call> to QRZ") rely on, so it must not change.
    const push = await screen.findByRole('button', { name: 'Push K0ABC to QRZ' })
    // Positive control: the sibling ClubLog push renders on the same row with its own label,
    // so a missing QRZ label below is a real absence, not a row that failed to render.
    const clublog = screen.getByRole('button', { name: 'Push K0ABC to ClubLog' })
    expect(clublog.textContent).toBe('CL')

    expect(push.textContent).toContain('QRZ')
  })
})
