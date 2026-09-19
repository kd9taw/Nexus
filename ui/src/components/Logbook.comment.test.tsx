// @vitest-environment jsdom
//
// #162 — A LONG LOGBOOK COMMENT CAN BE READ IN FULL. The Comment column clips to one line with an
// ellipsis, and the only way to see the rest was a hover tooltip. The cell now opens in place:
// click the comment and it wraps to its full length in that row; click again and it folds back.
// The table's columns do not change (the row grows; the virtualizer measures each row).
//
// jsdom lays nothing out, so this proves the CONTROL and its state; the wrap itself is checked
// in a real browser (see the commit).
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { render, waitFor, fireEvent } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'

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
  const getLog = vi.fn()
  return {
    getLog,
    // The Logbook reads the shared log store, which asks get_log_delta. Every answer here is
    // the whole log (a valid answer), stocked through `getLog` as before.
    getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: await getLog() })),
    deleteQso: noop(), editQso: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    saveTextToDownloads: noop(),
    logQso: noop(), markQslSent: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

const LONG =
  'Worked him on the 40 m long path at sunrise, running 5 W into a wire vertical; he asked for a ' +
  'card via the bureau and mentioned a DXpedition to the Pacific next spring'

function qso(comment: string | null) {
  return {
    call: 'K1ABC',
    grid: 'FN31',
    band: '40m',
    freqMhz: 7.074,
    mode: 'FT8',
    rstSent: '-10',
    rstRcvd: '-12',
    name: null,
    qth: null,
    comment,
    notes: null,
    country: 'United States',
    whenUnix: 1_700_000_000,
    confirmed: false,
    awardConfirmed: false,
    qslRcvd: null,
    qslSent: null,
    ota: null,
    upload: undefined,
  }
}

async function commentCell(comment: string | null): Promise<HTMLElement> {
  ;(api.getLog as ReturnType<typeof vi.fn>).mockResolvedValue([qso(comment)])
  const { container } = render(<Logbook defaultBand="40m" defaultFreqMhz={7.074} defaultMode="FT8" />)
  return waitFor(() => {
    const c = container.querySelector('.logbook-row:not(.head) .log-note')
    expect(c).not.toBeNull()
    return c as HTMLElement
  })
}

describe('logbook comment column (#162)', () => {
  it('the comment opens to its full text in the row, and folds back', async () => {
    const cell = await commentCell(LONG)
    const toggle = cell.querySelector('button')
    expect(toggle, 'no way to open a long comment except hovering').not.toBeNull()
    expect(toggle!.textContent).toBe(LONG)
    expect(toggle!.getAttribute('aria-expanded')).toBe('false')
    expect(cell.classList.contains('expanded')).toBe(false)

    fireEvent.click(toggle!)
    expect(toggle!.getAttribute('aria-expanded')).toBe('true')
    expect(cell.classList.contains('expanded')).toBe(true)

    fireEvent.click(toggle!)
    expect(toggle!.getAttribute('aria-expanded')).toBe('false')
    expect(cell.classList.contains('expanded')).toBe(false)
  })

  it('control: a row with no comment keeps its plain dash and has nothing to open', async () => {
    const cell = await commentCell(null)
    expect(cell.querySelector('button')).toBeNull()
    expect(cell.textContent).toBe('—')
  })
})
